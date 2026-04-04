use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, DocSource, DocumentStream, FetchAllOpts, FetchChangesOpts,
    HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata, PluginSecrets,
    RawDocument, SourceDocId,
};
use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::kv_store::KvStore;
use super::manifest::PluginManifest;

/// Error type for Host Function operations.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("host function error: {0}")]
    HostFn(String),
}

/// HTTP request payload sent by a WASM plugin via the `http_request` host function.
#[derive(Debug, serde::Deserialize)]
pub struct HttpRequest {
    pub url: String,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
}

/// HTTP response returned to the WASM plugin.
#[derive(Debug, serde::Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub headers: HashMap<String, String>,
}

pub struct WasmDocSourceAdapter {
    meta: PluginMetadata,
    plugin: Arc<Mutex<Plugin>>,
    manifest: PluginManifest,
    kv_store: KvStore,
    allowed_domains: Vec<String>,
}

impl WasmDocSourceAdapter {
    pub fn from_bytes(
        wasm_bytes: impl Into<Vec<u8>>,
        manifest: PluginManifest,
    ) -> Result<Self, PluginError> {
        if manifest.abi_version != 1 {
            return Err(PluginError::ConfigInvalid(format!(
                "unsupported abi_version: {} (expected 1)",
                manifest.abi_version
            )));
        }

        let bytes = wasm_bytes.into();
        let wasm = Wasm::data(bytes);
        let extism_manifest = Manifest::new([wasm]);
        let plugin = Plugin::new(&extism_manifest, [], true)
            .map_err(|e| PluginError::Internal(format!("wasm load failed: {e}")))?;

        let allowed_domains = manifest.http_domains.clone();
        Ok(Self {
            meta: PluginMetadata {
                id: manifest.plugin_id.clone(),
                name: manifest.display_name.clone(),
                version: manifest.version.clone(),
                kind: PluginKind::External,
            },
            plugin: Arc::new(Mutex::new(plugin)),
            manifest,
            kv_store: KvStore::new(),
            allowed_domains,
        })
    }

