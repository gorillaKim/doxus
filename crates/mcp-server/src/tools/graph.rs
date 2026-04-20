
use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

/// Resolves a document ID (numeric or string) to a (db_id, source_doc_id) pair.
fn resolve_doc_id(conn: &rusqlite::Connection, project: &str, id_val: &Value) -> Result<(i64, String), String> {
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
            Err(_) => Err(format!("document '{}' not found in project '{}'", s, project))
        }
    } else if let Some(n) = id_val.as_i64() {
        match conn.query_row(
            "SELECT d.id, d.source_doc_id FROM documents d JOIN projects p ON d.project_id = p.id WHERE p.name = ?1 AND d.id = ?2",
            params![project, n],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        ) {
            Ok(res) => Ok(res),
            Err(_) => Err(format!("document with db_id {} not found in project '{}'", n, project))
        }
    } else {
        Err("document ID must be a string (source_doc_id) or a number (db_id)".to_string())
    }
}

pub fn get_backlinks(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    links(server, id, args, false)
}

pub fn get_links(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    links(server, id, args, true)
}

/// `outgoing=true` → forward links; `outgoing=false` → backlinks
fn links(server: &McpServer, id: Value, args: &Value, outgoing: bool) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (db_id, _) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [],
            |r| r.get::<_, i64>(0),
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
        .query_map(params![db_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let items: Vec<Value> =
                rows.iter().map(|(d, t)| json!({ "id": d, "title": t })).collect();
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

pub async fn find_related(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::{SearchEngine, SearchMode, SearchQuery};

    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let k = args["k"].as_u64().unwrap_or(10) as usize;

    let (db_id, source_doc_id) = {
        let conn = server.conn();
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        match resolve_doc_id(&conn_lock, project, &args["id"]) {
            Ok(res) => res,
            Err(e) => return McpResponse::err(id, -32602, e),
        }
    };

    // 1. Fetch document content to use as query
    let pm = server.plugin_manager();
    let conn = server.conn();
    let content = {
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        let service = doxus_core::document::DocumentService::new(&conn_lock, Some(pm));
        // fetch_full_content is async and doesn't hold the lock across awaits if designed correctly,
        // but it's safer to drop here if needed.
        service.fetch_full_content(project, &source_doc_id).await
    };
    
    let content = match content {
        Ok(c) => c,
        Err(e) => return McpResponse::err(id, -32602, format!("failed to fetch content for similarity search: {e}")),
    };

    // 2. Use first 1000 chars as query for similarity
    let query_text: String = content.chars().take(1000).collect();
    
    // Get project ID
    let project_id = {
       let conn = server.conn();
       let cl = conn.lock().unwrap();
       cl.query_row("SELECT project_id FROM documents WHERE id = ?1", params![db_id], |r| r.get::<_, i64>(0)).unwrap_or(0)
    };

    let mut q = SearchQuery::new(query_text)
        .with_limit(k + 1) // +1 to account for self-exclusion
        .with_projects(vec![project_id]);
        
    q.mode = if server.embedder().is_some() { SearchMode::Hybrid } else { SearchMode::Fts };

    let hits: Result<Vec<doxus_core::search::Hit>, _> = if let Some(embedder) = server.embedder() {
        let engine = SearchEngine::with_embedder(server.conn(), embedder);
        engine.search_async(&q).await
    } else {
        let conn = server.conn();
        let cl = conn.lock().unwrap();
        let engine = SearchEngine::new(&*cl);
        engine.search(&q).map(|shs| shs.into_iter().map(doxus_core::search::Hit::from).collect())
    };

    match hits {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(hits) => {
            let items: Vec<Value> = hits
                .into_iter()
                .filter(|h| h.source_doc_id != source_doc_id) // Exclude self
                .take(k)
                .map(|h| json!({
                    "id": h.source_doc_id,
                    "db_id": h.document_id, // SearchHit.document_id is our db_id
                    "title": h.title,
                    "score": h.score,
                }))
                .collect();

            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            }))
        }
    }
}

