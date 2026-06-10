use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

pub struct OAuthCallbackServer {
    listener: TcpListener,
    addr: SocketAddr,
}

impl OAuthCallbackServer {
    pub async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let listener: tokio::net::TcpListener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        Ok(Self { listener, addr })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, std::io::Error> {
        Ok(self.addr)
    }

    pub async fn wait_for_callback(
        &self,
        timeout: Duration,
        expected_state: &str,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let listener = &self.listener;

        let result = tokio::time::timeout(timeout, async {
            // Explicitly type the socket to help compiler inference
            let (mut socket, _addr): (tokio::net::TcpStream, std::net::SocketAddr) = listener.accept().await?;

            let mut buf = [0; 4096];
            // Help n's type inference
            let n: usize = socket.read(&mut buf).await?;
            if n == 0 {
                return Err("Connection closed".into());
            }

            let request = String::from_utf8_lossy(&buf[..n]);

            // Parse GET request line: GET /callback?code=...&state=... HTTP/1.1
            let first_line = request.lines().next().ok_or("Empty request")?;
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() < 2 {
                return Err("Invalid request line".into());
            }

            let url = Url::parse(&format!("http://localhost{}", parts[1]))?;
            let query: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();

            let code = query.get("code").ok_or("Missing code")?.clone();
            let state = query.get("state").ok_or("Missing state")?.clone();

            if state != expected_state {
                let response = "HTTP/1.1 400 Bad Request\r\n\r\nState mismatch";
                socket.write_all(response.as_bytes()).await?;
                return Err("State mismatch".into());
            }

            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body>Auth successful! You can close this window.</body></html>";
            socket.write_all(response.as_bytes()).await?;

            Ok::< (String, String), Box<dyn std::error::Error + Send + Sync>>((code, state))
        }).await??;

        Ok(result)
    }
}
