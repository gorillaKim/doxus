use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SourceDocId,
};
use std::path::PathBuf;

pub struct ObsidianPlugin {
    meta: PluginMetadata,
    vault_path: Option<PathBuf>,
}

impl ObsidianPlugin {
    pub fn new() -> Self {
        Self {
            meta: PluginMetadata {
                id: "com.doxus.obsidian".into(),
                name: "Obsidian".into(),
                version: "0.1.0".into(),
                kind: PluginKind::Builtin,
            },
            vault_path: None,
        }
    }

    fn vault(&self) -> Result<&PathBuf, PluginError> {
        self.vault_path
            .as_ref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    fn read_markdown_files(&self, path: &PathBuf) -> Result<Vec<RawDocument>, std::io::Error> {
        let mut docs = Vec::new();
        let walker = walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                if !e.file_type().is_file() {
                    return false;
                }
                if e.path().extension().is_none_or(|ext| ext != "md") {
                    return false;
                }
                // Only check hidden dirs relative to vault root, not absolute path
                // components (which may contain system dirs like /var/folders/...)
                let rel = e.path().strip_prefix(path).unwrap_or(e.path());
                !rel.components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            });

        for entry in walker {
            let file_path = entry.path().to_path_buf();
            let content = std::fs::read_to_string(&file_path)?;
            let rel_path = file_path
                .strip_prefix(path)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();

            let title = content
                .lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l[2..].trim().to_string())
                .or_else(|| {
                    file_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                });

            docs.push(RawDocument {
                id: SourceDocId(rel_path.clone()),
                title,
                content,
                content_type: ContentType::Markdown,
                url: Some(format!("obsidian://open?path={rel_path}")),
                metadata: Default::default(),
                tags: vec![],
                updated_at: file_path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs() as i64)
                    }),
            });
        }

        Ok(docs)
    }
}

impl Default for ObsidianPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocSource for ObsidianPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: true,
            oauth: false,
            native_search: false,
        }
    }

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError> {
        let path = config
            .fields
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PluginError::ConfigInvalid("missing 'path' field".into()))?;

        if !std::path::Path::new(path).exists() {
            return Err(PluginError::ConfigInvalid(format!(
                "vault path does not exist: {path}"
            )));
        }
        Ok(())
    }

    async fn initialize(
        &mut self,
        config: PluginConfig,
        _secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        let path = config
            .fields
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PluginError::ConfigInvalid("missing 'path' field".into()))?;

        self.vault_path = Some(PathBuf::from(path));
        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let vault = self.vault()?;
        let docs = self
            .read_markdown_files(vault)
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        let page_size = opts.page_size;
        let offset: usize = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0usize);

        let page: Vec<_> = docs.iter().skip(offset).take(page_size).cloned().collect();
        let next_cursor = if offset + page_size < docs.len() {
            Some((offset + page_size).to_string())
        } else {
            None
        };

        Ok(DocumentStream {
            documents: page,
            next_cursor,
            estimated_total: Some(docs.len() as u64),
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        let vault = self.vault()?;
        let path = vault.join(&id.0);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| PluginError::NotFound(format!("{}: {e}", id.0)))?;

        let title = content
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()));

        Ok(RawDocument {
            id: id.clone(),
            title,
            content,
            content_type: ContentType::Markdown,
            url: Some(format!("obsidian://open?path={}", id.0)),
            metadata: Default::default(),
            tags: vec![],
            updated_at: None,
        })
    }

    async fn health_check(&self) -> HealthStatus {
        match &self.vault_path {
            None => HealthStatus { healthy: false, message: Some("not initialized".into()) },
            Some(path) => {
                if path.exists() {
                    HealthStatus { healthy: true, message: None }
                } else {
                    HealthStatus {
                        healthy: false,
                        message: Some(format!("vault not found: {}", path.display())),
                    }
                }
            }
        }
    }

    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Ok(ChangeSet { updated: vec![], deleted_ids: vec![], next_cursor: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_check_missing_vault_is_unhealthy() {
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!("/nonexistent/vault/path_xyz"));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }

    #[tokio::test]
    async fn health_check_existing_vault_is_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();
        let status = plugin.health_check().await;
        assert!(status.healthy);
    }

    #[tokio::test]
    async fn fetch_all_returns_markdown_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "# Alpha\ncontent").unwrap();
        std::fs::write(dir.path().join("b.md"), "# Beta\ncontent").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        assert_eq!(stream.documents.len(), 2);
    }

    #[tokio::test]
    async fn fetch_all_skips_non_markdown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "# Note\ncontent").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "plain text").unwrap();
        std::fs::write(dir.path().join("data.json"), r#"{"key":"val"}"#).unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        assert_eq!(stream.documents.len(), 1);
        assert_eq!(stream.documents[0].id.0, "note.md");
    }

    #[tokio::test]
    async fn health_check_before_init_is_unhealthy() {
        let plugin = ObsidianPlugin::new();
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }

    #[tokio::test]
    async fn validate_config_rejects_missing_path() {
        let plugin = ObsidianPlugin::new();
        let result = plugin.validate_config(&PluginConfig::default()).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn validate_config_rejects_nonexistent_path() {
        let plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!("/nonexistent/vault"));
        let result = plugin.validate_config(&config).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn fetch_all_reads_markdown_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "# My Note\nHello world").unwrap();
        std::fs::write(dir.path().join("other.md"), "# Other\nContent").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts { cursor: None, page_size: 100 })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 2);
        assert!(stream.documents.iter().any(|d| d.title.as_deref() == Some("My Note")));
    }

    #[tokio::test]
    async fn fetch_all_pagination() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("note{i}.md")), format!("# Note {i}")).unwrap();
        }

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let page1 = plugin
            .fetch_all(FetchAllOpts { cursor: None, page_size: 3 })
            .await
            .unwrap();
        assert_eq!(page1.documents.len(), 3);
        assert!(page1.next_cursor.is_some());

        let page2 = plugin
            .fetch_all(FetchAllOpts { cursor: page1.next_cursor, page_size: 3 })
            .await
            .unwrap();
        assert_eq!(page2.documents.len(), 2);
        assert!(page2.next_cursor.is_none());
    }
}
