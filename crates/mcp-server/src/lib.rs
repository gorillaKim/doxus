//! doxus MCP server - Model Context Protocol integration
//!
//! Exposes doxus_* tools for AI agent integration.

pub mod sync_loop;
pub mod retry_tracker;
pub mod http_server;

pub mod types;
pub use types::{McpRequest, McpResponse, McpError};

pub mod server;
pub use server::McpServer;

pub mod dispatch;
pub mod tools;

use serde_json::Value;

// Provide the dispatch methods directly on McpServer for backwards compatibility with main.rs
impl McpServer {
    pub async fn dispatch(&self, method: &str, id: Value, params: Option<&Value>) -> McpResponse {
        dispatch::dispatch(self, method, id, params).await
    }

    pub async fn dispatch_tool(&self, name: &str, id: Value, args: &Value) -> McpResponse {
        dispatch::dispatch_tool(self, name, id, args).await
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn test_server() -> McpServer {
        doxus_core::db::ensure_vec_extension();
        let conn = Connection::open_in_memory().expect("in-memory db");
        doxus_core::db::apply_pragmas(&conn).expect("pragmas");
        doxus_core::db::create_vec0_table(&conn).expect("vec0 table");
        doxus_core::db::migrate(&conn).expect("migrate");
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(std::path::PathBuf::from("/tmp/doxus-pm")));
        McpServer::new(Arc::new(Mutex::new(conn)), std::path::PathBuf::from(":memory:"), None, pm, std::path::PathBuf::from("/tmp/doxus-test-plugins"))
    }

    fn insert_project(server: &McpServer, name: &str, path: &str) {
        server
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO projects (name, display_name, path, source_project_id, created_at, updated_at) VALUES (?1, ?1, ?2, ?1, 0, 0)",
                params![name, path],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn test_initialize() {
        let server = test_server();
        let resp = server.dispatch("initialize", json!(1), None).await;
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_tools_list() {
        let server = test_server();
        let resp = server.dispatch("tools/list", json!(1), None).await;
        assert!(resp.error.is_none());
        let tools = &resp.result.unwrap()["tools"];
        assert!(tools.as_array().unwrap().len() >= 30);
    }

    #[tokio::test]
    async fn test_list_projects_empty() {
        let server = test_server();
        let resp = server.dispatch_tool("doxus_list_projects", json!(1), &json!({})).await;
        assert!(resp.error.is_none());
        let text = &resp.result.unwrap()["content"][0]["text"];
        assert!(text.as_str().unwrap().contains("No projects"));
    }

    #[tokio::test]
    async fn test_add_and_list_projects() {
        let server = test_server();
        let resp =
            server.dispatch_tool("doxus_add_project", json!(1), &json!({"name": "vault", "path": "/tmp/vault"})).await;
        assert!(resp.error.is_none());

        let resp = server.dispatch_tool("doxus_list_projects", json!(2), &json!({})).await;
        let text = resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("vault"));
    }

    #[tokio::test]
    async fn test_add_project_missing_name() {
        let server = test_server();
        let resp =
            server.dispatch_tool("doxus_add_project", json!(1), &json!({"path": "/tmp"})).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_remove_project() {
        let server = test_server();
        insert_project(&server, "todel", "/tmp");
        let resp =
            server.dispatch_tool("doxus_remove_project", json!(1), &json!({"name": "todel"})).await;
        assert!(resp.error.is_none());
        let count: i64 = server
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM projects WHERE name='todel'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_remove_project_not_found() {
        let server = test_server();
        let resp =
            server.dispatch_tool("doxus_remove_project", json!(1), &json!({"name": "ghost"})).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let server = test_server();
        insert_project(&server, "proj", "/tmp");
        let resp = server.dispatch_tool("doxus_search", json!(1), &json!({"query": "zzznoresults"})).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_search_project_not_found() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "doxus_search",
            json!(1),
            &json!({"query": "test", "project": "ghost"}),
        ).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_create_document_external_project_failed_if_not_exists() {
        let server = test_server();
        let resp = server.dispatch_tool(
            "doxus_create_document",
            json!(1),
            &json!({"title": "Test", "project": "ghost"}),
        ).await;
        // Should fail because project ghost doesn't exist in DB
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_create_document_external_project_failed_if_no_write_support() {
        let server = test_server();
        insert_project(&server, "ext-proj", "/tmp/ext");
        // By default, PluginManager has no factories, so loading source for project will fail or return None.
        // Wait, if source loading fails, tool should return error.
        let resp = server.dispatch_tool(
            "doxus_create_document",
            json!(1),
            &json!({"title": "Test", "project": "ext-proj"}),
        ).await;
        assert!(resp.error.is_some());
    }
}
