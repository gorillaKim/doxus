use std::collections::HashSet;
use std::sync::RwLock;
use once_cell::sync::Lazy;

static ENABLED_TAGS: Lazy<RwLock<HashSet<String>>> = Lazy::new(|| RwLock::new(HashSet::new()));

/// Set the enabled debug tags from configuration.
pub fn set_debug_tags(tags: Vec<String>) {
    if let Ok(mut guard) = ENABLED_TAGS.write() {
        *guard = tags.into_iter().collect();
    }
}

/// Check if a specific debug tag is enabled.
pub fn is_debug_enabled(tag: &str) -> bool {
    ENABLED_TAGS.read().map(|guard| guard.contains(tag)).unwrap_or(false)
}

/// Log a message if the given tag is enabled.
/// This macro is exported so it can be used across crates.
#[macro_export]
macro_rules! log_d {
    ($tag:expr, $($arg:tt)*) => {
        if $crate::observability::is_debug_enabled($tag) {
            println!($($arg)*);
        }
    };
}

/// Initialize tracing subscriber (call once at app startup)
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    // ORT INFO 로그가 stdout을 오염시키는 문제 방지.
    // RUST_LOG 미설정 시 "info,ort=error" 를 기본값으로 사용.
    // RUST_LOG 설정 시 해당 값을 그대로 사용 (사용자 오버라이드 가능).
    let default_filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "info,ort=error".to_string());
    let filter = EnvFilter::new(default_filter);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
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

impl AuditEvent {
    /// event_type 문자열 반환 (audit_log.event_type 컬럼용)
    pub fn event_type_str(&self) -> &'static str {
        match self {
            AuditEvent::IndexStart { .. } => "index_start",
            AuditEvent::IndexComplete { .. } => "index_complete",
            AuditEvent::PluginError { .. } => "plugin_error",
            AuditEvent::SyncStart { .. } => "sync_start",
            AuditEvent::SyncComplete { .. } => "sync_complete",
        }
    }

    /// project_id 반환 (없으면 None)
    pub fn project_id(&self) -> Option<i64> {
        match self {
            AuditEvent::IndexStart { project_id } => Some(*project_id),
            AuditEvent::IndexComplete { project_id, .. } => Some(*project_id),
            AuditEvent::PluginError { .. } => None,
            AuditEvent::SyncStart { .. } => None,
            AuditEvent::SyncComplete { .. } => None,
        }
    }
}

/// Log an audit event with tracing only (hot path용 — no DB I/O)
pub fn log_audit(event: &AuditEvent) {
    let json = serde_json::to_string(event).unwrap_or_default();
    tracing::info!(audit = true, event = %json, "audit");
}

/// Persist an audit event to the audit_log table.
/// tracing 로그도 함께 기록한다.
/// DB 쓰기 실패는 silently ignore (관측성 코드가 메인 플로우를 깨지 않음).
pub fn persist_audit(conn: &rusqlite::Connection, event: &AuditEvent) {
    log_audit(event);
    let payload = serde_json::to_string(event).unwrap_or_default();
    let _ = conn.execute(
        "INSERT INTO audit_log (project_id, event_type, payload, occurred_at) \
         VALUES (?1, ?2, ?3, unixepoch())",
        rusqlite::params![event.project_id(), event.event_type_str(), payload],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        // FK 제약 통과를 위해 테스트용 프로젝트 삽입
        conn.execute(
            "INSERT INTO projects (id, name, display_name, path, created_at, updated_at) \
             VALUES (1, 'test', 'Test', '/tmp', 0, 0)",
            [],
        ).unwrap();
        conn
    }

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

    #[test]
    fn persist_audit_inserts_into_db() {
        let conn = make_conn();
        let event = AuditEvent::IndexStart { project_id: 1 };
        persist_audit(&conn, &event);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_log WHERE event_type = 'index_start'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn persist_audit_stores_project_id() {
        let conn = make_conn();
        let event = AuditEvent::IndexComplete { project_id: 1, docs_indexed: 99 };
        persist_audit(&conn, &event);
        let project_id: Option<i64> = conn
            .query_row("SELECT project_id FROM audit_log WHERE event_type = 'index_complete'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(project_id, Some(1));
    }

    #[test]
    fn persist_audit_stores_payload_with_docs_count() {
        let conn = make_conn();
        let event = AuditEvent::IndexComplete { project_id: 1, docs_indexed: 55 };
        persist_audit(&conn, &event);
        let payload: Option<String> = conn
            .query_row("SELECT payload FROM audit_log WHERE event_type = 'index_complete'", [], |r| r.get(0))
            .unwrap();
        assert!(payload.unwrap().contains("55"));
    }

    #[test]
    fn persist_audit_plugin_error_no_project_id() {
        let conn = make_conn();
        let event = AuditEvent::PluginError {
            plugin_id: "com.doxus.test".to_string(),
            message: "timeout".to_string(),
        };
        persist_audit(&conn, &event);
        let row: (String, Option<i64>) = conn
            .query_row("SELECT event_type, project_id FROM audit_log", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(row.0, "plugin_error");
        assert_eq!(row.1, None);
    }

    #[test]
    fn persist_audit_multiple_events_all_stored() {
        let conn = make_conn();
        persist_audit(&conn, &AuditEvent::SyncStart { source_instance_id: 1 });
        persist_audit(&conn, &AuditEvent::SyncComplete { source_instance_id: 1, docs_synced: 10 });
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }
}
