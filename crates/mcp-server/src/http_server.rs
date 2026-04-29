use axum::{
    extract::State,
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

use crate::{McpRequest, McpServer};

#[derive(Clone)]
struct HttpState {
    server: Arc<McpServer>,
}

/// Build the axum Router for the HTTP MCP server.
pub fn build_router(server: Arc<McpServer>, token: String) -> Router {
    let state = HttpState { server };

    Router::new()
        .route("/mcp", post(mcp_handler))
        .route_layer(middleware::from_fn_with_state(token, auth_middleware))
        .route("/health", get(health_handler))
        .with_state(state)
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

        let token_str = token.to_string();
        let state = HttpState {
            server: Arc::clone(&server),
        };

        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .route_layer(middleware::from_fn_with_state(
                token_str,
                auth_middleware,
            ))
            .route("/health", get(health_handler))
            .with_state(state);

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
}
