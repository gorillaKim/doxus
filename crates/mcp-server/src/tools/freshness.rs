use crate::server::McpServer;
use crate::types::McpResponse;
use doxus_core::freshness::FreshnessService;
use serde_json::{json, Value};

pub fn get_freshness_report(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project_name = args["project_name"].as_str();

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let pid = if let Some(name) = project_name {
        let res: Result<i64, _> = conn_lock.query_row(
            "SELECT id FROM projects WHERE name = ?1",
            rusqlite::params![name],
            |r| r.get(0)
        );
        match res {
            Ok(pid) => Some(pid),
            Err(e) => return McpResponse::err(id, -32602, format!("project not found: {e}")),
        }
    } else {
        None
    };

    let service = FreshnessService::new(conn.clone());
    // Drop lock to avoid deadlock inside FreshnessService if it calls lock() itself
    drop(conn_lock);

    match service.get_project_freshness_report(pid) {
        Ok(report) => McpResponse::ok(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&report).unwrap_or_default()
                }]
            })
        ),
        Err(e) => McpResponse::err(id, -32603, format!("failed to get report: {e}")),
    }
}

pub fn update_freshness_config(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project_name = match args["project_name"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project_name"),
    };
    let source_doc_id = match args["source_doc_id"].as_str() {
        Some(sid) => sid,
        None => return McpResponse::err(id, -32602, "missing required arg: source_doc_id"),
    };
    let tier = match args["tier"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: tier"),
    };

    let conn = server.conn();
    let pid = {
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        match conn_lock.query_row(
            "SELECT id FROM projects WHERE name = ?1",
            rusqlite::params![project_name],
            |r| r.get(0)
        ) {
            Ok(pid) => pid,
            Err(e) => return McpResponse::err(id, -32602, format!("project not found: {e}")),
        }
    };

    let service = FreshnessService::new(conn.clone());
    match service.update_document_freshness_config(pid, source_doc_id, Some(tier)) {
        Ok(true) => McpResponse::text(id, format!("Successfully updated {source_doc_id} to tier {tier}")),
        Ok(false) => McpResponse::err(id, -32602, "Document not found"),
        Err(e) => McpResponse::err(id, -32603, format!("Failed to update config: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_core::db::{apply_pragmas, create_vec0_table, ensure_vec_extension, migrate};
    use std::sync::{Arc, Mutex};

    fn make_test_conn() -> rusqlite::Connection {
        ensure_vec_extension();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        create_vec0_table(&conn).unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn test_execute_get_freshness_report_no_project() {
        let conn = make_test_conn();
        // Since no projects or documents inserted, total_docs should be 0 safely
        let server = crate::server::McpServer::new(
            Arc::new(Mutex::new(conn)),
            std::path::PathBuf::from(":memory:"),
            None,
            Arc::new(doxus_core::plugin::PluginManager::new(std::path::PathBuf::from("/tmp"))),
            std::path::PathBuf::from("/tmp"),
        );

        let resp = get_freshness_report(&server, json!(1), &json!({}));
        let val = serde_json::to_value(resp).unwrap();
        
        // Assert we get a valid text response containing JSON payload
        let text_content = val["result"]["content"][0]["text"].as_str().expect("Expected string text field");
        assert!(text_content.contains("\"total_docs\""));
        assert!(text_content.contains("\"average_score\""));
    }
}
