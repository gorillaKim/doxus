use crate::server::McpServer;
use crate::tools::resolve_doc_id;
use crate::types::McpResponse;
use regex::Regex;
use rusqlite::params;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub async fn search(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::{SearchEngine, SearchMode, SearchQuery};

    let query_text = args["query"].as_str().unwrap_or("");
    let tags = args["tags"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<String>>())
        .unwrap_or_default();

    if query_text.is_empty() && tags.is_empty() {
        return McpResponse::err(id, -32602, "Missing required arg: either 'query' or 'tags' must be provided");
    }

    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let project_filter = args["project"].as_str();

    let mut q = SearchQuery::new(query_text)
        .with_limit(limit)
        .with_offset(offset)
        .with_tags(tags);

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

    // 날짜 필터 파라미터
    if let Some(v) = args["created_after"].as_i64()  { q.created_after  = Some(v); }
    if let Some(v) = args["created_before"].as_i64() { q.created_before = Some(v); }
    if let Some(v) = args["updated_after"].as_i64()  { q.updated_after  = Some(v); }
    if let Some(v) = args["updated_before"].as_i64() { q.updated_before = Some(v); }

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
                            "tags": h.tags,
                            "snippet": h.snippet,
                            "context": h.context_content,
                            "score": h.score,
                            "created_at": h.created_at,
                            "updated_at": h.updated_at,
                        })
                    })
                    .collect();
                
                let mut text_resp = serde_json::to_string_pretty(&items).unwrap_or_default();
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
                            "tags": h.tags,
                            "snippet": h.snippet,
                            "score": h.score,
                            "created_at": h.created_at,
                            "updated_at": h.updated_at,
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
    let (_db_id, source_doc_id) = {
        let conn = server.conn();
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };

        match resolve_doc_id(&conn_lock, project, &args["id"]) {
            Ok(res) => res,
            Err(e) => return McpResponse::err(id, -32602, e),
        }
    }; // <- 락이 여기서 자동으로 해제(Drop)됩니다.

    let pm = server.plugin_manager();
    let service = doxus_core::document::DocumentService::new_with_path(server.db_path(), Some(pm.clone()));

    match service.fetch_full_content(project, &source_doc_id).await {
        Err(e) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/doxus-error.log") 
            {
                use std::io::Write;
                let _ = writeln!(file, "[Error] fetch failed for {} in {}: {}", source_doc_id, project, e);
            }
            McpResponse::err(
                id,
                -32602,
                format!("Failed to fetch document '{}' in project '{}': {}", source_doc_id, project, e),
            )
        },
        Ok(doc) => {
            let mut title = doc.title.clone().unwrap_or(source_doc_id.clone());
            let tags = doc.tags.clone();
            let metadata_json = serde_json::to_string(&doc.metadata).ok();

            let project_name = project.to_string();
            let doc_clone = doc.clone();
            let indexer = server.indexer();

            let doc_sid = source_doc_id.clone();
            tokio::spawn(async move {
                let conn = indexer.conn();
                let project_id_res = {
                    let conn_lock = conn.lock().unwrap();
                    conn_lock.query_row(
                        "SELECT id, storage_strategy FROM projects WHERE name = ?1",
                        params![project_name],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                    )
                };

                if let Ok((pid, strategy)) = project_id_res {
                    if indexer.needs_reindexing(pid, &doc_clone.id.0, doc_clone.updated_at).await {
                        tracing::info!("[JIT-Indexer] Background indexing triggered for document: {}", doc_sid);
                        let _ = indexer.index_single_document(pid, doc_clone, &strategy).await;
                    }
                }
            });

            if title.is_empty() { title = format!("Source ID: {}", source_doc_id); }
            let mut header = format!("# {title}\n");
            if !tags.is_empty() {
                header.push_str(&format!("Tags: {}\n", tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")));
            }
            if let Some(json) = metadata_json {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&json) {
                    for (k, v) in map {
                        if k == "links" || k == "relative_path" { continue; }
                        header.push_str(&format!("{k}: {v}\n"));
                    }
                }
            }
            header.push_str("\n---\n\n");
            McpResponse::text(id, format!("{header}{}", doc.content))
        }
    }
}

