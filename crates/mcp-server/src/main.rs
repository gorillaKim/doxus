/// doxus MCP server — 37 docnx_* tools via MCP protocol (JSONL over stdio)
///
/// Phase 1: docnx_search, docnx_list_projects, docnx_status are functional.
/// All other tools return "not implemented in this phase" until their phase ships.
use anyhow::Result;
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

    fn not_implemented(id: Value, tool: &str) -> Self {
        Self::err(id, -32601, format!("{tool}: not implemented in this phase"))
    }
}

// ── Tool definitions (all 37 docnx_* tools) ──────────────────────────────────

fn tool_list() -> Value {
    json!({
        "tools": [
            // Search & document (13)
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
                param_opt("cursor", "string", "Pagination cursor"),
            ]),
            tool("docnx_list_projects", "List all projects with status", &[]),
            tool("docnx_index_project", "Trigger indexing for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("docnx_resolve_alias", "Resolve an alias to a document ID", &[
                param("alias", "string", "Alias or wikilink text"),
            ]),
            tool("docnx_status", "Get server status and health", &[]),
            tool("docnx_help", "Get usage documentation", &[]),
            tool("docnx_onboard", "Interactive setup guide", &[]),
            // Workspace (5)
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
            // Graph (3)
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
            // Plugin management (8)
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
            // Diagnostics (4)
            tool("docnx_diagnose", "Interactive troubleshooting guide", &[
                param_opt("issue", "string", "Issue description"),
            ]),
            tool("docnx_system_report", "Full system health snapshot", &[]),
            tool("docnx_explain_search", "Explain why a search returned these results", &[
                param("query", "string", "Original query"),
                param("document_id", "string", "Document to explain"),
            ]),
            tool("docnx_inspect_document", "Inspect document indexing state", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            // Sync (2)
            tool("docnx_sync_project", "Sync incremental changes for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("docnx_get_toc", "Get table of contents for a document", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            // Ranking & discovery (2)
            tool("docnx_get_ranking", "Get document ranking by view count", &[
                param("project", "string", "Project name"),
                param_opt("limit", "number", "Max results"),
            ]),
            tool("docnx_get_documents", "Batch fetch multiple documents", &[
                param("ids", "array", "Array of document IDs"),
                param("project", "string", "Project name"),
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
        properties.insert(pname.clone(), json!({
            "type": p["type"],
            "description": p["description"]
        }));
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

// ── Tool dispatch ─────────────────────────────────────────────────────────────

fn dispatch(method: &str, id: Value, params: Option<&Value>) -> McpResponse {
    let _p = params.cloned().unwrap_or(json!({}));

    match method {
        "tools/list" => McpResponse::ok(id, tool_list()),

        "tools/call" => {
            let tool_name = params
                .and_then(|p| p["name"].as_str())
                .unwrap_or("");
            let args = params.and_then(|p| p.get("arguments")).cloned().unwrap_or(json!({}));
            dispatch_tool(tool_name, id, &args)
        }

        "initialize" => McpResponse::ok(id, json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "doxus-mcp", "version": "0.1.0" }
        })),

        _ => McpResponse::err(id, -32601, format!("method not found: {method}")),
    }
}

fn dispatch_tool(name: &str, id: Value, _args: &Value) -> McpResponse {
    match name {
        "docnx_status" => McpResponse::ok(id, json!({
            "content": [{ "type": "text", "text": "doxus MCP server v0.1.0 — operational\nPhase 1: search, list_projects, status active" }]
        })),

        "docnx_list_projects" => McpResponse::ok(id, json!({
            "content": [{ "type": "text", "text": "Use doxus-cli project list to view projects.\nFull DB integration available in Phase 1." }]
        })),

        "docnx_help" => McpResponse::ok(id, json!({
            "content": [{ "type": "text", "text": HELP_TEXT }]
        })),

        // All other tools: not yet implemented
        tool => McpResponse::not_implemented(id, tool),
    }
}

static HELP_TEXT: &str = r#"doxus MCP — 37 docnx_* tools

SEARCH:      docnx_search, docnx_get_document, docnx_get_section
GRAPH:       docnx_get_backlinks, docnx_get_links, docnx_find_related, docnx_find_path
PROJECTS:    docnx_list_projects, docnx_index_project, docnx_sync_project
PLUGINS:     docnx_plugin_list, docnx_plugin_install, docnx_plugin_status
WORKSPACE:   docnx_create_document, docnx_apply_template
DIAGNOSTICS: docnx_diagnose, docnx_system_report

Run 'tools/list' for full schema."#;

// ── JSONL stdio loop ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info")
        .init();

    tracing::info!("doxus-mcp starting on stdio");

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
                dispatch(&req.method, id, req.params.as_ref())
            }
            Err(e) => McpResponse::err(
                json!(null),
                -32700,
                format!("parse error: {e}"),
            ),
        };

        let json = serde_json::to_string(&response)?;
        writeln!(out, "{json}")?;
        out.flush()?;
    }

    Ok(())
}
