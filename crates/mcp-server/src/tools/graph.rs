use crate::server::McpServer;
use crate::tools::{resolve_doc_id, resolve_doc_id_optional_project};
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

pub fn get_backlinks(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    links(server, id, args, false)
}

pub fn get_links(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    links(server, id, args, true)
}

/// `outgoing=true` → forward links; `outgoing=false` → backlinks
fn links(server: &McpServer, id: Value, args: &Value, outgoing: bool) -> McpResponse {
    let project_opt = args["project"].as_str();
    let conn = server.conn();
    let conn_lock = match conn.read_conn() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let (db_id, _, _) = match resolve_doc_id_optional_project(&conn_lock, project_opt, &args["id"])
    {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [],
            |r: &rusqlite::Row<'_>| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        return McpResponse::text(id, "[]");
    }

    let sql = if outgoing {
        "SELECT DISTINCT d2.source_doc_id, d2.title
         FROM document_links dl
         JOIN documents d1 ON dl.source_id = d1.id
         JOIN documents d2 ON dl.target_id = d2.id
         WHERE d1.id = ?1"
    } else {
        "SELECT DISTINCT d2.source_doc_id, d2.title
         FROM document_links dl
         JOIN documents d1 ON dl.target_id = d1.id
         JOIN documents d2 ON dl.source_id = d2.id
         WHERE d1.id = ?1"
    };

    let mut stmt = match conn_lock.prepare(sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![db_id], |r: &rusqlite::Row<'_>| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let items: Vec<Value> = rows
                .iter()
                .map(|(d, t)| json!({ "id": d, "title": t }))
                .collect();
            McpResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&items).unwrap_or_default()
                    }]
                }),
            )
        }
    }
}

pub fn find_path(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project_opt = args["project"].as_str();
    let from = &args["from"];
    let to = &args["to"];
    let max_hops = args["max_hops"].as_u64().unwrap_or(6) as usize;

    let conn = server.conn();
    let conn_lock = match conn.read_conn() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let (from_db_id, _, from_proj) =
        match resolve_doc_id_optional_project(&conn_lock, project_opt, from) {
            Ok(res) => res,
            Err(e) => return McpResponse::err(id, -32602, format!("'from' error: {e}")),
        };

    let (to_db_id, _, to_proj) = match resolve_doc_id_optional_project(&conn_lock, project_opt, to)
    {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, format!("'to' error: {e}")),
    };

    if project_opt.is_none() && from_proj != to_proj {
        return McpResponse::err(id, -32602, format!(
            "'from' and 'to' are in different projects ('{}' vs '{}'). Specify 'project' to override.",
            from_proj, to_proj
        ));
    }

    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [],
            |r: &rusqlite::Row<'_>| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        return McpResponse::text(id, json!({"path": null, "hops": null, "message": "document_links table not populated; run indexing first"}).to_string());
    }

    let sql = "WITH RECURSIVE path(doc_id, trail, depth) AS (
           SELECT d.id, d.source_doc_id, 0
           FROM documents d WHERE d.id = ?1
           UNION ALL
           SELECT dl.target_id, path.trail || '->' || d2.source_doc_id, path.depth + 1
           FROM path
           JOIN document_links dl ON dl.source_id = path.doc_id
           JOIN documents d2 ON d2.id = dl.target_id
           WHERE path.depth < ?3 AND '->' || path.trail || '->' NOT LIKE '%->' || d2.source_doc_id || '->%'
         )
         SELECT trail, depth FROM path WHERE doc_id = ?2
         ORDER BY depth LIMIT 1";

    let result: Result<(String, i64), _> = conn_lock.query_row(
        sql,
        params![from_db_id, to_db_id, max_hops as i64],
        |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?)),
    );

    match result {
        Ok((trail, depth)) => {
            let path_steps: Vec<&str> = trail.split("->").collect();
            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                    "path": path_steps,
                    "hops": depth,
                })).unwrap_or_default() }]
            }))
        }
        Err(_) => McpResponse::text(id, json!({"path": null, "hops": null, "message": format!("no path found from '{from}' to '{to}' within {max_hops} hops")}).to_string()),
    }
}

