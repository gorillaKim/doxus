use crate::server::McpServer;
use crate::tools::resolve_doc_id;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

pub async fn search(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::{SearchEngine, SearchMode, SearchQuery};

    let query_text = match args["query"].as_str() {
        Some(q) => q,
        None => return McpResponse::err(id, -32602, "missing required arg: query"),
    };
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let project_filter = args["project"].as_str();

    let mut q = SearchQuery::new(query_text)
        .with_limit(limit)
        .with_offset(offset);

    if let Some(proj) = project_filter {
        let conn = server.conn();
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        let pid: Result<i64, _> = conn_lock.query_row(
            "SELECT id FROM projects WHERE name=?1",
            params![proj],
            |r| r.get(0),
        );
        match pid {
            Ok(pid) => q = q.with_projects(vec![pid]),
            Err(_) => return McpResponse::err(id, -32602, format!("project '{proj}' not found")),
        }
    }

    q.mode = match args["mode"].as_str() {
        Some("fts") => SearchMode::Fts,
        Some("vector") => SearchMode::Vector,
        _ => if server.embedder().is_some() {
            SearchMode::Hybrid
        } else {
            SearchMode::Fts
        },
    };

    if let Some(embedder) = server.embedder() {
        // Use the Arc<Mutex<Connection>> directly
        let async_engine = SearchEngine::with_embedder(server.conn(), embedder);
        match async_engine.search_async(&q).await {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(hits) if hits.is_empty() => McpResponse::text(id, "No results found."),
            Ok(hits) => {
                let items: Vec<Value> = hits
                    .iter()
                    .map(|h| {
                        json!({
                            "id": h.document_id,
                            "source_id": h.source_doc_id,
                            "title": h.title,
                            "heading": h.heading_path,
                            "snippet": h.snippet,
                            "context": h.context_content,
                            "score": h.score,
                        })
                    })
                    .collect();
                
                let mut text_resp = serde_json::to_string_pretty(&items).unwrap_or_default();
                
                // Add Small-to-Big Retrieval hint if context is present but truncated
                if hits.iter().any(|h| h.context_content.as_ref().map(|s| s.contains("truncated")).unwrap_or(false)) {
                    text_resp.push_str("\n\nNOTE: Some contexts were truncated for token efficiency. Use 'doxus_get_section' with the 'heading' provided above for full content.");
                }

                McpResponse::ok(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": text_resp
                        }]
                    }),
                )
            }
        }
    } else {
        // Fallback to basic sync search (FTS only) while embedder is loading
        if q.mode == SearchMode::Vector || q.mode == SearchMode::Hybrid {
            tracing::info!("[Search] Embedder still loading, falling back to FTS");
        }
        let conn = server.conn();
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        let engine = SearchEngine::new(&*conn_lock);
        match engine.search(&q) {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(hits) if hits.is_empty() => McpResponse::text(id, "No results found."),
            Ok(hits) => {
                let items: Vec<Value> = hits
                    .iter()
                    .map(|h| {
                        json!({
                            "id": h.document_id,
                            "source_id": h.source_doc_id,
                            "title": h.title,
                            "snippet": h.snippet,
                            "score": h.score,
                        })
                    })
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
}

pub async fn get_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (db_id, source_doc_id) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let pm = server.plugin_manager();
    let service = doxus_core::document::DocumentService::new(&conn_lock, Some(pm));

    match service.fetch_full_content(project, &source_doc_id).await {
        Err(e) => McpResponse::err(
            id,
            -32602,
            format!("Failed to fetch document '{}' in project '{}': {}", source_doc_id, project, e),
        ),
        Ok(content) => {
            // Fetch title and metadata for the header
            let (title, meta_json): (Option<String>, Option<String>) = conn_lock.query_row(
                "SELECT title, metadata_json FROM documents WHERE id = ?1",
                params![db_id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
            ).unwrap_or((None, None));

            // Fetch tags
            let tags: Vec<String> = {
                let mut stmt = conn_lock.prepare(
                    "SELECT tag FROM document_tags WHERE document_id = ?1"
                ).ok().unwrap();
                stmt.query_map(params![db_id], |r| r.get::<_, String>(0)).ok().unwrap()
                    .filter_map(|r| r.ok())
                    .collect()
            };

            let mut header = title.map(|t| format!("# {t}\n")).unwrap_or_default();
            
            if !tags.is_empty() {
                header.push_str(&format!("Tags: {}\n", tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")));
            }

            if let Some(json) = meta_json {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&json) {
                    for (k, v) in map {
                        if k == "links" { continue; } // Links are usually too many
                        header.push_str(&format!("{k}: {v}\n"));
                    }
                }
            }
            
            header.push_str("\n---\n\n");
            McpResponse::text(id, format!("{header}{content}"))
        }
    }
}

pub async fn get_section(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let heading = match args["heading"].as_str() {
        Some(h) => h,
        None => return McpResponse::err(id, -32602, "missing required arg: heading"),
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (_db_id, source_doc_id) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let pm = server.plugin_manager();
    let service = doxus_core::document::DocumentService::new(&conn_lock, Some(pm));

    match service.fetch_full_content(project, &source_doc_id).await {
        Err(e) => McpResponse::err(
            id,
            -32602,
            format!("Failed to fetch document '{}' in project '{}': {}", source_doc_id, project, e),
        ),
        Ok(content) => {
            let section = extract_section(&content, heading);
            if section.is_empty() {
                McpResponse::err(
                    id,
                    -32602,
                    format!("section '{}' not found in document '{}'", heading, source_doc_id),
                )
            } else {
                McpResponse::text(id, section)
            }
        }
    }
}

pub fn get_metadata(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (db_id, source_doc_id) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let row: Result<(Option<String>, String, i64), _> = conn_lock.query_row(
        "SELECT title, content_hash, last_indexed FROM documents WHERE id = ?1",
        params![db_id],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
    );

    match row {
        Err(_) => McpResponse::err(
            id,
            -32602,
            format!("document '{}' not found in project '{}'", source_doc_id, project),
        ),
        Ok((title, hash, indexed)) => {
            let meta = json!({
                "id": db_id,
                "source_id": source_doc_id,
                "project": project,
                "title": title,
                "content_hash": hash,
                "last_indexed": indexed,
            });
            McpResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&meta).unwrap_or_default()
                    }]
                }),
            )
        }
    }
}

