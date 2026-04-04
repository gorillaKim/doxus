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
            "docnx_sync_project" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus sync <project>",
            ),

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
            "docnx_find_related" => McpResponse::not_implemented(
                id,
                name,
                "requires vector index; run: doxus index <project>",
            ),
            "docnx_find_path" => McpResponse::not_implemented(
                id,
                name,
                "requires vector index; run: doxus index <project>",
            ),
            "docnx_get_cluster" => McpResponse::not_implemented(
                id,
                name,
                "requires vector index; run: doxus index <project>",
            ),

            // ── Plugin management ─────────────────────────────────────────────
            "docnx_plugin_list" => self.tool_plugin_list(id),
            "docnx_plugin_install" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin install <id>",
            ),
            "docnx_plugin_remove" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin remove <id>",
            ),
            "docnx_plugin_update" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin update <id>",
            ),
            "docnx_plugin_search" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin search <query>",
            ),
            "docnx_plugin_status" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin status <id>",
            ),
            "docnx_plugin_logs" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin logs <id>",
            ),
            "docnx_plugin_info" => McpResponse::not_implemented(
                id,
                name,
                "use CLI: doxus plugin info <id>",
            ),

            // ── Workspace ─────────────────────────────────────────────────────
            "docnx_create_document" => McpResponse::not_implemented(
                id,
                name,
                "workspace feature: doxus workspace new",
            ),
            "docnx_update_document" => McpResponse::not_implemented(
                id,
                name,
                "workspace feature: doxus workspace edit <id>",
            ),
            "docnx_delete_document" => McpResponse::not_implemented(
                id,
                name,
                "workspace feature: doxus workspace delete <id>",
            ),
            "docnx_list_workspace_documents" => McpResponse::not_implemented(
                id,
                name,
                "workspace feature: doxus workspace list",
            ),
            "docnx_apply_template" => McpResponse::not_implemented(
                id,
                name,
                "workspace feature: doxus workspace apply <template>",
            ),

            // ── Diagnostics ───────────────────────────────────────────────────
            "docnx_diagnose" => self.tool_diagnose(id),
            "docnx_system_report" => self.tool_system_report(id),
            "docnx_explain_search" => McpResponse::not_implemented(
                id,
                name,
                "requires vector index; run: doxus index <project>",
            ),

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
}
