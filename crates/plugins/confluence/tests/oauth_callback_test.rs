use doxus_plugin_confluence::oauth_server::OAuthCallbackServer;
use std::time::Duration;

#[tokio::test]
async fn test_oauth_server_binds_random_port() {
    let res: Result<OAuthCallbackServer, Box<dyn std::error::Error + Send + Sync>> =
        OAuthCallbackServer::start().await;
    let server = res.expect("Failed to start server");
    let addr = server.local_addr().expect("Failed to get addr");
    assert!(addr.port() > 0);
}

#[tokio::test]
async fn test_oauth_server_receives_callback() {
    let res: Result<OAuthCallbackServer, Box<dyn std::error::Error + Send + Sync>> =
        OAuthCallbackServer::start().await;
    let server = res.expect("Failed to start server");
    let port = server.local_addr().expect("Failed to get addr").port();

    let expected_state = "test_state";

    // 백그라운드에서 HTTP 요청 전송
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?code=test_code&state=test_state",
            port
        );
        let _ = reqwest::get(&url).await.ok();
    });

    let callback_res: Result<(String, String), Box<dyn std::error::Error + Send + Sync>> = server
        .wait_for_callback(Duration::from_secs(5), expected_state)
        .await;

    let (code, state) = callback_res.expect("Failed to get callback");
    assert_eq!(code, "test_code");
    assert_eq!(state, "test_state");
}

#[tokio::test]
async fn test_oauth_server_rejects_missing_code() {
    let res: Result<OAuthCallbackServer, Box<dyn std::error::Error + Send + Sync>> =
        OAuthCallbackServer::start().await;
    let server = res.expect("Failed to start server");
    let port = server.local_addr().expect("Failed to get addr").port();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?state=only_state", port);
        let _ = reqwest::get(&url).await.ok();
    });

    let callback_res: Result<(String, String), Box<dyn std::error::Error + Send + Sync>> = server
        .wait_for_callback(Duration::from_secs(5), "only_state")
        .await;

    assert!(callback_res.is_err());
}

#[tokio::test]
async fn test_oauth_server_timeout() {
    let res: Result<OAuthCallbackServer, Box<dyn std::error::Error + Send + Sync>> =
        OAuthCallbackServer::start().await;
    let server = res.expect("Failed to start server");

    let callback_res: Result<(String, String), Box<dyn std::error::Error + Send + Sync>> = server
        .wait_for_callback(Duration::from_millis(100), "some_state")
        .await;

    assert!(callback_res.is_err());
}
