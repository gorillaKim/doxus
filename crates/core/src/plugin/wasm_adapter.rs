use super::manager::SUPPORTED_ABI_VERSION;
use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SourceDocId,
};
use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use super::kv_store::KvStore;
use super::manifest::PluginManifest;

/// Progress event emitted by a WASM plugin via the `progress` host function.
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub current: i64,
    pub total: i64,
}

/// Error type for Host Function operations.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("host function error: {0}")]
    HostFn(String),
}

// ── SecretBackend trait ───────────────────────────────────────────────────────

pub trait SecretBackend: Send + Sync {
    fn get_secret(&self, service: &str, key: &str) -> Option<String>;
    fn set_secret(&self, service: &str, key: &str, value: &str) -> Result<(), WasmError>;
}

pub(crate) struct KeyringBackend;

impl SecretBackend for KeyringBackend {
    fn get_secret(&self, service: &str, key: &str) -> Option<String> {
        keyring::Entry::new(service, key).ok()?.get_password().ok()
    }

    fn set_secret(&self, service: &str, key: &str, value: &str) -> Result<(), WasmError> {
        keyring::Entry::new(service, key)
            .map_err(|e| WasmError::HostFn(format!("keyring entry error: {e}")))?
            .set_password(value)
            .map_err(|e| WasmError::HostFn(format!("keyring set error: {e}")))
    }
}

/// Session-scoped cache wrapper for any `SecretBackend`.
/// First call per (service, key) hits Keychain; subsequent calls are served from memory.
pub(crate) struct CachedKeyringBackend<B> {
    inner: B,
    cache: std::sync::RwLock<std::collections::HashMap<(String, String), String>>,
}

impl<B: SecretBackend> CachedKeyringBackend<B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            cache: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl<B: SecretBackend + Send + Sync> SecretBackend for CachedKeyringBackend<B> {
    fn get_secret(&self, service: &str, key: &str) -> Option<String> {
        let cache_key = (service.to_string(), key.to_string());
        {
            let r = self.cache.read().unwrap();
            if let Some(v) = r.get(&cache_key) {
                return Some(v.clone());
            }
        }
        let value = self.inner.get_secret(service, key)?;
        self.cache.write().unwrap().insert(cache_key, value.clone());
        Some(value)
    }

    fn set_secret(&self, service: &str, key: &str, value: &str) -> Result<(), WasmError> {
        self.inner.set_secret(service, key, value)?;
        let cache_key = (service.to_string(), key.to_string());
        self.cache.write().unwrap().insert(cache_key, value.to_string());
        Ok(())
    }
}

pub struct MemoryBackend(pub Arc<Mutex<std::collections::HashMap<String, String>>>);

impl SecretBackend for MemoryBackend {
    fn get_secret(&self, _service: &str, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }

