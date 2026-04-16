use doxus_core::auth::{OAuthConfig, OAuthToken};
use doxus_plugin_confluence::ConfluencePlugin;
use doxus_plugin_sdk::{DocSource, FetchAllOpts, FetchChangesOpts, PluginError, DocumentStream, ChangeSet};
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use std::sync::Arc;

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

fn empty_page_list() -> serde_json::Value {
    serde_json::json!({
        "results": [],
        "start": 0,
        "limit": 25,
        "size": 0
    })
}

fn empty_cql_result() -> serde_json::Value {
    serde_json::json!({
        "results": [],
        "start": 0,
        "limit": 25,
        "size": 0
    })
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

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page_list()))
        .mount(&server)
        .await;

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

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_cql_result()))
        .mount(&server)
        .await;

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

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page_list()))
        .mount(&server)
        .await;

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

#[tokio::test]
async fn concurrent_fetch_does_not_double_refresh() {
    let server = MockServer::start().await;

    // Token endpoint must be called exactly once despite two concurrent fetches
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_refresh_response("shared-new-access")))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page_list()))
        .mount(&server)
        .await;

    let plugin = Arc::new(make_oauth_plugin(&server, "TEST"));

    let p1: Arc<ConfluencePlugin> = Arc::clone(&plugin);
    let p2: Arc<ConfluencePlugin> = Arc::clone(&plugin);

    let (r1, r2): (Result<DocumentStream, PluginError>, Result<DocumentStream, PluginError>) = tokio::join!(
        async { p1.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await },
        async { p2.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await },
    );

    assert!(r1.is_ok(), "r1: {:?}", r1);
    assert!(r2.is_ok(), "r2: {:?}", r2);
    server.verify().await;
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

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page_list()))
        .mount(&server)
        .await;

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
