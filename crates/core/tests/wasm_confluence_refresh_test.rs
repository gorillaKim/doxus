use std::sync::Arc;
use std::collections::HashMap;
use std::path::PathBuf;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path, header};
use serde_json::json;

use doxus_core::plugin::wasm_adapter::WasmDocSourceAdapter;
use doxus_core::plugin::manifest::PluginManifest;
use doxus_core::secrets::{SecretStore, MemorySecretStore};
use doxus_plugin_sdk::{DocSource, PluginConfig, PluginSecrets, SecretValue, FetchAllOpts};

/// WASM 플러그인 바이너리 경로를 워크스페이스 루트 기준으로 계산
fn wasm_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/core -> crates/
    p.pop(); // crates/ -> workspace root
    p.push("target/wasm32-unknown-unknown/debug/doxus_plugin_confluence.wasm");
    p
}

/// TDD 핵심 시나리오:
/// 1. 만료된 토큰으로 fetch_all 호출
/// 2. WASM 플러그인이 OAuth 갱신 수행
/// 3. 갱신된 토큰이 호스트의 SecretBackend(==Keychain in prod)에 저장됨
/// 4. 새 토큰으로 실제 Confluence API 호출 성공
#[tokio::test]
async fn test_token_refresh_pushes_to_secret_backend() {
    // ── Setup ──────────────────────────────────────────────────────────────────
    let server = MockServer::start().await;
    let base_url = server.uri();

    // Mock: OAuth token refresh endpoint
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token",
            "expires_in": 3600
        })))
        .expect(1) // 반드시 1번 호출돼야 함
        .mount(&server)
        .await;

    // Mock: Confluence API (새 토큰으로 인증돼야 통과)
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(header("Authorization", "Bearer new-access-token"))
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
        .expect(1)
        .mount(&server)
        .await;

    // ── WASM 플러그인 로드 ─────────────────────────────────────────────────────
    let wasm_bytes = std::fs::read(wasm_path())
        .expect("Confluence WASM 미빌드. 먼저 `cargo build -p doxus-plugin-confluence --target wasm32-unknown-unknown` 실행");

    let manifest = PluginManifest {
        plugin_id: "com.doxus.confluence".into(),
        display_name: "Confluence".into(),
        version: "0.1.0".into(),
        abi_version: 1,
        // 허용 도메인: wiremock(127.0.0.1)만 허용 (프로덕션에선 atlassian.com 추가)
        http_domains: vec!["127.0.0.1".into()],
        kv_namespaces: vec![],
        secrets: vec!["access_token".into(), "refresh_token".into(), "expires_at".into()],
    };

    // 프로덕션에서는 KeyringBackend가 기본값, 테스트에서는 MemorySecretStore 주입
    let test_backend = Arc::new(MemorySecretStore::new());

    let mut adapter = WasmDocSourceAdapter::from_bytes(
        wasm_bytes,
        manifest,
        None,
        Some(test_backend.clone()), // None이면 KeyringBackend(Keychain)가 사용됨
    ).expect("어댑터 생성 실패");

    // ── 초기화: 만료된 토큰 주입 ──────────────────────────────────────────────
    let config_fields: HashMap<String, serde_json::Value> = [
        ("base_url".into(), json!(base_url.clone())),
        ("space_key".into(), json!("TEST")),
        ("client_id".into(), json!("client-123")),
        ("client_secret".into(), json!("secret-456")),
        // 핵심: OAuth 서버를 wiremock으로 리다이렉트
        ("oauth_base_url".into(), json!(base_url)),
    ].into();

    let secret_fields: HashMap<String, SecretValue> = [
        ("access_token".into(), SecretValue::Text("expired-token".into())),
        ("refresh_token".into(), SecretValue::Text("old-refresh-token".into())),
        // expires_at = 0 으로 설정 → 항상 갱신 조건 충족
        ("expires_at".into(), SecretValue::Text("0".into())),
    ].into();

    adapter.initialize(
        PluginConfig { fields: config_fields },
        PluginSecrets { fields: secret_fields },
    ).await.expect("초기화 실패");

    // ── 실행: fetch_all → 내부적으로 토큰 갱신 발생 ──────────────────────────
    let stream = adapter.fetch_all(FetchAllOpts {
        cursor: None,
        page_size: 10,
    }).await.expect("fetch_all 실패");

    // 결과 검증
    assert!(!stream.documents.is_empty(), "문서가 최소 1개 이상이어야 함");
    assert_eq!(stream.documents[0].title, Some("Test Page".into()));

    // ── 핵심 검증: 시크릿 백엔드에 새 토큰이 저장됐는지 확인 ──────────────────
    let service = "com.doxus.confluence"; // wasm_adapter uses manifest.plugin_id
    assert_eq!(
        test_backend.get(service, "access_token").unwrap(),
        "new-access-token",
        "access_token이 SecretBackend(Keychain)에 저장돼야 함"
    );
    assert_eq!(
        test_backend.get(service, "refresh_token").unwrap(),
        "new-refresh-token",
        "refresh_token이 SecretBackend(Keychain)에 저장돼야 함"
    );

    // Wiremock: 모든 mock이 기대한 횟수만큼 호출됐는지 자동 검증 (MockServer drop 시)
}
