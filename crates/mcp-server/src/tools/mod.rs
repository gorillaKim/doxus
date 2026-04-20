pub mod core;
pub mod graph;
pub mod plugin;
pub mod project;
pub mod search;

use rusqlite::params;
use serde_json::Value;

/// Resolves a document ID (numeric or string) to a (db_id, source_doc_id) pair.
pub fn resolve_doc_id(
    conn: &rusqlite::Connection,
    project: &str,
    id_val: &Value,
) -> Result<(i64, String), String> {
    if let Some(s) = id_val.as_str() {
        // If it's a numeric string, try parsing it as a primary lookup
        if let Ok(n) = s.parse::<i64>() {
            if let Ok(res) = conn.query_row(
                "SELECT d.id, d.source_doc_id FROM documents d JOIN projects p ON d.project_id = p.id WHERE p.name = ?1 AND d.id = ?2",
                params![project, n],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            ) {
                return Ok(res);
            }
        }

        match conn.query_row(
            "SELECT d.id, d.source_doc_id FROM documents d JOIN projects p ON d.project_id = p.id WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, s],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        ) {
            Ok(res) => Ok(res),
            Err(_) => Err(format!(
                "document '{}' not found in project '{}' (Source ID check)",
                s, project
            )),
        }
    } else if let Some(n) = id_val.as_i64() {
        match conn.query_row(
            "SELECT d.id, d.source_doc_id FROM documents d JOIN projects p ON d.project_id = p.id WHERE p.name = ?1 AND d.id = ?2",
            params![project, n],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        ) {
            Ok(res) => Ok(res),
            Err(_) => Err(format!(
                "document with db_id {} not found in project '{}'",
                n, project
            )),
        }
    } else {
        Err("document ID must be a string (source_doc_id) or a number (db_id)".to_string())
    }
}

