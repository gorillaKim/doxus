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
        _config: PluginConfig,
        _secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        Ok(())
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

    // health_check_returns_unhealthy_for_minimal_wasm is defined below with other wasm bridge tests

    #[tokio::test]
    async fn fetch_all_errors_for_minimal_wasm_without_export() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
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
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let opts = doxus_plugin_sdk::FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 10,
        };
        let result = adapter.fetch_changes(opts).await;
        assert!(result.is_err(), "minimal wasm has no fetch_changes export");
    }

    #[tokio::test]
    async fn fetch_document_errors_for_minimal_wasm_without_export() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let result = adapter.fetch_document(&SourceDocId("doc1".into())).await;
        assert!(result.is_err(), "minimal wasm has no fetch_document export");
    }

    #[tokio::test]
    async fn health_check_returns_unhealthy_for_minimal_wasm() {
        let adapter =
            WasmDocSourceAdapter::from_bytes(minimal_wasm_bytes(), test_manifest()).unwrap();
        let status = adapter.health_check().await;
        assert!(!status.healthy, "minimal wasm has no health_check export");
        assert!(status.message.is_some());
    }

    #[tokio::test]
    #[ignore = "requires pre-built test_plugin.wasm fixture (wasm32-unknown-unknown)"]
    async fn fetch_all_calls_wasm_export() {
        let wasm_bytes = include_bytes!("../../../core/tests/fixtures/test_plugin.wasm");
        let adapter =
            WasmDocSourceAdapter::from_bytes(wasm_bytes.to_vec(), test_manifest()).unwrap();
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
