pub mod core;
pub mod graph;
pub mod plugin;
pub mod project;
pub mod reindex;
pub mod search;
pub mod freshness;

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
            Err(_) => {
                // [JIT Optimization] If not found in DB, return as a "virtual" document (db_id = 0)
                // This allows DocumentService to try fetching it from the source plugin.
                Ok((0, s.to_string()))
            },
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

/// `project`가 None이면 db_id (정수)로만 lookup — project 없이도 조회 가능.
/// 문자열 id는 project None 시 명시적 에러 반환 (ambiguous).
/// 성공 시 (db_id, source_doc_id, project_name) 반환.
pub fn resolve_doc_id_optional_project(
    conn: &rusqlite::Connection,
    project: Option<&str>,
    id_val: &Value,
) -> Result<(i64, String, String), String> {
    if let Some(proj) = project {
        let (db_id, source_doc_id) = resolve_doc_id(conn, proj, id_val)?;
        Ok((db_id, source_doc_id, proj.to_string()))
    } else if let Some(n) = id_val.as_i64() {
        match conn.query_row(
            "SELECT d.id, d.source_doc_id, p.name FROM documents d JOIN projects p ON d.project_id = p.id WHERE d.id = ?1",
            params![n],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        ) {
            Ok(res) => Ok(res),
            Err(_) => Err(format!(
                "document with db_id {} not found. Tip: use numeric id from doxus_search to omit 'project'.", n
            )),
        }
    } else if let Some(s) = id_val.as_str() {
        if let Ok(n) = s.parse::<i64>() {
            match conn.query_row(
                "SELECT d.id, d.source_doc_id, p.name FROM documents d JOIN projects p ON d.project_id = p.id WHERE d.id = ?1",
                params![n],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            ) {
                Ok(res) => Ok(res),
                Err(_) => Err(format!("document with db_id {} not found.", n)),
            }
        } else {
            Err(format!(
                "string id '{}' requires 'project' arg; use numeric db_id from doxus_search to omit project.",
                s
            ))
        }
    } else {
        Err("document ID must be a string (source_doc_id) or a number (db_id)".to_string())
    }
}

