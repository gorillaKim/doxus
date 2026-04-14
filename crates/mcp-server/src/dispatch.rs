use crate::server::McpServer;
use crate::tools;
use crate::types::McpResponse;
use serde_json::{json, Value};

pub fn dispatch(
    server: &McpServer,
    method: &str,
    id: Value,
    params: Option<&Value>,
) -> McpResponse {
    match method {
        "tools/list" => McpResponse::ok(id, tool_list()),

        "tools/call" => {
            let name = params.and_then(|p| p["name"].as_str()).unwrap_or("");
            let args = params
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));
            dispatch_tool(server, name, id, &args)
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

pub fn dispatch_tool(server: &McpServer, name: &str, id: Value, args: &Value) -> McpResponse {
    match name {
        // ── Core tools ────────────────────────────────────────────────────
        "doxus_status" => tools::core::status(server, id),
        "doxus_help" => McpResponse::text(id, HELP_TEXT),
        "doxus_onboard" => McpResponse::text(id, ONBOARD_TEXT),

        // ── Project management ────────────────────────────────────────────
        "doxus_list_projects" => tools::project::list_projects(server, id),
        "doxus_add_project" => tools::project::add_project(server, id, args),
        "doxus_remove_project" => tools::project::remove_project(server, id, args),
        "doxus_index_project" => tools::project::index_project(server, id, args),
        "doxus_sync_project" => tools::project::sync_project(server, id, args),

        // ── Search & documents ────────────────────────────────────────────
        "doxus_search" => tools::search::search(server, id, args),
        "doxus_get_document" => tools::search::get_document(server, id, args),
        "doxus_get_section" => tools::search::get_section(server, id, args),
        "doxus_get_metadata" => tools::search::get_metadata(server, id, args),
        "doxus_list_documents" => tools::search::list_documents(server, id, args),
        "doxus_get_documents" => tools::search::get_documents(server, id, args),
        "doxus_resolve_alias" => tools::search::resolve_alias(server, id, args),
        "doxus_get_toc" => tools::search::get_toc(server, id, args),
        "doxus_get_ranking" => tools::search::get_ranking(server, id, args),
        "doxus_inspect_document" => tools::search::inspect_document(server, id, args),

        // ── Graph ─────────────────────────────────────────────────────────
        "doxus_get_backlinks" => tools::graph::get_backlinks(server, id, args),
        "doxus_get_links" => tools::graph::get_links(server, id, args),
        "doxus_find_related" => tools::graph::find_related(server, id, args),
        "doxus_find_path" => tools::graph::find_path(server, id, args),
        "doxus_get_cluster" => tools::graph::get_cluster(server, id, args),

        // ── Plugin management ─────────────────────────────────────────────
        "doxus_plugin_list" => tools::plugin::list(server, id),
        "doxus_plugin_install" => tools::plugin::install(server, id, args),
        "doxus_plugin_remove" => tools::plugin::remove(server, id, args),
        "doxus_plugin_update" => tools::plugin::update(server, id, args),
        "doxus_plugin_search" => tools::plugin::search(server, id, args),
        "doxus_plugin_status" => tools::plugin::status(server, id, args),
        "doxus_plugin_logs" => tools::plugin::logs(server, id, args),
        "doxus_plugin_info" => tools::plugin::info(server, id, args),

        // ── Workspace ─────────────────────────────────────────────────────
        "doxus_create_document" => tools::workspace::create_document(server, id, args),
        "doxus_update_document" => tools::workspace::update_document(server, id, args),
        "doxus_delete_document" => tools::workspace::delete_document(server, id, args),
        "doxus_list_workspace_documents" => tools::workspace::list_documents(server, id, args),
        "doxus_apply_template" => tools::workspace::apply_template(server, id, args),
        "doxus_list_templates" => tools::workspace::list_templates(server, id),
        "doxus_get_template" => tools::workspace::get_template(server, id, args),

        // ── Diagnostics ───────────────────────────────────────────────────
        "doxus_diagnose" => tools::core::diagnose(server, id),
        "doxus_system_report" => tools::core::system_report(server, id),
        "doxus_explain_search" => tools::core::explain_search(server, id, args),

        unknown => McpResponse::err(id, -32601, format!("unknown tool: {unknown}")),
    }
}

pub fn tool_list() -> Value {
    json!({
        "tools": [
            // Search & document
            tool("doxus_search", "Hybrid search across indexed documents", &[
                param("query", "string", "Search query text"),
                param_opt("project", "string", "Restrict to project name"),
                param_opt("mode", "string", "Search mode: hybrid|fts|vector"),
                param_opt("limit", "number", "Max results (default 20)"),
            ]),
            tool("doxus_get_document", "Get full document content", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("doxus_get_section", "Get specific section by heading (token-efficient)", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
                param("heading", "string", "Heading text to find"),
            ]),
            tool("doxus_get_metadata", "Get document frontmatter and metadata", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("doxus_get_backlinks", "Get documents that link to this document", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("doxus_get_links", "Get documents this document links to", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
            ]),
            tool("doxus_list_documents", "List all documents in a project", &[
                param("project", "string", "Project name"),
                param_opt("cursor", "string", "Pagination cursor (numeric offset)"),
                param_opt("limit", "number", "Max results (default 50)"),
            ]),
            tool("doxus_get_documents", "Batch fetch multiple documents", &[
                param("ids", "array", "Array of document IDs"),
                param("project", "string", "Project name"),
            ]),
            tool("doxus_list_projects", "List all projects with status", &[]),
            tool("doxus_add_project", "Add a new project", &[
                param("name", "string", "Project slug (unique)"),
                param("path", "string", "Source path or identifier"),
                param_opt("display_name", "string", "Human-readable name"),
            ]),
            tool("doxus_remove_project", "Remove project index data (original files untouched)", &[
                param("name", "string", "Project name"),
            ]),
            tool("doxus_index_project", "Trigger indexing for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("doxus_sync_project", "Sync incremental changes for a project", &[
                param("project", "string", "Project name"),
            ]),
            tool("doxus_resolve_alias", "Resolve an alias to a document ID", &[
                param("alias", "string", "Alias or wikilink text"),
            ]),
            tool("doxus_get_toc", "Get table of contents for a document", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            tool("doxus_get_ranking", "Get document ranking by view count", &[
                param("project", "string", "Project name"),
                param_opt("limit", "number", "Max results"),
            ]),
            tool("doxus_inspect_document", "Inspect document indexing state", &[
                param("project", "string", "Project name"),
                param("id", "string", "Document ID"),
            ]),
            tool("doxus_status", "Get server status and health", &[]),
            tool("doxus_help", "Get usage documentation", &[]),
            tool("doxus_onboard", "Interactive setup guide", &[]),
            // Graph
            tool("doxus_find_related", "Find related documents via RRF ranking", &[
                param("project", "string", "Project name"),
                param("id", "string", "Source document ID"),
                param_opt("k", "number", "Number of results (default 10)"),
            ]),
            tool("doxus_find_path", "Find shortest path between two documents", &[
                param("from", "string", "Source document ID"),
                param("to", "string", "Target document ID"),
                param_opt("max_hops", "number", "Max hops (default 6)"),
            ]),
            tool("doxus_get_cluster", "Multi-hop graph traversal", &[
                param("project", "string", "Project name"),
                param("id", "string", "Start document ID"),
                param_opt("depth", "number", "Traversal depth (default 2, max 5)"),
            ]),
            // Plugin management
            tool("doxus_plugin_list", "List installed plugins", &[]),
            tool("doxus_plugin_search", "Search plugin marketplace", &[
                param("query", "string", "Search query"),
            ]),
            tool("doxus_plugin_install", "Install a plugin", &[
                param("id", "string", "Plugin ID"),
                param_opt("version", "string", "Version (default: latest)"),
            ]),
            tool("doxus_plugin_remove", "Remove an installed plugin", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("doxus_plugin_update", "Update a plugin", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("doxus_plugin_status", "Get plugin health status", &[
                param("id", "string", "Plugin ID"),
            ]),
            tool("doxus_plugin_logs", "Get plugin runtime logs", &[
                param("id", "string", "Plugin ID"),
                param_opt("level", "string", "Min log level"),
                param_opt("limit", "number", "Max entries"),
            ]),
            tool("doxus_plugin_info", "Get detailed plugin information", &[
                param("id", "string", "Plugin ID"),
            ]),
            // Workspace
            tool("doxus_create_document", "Create a workspace document", &[
                param("title", "string", "Document title"),
                param_opt("template", "string", "Template name"),
                param_opt("doc_type", "string", "note|meeting|decision|journal"),
            ]),
            tool("doxus_update_document", "Update a workspace document", &[
                param("id", "string", "Document ID"),
                param("content", "string", "New content"),
            ]),
            tool("doxus_delete_document", "Delete a workspace document", &[
                param("id", "string", "Document ID"),
            ]),
            tool("doxus_list_workspace_documents", "List workspace documents", &[
                param_opt("doc_type", "string", "Filter by type"),
                param_opt("status", "string", "Filter by status"),
            ]),
            tool("doxus_apply_template", "Apply a template to create a document with frontmatter auto-generated", &[
                param("template", "string", "Template name (use doxus_list_templates to discover)"),
                param_opt("variables", "object", "Template variables (use doxus_get_template to see required variables)"),
            ]),
            tool("doxus_list_templates", "List all available templates (builtin + custom). Returns name and description only — use doxus_get_template for content and variables.", &[]),
            tool("doxus_get_template", "Get a template's content and variable list. Variables are auto-extracted from {{placeholder}} syntax.", &[
                param("name", "string", "Template name"),
            ]),
            // Diagnostics
            tool("doxus_diagnose", "Interactive troubleshooting guide", &[
                param_opt("issue", "string", "Issue description"),
            ]),
            tool("doxus_system_report", "Full system health snapshot", &[]),
            tool("doxus_explain_search", "Explain why a search returned these results", &[
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

static HELP_TEXT: &str = r#"doxus MCP — 37 doxus_* tools

SEARCH:      doxus_search, doxus_get_document, doxus_get_section, doxus_get_metadata
GRAPH:       doxus_get_backlinks, doxus_get_links, doxus_find_related, doxus_find_path, doxus_get_cluster
PROJECTS:    doxus_list_projects, doxus_add_project, doxus_remove_project, doxus_index_project, doxus_sync_project
DOCUMENTS:   doxus_list_documents, doxus_get_documents, doxus_get_toc, doxus_get_ranking, doxus_resolve_alias
PLUGINS:     doxus_plugin_list, doxus_plugin_install, doxus_plugin_status
WORKSPACE:   doxus_create_document, doxus_apply_template
DIAGNOSTICS: doxus_diagnose, doxus_system_report, doxus_inspect_document

Run 'tools/list' for full schema."#;

static ONBOARD_TEXT: &str = r#"Welcome to doxus!

Quick start:
1. doxus_list_projects        — see your projects
2. doxus_add_project          — add a new project (name, path)
3. doxus index <project>      — index via CLI (required before search)
4. doxus_search               — search across indexed documents
5. doxus_system_report        — check overall health

For help: doxus_help"#;
