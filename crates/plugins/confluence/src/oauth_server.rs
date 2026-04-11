use doxus_plugin_sdk::PluginError;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct OAuthCallbackServer {
    listener: TcpListener,
}

impl OAuthCallbackServer {
    pub async fn start() -> Result<Self, PluginError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, PluginError> {
        self.listener
            .local_addr()
            .map_err(|e| PluginError::Internal(e.to_string()))
    }

    pub async fn wait_for_callback(
        self,
        timeout: Duration,
        expected_state: &str,
    ) -> Result<(String, String), PluginError> {
        let (code, state) = tokio::time::timeout(timeout, self.accept_one())
            .await
            .map_err(|_| PluginError::Internal("OAuth callback timeout".into()))??;
        if state != expected_state || state.is_empty() {
            return Err(PluginError::Internal(
                "OAuth state mismatch (possible CSRF)".into(),
            ));
        }
        Ok((code, state))
    }

    async fn accept_one(self) -> Result<(String, String), PluginError> {
        let (mut stream, _) = self
            .listener
            .accept()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        // 요청 읽기: \r\n\r\n 또는 4KB 한계까지 루프로 읽음 (partial read 방지)
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
            if buf.len() >= 4096 {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf);

        // 첫 줄에서 경로 추출: "GET /callback?code=...&state=... HTTP/1.1"
        let first_line = request.lines().next().unwrap_or("");
        let path = first_line.split_whitespace().nth(1).unwrap_or("");

        // query string 파싱
        let query = path.split('?').nth(1).unwrap_or("");
        let params: std::collections::HashMap<_, _> = query
            .split('&')
            .filter_map(|kv| {
                let mut parts = kv.splitn(2, '=');
                Some((parts.next()?, parts.next()?))
            })
            .collect();

        let code = params.get("code").copied().unwrap_or("");
        let state = params.get("state").copied().unwrap_or("");

        if code.is_empty() {
            // 400 응답
            let _ = stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(PluginError::Internal(
                "OAuth callback missing code".into(),
            ));
        }

        // 200 응답 (브라우저에서 확인 가능)
        let body =
            b"<html><body>Authentication successful. You may close this window.</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.write_all(body).await;

        Ok((code.to_string(), state.to_string()))
    }
}
