use std::path::Path;
use rusqlite::{params, Connection};
use serde_json::Value;
use crate::cache::{ContentCache, CacheError};
use crate::plugin::PluginManager;

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
            if let Some(source) = pm.get_source(&plugin_id) {
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