pub fn find_path(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = args["project"].as_str().unwrap_or("Brain"); // Default to Brain or from args
    let from = &args["from"];
    let to = &args["to"];
    let max_hops = args["max_hops"].as_u64().unwrap_or(6) as usize;

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (from_db_id, _) = match resolve_doc_id(&conn_lock, project, from) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, format!("'from' error: {e}")),
    };
    
    let (to_db_id, _) = match resolve_doc_id(&conn_lock, project, to) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, format!("'to' error: {e}")),
    };

    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [], |r| r.get::<_, i64>(0),
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
        sql, params![from_db_id, to_db_id, max_hops as i64], |r| Ok((r.get(0)?, r.get(1)?))
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
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (start_db_id, _) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };
    let table_exists: bool = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
            [], |r| r.get::<_, i64>(0),
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
        .query_map(params![start_db_id, depth], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?))
        })
        .and_then(|it| it.collect());

    let items: Vec<Value> = rows.unwrap_or_default()
        .iter()
        .map(|(d, t, lvl)| json!({ "id": d, "title": t, "depth": lvl }))
        .collect();

    McpResponse::ok(id, json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use crate::server::McpServer;

    fn setup_test_server() -> McpServer {
        let conn = Connection::open_in_memory().unwrap();
        doxus_core::db::apply_pragmas(&conn).unwrap();
        
        // Minimal schema for graph tools
        conn.execute_batch("
            CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT UNIQUE, display_name TEXT, path TEXT, source_project_id TEXT, source_type TEXT, config_json TEXT, status TEXT, created_at INTEGER, updated_at INTEGER);
            CREATE TABLE documents (id INTEGER PRIMARY KEY, project_id INTEGER, source_doc_id TEXT, title TEXT, file_path TEXT, url TEXT, metadata_json TEXT, last_indexed INTEGER, FOREIGN KEY(project_id) REFERENCES projects(id));
            CREATE TABLE chunks (id INTEGER PRIMARY KEY, document_id INTEGER, content TEXT, chunk_index INTEGER, heading_path TEXT, start_byte INTEGER, end_byte INTEGER, FOREIGN KEY(document_id) REFERENCES documents(id));
            CREATE TABLE document_links (source_id INTEGER, target_id INTEGER, target_raw TEXT, link_type TEXT);
            CREATE VIRTUAL TABLE chunks_fts USING fts5(content, tokenize='porter');
            CREATE TABLE content_cache (plugin_id TEXT, doc_id TEXT, content TEXT, cached_at INTEGER, expires_at INTEGER, PRIMARY KEY(plugin_id, doc_id));
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
            "INSERT INTO documents (project_id, source_doc_id, title, last_indexed) 
             VALUES (?1, 'doc1', 'Rust Features', 100)",
            params![pid]
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, last_indexed) 
             VALUES (?1, 'doc2', 'Rust Ownership', 101)",
            params![pid]
        ).unwrap();

        let pm = Arc::new(doxus_core::plugin::PluginManager::new(std::path::PathBuf::from("/tmp/doxus-pm-test")));
        McpServer::new(Arc::new(Mutex::new(conn)), None, pm, std::path::PathBuf::from("/tmp/doxus-plugins-test"))
    }

    #[test]
    fn test_resolve_doc_id_numeric_and_string() {
        let server = setup_test_server();
        let conn = server.conn();
        let cl = conn.lock().unwrap();

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

    #[tokio::test]
    async fn test_find_related_works_with_numeric_id() {
        let server = setup_test_server();
        
        // Populate cache for doc1
        {
            let conn = server.conn();
            let cl = conn.lock().unwrap();
            cl.execute(
                "INSERT INTO content_cache (plugin_id, doc_id, content, cached_at, expires_at) 
                 VALUES ('com.doxus.obsidian', 'doc1', 'Rust is a systems language focused on safety.', 0, 9999999999)",
                []
            ).unwrap();
            
            // Also add a related doc in FTS for search results
            let doc2_id: i64 = cl.query_row("SELECT id FROM documents WHERE source_doc_id='doc2'", [], |r| r.get(0)).unwrap();
            cl.execute("INSERT INTO chunks (id, document_id, content, chunk_index) VALUES (2, ?1, 'Rust safety features', 0)", [doc2_id]).unwrap();
            cl.execute("INSERT INTO chunks_fts (rowid, content) VALUES (2, 'Rust safety features')", []).unwrap();
        }

        let args = json!({
            "project": "Brain",
            "id": 1, // numeric ID
            "k": 5
        });

        let resp = find_related(&server, json!(1), &args).await;
        assert!(resp.error.is_none(), "Expected success, got error: {:?}", resp.error);
    }

    #[test]
    fn test_links_standardization() {
        let server = setup_test_server();
        let args = json!({
            "project": "Brain",
            "id": 1 // numeric
        });
        let resp = get_links(&server, json!(1), &args);
        assert!(resp.error.is_none());
    }
}
