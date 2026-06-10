use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use doxus_core::secrets::{SecretStore, UnifiedKeychainStore};
use std::sync::Arc;

/// 브릿지 서버에 공유할 상태값입니다.
#[derive(Clone)]
struct BridgeState {
    secret_store: Arc<UnifiedKeychainStore>,
}

/// 인증 브릿지 서버를 실행합니다.
pub async fn run_bridge_server(secret_store: Arc<UnifiedKeychainStore>, port: u16, token: String) {
    let state = BridgeState { secret_store };

    let app = Router::new()
        .route("/secrets/:plugin_id/:key", get(get_secret_handler))
        .route_layer(middleware::from_fn(move |req, next| {
            auth_middleware(req, next, token.clone())
        }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to bind bridge server to 127.0.0.1");

    eprintln!("[bridge] server running on http://127.0.0.1:{}", port);
    axum::serve(listener, app)
        .await
        .expect("Bridge server error");
}

/// Bearer 토큰을 검증하는 미들웨어입니다.
async fn auth_middleware(
    req: Request<axum::body::Body>,
    next: Next,
    token: String,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h: &axum::http::HeaderValue| h.to_str().ok());

    if let Some(auth_header) = auth_header {
        if auth_header == format!("Bearer {}", token) {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// 특정 플러그인의 시크릿 정보를 반환하는 핸들러입니다.
async fn get_secret_handler(
    Path((plugin_id, key)): Path<(String, String)>,
    State(state): State<BridgeState>,
) -> impl IntoResponse {
    match state.secret_store.get(&plugin_id, &key) {
        Ok(value) => (StatusCode::OK, Json(serde_json::json!({ "value": value }))).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Secret not found" })),
        )
            .into_response(),
    }
}
