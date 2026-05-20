use std::path::Path;
use rusqlite::{params, Connection};
use serde_json::Value;
use crate::cache::{ContentCache, CacheError};
use crate::plugin::PluginManager;
use crate::auth::inject_keychain_auth;
use crate::db::DbError;
use doxus_plugin_sdk::{PluginConfig, PluginSecrets};
use crate::observability::{persist_audit, AuditEvent};

fn ds_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/doxus-diagnostic.log") 
    {
        let _ = writeln!(file, "{}", msg);
        let _ = file.flush();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("cache error: {0}")]
    Cache(#[from] CacheError),
    #[error("plugin error: {0}")]
    Plugin(String),
    #[error("document not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

use std::sync::{Arc, Mutex};

use std::path::PathBuf;

pub struct DocumentService {
    conn: Arc<Mutex<Connection>>,
    db_path: Option<PathBuf>,
    plugin_manager: Option<Arc<PluginManager>>,
}

impl DocumentService {
    pub fn new(conn: Arc<Mutex<Connection>>, plugin_manager: Option<Arc<PluginManager>>) -> Self {
        Self { conn, db_path: None, plugin_manager }
    }

    pub fn new_with_path(db_path: PathBuf, plugin_manager: Option<Arc<PluginManager>>) -> Self {
        Self { 
            conn: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())), // Dummy, not used if path is set
            db_path: Some(db_path),
            plugin_manager 
        }
    }

    /// Fetches the full content of a document using a hybrid strategy:
    /// 1. Local file (if available)
    /// 2. Database cache (if valid)
    /// 3. Remote fetch via plugin (if available)
    pub async fn fetch_full_content(&self, project_name: &str, source_doc_id: &str) -> Result<doxus_plugin_sdk::RawDocument, ServiceError> {
        tracing::info!("[DS] Starting fetch_full_content for doc: {:?}", source_doc_id);
        ds_log(&format!("[DS] Starting fetch_full_content for doc: {}", source_doc_id));
        // 1. Get project metadata first
        let (_project_id, source_type, config_json, file_path) = {
            ds_log("[DS] Loading project metadata..."); tracing::info!("[DS] Loading project metadata...");
            
            if let Some(path) = &self.db_path {
                ds_log("[DS] Opening READ-ONLY DB for project metadata..."); tracing::info!("[DS] Opening READ-ONLY DB for project metadata...");
                let conn = crate::db::open_readonly(path).map_err(ServiceError::Db)?;
                let (pid, stype, cjson) = conn.query_row(
                    "SELECT id, source_type, config_json FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?;

                let fpath: Option<String> = conn.query_row(
                    "SELECT file_path FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                    params![pid, source_doc_id],
                    |row| row.get::<_, Option<String>>(0),
                ).ok().flatten();
                
                (pid, stype, cjson, fpath)
            } else {
                let conn = self.conn.lock().map_err(|_| ServiceError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
                let (pid, stype, cjson) = conn.query_row(
                    "SELECT id, source_type, config_json FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?;

                let fpath: Option<String> = conn.query_row(
                    "SELECT file_path FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                    params![pid, source_doc_id],
                    |row| row.get::<_, Option<String>>(0),
                ).ok().flatten();
                
                (pid, stype, cjson, fpath)
            }
        };
        ds_log("[DS] Project metadata loaded."); tracing::info!("[DS] Project metadata loaded.");

        // 3. Try Local File Strategy
        if let Some(path_str) = file_path {
            let log_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() % 1000000;
            ds_log(&format!("[DS][{}] Candidate: {}", log_id, path_str));
            let path = Path::new(&path_str);
            
            ds_log(&format!("[DS][{}] Calling exists()...", log_id));
            let exists = path.exists();
            ds_log(&format!("[DS][{}] exists() -> {}", log_id, exists));
            
            if exists {
                ds_log(&format!("[DS][{}] Calling is_file()...", log_id));
                let is_f = path.is_file();
                ds_log(&format!("[DS][{}] is_file() -> {}", log_id, is_f));
                
                if is_f {
                    ds_log(&format!("[DS][{}] Reading file...", log_id));
                    if let Ok(content) = std::fs::read_to_string(path) {
                        ds_log(&format!("[DS][{}] Read SUCCESS (len: {})", log_id, content.len()));
                        
                        // Try to supplement with DB metadata
                        let mut db_title = None;
                        let mut db_tags = Vec::new();
                        let mut db_created_at = None;
                        let mut db_updated_at = None;

                        let meta_query = "SELECT d.title, d.created_at, d.updated_at, \
                                         (SELECT GROUP_CONCAT(tag) FROM document_tags WHERE document_id = d.id) \
                                         FROM documents d JOIN projects p ON d.project_id = p.id \
                                         WHERE p.name = ?1 AND (d.source_doc_id = ?2 OR d.file_path = ?3)";
                        
                        let db_result = if let Some(path) = &self.db_path {
                            crate::db::open_readonly(path).ok().and_then(|c| {
                                c.query_row(meta_query, params![project_name, source_doc_id, path_str], |r| {
                                    let title: Option<String> = r.get(0)?;
                                    let created: Option<i64> = r.get(1)?;
                                    let updated: Option<i64> = r.get(2)?;
                                    let tags_str: Option<String> = r.get(3)?;
                                    Ok((title, created, updated, tags_str))
                                }).ok()
                            })
                        } else {
                            self.conn.lock().ok().and_then(|c| {
                                c.query_row(meta_query, params![project_name, source_doc_id, path_str], |r| {
                                    let title: Option<String> = r.get(0)?;
                                    let created: Option<i64> = r.get(1)?;
                                    let updated: Option<i64> = r.get(2)?;
                                    let tags_str: Option<String> = r.get(3)?;
                                    Ok((title, created, updated, tags_str))
                                }).ok()
                            })
                        };

                        if let Some((t, c, u, ts)) = db_result {
                            db_title = t;
                            db_created_at = c;
                            db_updated_at = u;
                            if let Some(s) = ts {
                                db_tags = s.split(',').map(|s| s.to_string()).collect();
                            }
                        }

                        return Ok(doxus_plugin_sdk::RawDocument {
                            id: doxus_plugin_sdk::SourceDocId(source_doc_id.to_string()),
                            title: db_title,
                            content,
                            content_type: doxus_plugin_sdk::ContentType::Markdown,
                            url: None,
                            metadata: std::collections::HashMap::new(),
                            tags: db_tags,
                            aliases: vec![],
                            links: vec![],
                            created_at: db_created_at,
                            updated_at: db_updated_at,
                            relative_path: Some(path_str),
                        });
                    }
                }
            } else {
                ds_log(&format!("[DS][{}] Not found.", log_id));
            }
        }

        // 3. Try Cache Strategy
        let plugin_id = PluginManager::normalize_id(&source_type);
        {
            tracing::info!("[DS] Checking cache for plugin: {}", plugin_id);
            if let Some(path) = &self.db_path {
                ds_log("[DS] Opening independent DB for cache check..."); tracing::info!("[DS] Opening independent DB for cache check...");
                let conn = crate::db::open_readonly(path).map_err(ServiceError::Db)?;
                ds_log("[DS] DB opened. Checking ContentCache..."); tracing::info!("[DS] DB opened. Checking ContentCache...");
                let cache = ContentCache::new(&conn);
                if let Ok(Some(data_json)) = cache.get_full(&plugin_id, source_doc_id) {
                    if let Ok(doc) = serde_json::from_str::<doxus_plugin_sdk::RawDocument>(&data_json) {
                        ds_log("[DS] Cache HIT."); tracing::info!("[DS] Cache HIT.");
                        return Ok(doc);
                    }
                }
                ds_log("[DS] Cache MISS."); tracing::info!("[DS] Cache MISS.");
            } else {
                ds_log("[DS] Locking shared DB for cache check..."); tracing::info!("[DS] Locking shared DB for cache check...");
                let conn = self.conn.lock().map_err(|_| ServiceError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
                let cache = ContentCache::new(&conn);
                if let Ok(Some(data_json)) = cache.get_full(&plugin_id, source_doc_id) {
                    if let Ok(doc) = serde_json::from_str::<doxus_plugin_sdk::RawDocument>(&data_json) {
                        ds_log("[DS] Cache HIT."); tracing::info!("[DS] Cache HIT.");
                        return Ok(doc);
                    }
                }
                ds_log("[DS] Cache MISS."); tracing::info!("[DS] Cache MISS.");
            }
        }

        // 4. Try Remote Fetch Strategy
        ds_log("[DS] Proceeding to Remote Fetch Strategy..."); tracing::info!("[DS] Proceeding to Remote Fetch Strategy...");
        if let Some(pm) = &self.plugin_manager {
            tracing::info!("[DS] Getting source for plugin: {}", plugin_id);
            if let Some(mut source) = pm.get_source(&plugin_id) {
                ds_log("[DS] Plugin source acquired. Preparing config..."); tracing::info!("[DS] Plugin source acquired. Preparing config...");
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
                ds_log("[DS] Calling inject_keychain_auth..."); tracing::info!("[DS] Calling inject_keychain_auth...");
                inject_keychain_auth(&plugin_id, &mut plugin_config, &mut plugin_secrets).await;
                ds_log("[DS] inject_keychain_auth done."); tracing::info!("[DS] inject_keychain_auth done.");

                // Initialize plugin
                if let Err(e) = source.initialize(plugin_config, plugin_secrets).await {
                    let msg = format!("Failed to initialize plugin: {}", e);
                    if let Ok(conn) = self.conn.lock() {
                        persist_audit(&conn, &AuditEvent::PluginError {
                            plugin_id: plugin_id.clone(),
                            message: msg,
                        });
                    }
                    return Err(ServiceError::Plugin(format!("Failed to initialize plugin {}: {}", plugin_id, e)));
                }

                // Determine TTL from config_json
                let config: Value = serde_json::from_str(&config_json).unwrap_or(Value::Null);
                let ttl_minutes = config["cache_ttl_minutes"]
                    .as_u64()
                    .map(|v| v as u32)
                    .unwrap_or(360); // Default 6 hours

                // Fetch from plugin with timeout
                let doc_id = doxus_plugin_sdk::SourceDocId(source_doc_id.to_string());
                tracing::info!("[DS] Calling source.fetch_document for {}", source_doc_id);
                ds_log(&format!("[DS] Calling source.fetch_document for {}", source_doc_id));
                
                let fetch_result = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    source.fetch_document(&doc_id)
                ).await;

                match fetch_result {
                    Ok(Ok(doc)) => {
                        // Save to cache (full object)
                        if let Ok(data_json) = serde_json::to_string(&doc) {
                            if let Some(path) = &self.db_path {
                                if let Ok(conn) = crate::db::open(path) {
                                    let cache = ContentCache::new(&conn);
                                    let _ = cache.set_full(&plugin_id, source_doc_id, &doc.content, &data_json, ttl_minutes);
                                }
                            } else if let Ok(conn) = self.conn.lock() {
                                let cache = ContentCache::new(&conn);
                                let _ = cache.set_full(&plugin_id, source_doc_id, &doc.content, &data_json, ttl_minutes);
                            }
                        }
                        ds_log("[DS] Fetch SUCCESS"); tracing::info!("[DS] Fetch SUCCESS");
                        return Ok(doc);
                    }
                    Ok(Err(e)) => {
                        tracing::error!("[DS] Fetch FAILED: {}", e);
                        ds_log(&format!("[ERR] [DS] Fetch FAILED: {}", e));
                        
                        // Persist error to audit log
                        let event = AuditEvent::DocumentFetchError {
                            project: project_name.to_string(),
                            doc_id: source_doc_id.to_string(),
                            message: e.to_string(),
                        };
                        if let Some(path) = &self.db_path {
                            if let Ok(conn) = crate::db::open(path) {
                                persist_audit(&conn, &event);
                            }
                        } else if let Ok(conn) = self.conn.lock() {
                            persist_audit(&conn, &event);
                        }

                        return Err(ServiceError::Plugin(format!("Failed to fetch document: {}", e)));
                    }
                    Err(_) => {
                        ds_log("[ERR] [DS] Fetch TIMEOUT (30s)"); tracing::error!("[DS] Fetch TIMEOUT (30s)");
                        
                        if let Some(path) = &self.db_path {
                            if let Ok(conn) = crate::db::open(path) {
                                crate::observability::persist_audit(&conn, &crate::observability::AuditEvent::DocumentFetchError {
                                    project: project_name.to_string(),
                                    doc_id: source_doc_id.to_string(),
                                    message: "Timeout (30s)".to_string(),
                                });
                            }
                        }
                        
                        return Err(ServiceError::Plugin("Plugin fetch timed out after 30s".to_string()));
                    }
                }
            }
        }

        Err(ServiceError::NotFound(format!("Content for {} is unavailable (no local file, no cache, and plugin fetch failed)", source_doc_id)))
    }

    /// Force refresh the cache and return new content
    pub async fn refresh_content(&self, project_name: &str, source_doc_id: &str) -> Result<doxus_plugin_sdk::RawDocument, ServiceError> {
        let (_source_type, _plugin_id) = {
            if let Some(path) = &self.db_path {
                let conn = crate::db::open(path).map_err(ServiceError::Db)?;
                let stype: String = conn.query_row(
                    "SELECT source_type FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| row.get(0),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?;
                let pid = PluginManager::normalize_id(&stype);
                
                let cache = ContentCache::new(&conn);
                cache.invalidate(&pid, source_doc_id)?;
                (stype, pid)
            } else {
                let conn = self.conn.lock().map_err(|_| ServiceError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
                let stype: String = conn.query_row(
                    "SELECT source_type FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| row.get(0),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?;
                let pid = PluginManager::normalize_id(&stype);
                
                let cache = ContentCache::new(&conn);
                cache.invalidate(&pid, source_doc_id)?;
                (stype, pid)
            }
        };
        
        self.fetch_full_content(project_name, source_doc_id).await
    }

    /// Creates a new document in the target project via its plugin
    pub async fn create_document(
        &self,
        project_name: &str,
        title: &str,
        content: &str,
        folder: Option<&str>,
        metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<doxus_plugin_sdk::SourceDocId, ServiceError> {
        let (source_type, config_json) = {
            if let Some(path) = &self.db_path {
                let conn = crate::db::open_readonly(path).map_err(ServiceError::Db)?;
                conn.query_row(
                    "SELECT source_type, config_json FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?
            } else {
                let conn = self.conn.lock().map_err(|_| ServiceError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
                conn.query_row(
                    "SELECT source_type, config_json FROM projects WHERE name = ?1",
                    params![project_name],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                ).map_err(|_| ServiceError::NotFound(format!("Project '{}' not found", project_name)))?
            }
        };

        let plugin_id = PluginManager::normalize_id(&source_type);
        let pm = self.plugin_manager.as_ref().ok_or_else(|| ServiceError::Plugin("Plugin manager not available".into()))?;
        let mut source = pm.get_source(&plugin_id).ok_or_else(|| ServiceError::Plugin(format!("Source plugin {} not found", plugin_id)))?;

        // Config preparation (same as fetch_full_content)
        let mut config_fields: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&config_json).unwrap_or_default();
        if let Some(inner) = config_fields.get("fields").and_then(|v| v.as_object()) {
            config_fields = inner.clone().into_iter().collect();
        }

        let mut plugin_config = doxus_plugin_sdk::PluginConfig { fields: config_fields };
        let mut plugin_secrets = doxus_plugin_sdk::PluginSecrets::default();
        inject_keychain_auth(&plugin_id, &mut plugin_config, &mut plugin_secrets).await;

        source.initialize(plugin_config, plugin_secrets).await
            .map_err(|e| {
                let msg = format!("Failed to initialize plugin {}: {}", plugin_id, e);
                if let Ok(conn) = self.conn.lock() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg.clone(),
                    });
                }
                ServiceError::Plugin(msg)
            })?;

        if !source.supports_write() {
            let msg = format!("Plugin {} does not support write operations", plugin_id);
            if let Ok(conn) = self.conn.lock() {
                persist_audit(&conn, &AuditEvent::PluginError {
                    plugin_id: plugin_id.clone(),
                    message: msg.clone(),
                });
            }
            return Err(ServiceError::Plugin(msg));
        }

        source.create_document(title, content, folder, metadata.as_ref())
            .await
            .map_err(|e| {
                let msg = format!("Failed to create document in plugin {}: {}", plugin_id, e);
                if let Ok(conn) = self.conn.lock() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg.clone(),
                    });
                }
                ServiceError::Plugin(msg)
            })
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
            Capabilities {
                incremental_sync: true,
                oauth: false,
                native_search: false,
                sync_policy: doxus_plugin_sdk::SyncPolicy::OnFocus,
            }
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
        db.conn.execute("INSERT INTO projects (name, display_name, path, source_type, config_json, created_at, updated_at) VALUES ('p1', 'P1', '/tmp', 'mock', '{\"base_url\":\"http://test\"}', 0, 0)", []).unwrap();
        db.conn.execute("INSERT INTO documents (project_id, source_doc_id, title, content_hash) VALUES (1, 'd1', 't1', 'h1')", []).unwrap();

        let initialized = Arc::new(AtomicBool::new(false));
        let config_capture = Arc::new(std::sync::Mutex::new(None));
        
        let mut pm = PluginManager::new(std::path::PathBuf::from("/tmp"));
        let init_clone = initialized.clone();
        let config_clone = config_capture.clone();
        pm.register_factory("mock", move || {
            Box::new(MockSource { 
                initialized: init_clone.clone(),
                config_capture: config_clone.clone(),
            })
        });
        let pm_arc = Arc::new(pm);
        let TestDb { conn } = db;
        let conn_arc = Arc::new(Mutex::new(conn));

        let service = DocumentService::new(conn_arc, Some(pm_arc));
        let doc = service.fetch_full_content("p1", "d1").await.unwrap();

        assert_eq!(doc.content, "Mock Content");
        assert!(initialized.load(Ordering::SeqCst), "Plugin should have been initialized");
        
        let captured = config_capture.lock().unwrap().take().unwrap();
        assert_eq!(captured.fields.get("base_url").and_then(|v| v.as_str()), Some("http://test"));
    }

    #[tokio::test]
    async fn test_fetch_full_content_under_exclusive_lock() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_cache.db");
        
        // 1. Setup DB with project and cache data
        {
            let conn = crate::db::open(&db_path).unwrap();
            conn.execute("INSERT INTO projects (name, display_name, path, source_type, config_json, created_at, updated_at) VALUES ('p1', 'P1', '/tmp', 'mock', '{\"cache_ttl_minutes\": 60}', 0, 0)", []).unwrap();
            conn.execute("INSERT INTO documents (project_id, source_doc_id, title, content_hash) VALUES (1, 'd1', 't1', 'h1')", []).unwrap();
            
            // Insert cache entry
            let cache = ContentCache::new(&conn);
            let raw_doc = RawDocument {
                id: SourceDocId("d1".to_string()),
                title: Some("Cached Doc".into()),
                content: "Cached Content".into(),
                content_type: doxus_plugin_sdk::ContentType::Markdown,
                url: None,
                metadata: HashMap::new(),
                tags: vec![],
                aliases: vec![],
                links: vec![],
                created_at: None,
                updated_at: None,
                relative_path: None,
            };
            let data_json = serde_json::to_string(&raw_doc).unwrap();
            cache.set_full("mock", "d1", "Cached Content", &data_json, 60).unwrap();
        }

        // 2. Open an exclusive write transaction in a separate connection to simulate concurrent write blocking
        let exclusive_conn = crate::db::open(&db_path).unwrap();
        exclusive_conn.execute("BEGIN EXCLUSIVE TRANSACTION;", []).unwrap();

        // 3. Create DocumentService using path
        let service = DocumentService::new_with_path(db_path.clone(), None);

        // 4. Try to fetch content.
        let result = service.fetch_full_content("p1", "d1").await;
        
        // Clean up exclusive transaction
        exclusive_conn.execute("ROLLBACK;", []).ok();

        let doc = result.unwrap();
        assert_eq!(doc.content, "Cached Content");
    }
}

