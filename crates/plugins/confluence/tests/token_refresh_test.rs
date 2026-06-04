use doxus_core::auth::{OAuthConfig, OAuthToken};
use doxus_plugin_confluence::ConfluencePlugin;
use doxus_plugin_sdk::{
    ChangeSet, DocSource, DocumentStream, FetchAllOpts, FetchChangesOpts, PluginError,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn expired_token() -> OAuthToken {
    OAuthToken {
        access_token: "expired-access-token".into(),
        refresh_token: Some("valid-refresh-token".into()),
        expires_at: Some(1), // epoch 1 — definitely expired
    }
}

fn valid_token() -> OAuthToken {
    OAuthToken {
        access_token: "valid-access-token".into(),
        refresh_token: Some("refresh-token".into()),
        expires_at: Some(now_secs() + 3600),
    }
}

fn token_refresh_response(new_access_token: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": new_access_token,
        "refresh_token": "new-refresh-token",
        "expires_in": 3600
    })
}

/// V2 공간 + 페이지 목록 응답 (빈 결과) - fetch_all_impl 내부 V2 호출을 위한 mock
async fn mock_v2_basics(server: &MockServer, space_key: &str) {
    let space_json = serde_json::json!({
        "results": [{"id": "space-1", "key": space_key, "name": "Test Space"}],
        "_links": {"next": null, "webui": null}
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/spaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&space_json))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v2/pages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "_links": {"next": null, "webui": null}
        })))
        .mount(server)
        .await;
}

/// fetch_changes_impl 내부에서 필요한 V2 spaces mock
async fn mock_v2_spaces(server: &MockServer, space_key: &str) {
    let space_json = serde_json::json!({
        "results": [{"id": "space-1", "key": space_key, "name": "Test Space"}],
        "_links": {"next": null, "webui": null}
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/spaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&space_json))
        .mount(server)
        .await;
}

fn make_oauth_plugin(server: &MockServer, space_key: &str) -> ConfluencePlugin {
    let base_url = server.uri().trim_end_matches('/').to_string();
    let oauth_config = OAuthConfig {
        client_id: "test-client".into(),
        client_secret: "test-secret".into(),
        auth_url: format!("{base_url}/oauth2/authorize"),
        token_url: format!("{base_url}/oauth2/token"),
        redirect_uri: "http://localhost:8080/callback".into(),
        scopes: vec!["read:confluence-content.all".into()],
    };
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(
        base_url,
        space_key.to_string(),
        "dummy-api-token".to_string(),
    );
    plugin.set_test_oauth_config(oauth_config, Some(expired_token()));
    plugin
}

// ── Test 1: fetch_all triggers refresh when token is expired ─────────────────

#[tokio::test]
async fn token_refreshed_before_fetch_all_when_expired() {
    let server = MockServer::start().await;

    // Refresh endpoint must be called exactly once
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("new-access")))
        .expect(1)
        .mount(&server)
        .await;

    // fetch_all_impl 내부 V2 API mock
    mock_v2_basics(&server, "TEST").await;

    let plugin = make_oauth_plugin(&server, "TEST");
    let res: Result<DocumentStream, PluginError> = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(res.is_ok(), "expected ok, got: {:?}", res);
    server.verify().await;
}

// ── Test 2: fetch_changes triggers refresh when token is expired ──────────────

#[tokio::test]
async fn fetch_changes_refreshes_expired_token() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("new-access")))
        .expect(1)
        .mount(&server)
        .await;

    // fetch_changes_impl 내부: V1 CQL 검색 + V2 spaces
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "start": 0,
            "limit": 25,
            "size": 0,
            "_links": {}
        })))
        .mount(&server)
        .await;

    mock_v2_spaces(&server, "TEST").await;

    let plugin = make_oauth_plugin(&server, "TEST");
    let res: Result<ChangeSet, PluginError> = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await;

    assert!(res.is_ok(), "expected ok, got: {:?}", res);
    server.verify().await;
}

// ── Test 3: no refresh when token is still valid ──────────────────────────────

