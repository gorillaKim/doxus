use axum::{
    extract::{Host, Query, State},
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use serde_json::json;
use std::sync::Arc;

use crate::{McpRequest, McpServer};

#[derive(Clone)]
struct HttpState {
    server: Arc<McpServer>,
    token: String,
}

/// Build the axum Router for the HTTP MCP server.
pub fn build_router(server: Arc<McpServer>, token: String) -> Router {
    let state = HttpState {
        server,
        token: token.clone(),
    };

    // OAuth discovery endpoints satisfy the RFC 9728 / RFC 8414 / RFC 7591
    // handshake that MCP SDK 1.x performs before every connection.
    // They are intentionally unauthenticated (they ARE the auth mechanism).
    // host_allowlist_middleware below mitigates DNS-rebinding attacks.
    let oauth_router = Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth_protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/oauth/register", post(oauth_register))
        .route("/oauth/authorize", get(oauth_authorize))
        .route("/oauth/token", post(oauth_token));

    Router::new()
        .route("/mcp", post(mcp_handler))
        .route_layer(middleware::from_fn_with_state(token, auth_middleware))
        .route("/health", get(health_handler))
        .merge(oauth_router)
        .with_state(state)
        // Wraps all routes: blocks requests with non-loopback Host headers.
        .layer(middleware::from_fn(host_allowlist_middleware))
}

/// Start an HTTP MCP server on the given port.
/// Returns the actual bound port (useful when port=0 for OS-assigned).
pub async fn run_http_server(
    server: Arc<McpServer>,
    port: u16,
    token: String,
) -> anyhow::Result<u16> {
    let app = build_router(server, token);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let actual_port = listener.local_addr()?.port();
    axum::serve(listener, app).await?;
    Ok(actual_port)
}

async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "service": "doxus-mcp"})),
    )
}

async fn mcp_handler(
    State(state): State<HttpState>,
    Json(req): Json<McpRequest>,
) -> Response {
    // Notifications have no id — acknowledge with 204 No Content
    let Some(id) = req.id.clone() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let response = state
        .server
        .dispatch(&req.method, id, req.params.as_ref())
        .await;
    (StatusCode::OK, Json(response)).into_response()
}

// Blocks DNS-rebinding attacks by rejecting requests whose Host header is not
// a loopback address. Absent Host header (e.g. internal/test calls) is allowed.
async fn host_allowlist_middleware(
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");

    let is_local = host.starts_with("127.0.0.1")
        || host.starts_with("localhost")
        || host.starts_with("[::1]");

    if is_local {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

// MCP SDK 1.x sends OAuth discovery preflight requests before connecting.
// These endpoints satisfy the RFC 9728 / RFC 8414 discovery handshake so the
// SDK stops trying OAuth and falls through to use the Bearer token from headers.
async fn oauth_protected_resource(host: Option<Host>) -> impl IntoResponse {
    let host = host.map(|h| h.0).unwrap_or_else(|| "127.0.0.1".to_string());
    (
        StatusCode::OK,
        Json(json!({
            "resource": format!("http://{}", host),
            "bearer_methods_supported": ["header"]
            // no authorization_servers → SDK uses this origin as auth server
        })),
    )
}

async fn oauth_authorization_server(host: Option<Host>) -> impl IntoResponse {
    let host = host.map(|h| h.0).unwrap_or_else(|| "127.0.0.1".to_string());
    (
        StatusCode::OK,
        Json(json!({
            "issuer": format!("http://{}", host),
            "authorization_endpoint": format!("http://{}/oauth/authorize", host),
            "token_endpoint": format!("http://{}/oauth/token", host),
            "registration_endpoint": format!("http://{}/oauth/register", host),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "client_credentials"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"]
            // http:// is correct: this server is loopback-only by design
        })),
    )
}

// Immediately redirect back with a static code — no user interaction needed.
// This lets MCP SDK 1.x complete the authorization_code + PKCE flow automatically.
async fn oauth_authorize(Query(params): Query<HashMap<String, String>>) -> Response {
    let redirect_uri = params.get("redirect_uri").cloned().unwrap_or_default();
    let state = params.get("state").cloned().unwrap_or_default();
    let location = if state.is_empty() {
        format!("{}?code=doxus-auth-code", redirect_uri)
    } else {
        format!("{}?code=doxus-auth-code&state={}", redirect_uri, state)
    };
    Redirect::temporary(&location).into_response()
}

async fn oauth_register() -> impl IntoResponse {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": "doxus-mcp-client",
            "client_id_issued_at": now,
            "redirect_uris": [],
            "grant_types": ["client_credentials"],
            "token_endpoint_auth_method": "none"
        })),
    )
}