pub fn list_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let limit = args["limit"].as_u64().unwrap_or(50) as i64;
    let cursor_offset = args["cursor"]
        .as_str()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(0);

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let mut stmt = match conn_lock.prepare(
        "SELECT d.source_doc_id, d.title
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         WHERE p.name = ?1
         ORDER BY d.source_doc_id
         LIMIT ?2 OFFSET ?3",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![project, limit, cursor_offset], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let next_cursor =
                if rows.len() as i64 == limit { Some(cursor_offset + limit) } else { None };
            let items: Vec<Value> = rows
                .iter()
                .map(|(doc_id, title)| {
                    json!({ "id": doc_id, "title": title })
                })
                .collect();
            McpResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&json!({
                            "documents": items,
                            "next_cursor": next_cursor,
                        })).unwrap_or_default()
                    }]
                }),
            )
        }
    }
}

pub fn get_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let ids = match args["ids"].as_array() {
        Some(a) => a,
        None => return McpResponse::err(id, -32602, "missing required arg: ids (array)"),
    };

    let mut results = vec![];
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    for doc_id_val in ids {
        let (_, source_doc_id) = match resolve_doc_id(&conn_lock, project, doc_id_val) {
            Ok(res) => res,
            Err(_) => continue,
        };
        let row: Result<(Option<String>, String), _> = conn_lock.query_row(
            "SELECT d.title, d.content
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, source_doc_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        );
        if let Ok((title, content)) = row {
            results.push(json!({ "id": source_doc_id, "title": title, "content": content }));
        }
    }

    McpResponse::ok(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&results).unwrap_or_default()
            }]
        }),
    )
}

