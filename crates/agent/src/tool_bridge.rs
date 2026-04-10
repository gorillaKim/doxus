//! JSONL tool bridge — routes `tool_use` messages to `doxus_*` MCP tools.
//!
//! The bridge decouples the agent crate from the mcp-server crate by accepting
//! a generic dispatcher: any `Fn(&str, serde_json::Value) -> serde_json::Value`.
//! In production, callers wire in `McpServer::dispatch_tool`; in tests, a mock
//! closure is injected instead.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSONL message types ────────────────────────────────────────────────────────

/// Incoming `tool_use` JSONL message from the Node.js sidecar.
#[derive(Debug, Deserialize, PartialEq)]
pub struct ToolUseMessage {
    pub name: String,
    pub input: Value,
}

/// Outgoing `tool_result` JSONL message sent back to the sidecar.
#[derive(Debug, Serialize, PartialEq)]
pub struct ToolResultMessage {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub name: String,
    pub result: Value,
}

/// Outgoing `tool_error` JSONL message sent back when a tool fails or is blocked.
#[derive(Debug, Serialize, PartialEq)]
pub struct ToolErrorMessage {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub name: String,
    pub error: String,
}

// ── ToolBridge ────────────────────────────────────────────────────────────────

/// Default allowed tool list (mirrors `tools.json` spec).
pub const DEFAULT_ALLOWED_TOOLS: &[&str] = &[
    "doxus_search",
    "doxus_get_document",
    "doxus_get_section",
    "doxus_list_projects",
    "doxus_get_backlinks",
    "doxus_find_related",
];

/// Routes a `tool_use` JSONL message to the appropriate `doxus_*` tool.
///
/// The `dispatcher` is a type-erased function that accepts `(tool_name, args)`
/// and returns a JSON result value. This keeps the agent crate free of a direct
/// dependency on `doxus-mcp`.
pub struct ToolBridge {
    allowed: Vec<String>,
    dispatcher: Arc<dyn Fn(&str, Value) -> Value + Send + Sync>,
}

impl ToolBridge {
    /// Create a new bridge with an explicit allow-list and dispatcher.
    pub fn new(
        allowed: Vec<String>,
        dispatcher: Arc<dyn Fn(&str, Value) -> Value + Send + Sync>,
    ) -> Self {
        Self { allowed, dispatcher }
    }

    /// Create a bridge using the default allowed tool list.
    pub fn with_default_tools(
        dispatcher: Arc<dyn Fn(&str, Value) -> Value + Send + Sync>,
    ) -> Self {
        Self::new(
            DEFAULT_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
            dispatcher,
        )
    }

    /// Process a raw JSONL line containing a `tool_use` message.
    ///
    /// Returns `Ok(line)` where `line` is a serialized `tool_result` or
    /// `tool_error` JSONL string (without trailing newline).
    pub fn handle_line(&self, line: &str) -> Result<String, BridgeError> {
        // Parse the outer envelope to check `type`.
        let envelope: Value =
            serde_json::from_str(line).map_err(|e| BridgeError::Parse(e.to_string()))?;

        let msg_type = envelope["type"].as_str().unwrap_or("");
        if msg_type != "tool_use" {
            return Err(BridgeError::NotToolUse(msg_type.to_string()));
        }

        let name = envelope["name"]
            .as_str()
            .ok_or_else(|| BridgeError::Parse("missing 'name' field".into()))?
            .to_string();
        let input = envelope["input"].clone();

        self.dispatch(&name, input)
    }

    /// Dispatch a tool call by name with the given input arguments.
    ///
    /// Returns a serialized JSONL string (`tool_result` or `tool_error`).
    pub fn dispatch(&self, name: &str, input: Value) -> Result<String, BridgeError> {
        if !self.allowed.iter().any(|a| a == name) {
            let msg = ToolErrorMessage {
                kind: "tool_error",
                name: name.to_string(),
                error: format!("tool '{name}' is not in the allowed list"),
            };
            return Ok(serde_json::to_string(&msg)
                .map_err(|e| BridgeError::Serialize(e.to_string()))?);
        }

        let result = (self.dispatcher)(name, input);

        let msg = ToolResultMessage { kind: "tool_result", name: name.to_string(), result };
        serde_json::to_string(&msg).map_err(|e| BridgeError::Serialize(e.to_string()))
    }
}