pub async fn get_toc(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
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
    let service = doxus_core::document::DocumentService::new_with_path(server.db_path(), Some(pm.clone()));

    match service.fetch_full_content(project, &source_doc_id).await {
        Err(e) => McpResponse::err(id, -32602, e.to_string()),
        Ok(doc) => {
            let toc = extract_toc(&doc.content);
            McpResponse::text(id, if toc.is_empty() { "No headings found.".into() } else { toc })
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
    let service = doxus_core::document::DocumentService::new_with_path(server.db_path(), Some(pm.clone()));

    match service.fetch_full_content(project, &source_doc_id).await {
        Err(e) => McpResponse::err(id, -32602, e.to_string()),
        Ok(doc) => {
            let section = extract_section(&doc.content, heading);
            McpResponse::text(id, if section.is_empty() { format!("Heading '{}' not found.", heading) } else { section })
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
        Err(_) => McpResponse::err(id, -32602, format!("document '{}' not found", source_doc_id)),
        Ok((title, hash, indexed)) => {
            let meta = json!({
                "id": db_id,
                "source_id": source_doc_id,
                "project": project,
                "title": title,
                "content_hash": hash,
                "last_indexed": indexed,
            });
            McpResponse::ok(id, json!({ "content": [{"type": "text", "text": serde_json::to_string_pretty(&meta).unwrap_or_default()}] }))
        }
    }
}

pub fn list_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let limit = args["limit"].as_u64().unwrap_or(50) as i64;
    let cursor_offset = args["cursor"].as_str().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);

    // sort_by: "title" | "created_at" | "updated_at" | "last_indexed" (기본: source_doc_id)
    let order_col = match args["sort_by"].as_str() {
        Some("title") => "d.title",
        Some("created_at") => "d.created_at",
        Some("updated_at") => "d.updated_at",
        Some("last_indexed") => "d.last_indexed",
        _ => "d.source_doc_id",
    };
    // sort_order: "asc" | "desc" (기본: "asc")
    let order_dir = match args["sort_order"].as_str() {
        Some("desc") => "DESC",
        _ => "ASC",
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let sql = format!(
        "SELECT d.source_doc_id, d.title, d.created_at, d.updated_at \
         FROM documents d JOIN projects p ON d.project_id = p.id \
         WHERE p.name = ?1 ORDER BY {order_col} {order_dir} LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = match conn_lock.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt.query_map(params![project, limit, cursor_offset], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<i64>>(3)?,
        ))
    }).and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let next_cursor = if rows.len() as i64 == limit { Some(cursor_offset + limit) } else { None };
            let items: Vec<Value> = rows.iter().map(|(doc_id, title, created_at, updated_at)| {
                json!({
                    "id": doc_id,
                    "title": title,
                    "created_at": created_at,
                    "updated_at": updated_at,
                })
            }).collect();
            McpResponse::ok(id, json!({ "content": [{"type": "text", "text": serde_json::to_string_pretty(&json!({ "documents": items, "next_cursor": next_cursor })).unwrap_or_default()}] }))
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
            "SELECT d.title, d.content FROM documents d JOIN projects p ON d.project_id = p.id WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, source_doc_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        );
        if let Ok((title, content)) = row {
            results.push(json!({ "id": source_doc_id, "title": title, "content": content }));
        }
    }

    McpResponse::ok(id, json!({ "content": [{"type": "text", "text": serde_json::to_string_pretty(&results).unwrap_or_default()}] }))
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
        "SELECT da.source_doc_id, p.name FROM document_aliases da JOIN documents d ON da.document_id = d.id JOIN projects p ON d.project_id = p.id WHERE da.alias = ?1 LIMIT 1",
        params![alias],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    match row {
        Ok((doc_id, project)) => McpResponse::text(id, format!("alias '{alias}' → project: {project}, id: {doc_id}")),
        Err(_) => McpResponse::err(id, -32602, format!("alias '{alias}' not found")),
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
        "SELECT d.source_doc_id, d.title, COALESCE(vc.view_count, 0) as views FROM documents d JOIN projects p ON d.project_id = p.id LEFT JOIN view_counts vc ON d.id = vc.document_id WHERE p.name = ?1 ORDER BY views DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };
    let rows: Result<Vec<_>, _> = stmt.query_map(params![project, limit], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?))).and_then(|it| it.collect());
    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => {
            let items: Vec<Value> = rows.iter().map(|(doc_id, title, views)| json!({ "id": doc_id, "title": title, "views": views })).collect();
            McpResponse::ok(id, json!({ "content": [{"type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default()}] }))
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
        "SELECT d.id, d.title, d.content_hash, d.last_indexed, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) as chunk_count FROM documents d WHERE d.id = ?1",
        params![db_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?)),
    );
    match row {
        Err(_) => McpResponse::err(id, -32602, format!("document '{}' not found", source_doc_id)),
        Ok((db_id, title, hash, indexed, chunks)) => {
            let info = json!({ "id": db_id, "source_id": source_doc_id, "project": project, "title": title, "content_hash": hash, "last_indexed": indexed, "chunk_count": chunks });
            McpResponse::ok(id, json!({ "content": [{"type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default()}] }))
        }
    }
}

pub async fn create_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let title = match args["title"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: title"),
    };
    let content = args["content"].as_str().unwrap_or("");
    let folder = args["folder"].as_str();
    let metadata = args["metadata"].as_object().map(|obj| {
        obj.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::HashMap<String, Value>>()
    });

    let pm = server.plugin_manager();
    let service = doxus_core::document::DocumentService::new_with_path(server.db_path(), Some(pm.clone()));

    match service.create_document(project, title, content, folder, metadata).await {
        Ok(new_id) => {
            // Immediate Sync
            if let Ok(doc) = service.fetch_full_content(project, &new_id.0).await {
                let indexer = server.indexer();
                let conn = indexer.conn();
                let project_info = {
                    let conn_lock = conn.lock().unwrap();
                    conn_lock.query_row(
                        "SELECT id, storage_strategy FROM projects WHERE name = ?1",
                        params![project],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                    )
                };
                if let Ok((pid, strategy)) = project_info {
                    let _ = indexer.index_single_document(pid, doc, &strategy).await;
                }
            }
            McpResponse::text(id, format!("Successfully created document '{}' in project '{}'", new_id.0, project))
        },
        Err(e) => McpResponse::err(id, -32603, format!("Failed to create document: {}", e)),
    }
}

static RE_HEADER: OnceLock<Regex> = OnceLock::new();
fn header_regex() -> &'static Regex { RE_HEADER.get_or_init(|| Regex::new(r"^\s{0,3}(#{1,6})(?:\s+(.*)|$)").unwrap()) }

pub fn extract_section(content: &str, heading: &str) -> String {
    let heading_lower = heading.to_lowercase();
    let re = header_regex();
    let mut in_section = false;
    let mut section_level = 0usize;
    let mut result = Vec::new();

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let level = caps.get(1).map_or(0, |m| m.as_str().len());
            let text = caps.get(2).map_or("", |m| m.as_str().trim()).to_lowercase();
            if in_section {
                if level <= section_level { break; }
            } else if text == heading_lower || text.contains(&heading_lower) {
                in_section = true;
                section_level = level;
            }
        }
        if in_section { result.push(line); }
    }
    result.join("\n")
}

pub fn extract_toc(content: &str) -> String {
    let re = header_regex();
    let mut toc = Vec::new();
    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let level = caps.get(1).map_or(0, |m| m.as_str().len());
            let text = caps.get(2).map_or("", |m| m.as_str().trim());
            toc.push(format!("{}{}", "  ".repeat(level - 1), text));
        }
    }
    toc.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServer;
    use doxus_core::search::{DocMeta, SyncSearchEngine};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn make_test_conn() -> rusqlite::Connection {
        doxus_core::db::ensure_vec_extension();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::apply_pragmas(&conn).unwrap();
        doxus_core::db::create_vec0_table(&conn).unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    fn setup_server() -> (McpServer, i64) {
        let conn = Arc::new(Mutex::new(make_test_conn()));
        {
            let c = conn.lock().unwrap();
            c.execute(
                "INSERT INTO projects(name, display_name, path, status, created_at, updated_at) \
                 VALUES ('tp', 'Test', '/tmp', 'active', unixepoch(), unixepoch())",
                [],
            ).unwrap();
        }
        let pid: i64 = conn.lock().unwrap()
            .query_row("SELECT id FROM projects WHERE name='tp'", [], |r| r.get::<_, i64>(0))
            .unwrap();
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(PathBuf::from("/tmp")));
        let server = McpServer::new(conn, PathBuf::from(":memory:"), None, pm, PathBuf::from("/tmp"));
        (server, pid)
    }

    fn insert_doc(server: &McpServer, pid: i64, sid: &str, title: &str, content: &str, created_at: i64, updated_at: i64) {
        let conn = server.conn();
        let c = conn.lock().unwrap();
        let engine = SyncSearchEngine::from_conn(&c);
        let meta = DocMeta { created_at: Some(created_at), updated_at: Some(updated_at), ..Default::default() };
        engine.index_document_with_meta(pid, sid, title, content, &meta, "full").unwrap();
    }

    fn parse_docs(text: &str) -> Vec<serde_json::Value> {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        v["documents"].as_array().cloned().unwrap_or_default()
    }

    fn get_text(resp: &McpResponse) -> String {
        let v = serde_json::to_value(resp).unwrap();
        v["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
    }

    // ── Step 3 TDD 테스트 ────────────────────────────────────────────────────

    #[test]
    fn test_list_documents_includes_created_updated_at() {
        let (server, pid) = setup_server();
        insert_doc(&server, pid, "doc1", "Doc 1", "content", 1234, 5678);

        let resp = list_documents(&server, json!(1), &json!({ "project": "tp" }));
        let text = get_text(&resp);
        let docs = parse_docs(&text);
        assert!(!docs.is_empty(), "문서 목록이 비어 있음");
        assert_eq!(docs[0]["created_at"].as_i64(), Some(1234), "created_at 포함");
        assert_eq!(docs[0]["updated_at"].as_i64(), Some(5678), "updated_at 포함");
    }

    #[test]
    fn test_list_documents_sort_by_created_at_desc() {
        let (server, pid) = setup_server();
        insert_doc(&server, pid, "old", "Old Doc", "old", 1000, 1000);
        insert_doc(&server, pid, "new", "New Doc", "new", 5000, 5000);

        let resp = list_documents(
            &server, json!(1),
            &json!({ "project": "tp", "sort_by": "created_at", "sort_order": "desc" }),
        );
        let text = get_text(&resp);
        let docs = parse_docs(&text);
        assert!(docs.len() >= 2, "두 문서 이상 있어야 함");
        assert_eq!(docs[0]["id"].as_str(), Some("new"), "최신 문서(created_at=5000)가 먼저");
        assert_eq!(docs[1]["id"].as_str(), Some("old"));
    }

    #[test]
    fn test_list_documents_sort_by_created_at_asc() {
        let (server, pid) = setup_server();
        insert_doc(&server, pid, "old", "Old Doc", "old", 1000, 1000);
        insert_doc(&server, pid, "new", "New Doc", "new", 5000, 5000);

        let resp = list_documents(
            &server, json!(1),
            &json!({ "project": "tp", "sort_by": "created_at", "sort_order": "asc" }),
        );
        let text = get_text(&resp);
        let docs = parse_docs(&text);
        assert!(docs.len() >= 2);
        assert_eq!(docs[0]["id"].as_str(), Some("old"), "오래된 문서(created_at=1000)가 먼저");
    }

    #[test]
    fn test_search_response_includes_date_fields() {
        let (server, pid) = setup_server();
        insert_doc(&server, pid, "d1", "Unique Keyword Title", "lorem", 1111, 2222);

        // SyncSearchEngine으로 직접 검색해 날짜가 DB에 저장됐는지 확인
        let conn = server.conn();
        let c = conn.lock().unwrap();
        let engine = SyncSearchEngine::from_conn(&c);
        let q = doxus_core::search::SearchQuery::new("Unique Keyword");
        let hits = engine.search(&q).unwrap();
        assert!(!hits.is_empty(), "FTS 결과가 있어야 함");
        assert_eq!(hits[0].created_at, Some(1111));
        assert_eq!(hits[0].updated_at, Some(2222));
    }

    #[test]
    fn test_search_created_after_filter_via_engine() {
        let (server, pid) = setup_server();
        insert_doc(&server, pid, "old", "Knowledge Base Old", "content", 1000, 1000);
        insert_doc(&server, pid, "new", "Knowledge Base New", "content", 5000, 5000);

        let conn = server.conn();
        let c = conn.lock().unwrap();
        let engine = SyncSearchEngine::from_conn(&c);
        let mut q = doxus_core::search::SearchQuery::new("Knowledge Base");
        q.created_after = Some(2000);
        let hits = engine.search(&q).unwrap();
        assert_eq!(hits.len(), 1, "created_after=2000 이면 created_at=5000 문서만 반환");
        assert_eq!(hits[0].title.as_deref(), Some("Knowledge Base New"));
    }
}