    /// Create an adapter with an explicit domain allowlist (useful for testing).
    pub fn new_with_domains(allowed_domains: Vec<String>) -> Self {
        let manifest = PluginManifest {
            plugin_id: "com.test.stub".into(),
            display_name: "Stub".into(),
            version: "0.0.0".into(),
            abi_version: 1,
            http_domains: allowed_domains.clone(),
            kv_namespaces: vec![],
        };
        // Minimal valid WASM module bytes
        let wasm_bytes: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, // magic
            0x01, 0x00, 0x00, 0x00, // version
        ];
        let wasm = Wasm::data(wasm_bytes);
        let extism_manifest = Manifest::new([wasm]);
        let plugin = Plugin::new(&extism_manifest, [], true).expect("minimal wasm load failed");
        Self {
            meta: PluginMetadata {
                id: manifest.plugin_id.clone(),
                name: manifest.display_name.clone(),
                version: manifest.version.clone(),
                kind: PluginKind::External,
            },
            plugin: Arc::new(Mutex::new(plugin)),
            manifest,
            kv_store: KvStore::new(),
            allowed_domains,
        }
    }

    pub fn kv_get(&self, key: &str) -> Option<Vec<u8>> {
        self.kv_store.get(key)
    }

    pub fn kv_set(&self, key: String, value: Vec<u8>) {
        self.kv_store.set(key, value)
    }

    pub fn is_http_allowed(&self, url: &str) -> bool {
        self.manifest.is_domain_allowed(url)
    }

    /// Execute an HTTP request on behalf of a WASM plugin.
    /// Enforces the domain allowlist from the plugin manifest (SSRF protection).
    pub async fn http_request(&self, req: &HttpRequest) -> Result<HttpResponse, WasmError> {
        let url = url::Url::parse(&req.url)
            .map_err(|e| WasmError::HostFn(format!("invalid URL: {e}")))?;
        let host = url
            .host_str()
            .ok_or_else(|| WasmError::HostFn("URL has no host".into()))?;

        if !self.is_domain_allowed(host) {
            return Err(WasmError::HostFn(format!(
                "domain '{host}' not in plugin allowlist"
            )));
        }

        let method = req.method.as_deref().unwrap_or("GET").to_uppercase();
        let client = reqwest::Client::new();
        let mut builder = match method.as_str() {
            "GET" => client.get(url.as_str()),
            "POST" => client.post(url.as_str()),
            "PUT" => client.put(url.as_str()),
            "DELETE" => client.delete(url.as_str()),
            m => return Err(WasmError::HostFn(format!("unsupported method: {m}"))),
        };
        if let Some(headers) = &req.headers {
            for (k, v) in headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
        }
        if let Some(body) = &req.body {
            builder = builder.body(body.clone());
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| WasmError::HostFn(format!("request failed: {e}")))?;
        let status = resp.status().as_u16();
        let resp_headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|vs| (k.to_string(), vs.to_string())))
            .collect();
        let body = resp
            .text()
            .await
            .map_err(|e| WasmError::HostFn(format!("failed to read body: {e}")))?;

        Ok(HttpResponse {
            status,
            body,
            headers: resp_headers,
        })
    }

    fn is_domain_allowed(&self, host: &str) -> bool {
        if self.allowed_domains.is_empty() {
            return false;
        }
        self.allowed_domains.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix("*.") {
                host == suffix || host.ends_with(&format!(".{suffix}"))
            } else {
                host == pattern.as_str()
            }
        })
    }

    /// Call a WASM function with JSON input, get JSON output
    #[allow(dead_code)]
    async fn call_wasm<I, O>(&self, func: &str, input: &I) -> Result<O, PluginError>
    where
        I: Serialize + Send + Sync,
        O: for<'de> Deserialize<'de> + Send + 'static,
    {
        let plugin = Arc::clone(&self.plugin);
        let input_bytes = serde_json::to_vec(input)
            .map_err(|e| PluginError::Internal(format!("serialize: {e}")))?;
        let func = func.to_string();

        tokio::task::spawn_blocking(move || {
            let mut guard = plugin
                .lock()
                .map_err(|_| PluginError::Internal("mutex poisoned".into()))?;
            let output = guard
                .call::<&[u8], &[u8]>(&func, &input_bytes)
                .map_err(|e| PluginError::Internal(format!("wasm call '{func}' failed: {e}")))?;
            serde_json::from_slice::<O>(output)
                .map_err(|e| PluginError::Internal(format!("deserialize: {e}")))
        })
        .await
        .map_err(|e| PluginError::Internal(format!("spawn_blocking: {e}")))?
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
        _config: PluginConfig,
        _secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        Ok(())
    }

    async fn fetch_all(&self, _opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        Ok(DocumentStream {
            documents: vec![],
            next_cursor: None,
            estimated_total: Some(0),
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        Err(PluginError::NotFound(id.0.clone()))
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus {
            healthy: true,
            message: None,
        }
    }

    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Ok(ChangeSet {
            updated: vec![],
            deleted_ids: vec![],
            next_cursor: None,
        })
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
        }
    }

    #[test]
    fn wasm_adapter_can_be_created() {
        let adapter = WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest());
        assert!(adapter.is_ok());
    }

    #[test]
    fn metadata_returns_correct_plugin_id() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        assert_eq!(adapter.metadata().id, "com.test.plugin");
    }

    #[test]
    fn capabilities_returns_struct() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let caps = adapter.capabilities();
        assert!(!caps.incremental_sync);
        assert!(!caps.oauth);
        assert!(!caps.native_search);
    }

    #[tokio::test]
    async fn health_check_returns_healthy_when_loaded() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let status = adapter.health_check().await;
        assert!(status.healthy);
        assert!(status.message.is_none());
    }

    #[tokio::test]
    async fn fetch_all_returns_empty_stream_for_minimal_wasm() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let opts = FetchAllOpts {
            cursor: None,
            page_size: 10,
        };
        let result = adapter.fetch_all(opts).await;
        assert!(result.is_ok());
        let stream = result.unwrap();
        assert!(stream.documents.is_empty());
        assert!(stream.next_cursor.is_none());
    }

    #[tokio::test]
    async fn fetch_changes_returns_empty_changeset() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let opts = doxus_plugin_sdk::FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 10,
        };
        let result = adapter.fetch_changes(opts).await;
        assert!(result.is_ok());
        let changeset = result.unwrap();
        assert!(changeset.updated.is_empty());
        assert!(changeset.deleted_ids.is_empty());
        assert!(changeset.next_cursor.is_none());
    }

    #[test]
    fn abi_version_must_be_1() {
        let manifest = PluginManifest {
            abi_version: 2,
            ..test_manifest()
        };
        let result = WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest);
        assert!(result.is_err());
        let err = result.err().unwrap();
        match err {
            PluginError::ConfigInvalid(msg) => assert!(msg.contains("abi_version")),
            other => panic!("expected ConfigInvalid, got {other:?}"),
        }
    }

    #[test]
    fn kv_store_works_via_adapter() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        assert!(adapter.kv_get("x").is_none());
        adapter.kv_set("x".into(), b"hello".to_vec());
        assert_eq!(adapter.kv_get("x"), Some(b"hello".to_vec()));
    }

    #[test]
    fn http_allowed_respects_manifest() {
        let manifest = PluginManifest {
            http_domains: vec!["example.com".into()],
            ..test_manifest()
        };
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest).unwrap();
        assert!(adapter.is_http_allowed("https://example.com/api"));
        assert!(!adapter.is_http_allowed("https://evil.com/api"));
    }

    #[tokio::test]
    async fn http_request_allowed_domain_succeeds() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/data"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("hello"))
            .mount(&server)
            .await;

        let host = url::Url::parse(&server.uri())
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let adapter = WasmDocSourceAdapter::new_with_domains(vec![host.clone()]);
        let req = HttpRequest {
            url: format!("{}/api/data", server.uri()),
            method: Some("GET".into()),
            headers: None,
            body: None,
        };
        let resp = adapter.http_request(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "hello");
    }

    #[tokio::test]
    async fn http_request_blocked_domain_returns_error() {
        let adapter =
            WasmDocSourceAdapter::new_with_domains(vec!["allowed.example.com".into()]);
        let req = HttpRequest {
            url: "http://evil.example.com/steal".into(),
            method: None,
            headers: None,
            body: None,
        };
        let result = adapter.http_request(&req).await;
        assert!(matches!(result, Err(WasmError::HostFn(_))));
    }

    #[tokio::test]
    async fn http_request_wildcard_domain_allowed() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let host = url::Url::parse(&server.uri())
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let adapter = WasmDocSourceAdapter::new_with_domains(vec![
            "*.example.com".into(),
            host.clone(),
        ]);
        let req = HttpRequest {
            url: format!("{}/", server.uri()),
            method: None,
            headers: None,
            body: None,
        };
        let resp = adapter.http_request(&req).await.unwrap();
        assert_eq!(resp.status, 204);
    }

    #[tokio::test]
    async fn http_request_empty_allowlist_blocks_all() {
        let adapter = WasmDocSourceAdapter::new_with_domains(vec![]);
        let req = HttpRequest {
            url: "http://example.com/api".into(),
            method: None,
            headers: None,
            body: None,
        };
        let result = adapter.http_request(&req).await;
        assert!(matches!(result, Err(WasmError::HostFn(_))));
    }

    #[tokio::test]
    async fn http_request_invalid_url_returns_error() {
        let adapter =
            WasmDocSourceAdapter::new_with_domains(vec!["example.com".into()]);
        let req = HttpRequest {
            url: "not a valid url".into(),
            method: None,
            headers: None,
            body: None,
        };
        let result = adapter.http_request(&req).await;
        assert!(matches!(result, Err(WasmError::HostFn(_))));
    }

    #[tokio::test]
    async fn http_request_unsupported_method_returns_error() {
        let adapter =
            WasmDocSourceAdapter::new_with_domains(vec!["example.com".into()]);
        let req = HttpRequest {
            url: "http://example.com/api".into(),
            method: Some("PATCH".into()),
            headers: None,
            body: None,
        };
        let result = adapter.http_request(&req).await;
        assert!(matches!(result, Err(WasmError::HostFn(_))));
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
        };
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), manifest).unwrap();
        assert_eq!(adapter.metadata().id, "com.test.plugin");
        assert_eq!(adapter.metadata().name, "Test Plugin");
        assert_eq!(adapter.metadata().version, "1.2.3");
    }
}
