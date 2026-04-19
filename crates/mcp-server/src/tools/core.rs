use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

pub fn status(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let projects: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM projects WHERE source_type != 'workspace'", [], |r| r.get(0))
        .unwrap_or(0);
    let documents: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap_or(0);
    McpResponse::text(
        id,
        format!(
            "doxus MCP server v0.1.0 — operational\nProjects: {projects}  Documents: {documents}"
        ),
    )
}

pub fn diagnose(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let projects: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap_or(0);
    let documents: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap_or(0);
    let chunks: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);

    let diag = json!({
        "projects": projects,
        "documents": documents,
        "chunks": chunks,
    });

    let mut lines = vec!["=== doxus diagnostics ===".to_string()];
    lines.push(format!("Projects : {projects}"));
    lines.push(format!("Documents: {documents}"));
    lines.push(format!("Chunks   : {chunks}"));
    if projects == 0 {
        lines.push("\nNo projects found. Add one with doxus_add_project.".to_string());
    }
    if documents == 0 && projects > 0 {
        lines.push("\nNo documents indexed yet. Run doxus_index_project.".to_string());
    }
    lines.push(format!("\nRaw: {}", serde_json::to_string(&diag).unwrap_or_default()));

    McpResponse::text(id, lines.join("\n"))
}

pub fn system_report(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let projects: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap_or(0);
    let active_projects: i64 = conn_lock
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let documents: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap_or(0);
    let chunks: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let plugins: i64 = conn_lock
        .query_row(
            "SELECT COUNT(DISTINCT plugin_id) FROM source_instances",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let report = json!({
        "server": "doxus-mcp",
        "version": "0.1.0",
        "db": {
            "projects": projects,
            "active_projects": active_projects,
            "documents": documents,
            "chunks": chunks,
            "plugins": plugins,
        }
    });

    McpResponse::ok(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&report).unwrap_or_default()
            }]
        }),
    )
}

pub fn explain_search(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let query = match args["query"].as_str() {
        Some(q) => q,
        None => return McpResponse::err(id, -32602, "missing required arg: query"),
    };
    let document_id = match args["document_id"].as_str() {
        Some(d) => d,
        None => return McpResponse::err(id, -32602, "missing required arg: document_id"),
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let row: Result<(Option<String>, String), _> = conn_lock.query_row(
        "SELECT d.title, d.content FROM documents d WHERE d.source_doc_id = ?1",
        params![document_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    match row {
        Err(_) => McpResponse::err(id, -32602, format!("document '{document_id}' not found")),
        Ok((title, content)) => {
            let query_terms: Vec<&str> = query.split_whitespace().collect();
            let content_lower = content.to_lowercase();
            let matches: Vec<Value> = query_terms.iter().map(|term| {
                let term_lower = term.to_lowercase();
                let count = content_lower.matches(&term_lower).count();
                json!({ "term": term, "occurrences": count })
            }).collect();

            let explanation = json!({
                "query": query,
                "document_id": document_id,
                "title": title,
                "term_matches": matches,
                "explanation": "FTS5 ranked by BM25; higher occurrence = higher relevance",
            });

            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&explanation).unwrap_or_default() }]
            }))
        }
    }
}

pub fn agent_summary(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    // 1. Overall stats
    let total_projects: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap_or(0);
    let total_docs: i64 = conn_lock
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap_or(0);

    // 2. Project list with details
    let mut stmt = conn_lock.prepare(
        "SELECT p.name, p.source_type, p.status, 
                (SELECT COUNT(*) FROM documents d WHERE d.project_id = p.id) as doc_count,
                p.last_synced
         FROM projects p
         ORDER BY p.updated_at DESC"
    ).unwrap();
    
    let projects: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "name": r.get::<_, String>(0)?,
            "type": r.get::<_, String>(1)?,
            "status": r.get::<_, String>(2)?,
            "document_count": r.get::<_, i64>(3)?,
            "last_synced": r.get::<_, Option<i64>>(4)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();

    // 3. Top tags across all documents
    let mut stmt = conn_lock.prepare(
        "SELECT tag, COUNT(*) as count FROM document_tags GROUP BY tag ORDER BY count DESC LIMIT 10"
    ).unwrap();
    let top_tags: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    // 4. Recently updated documents
    let mut stmt = conn_lock.prepare(
        "SELECT d.source_doc_id, d.title, p.name as project_name, d.last_indexed
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         ORDER BY d.last_indexed DESC
         LIMIT 5"
    ).unwrap();
    let recent_docs: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_, String>(0)?,
            "title": r.get::<_, Option<String>>(1)?,
            "project": r.get::<_, String>(2)?,
            "indexed_at": r.get::<_, i64>(3)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();

    let summary = json!({
        "overview": {
            "total_projects": total_projects,
            "total_documents": total_docs,
            "embedding_enabled": server.embedder().is_some()
        },
        "projects": projects,
        "knowledge_profile": {
            "top_tags": top_tags,
            "recently_updated": recent_docs
        },
        "agent_orientation": "You are connected to Doxus Knowledge Base. Use 'doxus_search' for deep queries or 'doxus_get_document' for full content. If a project indicates it is out of sync, recommend 'doxus_sync_project'."
    });

    McpResponse::ok(id, json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&summary).unwrap_or_default() }]
    }))
}