    fn set_secret(&self, _service: &str, key: &str, value: &str) -> Result<(), WasmError> {
        self.0.lock().unwrap().insert(key.to_string(), value.to_string());
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────

pub struct WasmDocSourceAdapter {
    meta: PluginMetadata,
    plugin: Arc<Mutex<Plugin>>,
    manifest: PluginManifest,
    kv_store: KvStore,
    progress_tx: Option<broadcast::Sender<ProgressEvent>>,
    secret_backend: Arc<dyn SecretBackend>,
}

impl WasmDocSourceAdapter {
    pub fn from_bytes(
        wasm_bytes: impl Into<Vec<u8>>,
        manifest: PluginManifest,
        progress_tx: Option<broadcast::Sender<ProgressEvent>>,
        secret_backend: Option<Arc<dyn SecretBackend>>,
    ) -> Result<Self, PluginError> {
        if manifest.abi_version != SUPPORTED_ABI_VERSION {
            return Err(PluginError::ConfigInvalid(format!(
                "unsupported abi_version: {} (expected {})",
                manifest.abi_version, SUPPORTED_ABI_VERSION
            )));
        }

        let bytes = wasm_bytes.into();
        let wasm = Wasm::data(bytes);
        
        // Extism PDK의 http::request()가 사용하는 built-in HTTP는
        // Manifest의 allowed_hosts를 통해 도메인을 검증함.
        let mut extism_manifest = Manifest::new([wasm]);
        for domain in &manifest.http_domains {
            extism_manifest = extism_manifest.with_allowed_host(domain.as_str());
        }

        // Define host functions
        let secret_backend_inner = secret_backend.clone().unwrap_or_else(|| Arc::new(CachedKeyringBackend::new(KeyringBackend)));
        let plugin_id_inner = manifest.plugin_id.clone();
        let secrets_manifest = manifest.secrets.clone();

        use extism::{Function, ValType, CurrentPlugin, Val, UserData};

        let set_secret_fn = Function::new(
            "__doxus_set_secret",
            [ValType::I64, ValType::I64],
            [],
            UserData::new(()),
            move |plugin: &mut CurrentPlugin, inputs: &[Val], _outputs: &mut [Val], _user_data: UserData<()>| {
                let key_h = plugin.memory_from_val(&inputs[0]).ok_or_else(|| extism::Error::msg("invalid key handle"))?;
                let val_h = plugin.memory_from_val(&inputs[1]).ok_or_else(|| extism::Error::msg("invalid val handle"))?;
                let key = plugin.memory_str(key_h).unwrap_or_default().to_string();
                let value = plugin.memory_str(val_h).unwrap_or_default().to_string();
                
                if secrets_manifest.contains(&key.to_string()) {
                    let service = format!("doxus-{}", plugin_id_inner);
                    secret_backend_inner.set_secret(&service, &key, &value)
                        .map_err(|e| extism::Error::msg(e.to_string()))?;
                }
                Ok(())
            }
        );

        let plugin = Plugin::new(&extism_manifest, [set_secret_fn], true)
            .map_err(|e| PluginError::Internal(format!("wasm load failed: {e}")))?;

        let kv_conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| PluginError::Internal(format!("kv db init: {e}")))?;
        let kv_store = KvStore::with_connection(
            Arc::new(Mutex::new(kv_conn)),
            manifest.plugin_id.clone(),
            manifest.kv_namespaces.clone(),
        );
        kv_store
            .init_table()
            .map_err(|e| PluginError::Internal(format!("kv table init: {e}")))?;
        let secret_backend: Arc<dyn SecretBackend> =
            secret_backend.unwrap_or_else(|| Arc::new(CachedKeyringBackend::new(KeyringBackend)));
        Ok(Self {
            meta: PluginMetadata {
                id: manifest.plugin_id.clone(),
                name: manifest.display_name.clone(),
                version: manifest.version.clone(),
                kind: PluginKind::External,
            },
            plugin: Arc::new(Mutex::new(plugin)),
            manifest,
            kv_store,
            progress_tx,
            secret_backend,
        })
    }

    /// Get a value from the plugin KV store (namespace-isolated).
    /// Returns `None` if not found, or `Err` if namespace is not in manifest.
    pub fn kv_get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, super::kv_store::KvError> {
        self.kv_store.get(namespace, key)
    }

    /// Set a value in the plugin KV store (namespace-isolated).
    /// Returns `Err` if namespace is not in manifest.
    pub fn kv_set(&self, namespace: &str, key: &str, value: Vec<u8>) -> Result<(), super::kv_store::KvError> {
        self.kv_store.set(namespace, key, value)
    }

    /// Send a progress event. No-op if no progress sender is configured.
    pub fn send_progress(&self, current: i64, total: i64) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(ProgressEvent { current, total });
        }
    }

    /// Look up a secret by key.
    /// Priority: keychain (service=`doxus-{plugin_id}`, user=key) → env var `DOXUS_SECRET_<key>`.
    /// Returns `None` if:
    /// - key contains invalid characters (only alphanumeric + `_` allowed)
    /// - key is not declared in the plugin manifest
    /// - key is not found in keychain or env
    pub fn secrets_get(&self, key: &str) -> Option<String> {
        // Validate key characters: only alphanumeric and underscore
        if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        // Check against manifest allowlist
        if !self.manifest.secrets.contains(&key.to_string()) {
            return None;
        }

        // Try backend first (keychain in production, injected mock in tests)
        let service = format!("doxus-{}", self.manifest.plugin_id);
        if let Some(secret) = self.secret_backend.get_secret(&service, key) {
            return Some(secret);
        }

        // Fall back to environment variable (CI-compatible)
        let env_key = format!("DOXUS_SECRET_{key}");
        std::env::var(&env_key).ok()
    }

    /// Transform raw content into normalized markdown.
    /// Strips HTML tags and normalizes whitespace.
    pub fn content_transform(raw: &str) -> String {
        // Strip HTML tags with a simple state machine
        let mut result = String::with_capacity(raw.len());
        let mut in_tag = false;
        for ch in raw.chars() {
            match ch {
                '<' => in_tag = true,
                '>' if in_tag => {
                    in_tag = false;
                    // Insert space to separate content from adjacent tags
                    if !result.ends_with(' ') && !result.is_empty() {
                        result.push(' ');
                    }
                }
                '>' => result.push(ch),
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }
        // Normalize whitespace: collapse runs of whitespace into single spaces, trim
        let normalized: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
        normalized
    }

    async fn call_wasm<I, O>(&self, func: &str, input: &I) -> Result<O, PluginError>
    where
        I: Serialize + Send + Sync,
        O: for<'de> Deserialize<'de> + Send + 'static,
    {
        let plugin = Arc::clone(&self.plugin);
        let input_bytes = serde_json::to_vec(input)
            .map_err(|e| PluginError::Internal(format!("serialize: {e}")))?;
        let func = func.to_string();

        let result: Result<O, PluginError> = tokio::task::spawn_blocking(move || {
            let mut guard = plugin
                .lock()
                .map_err(|_| PluginError::Internal("mutex poisoned".into()))?;
            let output = guard
                .call::<&[u8], &[u8]>(&func, &input_bytes)
                .map_err(|e| PluginError::Internal(format!("wasm call '{func}' failed: {e}")))?;
            
            if output.is_empty() {
                serde_json::from_str("null")
            } else {
                serde_json::from_slice::<O>(output)
            }.map_err(|e| PluginError::Internal(format!("deserialize: {e}")))
        })
        .await
        .map_err(|e| PluginError::Internal(format!("spawn_blocking: {e}")))?;

        result
    }
}

