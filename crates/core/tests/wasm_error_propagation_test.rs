use std::sync::Arc;
use std::collections::HashMap;
use std::path::PathBuf;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};
use serde_json::json;

use doxus_core::plugin::wasm_adapter::WasmDocSourceAdapter;
use doxus_core::plugin::manifest::PluginManifest;
use doxus_core::secrets::{SecretStore, SecretsError};
use doxus_plugin_sdk::{DocSource, PluginConfig, PluginSecrets, SecretValue, FetchAllOpts};

fn wasm_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/core -> crates/
    p.pop(); // crates/ -> workspace root
    p.push("target/wasm32-unknown-unknown/debug/doxus_plugin_confluence.wasm");
    p
}

/// 항상 실패하는 시크릿 백엔드
struct FailingBackend;
impl SecretStore for FailingBackend {
    fn get(&self, _service: &str, _key: &str) -> Result<String, SecretsError> {
        Err(SecretsError::NotFound("Simulated".into()))
    }
    fn set(&self, _service: &str, _key: &str, _value: &str) -> Result<(), SecretsError> {
        Err(SecretsError::Keychain("Simulated backend failure".into()))
    }
    fn delete(&self, _service: &str, _key: &str) -> Result<(), SecretsError> {
        Ok(())
    }
}

/// TDD Red 테스트:
/// __doxus_set_secret 호스트 함수가 실패하면 그 에러가 fetch_all까지 전파되어야 한다.
/// 현재 구현은 `let _ = __doxus_set_secret(...)` 로 에러를 무시하므로 이 테스트는 실패(Red)해야 한다.
#[tokio::test]
async fn test_set_secret_error_must_propagate() {
    let server = MockServer::start().await;
    let base_url = server.uri();

    // Mock: OAuth token refresh 성공
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;

    // Mock: Confluence API (새 토큰으로도 응답)
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "id": "page-1",
                "title": "Test Page",
                "type": "page",
                "_links": {"webui": "/display/TEST/Test-Page"},
                "body": {"storage": {"value": "<p>Hello</p>"}},
                "version": {"when": "2024-04-14T10:00:00Z"},
                "space": {"key": "TEST"}
            }],
            "start": 0, "limit": 10, "size": 1
        })))
        .mount(&server)
        .await;

    let wasm_bytes = std::fs::read(wasm_path()).expect("WASM build required");
    let manifest = PluginManifest {
        plugin_id: "com.doxus.confluence".into(),
        display_name: "Confluence".into(),
        version: "0.1.0".into(),
        abi_version: 1,
        http_domains: vec!["127.0.0.1".into()],
        kv_namespaces: vec![],
        secrets: vec!["access_token".into(), "refresh_token".into(), "expires_at".into()],
    };

    let mut adapter = WasmDocSourceAdapter::from_bytes(
        wasm_bytes,
        manifest,
        None,
        Some(Arc::new(FailingBackend)),
    ).expect("Adapter creation failed");

    let config_fields: HashMap<String, serde_json::Value> = [
        ("base_url".into(), json!(base_url.clone())),
        ("space_key".into(), json!("TEST")),
        ("client_id".into(), json!("client-123")),
        ("client_secret".into(), json!("secret-456")),
        ("oauth_base_url".into(), json!(base_url)),
    ].into();

    let secret_fields: HashMap<String, SecretValue> = [
        ("access_token".into(), SecretValue::Text("expired".into())),
        ("refresh_token".into(), SecretValue::Text("old-refresh".into())),
        // expires_at=0 → 토큰 갱신 강제
        ("expires_at".into(), SecretValue::Text("0".into())),
    ].into();

    adapter.initialize(
        PluginConfig { fields: config_fields },
        PluginSecrets { fields: secret_fields },
    ).await.unwrap();

    // 실행: refresh 성공 후 set_secret이 실패해야 에러 반환
    // 현재 구현(let _ = set_secret)은 에러를 무시하고 Ok()를 반환 → assert 실패(Red)
    let result = adapter.fetch_all(FetchAllOpts {
        cursor: None,
        page_size: 10,
    }).await;

    assert!(result.is_err(), "set_secret 실패가 에러로 전파되어야 함. 현재는 무시되어 Ok를 반환하므로 이 테스트가 Red로 실패해야 함.");
}
