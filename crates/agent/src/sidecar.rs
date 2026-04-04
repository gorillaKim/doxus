//! Async Node.js sidecar process manager with JSONL protocol.

use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::protocol::AgentMessage;

/// Errors produced by the sidecar manager.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("CLI not found: {0}")]
    CliNotFound(String),
    #[error("spawn failed: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// Messages sent from the Rust host to the Node.js sidecar (stdin).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidecarMessage {
    Start { session_id: String, prompt: String },
    Message { content: String },
    Cancel,
    Close,
}

/// Manages a Node.js sidecar process with async JSONL I/O.
pub struct SidecarManager {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl SidecarManager {
    /// Spawn `node <node_script>` and wire up stdin/stdout.
    pub async fn spawn(node_script: &Path) -> Result<Self, AgentError> {
        let mut child = Command::new("node")
            .arg(node_script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);

        Ok(Self { child: Some(child), stdin, stdout })
    }

    /// Send a message to the sidecar via stdin as a JSONL line.
    pub async fn send(&mut self, msg: &SidecarMessage) -> Result<(), AgentError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AgentError::Protocol("stdin not available".into()))?;
        let mut line = serde_json::to_string(msg)
            .map_err(|e| AgentError::Protocol(e.to_string()))?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AgentError::SpawnFailed)?;
        stdin.flush().await.map_err(AgentError::SpawnFailed)?;
        Ok(())
    }

    /// Receive one JSONL line from the sidecar stdout and deserialize it.
    pub async fn recv(&mut self) -> Result<AgentMessage, AgentError> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or_else(|| AgentError::Protocol("stdout not available".into()))?;
        let mut line = String::new();
        let n = stdout
            .read_line(&mut line)
            .await
            .map_err(AgentError::SpawnFailed)?;
        if n == 0 {
            return Err(AgentError::Protocol("sidecar stdout closed".into()));
        }
        serde_json::from_str(line.trim())
            .map_err(|e| AgentError::Protocol(format!("deserialize failed: {e}: {line}")))
    }

    /// Kill the sidecar process if it is still running.
    pub async fn shutdown(&mut self) {
        // Close stdin first so the Node process can exit gracefully.
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // Best-effort synchronous kill on drop.
        drop(self.stdin.take());
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_message_start_serializes() {
        let msg = SidecarMessage::Start {
            session_id: "s1".into(),
            prompt: "hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "start");
        assert_eq!(v["session_id"], "s1");
    }

    #[test]
    fn sidecar_message_cancel_serializes() {
        let msg = SidecarMessage::Cancel;
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "cancel");
    }

    #[test]
    fn sidecar_message_close_serializes() {
        let msg = SidecarMessage::Close;
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "close");
    }

    #[test]
    fn agent_error_display() {
        let e = AgentError::CliNotFound("claude".into());
        assert!(e.to_string().contains("claude"));
        let e = AgentError::Protocol("bad json".into());
        assert!(e.to_string().contains("protocol error"));
    }
}
