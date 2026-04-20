use std::path::Path;
use rusqlite::{params, Connection};
use serde_json::Value;
use crate::cache::{ContentCache, CacheError};
use crate::plugin::PluginManager;
use crate::auth::inject_keychain_auth;
use doxus_plugin_sdk::{PluginConfig, PluginSecrets};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cache error: {0}")]
    Cache(#[from] CacheError),
    #[error("plugin error: {0}")]
    Plugin(String),
    #[error("document not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct DocumentService<'a> {
    conn: &'a Connection,
    plugin_manager: Option<&'a PluginManager>,
}

impl<'a> DocumentService<'a> {
    pub fn new(conn: &'a Connection, plugin_manager: Option<&'a PluginManager>) -> Self {
        Self { conn, plugin_manager }
    }

    /// Fetches the full content of a document using a hybrid strategy:
    /// 1. Local file (if available)
    /// 2. Database cache (if valid)
    /// 3. Remote fetch via plugin (if available)
    pub async fn fetch_full_content(&self, project_name: &str, source_doc_id: &str) -> Result<String, ServiceError> {
        // 1. Get document and project metadata
        let (_project_id, source_type, config_json, file_path, _doc_title) = self.conn.query_row(
            "SELECT p.id, p.source_type, p.config_json, d.file_path, d.title
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project_name, source_doc_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        ).map_err(|_| ServiceError::NotFound(source_doc_id.to_string()))?;

        // 2. Try Local File Strategy
        if let Some(path_str) = file_path {
            let path = Path::new(&path_str);
            if path.exists() && path.is_file() {
                return Ok(std::fs::read_to_string(path)?);
            }
        }

        // 3. Try Cache Strategy
        let cache = ContentCache::new(self.conn);
        let plugin_id = PluginManager::normalize_id(&source_type);
        if let Ok(Some(cached)) = cache.get(&plugin_id, source_doc_id) {
            return Ok(cached);
        }

        // 4. Try Remote Fetch Strategy
        if let Some(pm) = self.plugin_manager {
            if let Some(mut source) = pm.get_source(&plugin_id) {
                // Determine TTL and prepare config
                let mut config_fields: std::collections::HashMap<String, serde_json::Value> =
                    serde_json::from_str(&config_json).unwrap_or_default();
                
                // Tauri style fix: extract from "fields" if nested
                if let Some(inner) = config_fields.get("fields").and_then(|v| v.as_object()) {
                    config_fields = inner.clone().into_iter().collect();
                }

                let mut plugin_config = PluginConfig { fields: config_fields };
                let mut plugin_secrets = PluginSecrets::default();
                
                // Inject credentials from keychain
                inject_keychain_auth(&plugin_id, &mut plugin_config, &mut plugin_secrets).await;

                // Initialize plugin
                if let Err(e) = source.initialize(plugin_config, plugin_secrets).await {
                    return Err(ServiceError::Plugin(format!("Failed to initialize plugin {}: {}", plugin_id, e)));
                }

                // Determine TTL from config_json
                let config: Value = serde_json::from_str(&config_json).unwrap_or(Value::Null);
                let ttl_minutes = config["cache_ttl_minutes"]
                    .as_u64()
                    .map(|v| v as u32)
                    .unwrap_or(360); // Default 6 hours

                // Fetch from plugin
                let doc_id = doxus_plugin_sdk::SourceDocId(source_doc_id.to_string());
                match source.fetch_document(&doc_id).await {
                    Ok(doc) => {
                        let content = doc.content;
                        // Save to cache
                        let _ = cache.set(&plugin_id, source_doc_id, &content, ttl_minutes);
                        return Ok(content);
                    }
                    Err(e) => {
                        return Err(ServiceError::Plugin(format!("Failed to fetch document: {}", e)));
                    }
                }
            }
        }

        Err(ServiceError::NotFound(format!("Content for {} is unavailable (no local file, no cache, and plugin fetch failed)", source_doc_id)))
    }