pub fn get_cluster(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let depth = args["depth"].as_u64().unwrap_or(2).min(5) as i64;

    let conn = server.conn();
    let conn_lock = match conn.read_conn() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let (start_db_id, _) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };
    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [],
            |r: &rusqlite::Row<'_>| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        return McpResponse::text(id, "[]");
    }

    let sql = "WITH RECURSIVE cluster(doc_id, source_doc_id, title, lvl) AS (
           SELECT d.id, d.source_doc_id, d.title, 0
           FROM documents d
           WHERE d.id = ?1
           UNION
           SELECT d2.id, d2.source_doc_id, d2.title, cluster.lvl + 1
           FROM cluster
           JOIN document_links dl ON dl.source_id = cluster.doc_id
           JOIN documents d2 ON d2.id = dl.target_id
           WHERE cluster.lvl < ?2
         )
         SELECT DISTINCT source_doc_id, title, lvl FROM cluster ORDER BY lvl, source_doc_id";

    let mut stmt = match conn_lock.prepare(sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![start_db_id, depth], |r: &rusqlite::Row<'_>| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .and_then(|it| it.collect());

    let items: Vec<Value> = rows
        .unwrap_or_default()
        .iter()
        .map(|(d, t, lvl)| json!({ "id": d, "title": t, "depth": lvl }))
        .collect();

    McpResponse::ok(
        id,
        json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
        }),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServer;
    use serde_json::json;
    use std::sync::Arc;

    struct TestServer {
        _temp_dir: tempfile::TempDir,
        server: McpServer,
    }

    fn setup_test_server() -> TestServer {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = doxus_core::db::create_pool(&db_path).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute_batch("
                CREATE TABLE IF NOT EXISTS document_links (source_id INTEGER, target_id INTEGER, target_raw TEXT, link_type TEXT);
            ").unwrap();

            // Add a test project
            conn.execute(
                "INSERT INTO projects (name, display_name, path, source_project_id, source_type, config_json, status, created_at, updated_at) 
                 VALUES ('Brain', 'Brain', '/tmp/brain', 'brain', 'obsidian', '{}', 'active', 0, 0)",
                []
            ).unwrap();
            let pid: i64 = conn.last_insert_rowid();

            // Add two documents
            conn.execute(
                "INSERT INTO documents (project_id, source_doc_id, title, last_indexed, content_hash) 
                 VALUES (?1, 'doc1', 'Rust Features', 100, 'hash1')",
                params![pid]
            ).unwrap();
            conn.execute(
                "INSERT INTO documents (project_id, source_doc_id, title, last_indexed, content_hash) 
                 VALUES (?1, 'doc2', 'Rust Ownership', 101, 'hash2')",
                params![pid]
            ).unwrap();
        }

        let pm = Arc::new(doxus_core::plugin::PluginManager::new(
            std::path::PathBuf::from("/tmp/doxus-pm-test"),
        ));
        let server = McpServer::new(
            pool,
            db_path,
            None,
            pm,
            std::path::PathBuf::from("/tmp/doxus-plugins-test"),
        );
        TestServer {
            _temp_dir: temp_dir,
            server,
        }
    }

    #[test]
    fn test_resolve_doc_id_numeric_and_string() {
        let ts = setup_test_server();
        let conn = ts.server.conn();
        let cl = conn.get().unwrap();

        // 1. Resolve by source_doc_id (string)
        let (id1, sid1) = resolve_doc_id(&cl, "Brain", &json!("doc1")).unwrap();
        assert_eq!(sid1, "doc1");

        // 2. Resolve by numeric db_id (integer)
        let (id2, sid2) = resolve_doc_id(&cl, "Brain", &json!(id1)).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(sid1, sid2);

        // 3. Resolve by numeric string (id hit from search)
        let (id3, _sid3) = resolve_doc_id(&cl, "Brain", &json!(id1.to_string())).unwrap();
        assert_eq!(id1, id3);
    }

    #[test]
    fn test_links_standardization() {
        let ts = setup_test_server();
        let args = json!({
            "project": "Brain",
            "id": 1 // numeric
        });
        let resp = get_links(&ts.server, json!(1), &args);
        assert!(resp.error.is_none());
    }
}