// /oauth/token is intentionally unauthenticated — it IS the token issuance endpoint.
// Validates grant_type=client_credentials (RFC 6749 §4.4) before returning the
// static bearer token. Exposure is limited by: loopback-only bind + host_allowlist_middleware.
async fn oauth_token(
    State(state): State<HttpState>,
    body: axum::body::Bytes,
) -> Response {
    let body_str = String::from_utf8_lossy(&body);
    let has_valid_grant = body_str.split('&').any(|pair| {
        let p = pair.trim();
        p == "grant_type=client_credentials" || p == "grant_type=authorization_code"
    });

    if !has_valid_grant {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unsupported_grant_type"})),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "access_token": state.token.as_str(),
            "token_type": "Bearer",
            "expires_in": 31536000
        })),
    )
        .into_response()
}

async fn auth_middleware(
    State(token): State<String>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth {
        Some(v) if v == format!("Bearer {token}") => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request as HttpRequest, StatusCode};
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    fn make_app(token: &str) -> (Router, Arc<McpServer>) {
        doxus_core::db::ensure_vec_extension();
        let conn = Connection::open_in_memory().expect("in-memory db");
        doxus_core::db::apply_pragmas(&conn).expect("pragmas");
        doxus_core::db::create_vec0_table(&conn).expect("vec0 table");
        doxus_core::db::migrate(&conn).expect("migrate");
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(
            std::path::PathBuf::from("/tmp/doxus-pm"),
        ));
        let server = Arc::new(McpServer::new(
            Arc::new(Mutex::new(conn)),
            std::path::PathBuf::from(":memory:"),
            None,
            pm,
            std::path::PathBuf::from("/tmp/doxus-test-plugins"),
        ));

        let app = build_router(Arc::clone(&server), token.to_string());
        (app, server)
    }

    #[tokio::test]
    async fn health_returns_200() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_without_auth_returns_401() {
        let (app, _) = make_app("secret");
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": null
        }))
        .unwrap();
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mcp_with_wrong_token_returns_401() {
        let (app, _) = make_app("secret");
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": null
        }))
        .unwrap();
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header("content-type", "application/json")
            .header("authorization", "Bearer wrongtoken")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mcp_initialize_returns_capabilities() {
        let (app, _) = make_app("secret");
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": null
        }))
        .unwrap();
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header("content-type", "application/json")
            .header("authorization", "Bearer secret")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(val.get("result").is_some(), "expected result field");
        assert!(val["result"].get("capabilities").is_some());
    }

    #[tokio::test]
    async fn mcp_tools_list_returns_tools() {
        let (app, _) = make_app("mytoken");
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": null
        }))
        .unwrap();
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header("content-type", "application/json")
            .header("authorization", "Bearer mytoken")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let tools = &val["result"]["tools"];
        assert!(tools.as_array().unwrap().len() >= 30);
    }

    // ── OAuth discovery endpoint tests ────────────────────────────────────────

    #[tokio::test]
    async fn oauth_protected_resource_returns_200_without_auth() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::GET)
            .uri("/.well-known/oauth-protected-resource")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn oauth_authorization_server_has_registration_endpoint() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::GET)
            .uri("/.well-known/oauth-authorization-server")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(val.get("registration_endpoint").is_some());
        assert!(val.get("token_endpoint").is_some());
        // MCP SDK 1.x requires these fields even for client_credentials-only servers
        assert!(val.get("authorization_endpoint").is_some());
        assert!(val["response_types_supported"].is_array());
    }

    #[tokio::test]
    async fn oauth_token_returns_token_with_valid_grant_type() {
        let (app, _) = make_app("mytoken");
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("grant_type=client_credentials"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["access_token"], "mytoken");
        assert_eq!(val["token_type"], "Bearer");
        assert_eq!(val["expires_in"], 31536000);
    }

    #[tokio::test]
    async fn oauth_token_rejects_missing_grant_type() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["error"], "unsupported_grant_type");
    }

    #[tokio::test]
    async fn oauth_register_returns_201_with_client_id() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/oauth/register")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"grant_types":["client_credentials"]}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(val.get("client_id").is_some());
        assert!(val["client_id_issued_at"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn host_allowlist_blocks_non_local_host() {
        let (app, _) = make_app("secret");
        let req = HttpRequest::builder()
            .method(Method::GET)
            .uri("/health")
            .header("host", "evil.attacker.com")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