    /// Force refresh the cache and return new content
    pub async fn refresh_content(&self, project_name: &str, source_doc_id: &str) -> Result<String, ServiceError> {
        let cache = ContentCache::new(self.conn);
        let (_, source_type) = self.conn.query_row(
            "SELECT d.id, p.source_type
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2",
            params![project_name, source_doc_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| ServiceError::NotFound(source_doc_id.to_string()))?;

        let plugin_id = PluginManager::normalize_id(&source_type);
        cache.invalidate(&plugin_id, source_doc_id)?;
        self.fetch_full_content(project_name, source_doc_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use doxus_plugin_sdk::{
        Capabilities, ChangeSet, DocumentStream, FetchAllOpts, FetchChangesOpts,
        HealthStatus, PluginConfig, PluginError, PluginMetadata, PluginSecrets,
        RawDocument, SourceDocId,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use crate::db::TestDb;

    struct MockSource {
        initialized: Arc<AtomicBool>,
        config_capture: Arc<std::sync::Mutex<Option<PluginConfig>>>,
    }

    #[async_trait]
    impl doxus_plugin_sdk::DocSource for MockSource {
        fn metadata(&self) -> &PluginMetadata {
            Box::leak(Box::new(PluginMetadata {
                id: "mock".into(),
                name: "Mock".into(),
                version: "1.0.0".into(),
                kind: doxus_plugin_sdk::PluginKind::Builtin,
            }))
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { incremental_sync: true, oauth: false, native_search: false }
        }
        async fn validate_config(&self, _: &PluginConfig) -> Result<(), PluginError> { Ok(()) }
        async fn initialize(&mut self, config: PluginConfig, _: PluginSecrets) -> Result<(), PluginError> {
            self.initialized.store(true, Ordering::SeqCst);
            *self.config_capture.lock().unwrap() = Some(config);
            Ok(())
        }
        async fn fetch_all(&self, _: FetchAllOpts) -> Result<DocumentStream, PluginError> { todo!() }
        async fn fetch_changes(&self, _: FetchChangesOpts) -> Result<ChangeSet, PluginError> { todo!() }
        async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
            if self.initialized.load(Ordering::SeqCst) {
                Ok(RawDocument {
                    id: id.clone(),
                    title: Some("Mock Doc".into()),
                    content: "Mock Content".into(),
                    content_type: doxus_plugin_sdk::ContentType::Markdown,
                    url: None,
                    metadata: HashMap::new(),
                    tags: vec![],
                    aliases: vec![],
                    links: vec![],
                    created_at: None,
                    updated_at: None,
                    relative_path: None,
                })
            } else {
                Err(PluginError::Internal("Not initialized".into()))
            }
        }
        async fn health_check(&self) -> HealthStatus { HealthStatus { healthy: true, message: None } }
    }

    use std::collections::HashMap;

    #[tokio::test]
    async fn test_fetch_full_content_initializes_plugin() {
        let db = TestDb::new();
        // Setup data
        db.conn.execute("INSERT INTO projects (name, source_type, config_json) VALUES ('p1', 'mock', '{\"base_url\":\"http://test\"}')", []).unwrap();
        db.conn.execute("INSERT INTO documents (project_id, source_doc_id, title) VALUES (1, 'd1', 't1')", []).unwrap();

        let initialized = Arc::new(AtomicBool::new(false));
        let config_capture = Arc::new(std::sync::Mutex::new(None));
        
        let mut pm = PluginManager::new(std::path::PathBuf::from("/tmp"));
        let init_clone = initialized.clone();
        let config_clone = config_capture.clone();
        pm.register_factory("com.doxus.mock", move || {
            Box::new(MockSource { 
                initialized: init_clone.clone(),
                config_capture: config_clone.clone(),
            })
        });

        let service = DocumentService::new(&db.conn, Some(&pm));
        let content = service.fetch_full_content("p1", "d1").await.unwrap();

        assert_eq!(content, "Mock Content");
        assert!(initialized.load(Ordering::SeqCst), "Plugin should have been initialized");
        
        let captured = config_capture.lock().unwrap().take().unwrap();
        assert_eq!(captured.fields.get("base_url").and_then(|v| v.as_str()), Some("http://test"));
    }
}
