use crate::server::McpServer;
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
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let doc_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
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
         JOIN projects p ON d1.project_id = p.id
         WHERE p.name = ?1 AND d1.source_doc_id = ?2"
    } else {
        "SELECT DISTINCT d2.source_doc_id, d2.title
         FROM document_links dl
         JOIN documents d1 ON dl.target_id = d1.id
         JOIN documents d2 ON dl.source_id = d2.id
         JOIN projects p ON d1.project_id = p.id
         WHERE p.name = ?1 AND d1.source_doc_id = ?2"
    };

    let mut stmt = match conn_lock.prepare(sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![project, doc_id], |r| {
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

pub fn find_related(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let doc_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };
    let k = args["k"].as_u64().unwrap_or(10) as i64;

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let content: Result<String, _> = conn_lock.query_row(
        "SELECT d.content FROM documents d JOIN projects p ON d.project_id = p.id
         WHERE p.name = ?1 AND d.source_doc_id = ?2",
        params![project, doc_id],
        |r| r.get(0),
    );

    let content = match content {
        Ok(c) => c,
        Err(_) => return McpResponse::err(id, -32602, format!("document '{doc_id}' not found in project '{project}'")),
    };

    let query_text: String = content.chars().take(200).collect();
    let fts_query = query_text.split_whitespace()
        .filter(|w| w.len() > 3)
        .take(10)
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");

    if fts_query.is_empty() {
        return McpResponse::text(id, "[]");
    }

    let mut stmt = match conn_lock.prepare(
        "SELECT d.source_doc_id, d.title
         FROM documents_fts fts
         JOIN documents d ON fts.rowid = d.id
         JOIN projects p ON d.project_id = p.id
         WHERE p.name = ?1 AND d.source_doc_id != ?2
           AND documents_fts MATCH ?3
         ORDER BY rank
         LIMIT ?4",
    ) {
        Ok(s) => s,
        Err(_) => {
            return McpResponse::text(id, "[]");
        }
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![project, doc_id, fts_query, k], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .and_then(|it| it.collect());

    let items: Vec<Value> = rows.unwrap_or_default()
        .iter()
        .map(|(d, t)| json!({ "id": d, "title": t }))
        .collect();

    McpResponse::ok(id, json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
    }))
}

pub fn find_path(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let from = match args["from"].as_str() {
        Some(f) => f,
        None => return McpResponse::err(id, -32602, "missing required arg: from"),
    };
    let to = match args["to"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: to"),
    };
    let max_hops = args["max_hops"].as_u64().unwrap_or(6) as usize;

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
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
           FROM documents d WHERE d.source_doc_id = ?1
           UNION ALL
           SELECT dl.target_id, path.trail || '->' || d2.source_doc_id, path.depth + 1
           FROM path
           JOIN document_links dl ON dl.source_id = path.doc_id
           JOIN documents d2 ON d2.id = dl.target_id
           WHERE path.depth < ?3 AND '->' || path.trail || '->' NOT LIKE '%->' || d2.source_doc_id || '->%'
         )
         SELECT trail, depth FROM path WHERE doc_id = (SELECT id FROM documents WHERE source_doc_id = ?2 LIMIT 1)
         ORDER BY depth LIMIT 1";

    let result: Result<(String, i64), _> = conn_lock.query_row(
        sql, params![from, to, max_hops as i64], |r| Ok((r.get(0)?, r.get(1)?))
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
    let doc_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };
    let depth = args["depth"].as_u64().unwrap_or(2).min(5) as i64;

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
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

    let sql = format!(
        "WITH RECURSIVE cluster(doc_id, source_doc_id, title, lvl) AS (
           SELECT d.id, d.source_doc_id, d.title, 0
           FROM documents d JOIN projects p ON d.project_id = p.id
           WHERE p.name = ?1 AND d.source_doc_id = ?2
           UNION
           SELECT d2.id, d2.source_doc_id, d2.title, cluster.lvl + 1
           FROM cluster
           JOIN document_links dl ON dl.source_id = cluster.doc_id
           JOIN documents d2 ON d2.id = dl.target_id
           WHERE cluster.lvl < ?3
         )
         SELECT DISTINCT source_doc_id, title, lvl FROM cluster ORDER BY lvl, source_doc_id"
    );

    let mut stmt = match conn_lock.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![project, doc_id, depth], |r| {
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
