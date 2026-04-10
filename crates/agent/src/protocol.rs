//! JSONL protocol types for Rust host ↔ Node.js sidecar communication.

use serde::{Deserialize, Serialize};

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
    ToolUse { id: String, name: String, input: serde_json::Value },
    Text { content: String },
    Result { content: String },
    Error { message: String },
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(back, HostMessage::Message { content } if content == "what is Rust?"));
    }
}
