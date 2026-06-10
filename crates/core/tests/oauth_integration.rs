use doxus_core::auth::{OAuthConfig, OAuthFlow, OAuthToken};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_config(token_url: String) -> OAuthConfig {
    OAuthConfig {
        client_id: "test-client".into(),
        client_secret: "test-secret".into(),
        auth_url: "https://unused.example.com/auth".into(),
        token_url,
        redirect_uri: "https://app.example.com/callback".into(),
        scopes: vec!["read".into(), "write".into()],
    }
}

// ── exchange_code success ─────────────────────────────────────────────────────

#[tokio::test]
async fn exchange_code_success() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=auth-code-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-abc",
            "refresh_token": "refresh-xyz",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let token = flow
        .exchange_code("auth-code-abc", "state-1", "state-1")
        .await
        .unwrap();

    assert_eq!(token.access_token, "access-abc");
    assert_eq!(token.refresh_token.as_deref(), Some("refresh-xyz"));
    assert!(token.expires_at.is_some());
    assert!(!OAuthFlow::is_expired(&token));
}

// ── exchange_code — server returns error ─────────────────────────────────────

#[tokio::test]
async fn exchange_code_server_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant",
            "error_description": "The authorization code is invalid or expired"
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let err = flow
        .exchange_code("bad-code", "state-1", "state-1")
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("invalid_grant"), "unexpected error: {msg}");
}

// ── refresh_token success ─────────────────────────────────────────────────────

#[tokio::test]
async fn refresh_token_success() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=old-refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "expires_in": 7200
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let token = flow.refresh_token("old-refresh").await.unwrap();

    assert_eq!(token.access_token, "new-access");
    assert_eq!(token.refresh_token.as_deref(), Some("new-refresh"));
    assert!(!OAuthFlow::is_expired(&token));
}

// ── refresh_token — server error ─────────────────────────────────────────────

#[tokio::test]
async fn refresh_token_server_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "invalid_token",
            "error_description": "Refresh token expired"
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let err = flow.refresh_token("expired-refresh").await.unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("invalid_token"), "unexpected error: {msg}");
}

// ── token without refresh_token ───────────────────────────────────────────────

#[tokio::test]
async fn exchange_code_no_refresh_token() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-only",
            "expires_in": 900
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let token = flow.exchange_code("code", "st", "st").await.unwrap();

    assert_eq!(token.access_token, "access-only");
    assert!(token.refresh_token.is_none());
}

// ── token without expires_in ──────────────────────────────────────────────────

#[tokio::test]
async fn exchange_code_no_expires_in() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-no-exp"
        })))
        .mount(&server)
        .await;

    let flow = OAuthFlow::new(make_config(format!("{}/oauth/token", server.uri())));
    let token = flow.exchange_code("code", "st", "st").await.unwrap();

    assert_eq!(token.access_token, "access-no-exp");
    assert!(token.expires_at.is_none());
    // no expiry info → not considered expired
    assert!(!OAuthFlow::is_expired(&token));
}

// ── is_expired with already-expired timestamp ─────────────────────────────────

#[test]
fn is_expired_with_past_timestamp() {
    let token = OAuthToken {
        access_token: "tok".into(),
        refresh_token: None,
        expires_at: Some(1), // epoch+1s — definitely past
    };
    assert!(OAuthFlow::is_expired(&token));
}
