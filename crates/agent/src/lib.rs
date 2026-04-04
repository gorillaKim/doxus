//! Agent sidecar - Node.js integration for doxus
//!
//! Manages CLI detection and agent lifecycle via JSONL protocol.

use serde::{Deserialize, Serialize};

// ── CLI Detection ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliKind {
    ClaudeCode,
    GeminiCli,
    None,
}

/// Detect which AI CLI is available in the current environment.
///
/// Priority:
/// 1. `CLAUDE_CODE_ENTRYPOINT` env var → ClaudeCode
/// 2. `claude` binary in PATH → ClaudeCode
/// 3. `GEMINI_CLI_PATH` env var → GeminiCli
/// 4. `gemini` binary in PATH → GeminiCli
/// 5. None
pub fn detect_cli() -> CliKind {
    if std::env::var("CLAUDE_CODE_ENTRYPOINT").is_ok() {
        return CliKind::ClaudeCode;
    }
    if which("claude") {
        return CliKind::ClaudeCode;
    }
    if std::env::var("GEMINI_CLI_PATH").is_ok() {
        return CliKind::GeminiCli;
    }
    if which("gemini") {
        return CliKind::GeminiCli;
    }
    CliKind::None
}

fn which(binary: &str) -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|dir| {
            let p = std::path::Path::new(dir).join(binary);
            p.is_file()
        })
}

// ── JSONL Protocol ────────────────────────────────────────────────────────────

/// Messages sent from Rust host → Node.js sidecar (via stdin).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Start { session_id: String, prompt: String },
    Message { content: String },
    Cancel,
    Close,
}

/// Messages sent from Node.js sidecar → Rust host (via stdout).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Init { model: String },
    Thought { content: String },
    Text { content: String },
    Result { content: String },
    Error { message: String },
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // Serialize all env-mutating tests to avoid races between parallel threads.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn detect_cli_returns_claude_code_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let orig = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "1");
        let kind = detect_cli();
        match orig {
            Some(v) => std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v),
            None => std::env::remove_var("CLAUDE_CODE_ENTRYPOINT"),
        }
        assert_eq!(kind, CliKind::ClaudeCode);
    }

    #[test]
    fn detect_cli_returns_gemini_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::set_var("GEMINI_CLI_PATH", "/usr/local/bin/gemini");
        // Point PATH somewhere with no `claude` binary
        let tmp = tempdir().unwrap();
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", orig);
        std::env::remove_var("GEMINI_CLI_PATH");
        assert_eq!(kind, CliKind::GeminiCli);
    }

    #[test]
    fn detect_cli_returns_none_with_empty_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let orig_claude = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        let orig_gemini = std::env::var("GEMINI_CLI_PATH").ok();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", &orig_path);
        if let Some(v) = orig_claude { std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v); }
        if let Some(v) = orig_gemini { std::env::set_var("GEMINI_CLI_PATH", v); }
        assert_eq!(kind, CliKind::None);
    }

    #[test]
    fn detect_cli_finds_binary_in_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        let tmp = tempdir().unwrap();
        // Create a fake `claude` binary
        let bin = tmp.path().join("claude");
        std::fs::write(&bin, b"").unwrap();
        // Make it executable (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", orig);
        assert_eq!(kind, CliKind::ClaudeCode);
    }

    #[test]
    fn host_message_start_serializes_correctly() {
        let msg = HostMessage::Start {
            session_id: "sess-1".into(),
            prompt: "hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "start");
        assert_eq!(v["session_id"], "sess-1");
        assert_eq!(v["prompt"], "hello");
    }

    #[test]
    fn host_message_cancel_serializes_correctly() {
        let msg = HostMessage::Cancel;
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "cancel");
    }

    #[test]
    fn agent_message_text_deserializes_correctly() {
        let json = r#"{"type":"text","content":"hello world"}"#;
        let msg: AgentMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, AgentMessage::Text { content } if content == "hello world"));
    }

    #[test]
    fn agent_message_cancelled_roundtrip() {
        let msg = AgentMessage::Cancelled;
        let json = serde_json::to_string(&msg).unwrap();
        let back: AgentMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, AgentMessage::Cancelled));
    }

    #[test]
    fn host_message_jsonl_roundtrip() {
        let msg = HostMessage::Message {
            content: "what is Rust?".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: HostMessage = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(back, HostMessage::Message { content } if content == "what is Rust?")
        );
    }
}
