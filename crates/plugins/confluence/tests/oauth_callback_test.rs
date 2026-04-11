use doxus_plugin_confluence::oauth_server::OAuthCallbackServer;
use std::time::Duration;

#[tokio::test]
async fn test_oauth_server_binds_random_port() {
    let server = OAuthCallbackServer::start().await.unwrap();
    let addr = server.local_addr().unwrap();
    assert!(addr.port() > 0);
}

#[tokio::test]
async fn test_oauth_server_receives_callback() {
    let server = OAuthCallbackServer::start().await.unwrap();
    let port = server.local_addr().unwrap().port();

    let expected_state = "test_state";

    // 백그라운드에서 HTTP 요청 전송
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?code=test_code&state=test_state",
            port
        );
        reqwest::get(&url).await.ok();
    });

    let (code, state) = server
        .wait_for_callback(Duration::from_secs(5), expected_state)
        .await
        .unwrap();
    assert_eq!(code, "test_code");
    assert_eq!(state, "test_state");
}

#[tokio::test]
async fn test_oauth_server_rejects_missing_code() {
    let server = OAuthCallbackServer::start().await.unwrap();
    let port = server.local_addr().unwrap().port();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?state=only_state", port);
        reqwest::get(&url).await.ok();
    });

    let result = server
        .wait_for_callback(Duration::from_secs(5), "only_state")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_oauth_server_timeout() {
    let server = OAuthCallbackServer::start().await.unwrap();
    let result = server
        .wait_for_callback(Duration::from_millis(100), "some_state")
        .await;
    assert!(result.is_err());
}