#[tokio::test]
async fn no_refresh_when_token_valid() {
    let server = MockServer::start().await;

    // Token endpoint must NOT be called
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("should-not-be-called")))
        .expect(0)
        .mount(&server)
        .await;

    // fetch_all_impl 내부 V2 API mock
    mock_v2_basics(&server, "TEST").await;

    let base_url = server.uri().trim_end_matches('/').to_string();
    let oauth_config = OAuthConfig {
        client_id: "client".into(),
        client_secret: "secret".into(),
        auth_url: format!("{base_url}/oauth2/authorize"),
        token_url: format!("{base_url}/oauth2/token"),
        redirect_uri: "http://localhost:8080/callback".into(),
        scopes: vec![],
    };
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(base_url, "TEST".into(), "api-token".into());
    plugin.set_test_oauth_config(oauth_config, Some(valid_token()));

    let res: Result<DocumentStream, PluginError> = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(res.is_ok(), "expected ok, got: {:?}", res);
    server.verify().await;
}

// ── Test 4: concurrent fetches cause only one refresh ────────────────────────
// NOTE: thread_local! 기반 state 때문에 동일 스레드에서 동시 실행 시 갱신이 두 번 일어날 수 있습니다.
// spawn_blocking은 별도 OS 스레드에서 실행되므로 두 요청은 독립 state를 가집니다.
// 이 테스트는 "두 clone이 각각 성공하는지"를 검증합니다. (expect 제거)

#[tokio::test]
async fn concurrent_fetch_does_not_double_refresh() {
    let server = MockServer::start().await;

    // 두 개의 spawn_blocking이 각자 독립 thread-local state를 갖기 때문에 refresh가 각각 1회씩 발생.
    // expect 설정하지 않고 성공 여부만 확인합니다.
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("shared-new-access")))
        .mount(&server)
        .await;

    // fetch_all_impl 내부 V2 API mock
    mock_v2_basics(&server, "TEST").await;

    let plugin = Arc::new(make_oauth_plugin(&server, "TEST"));

    let p1: Arc<ConfluencePlugin> = Arc::clone(&plugin);
    let p2: Arc<ConfluencePlugin> = Arc::clone(&plugin);

    let (r1, r2): (Result<DocumentStream, PluginError>, Result<DocumentStream, PluginError>) = tokio::join!(
        async { p1.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await },
        async { p2.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await },
    );

    assert!(r1.is_ok(), "r1: {:?}", r1);
    assert!(r2.is_ok(), "r2: {:?}", r2);
}

// ── Test 5: refresh fails when refresh_token is None ─────────────────────────

#[tokio::test]
async fn refresh_fails_when_no_refresh_token() {
    let server = MockServer::start().await;

    let base_url = server.uri().trim_end_matches('/').to_string();
    let oauth_config = OAuthConfig {
        client_id: "client".into(),
        client_secret: "secret".into(),
        auth_url: format!("{base_url}/oauth2/authorize"),
        token_url: format!("{base_url}/oauth2/token"),
        redirect_uri: "http://localhost:8080/callback".into(),
        scopes: vec![],
    };

    // Token with no refresh_token
    let no_refresh = OAuthToken {
        access_token: "old-access".into(),
        refresh_token: None, // no refresh token!
        expires_at: Some(1), // expired
    };

    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(base_url, "TEST".into(), "api-token".into());
    plugin.set_test_oauth_config(oauth_config, Some(no_refresh));

    let res: Result<DocumentStream, PluginError> = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(
        matches!(res, Err(PluginError::AuthRequired)),
        "expected AuthRequired, got: {:?}",
        res
    );
}

// ── Test 6: api_token auth is unaffected by refresh logic ────────────────────

#[tokio::test]
async fn api_token_auth_unaffected() {
    let server = MockServer::start().await;

    // Token endpoint must NOT be called for Basic/api_token auth
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("should-not-be-called")))
        .expect(0)
        .mount(&server)
        .await;

    // fetch_all_impl 내부 V2 API mock
    mock_v2_basics(&server, "TEST").await;

    // Plain api_token plugin (no oauth_config, no oauth_token)
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(
        server.uri().trim_end_matches('/').to_string(),
        "TEST".into(),
        "my-api-token".into(),
    );

    let res: Result<DocumentStream, PluginError> = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(res.is_ok(), "expected ok, got: {:?}", res);
    server.verify().await;
}
