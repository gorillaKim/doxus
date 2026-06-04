// use rusqlite::Connection;
use serde_json::json;
use std::sync::Arc;
use doxus_mcp::McpServer;
use async_trait::async_trait;
use doxus_plugin_sdk::{DocSource, PluginConfig, PluginSecrets, SourceDocId, RawDocument, ContentType, PluginMetadata, PluginKind, Capabilities, SyncPolicy, FetchAllOpts, DocumentStream, HealthStatus, PluginError};

struct MockWriteSource {
    meta: PluginMetadata,
}

#[async_trait]
impl DocSource for MockWriteSource {
    fn metadata(&self) -> &PluginMetadata { &self.meta }
    fn capabilities(&self) -> Capabilities { Capabilities { incremental_sync: false, oauth: false, native_search: false, sync_policy: SyncPolicy::Manual } }
    async fn validate_config(&self, _: &PluginConfig) -> Result<(), PluginError> { Ok(()) }
    async fn initialize(&mut self, _: PluginConfig, _: PluginSecrets) -> Result<(), PluginError> { Ok(()) }
    async fn fetch_all(&self, _: FetchAllOpts) -> Result<DocumentStream, PluginError> { 
        Ok(DocumentStream { documents: vec![], next_cursor: None, estimated_total: None }) 
    }
    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        Ok(RawDocument {
            id: id.clone(),
            title: Some("Mock Title".into()),
            content: "Mock Content".into(),
            content_type: ContentType::Markdown,
            url: None,
            metadata: Default::default(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: None,
        })
    }
    async fn health_check(&self) -> HealthStatus { HealthStatus { healthy: true, message: None } }
    
    fn supports_write(&self) -> bool { true }
    
    async fn create_document(&self, _title: &str, _content: &str, _folder: Option<&str>, _metadata: Option<&std::collections::HashMap<String, serde_json::Value>>) -> Result<SourceDocId, PluginError> {
        Ok(SourceDocId("mock-id.md".into()))
    }

    async fn update_document(&self, _id: &SourceDocId, _content: Option<&str>, _metadata: Option<&std::collections::HashMap<String, serde_json::Value>>) -> Result<(), PluginError> {
        Ok(())
    }

    async fn delete_document(&self, _id: &SourceDocId) -> Result<(), PluginError> {
        Ok(())
    }
}

fn setup_server() -> McpServer {
    let db_path = std::path::PathBuf::from("/tmp/doxus-test-writeback.sqlite");
    if db_path.exists() {
        let _ = std::fs::remove_file(&db_path);
    }
    
    let pool = doxus_core::db::create_pool(&db_path).unwrap();
    {
        let conn = pool.get().unwrap();
        // Seed project with 'mock-source'
        conn.execute(
            "INSERT INTO projects (name, display_name, path, source_type, source_project_id, status, config_json, is_default, created_at, updated_at) VALUES ('test-proj', 'Test Project', '/tmp', 'mock-plugin', 'test-proj', 'active', '{\"fields\":{}}', 1, 0, 0)",
            []
        ).unwrap();
        let project_id: i64 = conn.last_insert_rowid();

        // Must satisfy foreign key from source_instances to plugins
        conn.execute(
            "INSERT INTO plugins (id, name, version, installed_at) VALUES ('mock-plugin', 'Mock', '1.0.0', 0)",
            []
        ).unwrap();

        conn.execute(
            "INSERT INTO source_instances (project_id, plugin_id, name, config_json, created_at) VALUES (?1, 'mock-plugin', 'Mock Instance', '{\"fields\":{}}', 0)",
            rusqlite::params![project_id]
        ).unwrap();
    }

    let mut pm = doxus_core::plugin::PluginManager::new(std::path::PathBuf::from("/tmp/doxus"));
    pm.register_factory("mock-plugin", || {
        Box::new(MockWriteSource { 
            meta: PluginMetadata {
                id: "mock-plugin".into(),
                name: "Mock Plugin".into(),
                version: "1.0.0".into(),
                kind: PluginKind::Builtin,
            }
        })
    });
    
    McpServer::new(pool, db_path, None, Arc::new(pm), std::path::PathBuf::from("/tmp/plugins"))
}

#[tokio::test]
async fn test_create_document_with_immediate_sync() {
    let server = setup_server();
    
    // Call doxus_create_document
    let resp = server.dispatch_tool(
        "doxus_create_document",
        json!(1),
        &json!({
            "title": "New Doc",
            "project": "test-proj"
        })
    ).await;
    
    assert!(resp.error.is_none(), "Tool call failed: {:?}", resp.error);
    
    // Check if document exists in DB (Immediate Sync verification with retry loop to allow async background indexing)
    let conn = server.conn();
    let mut doc_count = 0;
    for _ in 0..40 {
        let c = conn.read_conn().unwrap();
        doc_count = c.query_row(
            "SELECT COUNT(*) FROM documents WHERE source_doc_id = 'mock-id.md'",
            [],
            |r: &rusqlite::Row<'_>| r.get::<_, Option<i64>>(0),
        ).unwrap().unwrap_or(0);
        if doc_count == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    
    assert_eq!(doc_count, 1, "Document should be synced to DB immediately after creation");
}