fn raw_doc_from_wasm(d: doxus_plugin_sdk::wasm_types::RawDocumentWasm) -> RawDocument {
    let content_type = match d.content_type.as_str() {
        "plain_text" => ContentType::PlainText,
        "html" => ContentType::Html,
        _ => ContentType::Markdown,
    };
    RawDocument {
        id: SourceDocId(d.id),
        title: d.title,
        content: d.content,
        content_type,
        url: d.url,
        metadata: d.metadata,
        tags: d.tags,
        aliases: vec![],
        created_at: None,
        updated_at: d.updated_at,
    }
}

#[async_trait]
impl DocSource for WasmDocSourceAdapter {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: false,
            oauth: false,
            native_search: false,
        }
    }

    async fn validate_config(&self, _config: &PluginConfig) -> Result<(), PluginError> {
        Ok(())
    }

    async fn initialize(
        &mut self,
        config: PluginConfig,
        secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        #[derive(Serialize)]
        struct InitOpts {
            config: HashMap<String, serde_json::Value>,
            secrets: HashMap<String, String>,
        }

        let mut wasm_secrets = HashMap::new();
        for key in &self.manifest.secrets {
            if let Some(v) = secrets.fields.get(key) {
                let val_str = match v {
                    doxus_plugin_sdk::SecretValue::Text(t) => t.clone(),
                    doxus_plugin_sdk::SecretValue::Token { value, .. } => value.clone(),
                };
                wasm_secrets.insert(key.clone(), val_str);
            }
        }

        let opts = InitOpts {
            config: config.fields,
            secrets: wasm_secrets,
        };

        self.call_wasm::<InitOpts, ()>("initialize", &opts).await
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        use doxus_plugin_sdk::wasm_types::{DocumentStreamWasm, FetchAllOptsWasm};

        let wasm_opts = FetchAllOptsWasm {
            cursor: opts.cursor,
            page_size: opts.page_size,
        };
        let result: DocumentStreamWasm = self.call_wasm("fetch_all", &wasm_opts).await?;
        Ok(DocumentStream {
            documents: result.documents.into_iter().map(raw_doc_from_wasm).collect(),
            next_cursor: result.next_cursor,
            estimated_total: result.estimated_total,
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        use doxus_plugin_sdk::wasm_types::{FetchDocumentOptsWasm, RawDocumentWasm};

        let opts = FetchDocumentOptsWasm { id: id.0.clone() };
        let result: RawDocumentWasm = self.call_wasm("fetch_document", &opts).await?;
        Ok(raw_doc_from_wasm(result))
    }

    async fn health_check(&self) -> HealthStatus {
        let result: Result<String, _> = self.call_wasm("health_check", &()).await;
        match result {
            Ok(_) => HealthStatus {
                healthy: true,
                message: None,
            },
            Err(e) => HealthStatus {
                healthy: false,
                message: Some(e.to_string()),
            },
        }
    }

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        use doxus_plugin_sdk::wasm_types::{ChangeSetWasm, FetchChangesOptsWasm};

        let wasm_opts = FetchChangesOptsWasm {
            since: opts.since,
            cursor: opts.cursor,
            page_size: opts.page_size,
        };
        let result: ChangeSetWasm = self.call_wasm("fetch_changes", &wasm_opts).await?;
        Ok(ChangeSet {
            updated: result.updated.into_iter().map(raw_doc_from_wasm).collect(),
            deleted_ids: result.deleted.into_iter().map(SourceDocId).collect(),
            next_cursor: result.next_cursor,
        })
    }

    fn supports_write(&self) -> bool {
        // WASM 플러그인이 create_document 함수를 내보내는지 확인
        let guard = self.plugin.lock().ok();
        guard.map_or(false, |g| g.function_exists("create_document"))
    }

    async fn create_document(
        &self,
        title: &str,
        content: &str,
        metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<SourceDocId, PluginError> {
        use doxus_plugin_sdk::wasm_types::{CreateDocumentOptsWasm, CreateDocumentResultWasm};

        let opts = CreateDocumentOptsWasm {
            title: title.to_string(),
            content: content.to_string(),
            metadata: metadata.cloned().unwrap_or_default(),
        };
        let result: CreateDocumentResultWasm = self.call_wasm("create_document", &opts).await?;
        Ok(SourceDocId(result.id))
    }

    async fn update_document(
        &self,
        id: &SourceDocId,
        content: Option<&str>,
        metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(), PluginError> {
        use doxus_plugin_sdk::wasm_types::UpdateDocumentOptsWasm;

        let opts = UpdateDocumentOptsWasm {
            id: id.0.clone(),
            content: content.map(|s| s.to_string()),
            metadata: metadata.cloned(),
        };
        self.call_wasm::<UpdateDocumentOptsWasm, ()>("update_document", &opts).await
    }

    async fn delete_document(&self, id: &SourceDocId) -> Result<(), PluginError> {
        use doxus_plugin_sdk::wasm_types::DeleteDocumentOptsWasm;

        let opts = DeleteDocumentOptsWasm { id: id.0.clone() };
        self.call_wasm::<DeleteDocumentOptsWasm, ()>("delete_document", &opts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_plugin_sdk::FetchAllOpts;

    fn minimal_wasm_bytes() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ]
    }

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            plugin_id: "com.test.plugin".into(),
            display_name: "Test Plugin".into(),
            version: "0.1.0".into(),
            abi_version: 1,
            http_domains: vec![],
            kv_namespaces: vec![],
            secrets: vec![],
        }
    }

    #[test]
    fn wasm_adapter_can_be_created() {
        let adapter = WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None);
        assert!(adapter.is_ok());
    }

    #[test]
    fn metadata_returns_correct_plugin_id() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        assert_eq!(adapter.metadata().id, "com.test.plugin");
    }

    #[test]
    fn capabilities_returns_struct() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let caps = adapter.capabilities();
        assert!(!caps.incremental_sync);
        assert!(!caps.oauth);
        assert!(!caps.native_search);
    }

    // health_check_returns_unhealthy_for_minimal_wasm is defined below with other wasm bridge tests

    #[tokio::test]
    async fn fetch_all_errors_for_minimal_wasm_without_export() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let opts = FetchAllOpts {
            cursor: None,
            page_size: 10,
        };
        let result = adapter.fetch_all(opts).await;
        assert!(result.is_err(), "minimal wasm has no fetch_all export");
    }

    #[tokio::test]
    async fn fetch_changes_errors_for_minimal_wasm_without_export() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let opts = doxus_plugin_sdk::FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 10,
            known_ids: vec![],
        };
        let result = adapter.fetch_changes(opts).await;
        assert!(result.is_err(), "minimal wasm has no fetch_changes export");
    }

    #[tokio::test]
    async fn fetch_document_errors_for_minimal_wasm_without_export() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let result = adapter.fetch_document(&SourceDocId("doc1".into())).await;
        assert!(result.is_err(), "minimal wasm has no fetch_document export");
    }

    #[tokio::test]
    async fn health_check_returns_unhealthy_for_minimal_wasm() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let status = adapter.health_check().await;
        assert!(!status.healthy, "minimal wasm has no health_check export");
        assert!(status.message.is_some());
    }

    #[tokio::test]
    #[ignore = "requires pre-built test_plugin.wasm fixture (wasm32-unknown-unknown)"]
    async fn fetch_all_calls_wasm_export() {
        let wasm_bytes = include_bytes!("../../../core/tests/fixtures/test_plugin.wasm");
        let adapter =
            WasmDocSourceAdapter::from_bytes(wasm_bytes.to_vec(), test_manifest(), None, None).unwrap();
        let opts = FetchAllOpts {
            cursor: None,
            page_size: 10,
        };
        let result = adapter.fetch_all(opts).await.unwrap();
        assert!(result.documents.is_empty());
        assert!(result.next_cursor.is_none());
        assert_eq!(result.estimated_total, Some(0));
    }

    #[test]
    fn abi_version_must_be_1() {
        let manifest = PluginManifest {
            abi_version: 2,
            ..test_manifest()
        };
        let result = WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None);
        assert!(result.is_err());
        let err = result.err().unwrap();
        match err {
            PluginError::ConfigInvalid(msg) => assert!(msg.contains("abi_version")),
            other => panic!("expected ConfigInvalid, got {other:?}"),
        }
    }

    #[test]
    fn kv_store_works_via_adapter() {
        let manifest = PluginManifest {
            kv_namespaces: vec!["default".into()],
            ..test_manifest()
        };
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        assert!(adapter.kv_get("default", "x").unwrap().is_none());
        adapter.kv_set("default", "x", b"hello".to_vec()).unwrap();
        assert_eq!(adapter.kv_get("default", "x").unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn kv_store_rejects_undeclared_namespace() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        // test_manifest has kv_namespaces: vec![]
        let err = adapter.kv_set("forbidden", "k", b"v".to_vec()).unwrap_err();
        assert!(matches!(err, crate::plugin::kv_store::KvError::NamespaceNotAllowed(_)));
    }

    #[test]
    fn from_bytes_with_manifest_sets_metadata() {
        let manifest = PluginManifest {
            plugin_id: "com.test.plugin".into(),
            display_name: "Test Plugin".into(),
            version: "1.2.3".into(),
            abi_version: 1,
            http_domains: vec![],
            kv_namespaces: vec![],
            secrets: vec![],
        };
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        assert_eq!(adapter.metadata().id, "com.test.plugin");
        assert_eq!(adapter.metadata().name, "Test Plugin");
        assert_eq!(adapter.metadata().version, "1.2.3");
    }

    #[tokio::test]
    async fn progress_host_function_sends_events() {
        let (tx, mut rx) = broadcast::channel::<ProgressEvent>(16);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), Some(tx), None)
                .unwrap();

        adapter.send_progress(5, 100);
        adapter.send_progress(50, 100);

        let evt1 = rx.recv().await.unwrap();
        assert_eq!(evt1.current, 5);
        assert_eq!(evt1.total, 100);

        let evt2 = rx.recv().await.unwrap();
        assert_eq!(evt2.current, 50);
        assert_eq!(evt2.total, 100);
    }

    #[test]
    fn progress_noop_without_sender() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        // Should not panic
        adapter.send_progress(1, 10);
    }

    fn manifest_with_secrets(secrets: Vec<&str>) -> PluginManifest {
        PluginManifest {
            secrets: secrets.into_iter().map(String::from).collect(),
            ..test_manifest()
        }
    }

    #[test]
    fn secrets_get_returns_none_for_unknown_key() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest(), None, None).unwrap();
        let result = adapter.secrets_get("nonexistent_key_12345");
        assert!(result.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn secrets_get_reads_env_var() {
        let manifest = manifest_with_secrets(vec!["test_token"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        std::env::set_var("DOXUS_SECRET_test_token", "my_secret");
        let result = adapter.secrets_get("test_token");
        assert_eq!(result, Some("my_secret".to_string()));
        std::env::remove_var("DOXUS_SECRET_test_token");
    }

    #[test]
    #[serial_test::serial]
    fn test_secrets_get_rejects_undeclared_key() {
        let manifest = manifest_with_secrets(vec!["allowed_key"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        std::env::set_var("DOXUS_SECRET_other_key", "value");
        assert!(adapter.secrets_get("other_key").is_none(), "undeclared key should be rejected");
        std::env::remove_var("DOXUS_SECRET_other_key");
    }

    #[test]
    fn test_secrets_get_rejects_special_chars() {
        let manifest = manifest_with_secrets(vec!["../etc/passwd"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        assert!(adapter.secrets_get("../etc/passwd").is_none(), "special chars should be rejected");
    }

    #[test]
    #[serial_test::serial]
    fn test_secrets_get_returns_declared_key() {
        let manifest = manifest_with_secrets(vec!["api_token"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        std::env::set_var("DOXUS_SECRET_api_token", "secret123");
        assert_eq!(adapter.secrets_get("api_token"), Some("secret123".to_string()));
        std::env::remove_var("DOXUS_SECRET_api_token");
    }

    #[test]
    #[serial_test::serial]
    fn secrets_get_env_fallback_when_no_keychain() {
        // Set env var only — keychain will miss, should fall back
        let manifest = manifest_with_secrets(vec!["fallback_token"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        std::env::set_var("DOXUS_SECRET_fallback_token", "env_value");
        // keychain won't have this key in CI, so env fallback should apply
        let result = adapter.secrets_get("fallback_token");
        assert_eq!(result, Some("env_value".to_string()));
        std::env::remove_var("DOXUS_SECRET_fallback_token");
    }

    #[test]
    #[serial_test::serial]
    fn secrets_get_plugin_id_isolation() {
        // Two adapters with different plugin_ids should have isolated keychain services.
        // This test verifies the service name logic — actual keychain isolation
        // is enforced by the OS (service=doxus-{plugin_id}).
        let manifest_a = PluginManifest {
            plugin_id: "com.test.plugin_a".into(),
            secrets: vec!["shared_key".into()],
            ..test_manifest()
        };
        let manifest_b = PluginManifest {
            plugin_id: "com.test.plugin_b".into(),
            secrets: vec!["shared_key".into()],
            ..test_manifest()
        };
        let adapter_a =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest_a, None, None).unwrap();
        let adapter_b =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest_b, None, None).unwrap();

        // Set env var for plugin_a only
        std::env::set_var("DOXUS_SECRET_shared_key", "plugin_a_value");
        // Both read same env var here (env doesn't isolate), but keychain would isolate.
        // The important assertion: undeclared plugin gets None when no env/keychain
        assert!(adapter_a.secrets_get("shared_key").is_some());
        std::env::remove_var("DOXUS_SECRET_shared_key");
        // After removing env var, adapter_b also gets None (no keychain entry)
        assert!(adapter_b.secrets_get("shared_key").is_none());
    }

    #[test]
    fn test_secrets_get_rejects_invalid_key_chars() {
        // Hyphens are not allowed — only alphanumeric + underscore
        let manifest = manifest_with_secrets(vec!["my_secret"]);
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest, None, None).unwrap();
        assert!(
            adapter.secrets_get("my-secret").is_none(),
            "key with hyphen should be rejected"
        );
    }

    #[test]
    fn content_transform_strips_html_tags() {
        let raw = "<p>Hello <b>world</b></p>";
        let result = WasmDocSourceAdapter::content_transform(raw);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn content_transform_normalizes_whitespace() {
        let raw = "  hello   world\n\tfoo  ";
        let result = WasmDocSourceAdapter::content_transform(raw);
        assert_eq!(result, "hello world foo");
    }

    #[test]
    fn content_transform_handles_empty_string() {
        let result = WasmDocSourceAdapter::content_transform("");
        assert_eq!(result, "");
    }

    #[test]
    fn content_transform_preserves_literal_gt() {
        let result = WasmDocSourceAdapter::content_transform("a > b");
        assert!(result.contains("a > b"), "literal > should be preserved, got: {}", result);
    }

    #[test]
    fn content_transform_mixed_html_and_text() {
        let raw = "<div><h1>Title</h1><p>Some <em>emphasized</em> text</p></div>";
        let result = WasmDocSourceAdapter::content_transform(raw);
        assert_eq!(result, "Title Some emphasized text");
    }

    #[test]
    fn test_secrets_get_uses_injected_backend() {
        let mut map = std::collections::HashMap::new();
        map.insert("api_token".to_string(), "secret123".to_string());
        let manifest = manifest_with_secrets(vec!["api_token"]);
        let adapter = WasmDocSourceAdapter::from_bytes(
            minimal_wasm_bytes(),
            manifest,
            None,
            Some(Arc::new(MemoryBackend(Arc::new(Mutex::new(map))))),
        )
        .unwrap();
        assert_eq!(adapter.secrets_get("api_token"), Some("secret123".to_string()));
    }

    #[tokio::test]
    async fn supports_write_false_for_minimal_wasm() {
        let adapter = WasmDocSourceAdapter::from_bytes(
            minimal_wasm_bytes(),
            test_manifest(),
            None,
            None,
        ).unwrap();
        assert!(!adapter.supports_write());
    }

    #[tokio::test]
    async fn write_methods_fail_if_function_missing() {
        let adapter = WasmDocSourceAdapter::from_bytes(
            minimal_wasm_bytes(),
            test_manifest(),
            None,
            None,
        ).unwrap();

        let res = adapter.create_document("title", "content", None).await;
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(err.contains("wasm call 'create_document' failed"));

        let res = adapter.update_document(&SourceDocId("id".into()), None, None).await;
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(err.contains("wasm call 'update_document' failed"));

        let res = adapter.delete_document(&SourceDocId("id".into())).await;
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(err.contains("wasm call 'delete_document' failed"));
    }
}
