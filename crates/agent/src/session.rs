//! Agent session runner: connects SidecarManager + ToolBridge.
//!
//! `SessionRunner` reads raw JSONL lines from the sidecar. If a line is a
//! `tool_use` message the bridge handles it and sends a `tool_result` back;
//! all other messages are returned to the caller as parsed `AgentMessage`s.

use crate::{
    protocol::AgentMessage,
    sidecar::{AgentError, SidecarManager},
    tool_bridge::ToolBridge,
};

/// Connects a live [`SidecarManager`] to a [`ToolBridge`].
///
/// On each call to [`SessionRunner::process_one`] the runner reads one raw
/// JSONL line from the sidecar. `tool_use` lines are routed through the bridge
/// and the resulting `tool_result` line is written back; all other lines are
/// deserialized and returned to the caller.
pub struct SessionRunner {
    pub sidecar: SidecarManager,
    pub bridge: ToolBridge,
}

impl SessionRunner {
    /// Create a new runner from an already-spawned sidecar and a configured bridge.
    pub fn new(sidecar: SidecarManager, bridge: ToolBridge) -> Self {
        Self { sidecar, bridge }
    }

    /// Read and process one message from the sidecar.
    ///
    /// * If the line is a `tool_use` message it is dispatched through the
    ///   bridge, the result is written back to the sidecar, and `Ok(None)` is
    ///   returned (the message was handled internally).
    /// * For all other lines the raw text is parsed into an [`AgentMessage`]
    ///   and returned as `Ok(Some(msg))`.
    pub async fn process_one(&mut self) -> Result<Option<AgentMessage>, AgentError> {
        let raw_line = self.sidecar.recv_raw().await?;

        // Try to route through ToolBridge first.
        if let Ok(result_line) = self.bridge.handle_line(&raw_line) {
            let with_newline = result_line + "\n";
            self.sidecar.send_raw(&with_newline).await?;
            return Ok(None);
        }

        // Not a tool_use — parse as AgentMessage for the caller.
        let msg: AgentMessage = serde_json::from_str(raw_line.trim())
            .map_err(|e| AgentError::Protocol(format!("deserialize: {e}")))?;
        Ok(Some(msg))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::tool_bridge::ToolBridge;
    use std::sync::Arc;

    fn echo_bridge() -> ToolBridge {
        ToolBridge::with_default_tools(Arc::new(
            |name, input| serde_json::json!({"tool": name, "input": input}),
        ))
    }

    /// ToolBridge routes an allowed tool_use and returns a tool_result.
    #[test]
    fn tool_bridge_routes_tool_use_correctly() {
        let bridge = echo_bridge();
        let line =
            r#"{"type":"tool_use","id":"tu1","name":"doxus_search","input":{"query":"hello"}}"#;
        let result = bridge.handle_line(line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["name"], "doxus_search");
    }

    /// ToolBridge rejects a disallowed tool and returns a tool_error.
    #[test]
    fn tool_bridge_rejects_disallowed_tool() {
        let bridge = echo_bridge();
        let line = r#"{"type":"tool_use","id":"tu2","name":"doxus_add_project","input":{}}"#;
        let result = bridge.handle_line(line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["type"], "tool_error");
    }

    /// Non-tool_use lines produce BridgeError::NotToolUse.
    #[test]
    fn tool_bridge_rejects_non_tool_use_lines() {
        let bridge = echo_bridge();
        let line = r#"{"type":"text","content":"hello"}"#;
        assert!(bridge.handle_line(line).is_err());
    }
}