// ── BridgeError ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("not a tool_use message (type={0})")]
    NotToolUse(String),
    #[error("serialize error: {0}")]
    Serialize(String),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Returns a bridge backed by a simple echo dispatcher.
    fn echo_bridge() -> ToolBridge {
        ToolBridge::with_default_tools(Arc::new(|name, input| {
            json!({ "echo": name, "input": input })
        }))
    }

    // ── Allowlist tests ────────────────────────────────────────────────────

    #[test]
    fn allowed_tool_returns_tool_result() {
        let bridge = echo_bridge();
        let line =
            r#"{"type":"tool_use","name":"doxus_search","input":{"query":"rust","project":"vault"}}"#;
        let out = bridge.handle_line(line).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["name"], "doxus_search");
        assert!(v["result"].is_object());
    }

    #[test]
    fn blocked_tool_returns_tool_error() {
        let bridge = echo_bridge();
        let line = r#"{"type":"tool_use","name":"doxus_add_project","input":{}}"#;
        let out = bridge.handle_line(line).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["type"], "tool_error");
        assert_eq!(v["name"], "doxus_add_project");
        assert!(v["error"].as_str().unwrap().contains("not in the allowed list"));
    }

    #[test]
    fn unknown_tool_returns_tool_error() {
        let bridge = echo_bridge();
        let line = r#"{"type":"tool_use","name":"rm_rf","input":{}}"#;
        let out = bridge.handle_line(line).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["type"], "tool_error");
        assert_eq!(v["name"], "rm_rf");
    }

    // ── All default allowed tools are accepted ─────────────────────────────

    #[test]
    fn all_default_allowed_tools_pass() {
        let bridge = echo_bridge();
        for tool in DEFAULT_ALLOWED_TOOLS {
            let line = format!(r#"{{"type":"tool_use","name":"{tool}","input":{{}}}}"#);
            let out = bridge.handle_line(&line).unwrap();
            let v: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(v["type"], "tool_result", "expected tool_result for {tool}");
        }
    }

    // ── handle_line error cases ────────────────────────────────────────────

    #[test]
    fn handle_line_returns_err_for_non_tool_use_type() {
        let bridge = echo_bridge();
        let line = r#"{"type":"text","content":"hello"}"#;
        let err = bridge.handle_line(line).unwrap_err();
        assert!(matches!(err, BridgeError::NotToolUse(_)));
    }

    #[test]
    fn handle_line_returns_err_for_invalid_json() {
        let bridge = echo_bridge();
        let err = bridge.handle_line("not json{{{").unwrap_err();
        assert!(matches!(err, BridgeError::Parse(_)));
    }

    #[test]
    fn handle_line_returns_err_when_name_missing() {
        let bridge = echo_bridge();
        let line = r#"{"type":"tool_use","input":{}}"#;
        let err = bridge.handle_line(line).unwrap_err();
        assert!(matches!(err, BridgeError::Parse(_)));
    }

    // ── Dispatcher receives correct arguments ──────────────────────────────

    #[test]
    fn dispatcher_receives_tool_name_and_input() {
        use std::sync::Mutex;

        let captured: Arc<Mutex<Option<(String, Value)>>> = Arc::new(Mutex::new(None));
        let cap2 = Arc::clone(&captured);

        let bridge = ToolBridge::with_default_tools(Arc::new(move |name, input| {
            *cap2.lock().unwrap() = Some((name.to_string(), input.clone()));
            json!({ "ok": true })
        }));

        let line = r#"{"type":"tool_use","name":"doxus_search","input":{"query":"hello"}}"#;
        bridge.handle_line(line).unwrap();

        let guard = captured.lock().unwrap();
        let (name, input) = guard.as_ref().unwrap();
        assert_eq!(name, "doxus_search");
        assert_eq!(input["query"], "hello");
    }

    // ── Custom allow-list ──────────────────────────────────────────────────

    #[test]
    fn custom_allowlist_overrides_default() {
        let bridge = ToolBridge::new(
            vec!["doxus_search".to_string()],
            Arc::new(|_, _| json!({})),
        );
        // doxus_list_projects is in default but NOT in custom list
        let line = r#"{"type":"tool_use","name":"doxus_list_projects","input":{}}"#;
        let out = bridge.handle_line(line).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["type"], "tool_error");
    }

    // ── JSONL output format ────────────────────────────────────────────────

    #[test]
    fn tool_result_serializes_with_correct_type_field() {
        let msg = ToolResultMessage {
            kind: "tool_result",
            name: "doxus_search".to_string(),
            result: json!({"hits": []}),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["name"], "doxus_search");
    }

    #[test]
    fn tool_error_serializes_with_correct_type_field() {
        let msg = ToolErrorMessage {
            kind: "tool_error",
            name: "bad_tool".to_string(),
            error: "not allowed".to_string(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], "tool_error");
        assert_eq!(v["name"], "bad_tool");
        assert_eq!(v["error"], "not allowed");
    }
}