pub fn resolve_alias(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let alias = match args["alias"].as_str() {
        Some(a) => a,
        None => return McpResponse::err(id, -32602, "missing required arg: alias"),
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let row: Result<(String, String), _> = conn_lock.query_row(
        "SELECT da.source_doc_id, p.name
         FROM document_aliases da
         JOIN documents d ON da.document_id = d.id
         JOIN projects p ON d.project_id = p.id
         WHERE da.alias = ?1
         LIMIT 1",
        params![alias],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );

    match row {
        Ok((doc_id, project)) => McpResponse::text(
            id,
            format!("alias '{alias}' → project: {project}, id: {doc_id}"),
        ),
        Err(_) => McpResponse::err(id, -32602, format!("alias '{alias}' not found")),
    }
}

pub fn get_toc(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (db_id, source_doc_id) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let content: Result<String, _> = conn_lock.query_row(
        "SELECT content FROM documents WHERE id = ?1",
        params![db_id],
        |r| r.get::<_, String>(0),
    );

    match content {
        Err(_) => McpResponse::err(
            id,
            -32602,
            format!("document '{}' not found in project '{}'", source_doc_id, project),
        ),
        Ok(content) => {
            let toc = extract_toc(&content);
            McpResponse::text(id, if toc.is_empty() { "No headings found.".into() } else { toc })
        }
    }
}

pub fn get_ranking(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let limit = args["limit"].as_u64().unwrap_or(20) as i64;

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let mut stmt = match conn_lock.prepare(
        "SELECT d.source_doc_id, d.title, COALESCE(vc.view_count, 0) as views
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         LEFT JOIN view_counts vc ON d.id = vc.document_id
         WHERE p.name = ?1
         ORDER BY views DESC
         LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![project, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let items: Vec<Value> = rows
                .iter()
                .map(|(doc_id, title, views)| json!({ "id": doc_id, "title": title, "views": views }))
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

pub fn inspect_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };

    let (db_id, source_doc_id) = match resolve_doc_id(&conn_lock, project, &args["id"]) {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32602, e),
    };

    let row: Result<(i64, Option<String>, String, i64, i64), _> = conn_lock.query_row(
        "SELECT d.id, d.title, d.content_hash, d.last_indexed,
                (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) as chunk_count
         FROM documents d
         WHERE d.id = ?1",
        params![db_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?)),
    );

    match row {
        Err(_) => McpResponse::err(
            id,
            -32602,
            format!("document '{}' not found in project '{}'", source_doc_id, project),
        ),
        Ok((db_id, title, hash, indexed, chunks)) => {
            let info = json!({
                "id": db_id,
                "source_id": source_doc_id,
                "project": project,
                "title": title,
                "content_hash": hash,
                "last_indexed": indexed,
                "chunk_count": chunks,
            });
            McpResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&info).unwrap_or_default()
                    }]
                }),
            )
        }
    }
}

/// Extract a section starting at `heading` until the next same-or-higher level heading or EOF.
pub fn extract_section(content: &str, heading: &str) -> String {
    let heading_lower = heading.to_lowercase();
    let mut in_section = false;
    let mut section_level = 0usize;
    let mut result = Vec::new();

    for line in content.lines() {
        if line.starts_with('#') {
            let level = line.chars().take_while(|&c| c == '#').count();
            let text = line.trim_start_matches('#').trim().to_lowercase();

            if in_section {
                if level <= section_level {
                    break;
                }
            } else if text == heading_lower || text.contains(&heading_lower) {
                in_section = true;
                section_level = level;
                result.push(line.to_string());
                continue;
            }
        }

        if in_section {
            result.push(line.to_string());
        }
    }

    result.join("\n")
}

/// Build a table of contents from markdown headings.
pub fn extract_toc(content: &str) -> String {
    let mut lines = vec![];
    for line in content.lines() {
        if line.starts_with('#') {
            let level = line.chars().take_while(|&c| c == '#').count();
            let text = line.trim_start_matches('#').trim();
            let indent = "  ".repeat(level.saturating_sub(1));
            lines.push(format!("{indent}- {text}"));
        }
    }
    lines.join("\n")
}
