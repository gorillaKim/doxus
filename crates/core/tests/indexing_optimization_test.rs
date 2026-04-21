use async_trait::async_trait;
use doxus_core::plugin::manager::PluginManager;
use doxus_core::search::SearchEngine;
use doxus_core::indexing::IndexingService;
use doxus_core::embedding::MockEmbedder;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, DocSource, DocumentStream, FetchAllOpts, FetchChangesOpts,
    HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata, PluginSecrets, RawDocument,
    SourceDocId,
};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

struct MockSource {
    meta: PluginMetadata,
    docs: Arc<Mutex<Vec<RawDocument>>>,
}

#[async_trait]
impl DocSource for MockSource {
    fn metadata(&self) -> &PluginMetadata { &self.meta }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: true,
            oauth: false,
            native_search: false,
            sync_policy: doxus_plugin_sdk::SyncPolicy::OnFocus,
        }
    }
    async fn validate_config(&self, _config: &PluginConfig) -> Result<(), PluginError> { Ok(()) }
    async fn initialize(&mut self, _config: PluginConfig, _secrets: PluginSecrets) -> Result<(), PluginError> { Ok(()) }
    async fn fetch_all(&self, _opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        Ok(DocumentStream {
            documents: self.docs.lock().unwrap().clone(),
            next_cursor: None,
            estimated_total: None,
        })
    }
    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Ok(ChangeSet { updated: vec![], deleted_ids: vec![], next_cursor: None })
    }
    async fn fetch_document(&self, _id: &SourceDocId) -> Result<RawDocument, PluginError> {
        Err(PluginError::NotFound("mock".into()))
    }
    async fn health_check(&self) -> HealthStatus { HealthStatus { healthy: true, message: None } }
}

#[tokio::test]
async fn test_indexing_skip_unchanged_documents() {
    doxus_core::db::ensure_vec_extension();
    let conn_raw = rusqlite::Connection::open_in_memory().unwrap();
    doxus_core::db::apply_pragmas(&conn_raw).unwrap();
    doxus_core::db::create_vec0_table(&conn_raw).unwrap();
    doxus_core::db::migrate(&conn_raw).unwrap();
    
    let conn = Arc::new(Mutex::new(conn_raw));
    
    // 1. Setup plugin and project
    {
        let c = conn.lock().unwrap();
        // Register plugin first
        c.execute(
            "INSERT INTO plugins (id, name, version, kind, trust_level, manifest_json, installed_at)
             VALUES ('mock.plugin', 'Mock Plugin', '0.1.0', 'external', 'verified', '{}', 100)",
            []
        ).unwrap();

        c.execute(
            "INSERT INTO projects (name, display_name, status, path, storage_strategy, created_at, updated_at) 
             VALUES ('test-project', 'Test Project', 'active', '', 'full', 100, 100)",
            []
        ).unwrap();
        let project_id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO source_instances (project_id, plugin_id, name, config_json, created_at)
             VALUES (?1, 'mock.plugin', 'test-source', '{}', 100)",
            rusqlite::params![project_id]
        ).unwrap();
    }

    let shared_docs = Arc::new(Mutex::new(vec![
        RawDocument {
            id: SourceDocId("doc1".into()),
            title: Some("Title 1".into()),
            content: "Content 1".into(),
            content_type: doxus_plugin_sdk::ContentType::Markdown,
            url: None,
            metadata: HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: Some(1000),
            updated_at: Some(1000), // Original timestamp
            relative_path: None,
        }
    ]));

    let mut plugin_manager = PluginManager::new(std::path::PathBuf::from("/tmp"));
    let docs_for_plugin = Arc::clone(&shared_docs);
    plugin_manager.register_factory("mock.plugin", move || {
        Box::new(MockSource {
            meta: PluginMetadata {
                id: "mock.plugin".into(),
                name: "Mock Plugin".into(),
                version: "0.1.0".into(),
                kind: PluginKind::External,
            },
            docs: Arc::clone(&docs_for_plugin),
        })
    });

    let embedder = Arc::new(MockEmbedder::new(384));
    let engine = Arc::new(SearchEngine::with_embedder(Arc::clone(&conn), embedder));

    let service = IndexingService::new(
        Arc::clone(&conn),
        Arc::new(plugin_manager),
        engine
    );

    // --- Execution 1: First indexing ---
    let total = service.index_project("test-project").await.unwrap();
    assert_eq!(total, 1, "Should index 1 document on first run");

    // --- Execution 2: Second indexing (unchanged) ---
    let total = service.index_project("test-project").await.unwrap();
    assert_eq!(total, 0, "Should skip document since updated_at is identical");

    // --- Execution 3: Third indexing (changed timestamp) ---
    {
        let mut docs = shared_docs.lock().unwrap();
        docs[0].updated_at = Some(1001);
    }
    let total = service.index_project("test-project").await.unwrap();
    assert_eq!(total, 1, "Should re-index since updated_at changed");

    // --- Execution 4: Fourth indexing (timestamp removed - safety re-index) ---
    {
        let mut docs = shared_docs.lock().unwrap();
        docs[0].updated_at = None;
    }
    let total = service.index_project("test-project").await.unwrap();
    assert_eq!(total, 1, "Should re-index since updated_at is None (safety first)");
}
