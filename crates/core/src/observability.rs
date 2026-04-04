/// Initialize tracing subscriber (call once at app startup)
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

/// Audit event types
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEvent {
    IndexStart { project_id: i64 },
    IndexComplete { project_id: i64, docs_indexed: usize },
    PluginError { plugin_id: String, message: String },
    SyncStart { source_instance_id: i64 },
    SyncComplete { source_instance_id: i64, docs_synced: usize },
}

/// Log an audit event with tracing
pub fn log_audit(event: &AuditEvent) {
    let json = serde_json::to_string(event).unwrap_or_default();
    tracing::info!(audit = true, event = %json, "audit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_serializes_correctly() {
        let event = AuditEvent::IndexStart { project_id: 1 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("index_start"));
        assert!(json.contains("project_id"));
    }

    #[test]
    fn audit_event_plugin_error_has_message() {
        let event = AuditEvent::PluginError {
            plugin_id: "com.doxus.confluence".to_string(),
            message: "rate limited".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("plugin_error"));
        assert!(json.contains("rate limited"));
    }
}
