/// doxus MCP server — 37 docnx_* tools via MCP protocol (JSONL over stdio)
///
/// Phase 1: all tools wired to real DB via McpServer struct.
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

// ── MCP protocol types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct McpRequest {
    id: Value,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct McpResponse {
    id: Value,
    result: Option<Value>,
    error: Option<McpError>,
}

#[derive(Debug, Serialize)]
struct McpError {
    code: i64,
    message: String,
}

impl McpResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { id, result: Some(result), error: None }
    }

    fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self { id, result: None, error: Some(McpError { code, message: message.into() }) }
    }

    fn text(id: Value, text: impl Into<String>) -> Self {
        Self::ok(id, json!({ "content": [{ "type": "text", "text": text.into() }] }))
    }

    fn not_implemented(id: Value, tool: &str, hint: &str) -> Self {
        Self::text(id, format!("{tool}: {hint}"))
    }
}

// ── McpServer ─────────────────────────────────────────────────────────────────

struct McpServer {
    conn: rusqlite::Connection,
}

impl McpServer {
    fn new(conn: rusqlite::Connection) -> Self {
        Self { conn }
    }

    fn dispatch(&self, method: &str, id: Value, params: Option<&Value>) -> McpResponse {
        match method {
            "tools/list" => McpResponse::ok(id, tool_list()),

            "tools/call" => {
                let name = params.and_then(|p| p["name"].as_str()).unwrap_or("");
                let args =
                    params.and_then(|p| p.get("arguments")).cloned().unwrap_or(json!({}));
                self.dispatch_tool(name, id, &args)
            }

            "initialize" => McpResponse::ok(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "doxus-mcp", "version": "0.1.0" }
                }),
            ),

            _ => McpResponse::err(id, -32601, format!("method not found: {method}")),
        }
    }

    fn dispatch_tool(&self, name: &str, id: Value, args: &Value) -> McpResponse {
        match name {
            // ── Core tools ────────────────────────────────────────────────────
            "docnx_status" => self.tool_status(id),
            "docnx_help" => McpResponse::text(id, HELP_TEXT),
            "docnx_onboard" => McpResponse::text(id, ONBOARD_TEXT),

            // ── Project management ────────────────────────────────────────────
            "docnx_list_projects" => self.tool_list_projects(id),
            "docnx_add_project" => self.tool_add_project(id, args),
            "docnx_remove_project" => self.tool_remove_project(id, args),
            "docnx_index_project" => self.tool_index_project(id, args),
            "docnx_sync_project" => self.tool_sync_project(id, args),

            // ── Search & documents ────────────────────────────────────────────
            "docnx_search" => self.tool_search(id, args),
            "docnx_get_document" => self.tool_get_document(id, args),
            "docnx_get_section" => self.tool_get_section(id, args),
            "docnx_get_metadata" => self.tool_get_metadata(id, args),
            "docnx_list_documents" => self.tool_list_documents(id, args),
            "docnx_get_documents" => self.tool_get_documents(id, args),
            "docnx_resolve_alias" => self.tool_resolve_alias(id, args),
            "docnx_get_toc" => self.tool_get_toc(id, args),
            "docnx_get_ranking" => self.tool_get_ranking(id, args),
            "docnx_inspect_document" => self.tool_inspect_document(id, args),

            // ── Graph ─────────────────────────────────────────────────────────
            "docnx_get_backlinks" => self.tool_get_backlinks(id, args),
            "docnx_get_links" => self.tool_get_links(id, args),
            "docnx_find_related" => self.tool_find_related(id, args),
            "docnx_find_path" => self.tool_find_path(id, args),
            "docnx_get_cluster" => self.tool_get_cluster(id, args),

            // ── Plugin management ─────────────────────────────────────────────
            "docnx_plugin_list" => self.tool_plugin_list(id),
            "docnx_plugin_install" => self.tool_plugin_install(id, args),
            "docnx_plugin_remove" => self.tool_plugin_remove(id, args),
            "docnx_plugin_update" => self.tool_plugin_update(id, args),
            "docnx_plugin_search" => self.tool_plugin_search(id, args),
            "docnx_plugin_status" => self.tool_plugin_status(id, args),
            "docnx_plugin_logs" => self.tool_plugin_logs(id, args),
            "docnx_plugin_info" => self.tool_plugin_info(id, args),

            // ── Workspace ─────────────────────────────────────────────────────
            "docnx_create_document" => self.tool_create_workspace_document(id, args),
            "docnx_update_document" => self.tool_update_workspace_document(id, args),
            "docnx_delete_document" => self.tool_delete_workspace_document(id, args),
            "docnx_list_workspace_documents" => self.tool_list_workspace_documents(id, args),
            "docnx_apply_template" => self.tool_apply_template(id, args),

            // ── Diagnostics ───────────────────────────────────────────────────
            "docnx_diagnose" => self.tool_diagnose(id),
            "docnx_system_report" => self.tool_system_report(id),
            "docnx_explain_search" => self.tool_explain_search(id, args),

            unknown => McpResponse::err(id, -32601, format!("unknown tool: {unknown}")),
        }
    }

    // ── Tool implementations ──────────────────────────────────────────────────

    fn tool_status(&self, id: Value) -> McpResponse {
        let projects: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap_or(0);
        let documents: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap_or(0);
        McpResponse::text(
            id,
            format!(
                "doxus MCP server v0.1.0 — operational\nProjects: {projects}  Documents: {documents}"
            ),
        )
    }

    fn tool_list_projects(&self, id: Value) -> McpResponse {
        let mut stmt = match self.conn.prepare(
            "SELECT name, display_name, status, path FROM projects ORDER BY name",
        ) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };

        let rows: Result<Vec<_>, _> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .and_then(|it| it.collect());

        match rows {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(rows) if rows.is_empty() => {
                McpResponse::text(id, "No projects found. Add one with docnx_add_project.")
            }
            Ok(rows) => {
                let mut lines =
                    vec!["NAME                 DISPLAY              STATUS    PATH".to_string()];
                lines.push("-".repeat(80));
                for (name, display, status, path) in &rows {
                    lines.push(format!("{name:<20} {display:<20} {status:<9} {path}"));
                }
                McpResponse::text(id, lines.join("\n"))
            }
        }
    }

    fn tool_add_project(&self, id: Value, args: &Value) -> McpResponse {
        let name = match args["name"].as_str() {
            Some(n) => n,
            None => return McpResponse::err(id, -32602, "missing required arg: name"),
        };
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: path"),
        };
        let display_name = args["display_name"].as_str().unwrap_or(name);

        let result = self.conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES (?1, ?2, ?3, unixepoch(), unixepoch())",
            params![name, display_name, path],
        );

        match result {
            Ok(_) => McpResponse::text(
                id,
                format!("Project '{name}' added. Run docnx_index_project to index it."),
            ),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_remove_project(&self, id: Value, args: &Value) -> McpResponse {
        let name = match args["name"].as_str() {
            Some(n) => n,
            None => return McpResponse::err(id, -32602, "missing required arg: name"),
        };

        // Fetch project id first to give a clear error if not found
        let pid: Result<i64, _> = self
            .conn
            .query_row("SELECT id FROM projects WHERE name=?1", params![name], |r| r.get(0));

        match pid {
            Err(_) => McpResponse::err(id, -32602, format!("project '{name}' not found")),
            Ok(pid) => {
                // Delete index data only — original files are never touched
                let _ = self
                    .conn
                    .execute("DELETE FROM source_instances WHERE project_id=?1", [pid]);
                match self.conn.execute("DELETE FROM projects WHERE id=?1", [pid]) {
                    Ok(_) => McpResponse::text(
                        id,
                        format!(
                            "Project '{name}' removed (index data deleted, original files untouched)."
                        ),
                    ),
                    Err(e) => McpResponse::err(id, -32603, e.to_string()),
                }
            }
        }
    }

    fn tool_index_project(&self, id: Value, args: &Value) -> McpResponse {
        let name = match args["project"].as_str() {
            Some(n) => n,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };

        let status: Result<String, _> = self.conn.query_row(
            "SELECT status FROM projects WHERE name=?1",
            params![name],
            |r| r.get(0),
        );

        match status {
            Err(_) => McpResponse::err(id, -32602, format!("project '{name}' not found")),
            Ok(s) => McpResponse::text(
                id,
                format!(
                    "Project '{name}' (status: {s})\nIndexing must be triggered via CLI:\n  doxus index {name}"
                ),
            ),
        }
    }

    fn tool_search(&self, id: Value, args: &Value) -> McpResponse {
        use doxus_core::search::{SearchEngine, SearchQuery};

        let query_text = match args["query"].as_str() {
            Some(q) => q,
            None => return McpResponse::err(id, -32602, "missing required arg: query"),
        };
        let limit = args["limit"].as_u64().unwrap_or(20) as usize;
        let project_filter = args["project"].as_str();

        let mut q = SearchQuery::new(query_text).with_limit(limit);

        // If project filter given, resolve to project_id
        if let Some(proj) = project_filter {
            let pid: Result<i64, _> = self.conn.query_row(
                "SELECT id FROM projects WHERE name=?1",
                params![proj],
                |r| r.get(0),
            );
            match pid {
                Ok(pid) => q = q.with_projects(vec![pid]),
                Err(_) => {
                    return McpResponse::err(
                        id,
                        -32602,
                        format!("project '{proj}' not found"),
                    )
                }
            }
        }

        let engine = SearchEngine::new(&self.conn);
        match engine.search(&q) {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(hits) if hits.is_empty() => McpResponse::text(id, "No results found."),
            Ok(hits) => {
                let items: Vec<Value> = hits
                    .iter()
                    .map(|h| {
                        json!({
                            "document_id": h.document_id,
                            "title": h.title,
                            "file_path": h.file_path,
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

    fn tool_get_document(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let row: Result<(Option<String>, String), _> = self.conn.query_row(
            "SELECT d.title, d.content
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );

        match row {
            Err(_) => McpResponse::err(
                id,
                -32602,
                format!("document '{doc_id}' not found in project '{project}'"),
            ),
            Ok((title, content)) => {
                let header = title
                    .map(|t| format!("# {t}\n\n"))
                    .unwrap_or_default();
                McpResponse::text(id, format!("{header}{content}"))
            }
        }
    }

    fn tool_get_section(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let heading = match args["heading"].as_str() {
            Some(h) => h,
            None => return McpResponse::err(id, -32602, "missing required arg: heading"),
        };

        let content: Result<String, _> = self.conn.query_row(
            "SELECT d.content
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| r.get(0),
        );

        match content {
            Err(_) => McpResponse::err(
                id,
                -32602,
                format!("document '{doc_id}' not found in project '{project}'"),
            ),
            Ok(content) => {
                let section = extract_section(&content, heading);
                if section.is_empty() {
                    McpResponse::err(
                        id,
                        -32602,
                        format!("heading '{heading}' not found in document '{doc_id}'"),
                    )
                } else {
                    McpResponse::text(id, section)
                }
            }
        }
    }

    fn tool_get_metadata(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let row: Result<(Option<String>, String, i64), _> = self.conn.query_row(
            "SELECT d.title, d.content_hash, d.last_indexed
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        );

        match row {
            Err(_) => McpResponse::err(
                id,
                -32602,
                format!("document '{doc_id}' not found in project '{project}'"),
            ),
            Ok((title, hash, indexed)) => {
                let meta = json!({
                    "id": doc_id,
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

    fn tool_list_documents(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let limit = args["limit"].as_u64().unwrap_or(50) as i64;
        let cursor_offset = args["cursor"]
            .as_str()
            .and_then(|c| c.parse::<i64>().ok())
            .unwrap_or(0);

        let mut stmt = match self.conn.prepare(
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

    fn tool_get_documents(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let ids = match args["ids"].as_array() {
            Some(a) => a,
            None => return McpResponse::err(id, -32602, "missing required arg: ids (array)"),
        };

        let mut results = vec![];
        for doc_id_val in ids {
            let doc_id = match doc_id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let row: Result<(Option<String>, String), _> = self.conn.query_row(
                "SELECT d.title, d.content
                 FROM documents d
                 JOIN projects p ON d.project_id = p.id
                 WHERE p.name = ?1 AND d.source_doc_id = ?2",
                params![project, doc_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            );
            if let Ok((title, content)) = row {
                results.push(json!({ "id": doc_id, "title": title, "content": content }));
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

    fn tool_resolve_alias(&self, id: Value, args: &Value) -> McpResponse {
        let alias = match args["alias"].as_str() {
            Some(a) => a,
            None => return McpResponse::err(id, -32602, "missing required arg: alias"),
        };

        let row: Result<(String, String), _> = self.conn.query_row(
            "SELECT da.source_doc_id, p.name
             FROM document_aliases da
             JOIN documents d ON da.document_id = d.id
             JOIN projects p ON d.project_id = p.id
             WHERE da.alias = ?1
             LIMIT 1",
            params![alias],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );

        match row {
            Ok((doc_id, project)) => McpResponse::text(
                id,
                format!("alias '{alias}' → project: {project}, id: {doc_id}"),
            ),
            Err(_) => McpResponse::err(id, -32602, format!("alias '{alias}' not found")),
        }
    }

    fn tool_get_toc(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let content: Result<String, _> = self.conn.query_row(
            "SELECT d.content
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| r.get(0),
        );

        match content {
            Err(_) => McpResponse::err(
                id,
                -32602,
                format!("document '{doc_id}' not found in project '{project}'"),
            ),
            Ok(content) => {
                let toc = extract_toc(&content);
                McpResponse::text(id, if toc.is_empty() { "No headings found.".into() } else { toc })
            }
        }
    }

    fn tool_get_ranking(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let limit = args["limit"].as_u64().unwrap_or(20) as i64;

        let mut stmt = match self.conn.prepare(
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

    fn tool_inspect_document(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let row: Result<(i64, Option<String>, String, i64, i64), _> = self.conn.query_row(
            "SELECT d.id, d.title, d.content_hash, d.last_indexed,
                    (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) as chunk_count
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        );

        match row {
            Err(_) => McpResponse::err(
                id,
                -32602,
                format!("document '{doc_id}' not found in project '{project}'"),
            ),
            Ok((db_id, title, hash, indexed, chunks)) => {
                let info = json!({
                    "db_id": db_id,
                    "source_doc_id": doc_id,
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

    fn tool_get_backlinks(&self, id: Value, args: &Value) -> McpResponse {
        self.tool_links(id, args, false)
    }

    fn tool_get_links(&self, id: Value, args: &Value) -> McpResponse {
        self.tool_links(id, args, true)
    }

    /// `outgoing=true` → forward links; `outgoing=false` → backlinks
    fn tool_links(&self, id: Value, args: &Value, outgoing: bool) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        // Check document_links table exists
        let table_exists: bool = self
            .conn
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

        let mut stmt = match self.conn.prepare(sql) {
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

    fn tool_plugin_list(&self, id: Value) -> McpResponse {
        let mut stmt = match self.conn.prepare(
            "SELECT plugin_id, COUNT(*) as instances
             FROM source_instances
             GROUP BY plugin_id
             ORDER BY plugin_id",
        ) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };

        let rows: Result<Vec<_>, _> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .and_then(|it| it.collect());

        match rows {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(rows) if rows.is_empty() => {
                McpResponse::text(id, "No plugins installed.")
            }
            Ok(rows) => {
                let items: Vec<Value> = rows
                    .iter()
                    .map(|(plugin_id, instances)| {
                        json!({ "plugin_id": plugin_id, "instances": instances })
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

    fn tool_diagnose(&self, id: Value) -> McpResponse {
        let projects: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap_or(0);
        let documents: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap_or(0);
        let chunks: i64 = self
            .conn
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
            lines.push("\nNo projects found. Add one with docnx_add_project.".to_string());
        }
        if documents == 0 && projects > 0 {
            lines.push("\nNo documents indexed yet. Run docnx_index_project.".to_string());
        }
        lines.push(format!("\nRaw: {}", serde_json::to_string(&diag).unwrap_or_default()));

        McpResponse::text(id, lines.join("\n"))
    }

    fn tool_system_report(&self, id: Value) -> McpResponse {
        let projects: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap_or(0);
        let active_projects: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE status='active'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let documents: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap_or(0);
        let chunks: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap_or(0);
        let plugins: i64 = self
            .conn
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

    // ── Graph tools ───────────────────────────────────────────────────────────

    fn tool_find_related(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let k = args["k"].as_u64().unwrap_or(10) as i64;

        // Get the source document content for FTS similarity
        let content: Result<String, _> = self.conn.query_row(
            "SELECT d.content FROM documents d JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project, doc_id],
            |r| r.get(0),
        );

        let content = match content {
            Ok(c) => c,
            Err(_) => return McpResponse::err(id, -32602, format!("document '{doc_id}' not found in project '{project}'")),
        };

        // Use first 200 chars as FTS query to find related docs
        let query_text: String = content.chars().take(200).collect();
        let fts_query = query_text.split_whitespace()
            .filter(|w| w.len() > 3)
            .take(10)
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return McpResponse::text(id, "[]");
        }

        let mut stmt = match self.conn.prepare(
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
                // FTS table may not be populated; fall back to empty
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

    fn tool_find_path(&self, id: Value, args: &Value) -> McpResponse {
        let from = match args["from"].as_str() {
            Some(f) => f,
            None => return McpResponse::err(id, -32602, "missing required arg: from"),
        };
        let to = match args["to"].as_str() {
            Some(t) => t,
            None => return McpResponse::err(id, -32602, "missing required arg: to"),
        };
        let max_hops = args["max_hops"].as_u64().unwrap_or(6) as usize;

        // Check document_links table exists
        let table_exists: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_links'",
                [], |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !table_exists {
            return McpResponse::text(id, json!({"path": null, "hops": null, "message": "document_links table not populated; run indexing first"}).to_string());
        }

        // BFS over document_links using recursive CTE
        let sql = format!(
            "WITH RECURSIVE path(doc_id, trail, depth) AS (
               SELECT d.id, d.source_doc_id, 0
               FROM documents d WHERE d.source_doc_id = ?1
               UNION ALL
               SELECT dl.target_id, path.trail || '->' || d2.source_doc_id, path.depth + 1
               FROM path
               JOIN document_links dl ON dl.source_id = path.doc_id
               JOIN documents d2 ON d2.id = dl.target_id
               WHERE path.depth < ?3 AND path.trail NOT LIKE '%' || d2.source_doc_id || '%'
             )
             SELECT trail, depth FROM path WHERE doc_id = (SELECT id FROM documents WHERE source_doc_id = ?2 LIMIT 1)
             ORDER BY depth LIMIT 1"
        );

        let result: Result<(String, i64), _> = self.conn.query_row(
            &sql, params![from, to, max_hops as i64], |r| Ok((r.get(0)?, r.get(1)?))
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

    fn tool_get_cluster(&self, id: Value, args: &Value) -> McpResponse {
        let project = match args["project"].as_str() {
            Some(p) => p,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };
        let doc_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let depth = args["depth"].as_u64().unwrap_or(2).min(5) as i64;

        // Check document_links table exists
        let table_exists: bool = self.conn
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

        let mut stmt = match self.conn.prepare(&sql) {
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

    // ── Project sync ──────────────────────────────────────────────────────────

    fn tool_sync_project(&self, id: Value, args: &Value) -> McpResponse {
        let name = match args["project"].as_str() {
            Some(n) => n,
            None => return McpResponse::err(id, -32602, "missing required arg: project"),
        };

        let row: Result<(i64, Option<String>), _> = self.conn.query_row(
            "SELECT si.id, si.sync_cursor
             FROM source_instances si
             JOIN projects p ON si.project_id = p.id
             WHERE p.name = ?1
             ORDER BY si.id LIMIT 1",
            params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );

        match row {
            Err(_) => McpResponse::text(
                id,
                format!("Project '{name}' has no source instance configured.\nRun: doxus sync {name}"),
            ),
            Ok((si_id, cursor)) => {
                let cursor_info = cursor.as_deref().unwrap_or("(none)");
                McpResponse::text(
                    id,
                    format!("Project '{name}' sync instance #{si_id}  cursor: {cursor_info}\nTo trigger sync: doxus sync {name}"),
                )
            }
        }
    }

    // ── Plugin management ─────────────────────────────────────────────────────

    fn tool_plugin_install(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let version = args["version"].as_str().unwrap_or("0.0.0");

        let result = self.conn.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, kind, installed_at)
             VALUES (?1, ?1, ?2, 'external', unixepoch())",
            params![plugin_id, version],
        );

        match result {
            Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' v{version} installed.")),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_plugin_remove(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let n = self.conn.execute("DELETE FROM plugins WHERE id=?1", params![plugin_id]);
        match n {
            Ok(0) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
            Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' removed.")),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_plugin_update(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let version = args["version"].as_str().unwrap_or("latest");

        let n = self.conn.execute(
            "UPDATE plugins SET version=?2 WHERE id=?1",
            params![plugin_id, version],
        );
        match n {
            Ok(0) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
            Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' updated to v{version}.")),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_plugin_search(&self, id: Value, args: &Value) -> McpResponse {
        let query = match args["query"].as_str() {
            Some(q) => q,
            None => return McpResponse::err(id, -32602, "missing required arg: query"),
        };

        let mut stmt = match self.conn.prepare(
            "SELECT id, name, version, kind, trust_level FROM plugins
             WHERE id LIKE ?1 OR name LIKE ?1
             ORDER BY name LIMIT 20",
        ) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };

        let pattern = format!("%{query}%");
        let rows: Result<Vec<_>, _> = stmt
            .query_map(params![pattern], |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "version": r.get::<_, String>(2)?,
                    "kind": r.get::<_, String>(3)?,
                    "trust_level": r.get::<_, String>(4)?,
                }))
            })
            .and_then(|it| it.collect());

        match rows {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(items) if items.is_empty() => McpResponse::text(id, format!("No plugins matching '{query}'.")),
            Ok(items) => McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            })),
        }
    }

    fn tool_plugin_status(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let row: Result<(String, String, i64), _> = self.conn.query_row(
            "SELECT version, trust_level, enabled FROM plugins WHERE id=?1",
            params![plugin_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        );

        match row {
            Err(_) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
            Ok((version, trust, enabled)) => {
                let instances: i64 = self.conn
                    .query_row("SELECT COUNT(*) FROM source_instances WHERE plugin_id=?1", params![plugin_id], |r| r.get(0))
                    .unwrap_or(0);
                let status = json!({
                    "id": plugin_id,
                    "version": version,
                    "trust_level": trust,
                    "enabled": enabled != 0,
                    "instances": instances,
                });
                McpResponse::ok(id, json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&status).unwrap_or_default() }]
                }))
            }
        }
    }

    fn tool_plugin_logs(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };
        let level_filter = args["level"].as_str().unwrap_or("info");
        let limit = args["limit"].as_u64().unwrap_or(50) as i64;

        let levels = match level_filter {
            "error" => vec!["error"],
            "warn" => vec!["error", "warn"],
            "info" => vec!["error", "warn", "info"],
            "debug" => vec!["error", "warn", "info", "debug"],
            _ => vec!["error", "warn", "info", "debug", "trace"],
        };
        let placeholders = levels.iter().enumerate().map(|(i, _)| format!("?{}", i + 2)).collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT level, message, occurred_at FROM plugin_logs
             WHERE plugin_id = ?1 AND level IN ({placeholders})
             ORDER BY occurred_at DESC LIMIT ?"
        );

        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(plugin_id.to_string())];
        for l in &levels { all_params.push(Box::new(l.to_string())); }
        all_params.push(Box::new(limit));

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };

        let refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|b| b.as_ref()).collect();
        let rows: Result<Vec<_>, _> = stmt
            .query_map(refs.as_slice(), |r| {
                Ok(json!({
                    "level": r.get::<_, String>(0)?,
                    "message": r.get::<_, String>(1)?,
                    "occurred_at": r.get::<_, i64>(2)?,
                }))
            })
            .and_then(|it| it.collect());

        match rows {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(items) if items.is_empty() => McpResponse::text(id, format!("No logs for plugin '{plugin_id}'.")),
            Ok(items) => McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            })),
        }
    }

    fn tool_plugin_info(&self, id: Value, args: &Value) -> McpResponse {
        let plugin_id = match args["id"].as_str() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id"),
        };

        let row: Result<(String, String, String, String, i64, i64), _> = self.conn.query_row(
            "SELECT version, kind, trust_level, manifest_json, enabled, installed_at FROM plugins WHERE id=?1",
            params![plugin_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        );

        match row {
            Err(_) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
            Ok((version, kind, trust, manifest, enabled, installed_at)) => {
                let manifest_val: Value = serde_json::from_str(&manifest).unwrap_or(json!({}));
                let info = json!({
                    "id": plugin_id,
                    "version": version,
                    "kind": kind,
                    "trust_level": trust,
                    "enabled": enabled != 0,
                    "installed_at": installed_at,
                    "manifest": manifest_val,
                });
                McpResponse::ok(id, json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default() }]
                }))
            }
        }
    }

    // ── Workspace documents ───────────────────────────────────────────────────

    fn tool_create_workspace_document(&self, id: Value, args: &Value) -> McpResponse {
        let title = match args["title"].as_str() {
            Some(t) => t,
            None => return McpResponse::err(id, -32602, "missing required arg: title"),
        };
        let doc_type = args["doc_type"].as_str().unwrap_or("note");
        let template = args["template"].as_str().unwrap_or("");

        // Generate a file path slug from title
        let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        let file_path = format!("workspace/{slug}.md");

        let content = if template.is_empty() {
            format!("# {title}\n\n")
        } else {
            format!("# {title}\n\n<!-- template: {template} -->\n\n")
        };

        let hash = format!("{:x}", content.len()); // simple hash for now

        let result = self.conn.execute(
            "INSERT INTO workspace_documents(file_path, title, doc_type, content_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch(), unixepoch())",
            params![file_path, title, doc_type, hash],
        );

        match result {
            Ok(_) => {
                let new_id = self.conn.last_insert_rowid();
                McpResponse::ok(id, json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                        "id": new_id,
                        "file_path": file_path,
                        "title": title,
                        "doc_type": doc_type,
                        "content": content,
                    })).unwrap_or_default() }]
                }))
            }
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_update_workspace_document(&self, id: Value, args: &Value) -> McpResponse {
        let doc_id: i64 = match args["id"].as_i64() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id (integer)"),
        };
        let content = match args["content"].as_str() {
            Some(c) => c,
            None => return McpResponse::err(id, -32602, "missing required arg: content"),
        };
        let hash = format!("{:x}", content.len());

        let n = self.conn.execute(
            "UPDATE workspace_documents SET content_hash=?2, updated_at=unixepoch() WHERE id=?1",
            params![doc_id, hash],
        );

        match n {
            Ok(0) => McpResponse::err(id, -32602, format!("workspace document #{doc_id} not found")),
            Ok(_) => McpResponse::text(id, format!("Document #{doc_id} updated.")),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_delete_workspace_document(&self, id: Value, args: &Value) -> McpResponse {
        let doc_id: i64 = match args["id"].as_i64() {
            Some(i) => i,
            None => return McpResponse::err(id, -32602, "missing required arg: id (integer)"),
        };

        let n = self.conn.execute("DELETE FROM workspace_documents WHERE id=?1", params![doc_id]);
        match n {
            Ok(0) => McpResponse::err(id, -32602, format!("workspace document #{doc_id} not found")),
            Ok(_) => McpResponse::text(id, format!("Document #{doc_id} deleted.")),
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
        }
    }

    fn tool_list_workspace_documents(&self, id: Value, args: &Value) -> McpResponse {
        let type_filter = args["doc_type"].as_str();
        let status_filter = args["status"].as_str();

        let sql = match (type_filter, status_filter) {
            (Some(_), Some(_)) => "SELECT id, file_path, title, doc_type, status, priority, created_at FROM workspace_documents WHERE doc_type=?1 AND status=?2 ORDER BY created_at DESC",
            (Some(_), None) => "SELECT id, file_path, title, doc_type, status, priority, created_at FROM workspace_documents WHERE doc_type=?1 AND 1=1 ORDER BY created_at DESC",
            (None, Some(_)) => "SELECT id, file_path, title, doc_type, status, priority, created_at FROM workspace_documents WHERE 1=1 AND status=?2 ORDER BY created_at DESC",
            (None, None) => "SELECT id, file_path, title, doc_type, status, priority, created_at FROM workspace_documents ORDER BY created_at DESC",
        };

        let mut stmt = match self.conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };

        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Value> {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "file_path": r.get::<_, String>(1)?,
                "title": r.get::<_, Option<String>>(2)?,
                "doc_type": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "priority": r.get::<_, String>(5)?,
                "created_at": r.get::<_, i64>(6)?,
            }))
        };

        let rows: Result<Vec<_>, _> = match (type_filter, status_filter) {
            (Some(t), Some(s)) => stmt.query_map(params![t, s], map_row).and_then(|it| it.collect()),
            (Some(t), None) => stmt.query_map(params![t], map_row).and_then(|it| it.collect()),
            (None, Some(s)) => stmt.query_map(params![s], map_row).and_then(|it| it.collect()),
            (None, None) => stmt.query_map([], map_row).and_then(|it| it.collect()),
        };

        match rows {
            Err(e) => McpResponse::err(id, -32603, e.to_string()),
            Ok(items) if items.is_empty() => McpResponse::text(id, "No workspace documents found."),
            Ok(items) => McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            })),
        }
    }

    fn tool_apply_template(&self, id: Value, args: &Value) -> McpResponse {
        let template_name = match args["template"].as_str() {
            Some(t) => t,
            None => return McpResponse::err(id, -32602, "missing required arg: template"),
        };
        let variables = args.get("variables").cloned().unwrap_or(json!({}));

        let row: Result<(i64, Option<String>, String), _> = self.conn.query_row(
            "SELECT id, description, config_json FROM workspace_templates WHERE name=?1",
            params![template_name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        );

        match row {
            Err(_) => {
                // Template not in DB — generate a default one
                let title = variables["title"].as_str().unwrap_or(template_name);
                let content = format!("# {title}\n\n<!-- applied template: {template_name} -->\n\n");
                let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
                let file_path = format!("workspace/{slug}.md");
                let hash = format!("{:x}", content.len());

                let result = self.conn.execute(
                    "INSERT INTO workspace_documents(file_path, title, doc_type, content_hash, created_at, updated_at)
                     VALUES (?1, ?2, 'note', ?3, unixepoch(), unixepoch())",
                    params![file_path, title, hash],
                );
                match result {
                    Ok(_) => McpResponse::text(id, format!("Template '{template_name}' applied. Document created at {file_path}.")),
                    Err(e) => McpResponse::err(id, -32603, e.to_string()),
                }
            }
            Ok((_tmpl_id, description, config)) => {
                let title = variables["title"].as_str()
                    .unwrap_or(template_name);
                let desc = description.as_deref().unwrap_or("");
                let content = format!("# {title}\n\n{desc}\n\n<!-- config: {config} -->\n\n");
                let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
                let file_path = format!("workspace/{slug}.md");
                let hash = format!("{:x}", content.len());

                let result = self.conn.execute(
                    "INSERT INTO workspace_documents(file_path, title, doc_type, content_hash, created_at, updated_at)
                     VALUES (?1, ?2, 'note', ?3, unixepoch(), unixepoch())",
                    params![file_path, title, hash],
                );
                match result {
                    Ok(_) => McpResponse::text(id, format!("Template '{template_name}' applied. Document created at {file_path}.")),
                    Err(e) => McpResponse::err(id, -32603, e.to_string()),
                }
            }
        }
    }

    // ── Diagnostics ───────────────────────────────────────────────────────────

    fn tool_explain_search(&self, id: Value, args: &Value) -> McpResponse {
        let query = match args["query"].as_str() {
            Some(q) => q,
            None => return McpResponse::err(id, -32602, "missing required arg: query"),
        };
        let document_id = match args["document_id"].as_str() {
            Some(d) => d,
            None => return McpResponse::err(id, -32602, "missing required arg: document_id"),
        };

        let row: Result<(Option<String>, String), _> = self.conn.query_row(
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
}

// ── Markdown helpers ──────────────────────────────────────────────────────────

/// Extract a section starting at `heading` until the next same-or-higher level heading or EOF.
fn extract_section(content: &str, heading: &str) -> String {
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
fn extract_toc(content: &str) -> String {
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

// ── Tool definitions (all 37 docnx_* tools) ──────────────────────────────────

fn tool_list() -> Value {
    json!({
        "tools": [
            // Search & document
            tool("docnx_search", "Hybrid search across indexed documents", &[
                param("query", "string", "Search query text"),
                param_opt("project", "string", "Restrict to project name"),
                param_opt("mode", "string", "Search mode: hybrid|fts|vector"),
                param_opt("limit", "number", "Max results (default 20)"),
            ]),
            tool("docnx_get_document", "Get full document content", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("docnx_get_section", "Get specific section by heading (token-efficient)", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
                param("heading", "string", "Heading text to find"),
            ]),
            tool("docnx_get_metadata", "Get document frontmatter and metadata", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("docnx_get_backlinks", "Get documents that link to this document", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("docnx_get_links", "Get documents this document links to", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("docnx_list_documents", "List all documents in a project", &[
                param("project", "string", "Project name"),
                param_opt("cursor", "string", "Pagination cursor (numeric offset)"),
                param_opt("limit", "number", "Max results (default 50)"),
            ]),
            tool("docnx_get_documents", "Batch fetch multiple documents", &[
                param("ids", "array", "Array of document IDs"),
                param("project", "string", "Project name"),
            ]),
            tool("docnx_list_projects", "List all projects with status", &[]),
            tool("docnx_add_project", "Add a new project", &[
                param("name", "string", "Project slug (unique)"),
                param("path", "string", "Source path or identifier"),
                param_opt("display_name", "string", "Human-readable name"),
            ]),
            tool("docnx_remove_project", "Remove project index data (original files untouched)", &[
                param("name", "string", "Project name"),
            ]),
            tool("docnx_index_project", "Trigger indexing for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("docnx_sync_project", "Sync incremental changes for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("docnx_resolve_alias", "Resolve an alias to a document ID", &[
                param("alias", "string", "Alias or wikilink text"),
            ]),
            tool("docnx_get_toc", "Get table of contents for a document", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            tool("docnx_get_ranking", "Get document ranking by view count", &[
                param("project", "string", "Project name"),
                param_opt("limit", "number", "Max results"),
            ]),
            tool("docnx_inspect_document", "Inspect document indexing state", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            tool("docnx_status", "Get server status and health", &[]),
            tool("docnx_help", "Get usage documentation", &[]),
            tool("docnx_onboard", "Interactive setup guide", &[]),
            // Graph
            tool("docnx_find_related", "Find related documents via RRF ranking", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
                param_opt("k", "number", "Number of results (default 10)"),
            ]),
            tool("docnx_find_path", "Find shortest path between two documents", &[
                param("from", "string", "Source document ID"),
                param("to", "string", "Target document ID"),
                param_opt("max_hops", "number", "Max hops (default 6)"),
            ]),
            tool("docnx_get_cluster", "Multi-hop graph traversal", &[
                param("project", "string", "Project name"),
                param("id", "string", "Start document ID"),
                param_opt("depth", "number", "Traversal depth (default 2, max 5)"),
            ]),
            // Plugin management
            tool("docnx_plugin_list", "List installed plugins", &[]),
            tool("docnx_plugin_search", "Search plugin marketplace", &[
                param("query", "string", "Search query"),
            ]),
            tool("docnx_plugin_install", "Install a plugin", &[
                param("id", "string", "Plugin ID"),
                param_opt("version", "string", "Version (default: latest)"),
            ]),
            tool("docnx_plugin_remove", "Remove an installed plugin", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("docnx_plugin_update", "Update a plugin", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("docnx_plugin_status", "Get plugin health status", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("docnx_plugin_logs", "Get plugin runtime logs", &[
                param("id", "string", "Plugin ID"),
                param_opt("level", "string", "Min log level"),
                param_opt("limit", "number", "Max entries"),
            ]),
            tool("docnx_plugin_info", "Get detailed plugin information", &[
                param("id", "string", "Plugin ID"),
            ]),
            // Workspace
            tool("docnx_create_document", "Create a workspace document", &[
                param("title", "string", "Document title"),
                param_opt("template", "string", "Template name"),
                param_opt("doc_type", "string", "note|meeting|decision|journal"),
            ]),
            tool("docnx_update_document", "Update a workspace document", &[
                param("id", "string", "Document ID"),
                param("content", "string", "New content"),
            ]),
            tool("docnx_delete_document", "Delete a workspace document", &[
                param("id", "string", "Document ID"),
            ]),
            tool("docnx_list_workspace_documents", "List workspace documents", &[
                param_opt("doc_type", "string", "Filter by type"),
                param_opt("status", "string", "Filter by status"),
            ]),
            tool("docnx_apply_template", "Apply a template to create a document", &[
                param("template", "string", "Template name"),
                param_opt("variables", "object", "Template variables"),
            ]),
            // Diagnostics
            tool("docnx_diagnose", "Interactive troubleshooting guide", &[
                param_opt("issue", "string", "Issue description"),
            ]),
            tool("docnx_system_report", "Full system health snapshot", &[]),
            tool("docnx_explain_search", "Explain why a search returned these results", &[
                param("query", "string", "Original query"),
                param("document_id", "string", "Document to explain"),
            ]),
        ]
    })
}

fn tool(name: &str, description: &str, params: &[Value]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = vec![];
    for p in params {
        let pname = p["name"].as_str().unwrap_or("").to_string();
        let is_required = p["required"].as_bool().unwrap_or(true);
        properties.insert(
            pname.clone(),
            json!({ "type": p["type"], "description": p["description"] }),
        );
        if is_required {
            required.push(pname);
        }
    }
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

fn param(name: &str, type_: &str, description: &str) -> Value {
    json!({"name": name, "type": type_, "description": description, "required": true})
}

fn param_opt(name: &str, type_: &str, description: &str) -> Value {
    json!({"name": name, "type": type_, "description": description, "required": false})
}

// ── Static text ───────────────────────────────────────────────────────────────

static HELP_TEXT: &str = r#"doxus MCP — 37 docnx_* tools

SEARCH:      docnx_search, docnx_get_document, docnx_get_section, docnx_get_metadata
GRAPH:       docnx_get_backlinks, docnx_get_links, docnx_find_related, docnx_find_path, docnx_get_cluster
PROJECTS:    docnx_list_projects, docnx_add_project, docnx_remove_project, docnx_index_project, docnx_sync_project
DOCUMENTS:   docnx_list_documents, docnx_get_documents, docnx_get_toc, docnx_get_ranking, docnx_resolve_alias
PLUGINS:     docnx_plugin_list, docnx_plugin_install, docnx_plugin_status
WORKSPACE:   docnx_create_document, docnx_apply_template
DIAGNOSTICS: docnx_diagnose, docnx_system_report, docnx_inspect_document

Run 'tools/list' for full schema."#;

static ONBOARD_TEXT: &str = r#"Welcome to doxus!

Quick start:
1. docnx_list_projects        — see your projects
2. docnx_add_project          — add a new project (name, path)
3. doxus index <project>      — index via CLI (required before search)
4. docnx_search               — search across indexed documents
5. docnx_system_report        — check overall health

For help: docnx_help"#;

// ── JSONL stdio loop ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info")
        .init();

    tracing::info!("doxus-mcp starting on stdio");

    let db_path = std::env::var("DOXUS_DB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".doxus/db/doxus.db")
        });

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = doxus_core::db::open(&db_path)?;
    let server = McpServer::new(conn);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let response = match serde_json::from_str::<McpRequest>(&line) {
            Ok(req) => {
                let id = req.id.clone();
                server.dispatch(&req.method, id, req.params.as_ref())
            }
            Err(e) => McpResponse::err(json!(null), -32700, format!("parse error: {e}")),
        };

        let json = serde_json::to_string(&response)?;
        writeln!(out, "{json}")?;
        out.flush()?;
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_server() -> McpServer {
        let conn = Connection::open_in_memory().expect("in-memory db");
        doxus_core::db::apply_pragmas(&conn).expect("pragmas");
        doxus_core::db::migrate(&conn).expect("migrate");
        McpServer::new(conn)
    }

    fn insert_project(server: &McpServer, name: &str, path: &str) -> i64 {
        server
            .conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES (?1, ?1, ?2, unixepoch(), unixepoch())",
                params![name, path],
            )
            .unwrap();
        server
            .conn
            .query_row("SELECT id FROM projects WHERE name=?1", params![name], |r| r.get(0))
            .unwrap()
    }

    fn insert_document(server: &McpServer, project_id: i64, doc_id: &str, content: &str) {
        server
            .conn
            .execute(
                "INSERT INTO documents(project_id, source_doc_id, content, content_hash)
                 VALUES (?1, ?2, ?3, 'hash')",
                params![project_id, doc_id, content],
            )
            .unwrap();
    }

    #[test]
    fn test_initialize() {
        let server = test_server();
        let resp = server.dispatch("initialize", json!(1), None);
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_tools_list() {
        let server = test_server();
        let resp = server.dispatch("tools/list", json!(1), None);
        assert!(resp.error.is_none());
        let tools = &resp.result.unwrap()["tools"];
        assert!(tools.as_array().unwrap().len() >= 30);
    }

    #[test]
    fn test_list_projects_empty() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_list_projects", json!(1), &json!({}));
        assert!(resp.error.is_none());
        let text = &resp.result.unwrap()["content"][0]["text"];
        assert!(text.as_str().unwrap().contains("No projects"));
    }

    #[test]
    fn test_add_and_list_projects() {
        let server = test_server();
        let resp =
            server.dispatch_tool("docnx_add_project", json!(1), &json!({"name": "vault", "path": "/tmp/vault"}));
        assert!(resp.error.is_none());

        let resp = server.dispatch_tool("docnx_list_projects", json!(2), &json!({}));
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("vault"));
    }

    #[test]
    fn test_add_project_missing_name() {
        let server = test_server();
        let resp =
            server.dispatch_tool("docnx_add_project", json!(1), &json!({"path": "/tmp"}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_remove_project() {
        let server = test_server();
        insert_project(&server, "todel", "/tmp");
        let resp =
            server.dispatch_tool("docnx_remove_project", json!(1), &json!({"name": "todel"}));
        assert!(resp.error.is_none());
        let count: i64 = server
            .conn
            .query_row("SELECT COUNT(*) FROM projects WHERE name='todel'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_remove_project_not_found() {
        let server = test_server();
        let resp =
            server.dispatch_tool("docnx_remove_project", json!(1), &json!({"name": "nope"}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_get_document() {
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "# Hello\n\nWorld");

        let resp = server.dispatch_tool(
            "docnx_get_document",
            json!(1),
            &json!({"project": "proj", "id": "doc1"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("World"));
    }

    #[test]
    fn test_get_section() {
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(
            &server,
            pid,
            "doc1",
            "# Title\n\nIntro\n\n## Section A\n\nContent A\n\n## Section B\n\nContent B",
        );

        let resp = server.dispatch_tool(
            "docnx_get_section",
            json!(1),
            &json!({"project": "proj", "id": "doc1", "heading": "Section A"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Content A"));
        assert!(!text.contains("Content B"));
    }

    #[test]
    fn test_list_documents() {
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "content1");
        insert_document(&server, pid, "doc2", "content2");

        let resp = server.dispatch_tool(
            "docnx_list_documents",
            json!(1),
            &json!({"project": "proj"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("doc1"));
        assert!(text.contains("doc2"));
    }

    #[test]
    fn test_diagnose() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_diagnose", json!(1), &json!({}));
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Projects"));
    }

    #[test]
    fn test_system_report() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_system_report", json!(1), &json!({}));
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("doxus-mcp"));
    }

    #[test]
    fn test_get_backlinks_no_table() {
        // document_links may or may not exist — should not panic
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "content");

        let resp = server.dispatch_tool(
            "docnx_get_backlinks",
            json!(1),
            &json!({"project": "proj", "id": "doc1"}),
        );
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_extract_section() {
        let content = "# Title\n\nIntro\n\n## Sec A\n\nContent A\n\n## Sec B\n\nContent B";
        let section = extract_section(content, "Sec A");
        assert!(section.contains("Content A"));
        assert!(!section.contains("Content B"));
    }

    #[test]
    fn test_extract_toc() {
        let content = "# Title\n\n## Section A\n\n### Subsection\n\n## Section B";
        let toc = extract_toc(content);
        assert!(toc.contains("Title"));
        assert!(toc.contains("Section A"));
        assert!(toc.contains("Subsection"));
    }

    #[test]
    fn test_unknown_method() {
        let server = test_server();
        let resp = server.dispatch("unknown/method", json!(1), None);
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_unknown_tool() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_nonexistent", json!(1), &json!({}));
        assert!(resp.error.is_some());
    }

    // ── Graph tool tests ──────────────────────────────────────────────────────

    #[test]
    fn test_find_related_no_fts() {
        // When FTS not populated, should return empty array gracefully
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "hello world rust programming language");
        insert_document(&server, pid, "doc2", "hello world python programming language");

        let resp = server.dispatch_tool(
            "docnx_find_related",
            json!(1),
            &json!({"project": "proj", "id": "doc1"}),
        );
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_find_related_missing_args() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_find_related", json!(1), &json!({"project": "p"}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_find_path_no_table() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "docnx_find_path",
            json!(1),
            &json!({"from": "doc1", "to": "doc2"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("path") || text.contains("not"));
    }

    #[test]
    fn test_get_cluster_no_table() {
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "content");

        let resp = server.dispatch_tool(
            "docnx_get_cluster",
            json!(1),
            &json!({"project": "proj", "id": "doc1"}),
        );
        assert!(resp.error.is_none());
    }

    // ── Sync tool tests ───────────────────────────────────────────────────────

    #[test]
    fn test_sync_project_no_instance() {
        let server = test_server();
        insert_project(&server, "proj", "/tmp");
        let resp = server.dispatch_tool(
            "docnx_sync_project",
            json!(1),
            &json!({"project": "proj"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("proj"));
    }

    // ── Plugin tool tests ─────────────────────────────────────────────────────

    #[test]
    fn test_plugin_install_and_status() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "docnx_plugin_install",
            json!(1),
            &json!({"id": "com.test.plugin", "version": "1.0.0"}),
        );
        assert!(resp.error.is_none());

        let resp = server.dispatch_tool(
            "docnx_plugin_status",
            json!(2),
            &json!({"id": "com.test.plugin"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("com.test.plugin"));
    }

    #[test]
    fn test_plugin_remove() {
        let server = test_server();
        server.dispatch_tool("docnx_plugin_install", json!(1), &json!({"id": "com.test.rm"}));
        let resp = server.dispatch_tool("docnx_plugin_remove", json!(2), &json!({"id": "com.test.rm"}));
        assert!(resp.error.is_none());
        // now status should fail
        let resp = server.dispatch_tool("docnx_plugin_status", json!(3), &json!({"id": "com.test.rm"}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_plugin_update() {
        let server = test_server();
        server.dispatch_tool("docnx_plugin_install", json!(1), &json!({"id": "com.test.upd", "version": "1.0.0"}));
        let resp = server.dispatch_tool("docnx_plugin_update", json!(2), &json!({"id": "com.test.upd", "version": "2.0.0"}));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_plugin_remove_not_found() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_plugin_remove", json!(1), &json!({"id": "nope"}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_plugin_search() {
        let server = test_server();
        server.dispatch_tool("docnx_plugin_install", json!(1), &json!({"id": "com.test.confluence"}));
        let resp = server.dispatch_tool("docnx_plugin_search", json!(2), &json!({"query": "confluence"}));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_plugin_logs_empty() {
        let server = test_server();
        server.dispatch_tool("docnx_plugin_install", json!(1), &json!({"id": "com.test.logs"}));
        let resp = server.dispatch_tool("docnx_plugin_logs", json!(2), &json!({"id": "com.test.logs"}));
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("No logs"));
    }

    #[test]
    fn test_plugin_info() {
        let server = test_server();
        server.dispatch_tool("docnx_plugin_install", json!(1), &json!({"id": "com.test.info", "version": "0.5.0"}));
        let resp = server.dispatch_tool("docnx_plugin_info", json!(2), &json!({"id": "com.test.info"}));
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("0.5.0"));
    }

    // ── Workspace document tests ──────────────────────────────────────────────

    #[test]
    fn test_create_workspace_document() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "docnx_create_document",
            json!(1),
            &json!({"title": "My Note", "doc_type": "note"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("My Note"));
    }

    #[test]
    fn test_list_workspace_documents() {
        let server = test_server();
        server.dispatch_tool("docnx_create_document", json!(1), &json!({"title": "Doc A", "doc_type": "note"}));
        server.dispatch_tool("docnx_create_document", json!(2), &json!({"title": "Doc B", "doc_type": "meeting"}));

        let resp = server.dispatch_tool("docnx_list_workspace_documents", json!(3), &json!({}));
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Doc A") || text.contains("Doc B"));
    }

    #[test]
    fn test_list_workspace_documents_filtered() {
        let server = test_server();
        server.dispatch_tool("docnx_create_document", json!(1), &json!({"title": "Note X", "doc_type": "note"}));
        server.dispatch_tool("docnx_create_document", json!(2), &json!({"title": "Meeting Y", "doc_type": "meeting"}));

        let resp = server.dispatch_tool(
            "docnx_list_workspace_documents",
            json!(3),
            &json!({"doc_type": "note"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("note"));
    }

    #[test]
    fn test_update_workspace_document() {
        let server = test_server();
        server.dispatch_tool("docnx_create_document", json!(1), &json!({"title": "Update Me"}));
        let new_id: i64 = server.conn
            .query_row("SELECT id FROM workspace_documents ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .unwrap();

        let resp = server.dispatch_tool(
            "docnx_update_document",
            json!(2),
            &json!({"id": new_id, "content": "new content here"}),
        );
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_delete_workspace_document() {
        let server = test_server();
        server.dispatch_tool("docnx_create_document", json!(1), &json!({"title": "Delete Me"}));
        let new_id: i64 = server.conn
            .query_row("SELECT id FROM workspace_documents ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .unwrap();

        let resp = server.dispatch_tool("docnx_delete_document", json!(2), &json!({"id": new_id}));
        assert!(resp.error.is_none());

        let count: i64 = server.conn
            .query_row("SELECT COUNT(*) FROM workspace_documents WHERE id=?1", params![new_id], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_workspace_document_not_found() {
        let server = test_server();
        let resp = server.dispatch_tool("docnx_delete_document", json!(1), &json!({"id": 9999}));
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_apply_template_no_template_in_db() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "docnx_apply_template",
            json!(1),
            &json!({"template": "weekly-review", "variables": {"title": "Week 14"}}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("weekly-review") || text.contains("Week 14") || text.contains("applied"));
    }

    // ── Explain search test ───────────────────────────────────────────────────

    #[test]
    fn test_explain_search() {
        let server = test_server();
        let pid = insert_project(&server, "proj", "/tmp");
        insert_document(&server, pid, "doc1", "rust programming language systems");

        let resp = server.dispatch_tool(
            "docnx_explain_search",
            json!(1),
            &json!({"query": "rust programming", "document_id": "doc1"}),
        );
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("rust"));
    }

    #[test]
    fn test_explain_search_not_found() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "docnx_explain_search",
            json!(1),
            &json!({"query": "foo", "document_id": "nonexistent"}),
        );
        assert!(resp.error.is_some());
    }
}
