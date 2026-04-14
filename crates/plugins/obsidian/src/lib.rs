use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SourceDocId,
};
use std::collections::HashSet;
use std::path::PathBuf;


/// Same as `parse_links` but also excludes links equal to `self_id`.
fn parse_links_for_doc(content: &str, self_id: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    // [[target]] or [[target|alias]]
    let mut rest = content;
    while let Some(start) = rest.find("[[") {
        rest = &rest[start + 2..];
        if let Some(end) = rest.find("]]") {
            let inner = &rest[..end];
            // take target (before | if alias present)
            let target = inner.split('|').next().unwrap_or(inner).trim();
            if !target.is_empty() && target != self_id && seen.insert(target.to_string()) {
                links.push(target.to_string());
            }
            rest = &rest[end + 2..];
        }
    }

    // [text](path) — relative only
    let mut rest = content;
    while let Some(bracket) = rest.find("](") {
        // walk back to find matching [
        let prefix = &rest[..bracket];
        rest = &rest[bracket + 2..];
        if let Some(end) = rest.find(')') {
            let path = rest[..end].trim();
            rest = &rest[end + 1..];
            // skip absolute URLs
            if path.starts_with("http://")
                || path.starts_with("https://")
                || path.starts_with("obsidian://")
                || path.is_empty()
            {
                continue;
            }
            // must look like a relative path (no scheme)
            let _ = prefix; // bracket position already consumed
            if path != self_id && seen.insert(path.to_string()) {
                links.push(path.to_string());
            }
        }
    }

    links
}

/// Parse tags from YAML frontmatter and inline #hashtags.
fn parse_tags(content: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut body = content;

    // Extract frontmatter between leading `---` delimiters
    if let Some(rest) = content.strip_prefix("---") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            tags.extend(parse_frontmatter_tags(fm));
            body = &rest[end + 4..];
        }
    }

    // Extract inline #tags from body
    for word in body.split_whitespace() {
        if let Some(tag) = word.strip_prefix('#') {
            let tag = tag.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if !tag.is_empty() {
                tags.push(tag.to_string());
            }
        }
    }

    tags
}

/// Extract `aliases:` and `created:` / `date:` from YAML frontmatter.
fn parse_frontmatter_meta(fm: &str) -> (Vec<String>, Option<i64>) {
    let mut aliases: Vec<String> = Vec::new();
    let mut created_at: Option<i64> = None;
    let mut lines = fm.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        // aliases: [a, b] or block list
        if let Some(after) = trimmed.strip_prefix("aliases:") {
            let after = after.trim();
            if after.starts_with('[') {
                let inner = after.trim_start_matches('[').trim_end_matches(']');
                for item in inner.split(',') {
                    let a = item.trim().trim_matches('"').trim_matches('\'');
                    if !a.is_empty() { aliases.push(a.to_string()); }
                }
            } else if after.is_empty() {
                while let Some(next) = lines.peek() {
                    let nt = next.trim();
                    if let Some(item) = nt.strip_prefix("- ") {
                        aliases.push(item.trim().trim_matches('"').trim_matches('\'').to_string());
                        lines.next();
                    } else { break; }
                }
            } else {
                // aliases: single value
                let a = after.trim_matches('"').trim_matches('\'');
                if !a.is_empty() { aliases.push(a.to_string()); }
            }
        }
        // created: / date: → Unix timestamp (ISO 8601 or YYYY-MM-DD)
        if trimmed.starts_with("created:") || trimmed.starts_with("date:") {
            let val = trimmed.split_once(':').map(|x| x.1).unwrap_or("").trim();
            let val = val.trim_matches('"').trim_matches('\'');
            // Try ISO 8601 parsing via simple heuristic (YYYY-MM-DD prefix)
            if val.len() >= 10 {
                let date_part = &val[..10]; // "YYYY-MM-DD"
                let parts: Vec<&str> = date_part.split('-').collect();
                if parts.len() == 3 {
                    if let (Ok(y), Ok(m), Ok(d)) = (
                        parts[0].parse::<i64>(),
                        parts[1].parse::<u32>(),
                        parts[2].parse::<u32>(),
                    ) {
                        // Rough Unix timestamp: days since epoch
                        let _ = (y, m, d); // suppress unused warnings
                        // Simple approximation: (y-1970)*365.25 + day_of_year
                        let days = (y - 1970) * 365 + (y - 1969) / 4
                            + match m {
                                1 => 0, 2 => 31, 3 => 59, 4 => 90, 5 => 120, 6 => 151,
                                7 => 181, 8 => 212, 9 => 243, 10 => 273, 11 => 304, _ => 334,
                            } as i64
                            + d as i64 - 1;
                        created_at = Some(days * 86400);
                    }
                }
            }
        }
    }
    (aliases, created_at)
}

/// Parse `tags:` field from YAML frontmatter string (no external crate).
fn parse_frontmatter_tags(fm: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut lines = fm.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if let Some(after) = trimmed.strip_prefix("tags:") {
            let after = after.trim();
            if after.starts_with('[') {
                // Inline list: tags: [rust, doxus]
                let inner = after.trim_start_matches('[').trim_end_matches(']');
                for item in inner.split(',') {
                    let t = item.trim().trim_matches('"').trim_matches('\'');
                    if !t.is_empty() {
                        tags.push(t.to_string());
                    }
                }
            } else if after.is_empty() {
                // Block list:
                //   - alpha
                //   - beta
                while let Some(next) = lines.peek() {
                    let nt = next.trim();
                    if let Some(item) = nt.strip_prefix("- ") {
                        tags.push(item.trim().trim_matches('"').trim_matches('\'').to_string());
                        lines.next();
                    } else {
                        break;
                    }
                }
            }
            break;
        }
    }
    tags
}

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

    /// Collect sorted markdown file paths relative to the vault root.
    /// No file content is read — only directory traversal.
    fn collect_markdown_paths(&self, vault: &PathBuf) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(vault)
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
                let rel = e.path().strip_prefix(vault).unwrap_or(e.path());
                !rel.components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            })
            .map(|e| e.path().to_path_buf())
            .collect();
        paths.sort();
        paths
    }

    /// Read and parse a single markdown file into a `RawDocument`.
    fn read_markdown_file(
        &self,
        vault: &PathBuf,
        file_path: &PathBuf,
    ) -> Result<RawDocument, std::io::Error> {
        let content = std::fs::read_to_string(file_path)?;
        let rel_path = file_path
            .strip_prefix(vault)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let title = content
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .or_else(|| file_path.file_stem().map(|s| s.to_string_lossy().to_string()));

        let tags = parse_tags(&content);
        let links = parse_links_for_doc(&content, &rel_path);
        let file_meta = file_path.metadata().ok();
        let updated_at = file_meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64));
        let fs_created_at = file_meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64));

        // frontmatter aliases + created_at
        let (aliases, fm_created_at) = if let Some(rest) = content.strip_prefix("---") {
            let rest = rest.strip_prefix('\n').unwrap_or(rest);
            if let Some(end) = rest.find("\n---") {
                parse_frontmatter_meta(&rest[..end])
            } else {
                (vec![], None)
            }
        } else {
            (vec![], None)
        };
        let created_at = fm_created_at.or(fs_created_at);

        let mut metadata: std::collections::HashMap<String, serde_json::Value> =
            Default::default();
        if !links.is_empty() {
            metadata.insert(
                "links".into(),
                serde_json::Value::Array(
                    links.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }

        Ok(RawDocument {
            id: SourceDocId(rel_path.clone()),
            title,
            content,
            content_type: ContentType::Markdown,
            url: Some(format!("obsidian://open?path={rel_path}")),
            metadata,
            tags,
            aliases,
            created_at,
            updated_at,
        })
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

        // Collect only file paths — no content reads yet
        let all_paths = self.collect_markdown_paths(vault);
        let total = all_paths.len();

        let page_size = opts.page_size;
        let offset: usize = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0usize);

        // Read only the files in the requested page
        let page_paths = all_paths.into_iter().skip(offset).take(page_size);
        let documents: Result<Vec<_>, _> =
            page_paths.map(|p| self.read_markdown_file(vault, &p)).collect();
        let documents =
            documents.map_err(|e| PluginError::Internal(e.to_string()))?;

        let next_cursor = if offset + page_size < total {
            Some((offset + page_size).to_string())
        } else {
            None
        };

        Ok(DocumentStream {
            documents,
            next_cursor,
            estimated_total: Some(total as u64),
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

        let tags = parse_tags(&content);
        Ok(RawDocument {
            id: id.clone(),
            title,
            content,
            content_type: ContentType::Markdown,
            url: Some(format!("obsidian://open?path={}", id.0)),
            metadata: Default::default(),
            tags,
            aliases: vec![],
            created_at: None,
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

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        let vault = self.vault()?;

        // Collect only file paths — no content reads yet
        let all_paths = self.collect_markdown_paths(vault);

        // Build the set of on-disk relative IDs without reading content
        let on_disk: HashSet<String> = all_paths
            .iter()
            .map(|p| {
                p.strip_prefix(vault)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        // Read only files modified after `since`
        let mut updated = Vec::new();
        for file_path in &all_paths {
            let mtime = file_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
                });
            if mtime.map(|t| t > opts.since).unwrap_or(false) {
                let doc = self
                    .read_markdown_file(vault, file_path)
                    .map_err(|e| PluginError::Internal(e.to_string()))?;
                updated.push(doc);
            }
        }

        // Detect deletions: known_ids not present on disk
        let deleted_ids: Vec<SourceDocId> = opts
            .known_ids
            .into_iter()
            .filter(|id| !on_disk.contains(&id.0))
            .collect();

        Ok(ChangeSet { updated, deleted_ids, next_cursor: None })
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

    // ── Goal 2b: frontmatter tag parsing ──────────────────────────────────────

    #[tokio::test]
    async fn tags_parsed_from_frontmatter_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("note.md"),
            "---\ntags: [rust, doxus]\n---\n# Note\nHello",
        )
        .unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        assert_eq!(stream.documents[0].tags, vec!["rust", "doxus"]);
    }

    #[tokio::test]
    async fn tags_parsed_from_frontmatter_block() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("note.md"),
            "---\ntags:\n  - alpha\n  - beta\n---\n# Note\nHello",
        )
        .unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        assert_eq!(stream.documents[0].tags, vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn inline_tags_extracted_from_body() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("note.md"),
            "# Note\nThis is #rust and #doxus content.",
        )
        .unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        let tags = &stream.documents[0].tags;
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"doxus".to_string()));
    }

    #[tokio::test]
    async fn no_frontmatter_no_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plain.md"), "# Plain\nNo frontmatter here.").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        assert_eq!(stream.documents[0].tags, Vec::<String>::new());
    }

    // ── H2: lazy pagination — second page reads only its own files ────────────

    #[tokio::test]
    async fn fetch_all_second_page_excludes_first_page_files() {
        let dir = tempfile::tempdir().unwrap();
        // Create 100 markdown files named 000.md … 099.md
        for i in 0..100usize {
            std::fs::write(
                dir.path().join(format!("{i:03}.md")),
                format!("# Doc {i}\ncontent"),
            )
            .unwrap();
        }

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        // First page: items 0..10
        let page1 = plugin
            .fetch_all(FetchAllOpts { cursor: None, page_size: 10 })
            .await
            .unwrap();
        assert_eq!(page1.documents.len(), 10);
        assert!(page1.next_cursor.is_some());

        // Second page: items 10..20
        let page2 = plugin
            .fetch_all(FetchAllOpts { cursor: page1.next_cursor.clone(), page_size: 10 })
            .await
            .unwrap();
        assert_eq!(page2.documents.len(), 10);

        // The two pages must be disjoint
        let ids1: std::collections::HashSet<_> =
            page1.documents.iter().map(|d| &d.id.0).collect();
        let ids2: std::collections::HashSet<_> =
            page2.documents.iter().map(|d| &d.id.0).collect();
        assert!(ids1.is_disjoint(&ids2), "pages overlap: {ids1:?} ∩ {ids2:?}");

        // estimated_total should reflect the full vault
        assert_eq!(page1.estimated_total, Some(100));

        // Verify cursor arithmetic: last page has no next_cursor
        let last_cursor = Some("90".to_string());
        let last_page = plugin
            .fetch_all(FetchAllOpts { cursor: last_cursor, page_size: 10 })
            .await
            .unwrap();
        assert_eq!(last_page.documents.len(), 10);
        assert!(last_page.next_cursor.is_none());
    }

    // ── Goal 2a: fetch_changes mtime-based ────────────────────────────────────

    #[tokio::test]
    async fn fetch_changes_returns_modified_files() {
        let dir = tempfile::tempdir().unwrap();
        let note_path = dir.path().join("changed.md");
        std::fs::write(&note_path, "# Changed\ncontent").unwrap();

        let since: i64 = 0;

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let changes = plugin
            .fetch_changes(FetchChangesOpts {
                since,
                cursor: None,
                page_size: 100,
                known_ids: vec![],
            })
            .await
            .unwrap();

        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.updated[0].id.0, "changed.md");
    }

    #[tokio::test]
    async fn fetch_changes_excludes_old_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.md"), "# Old\ncontent").unwrap();

        let since: i64 = i64::MAX;

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let changes = plugin
            .fetch_changes(FetchChangesOpts {
                since,
                cursor: None,
                page_size: 100,
                known_ids: vec![],
            })
            .await
            .unwrap();

        assert!(changes.updated.is_empty());
    }

    #[tokio::test]
    async fn fetch_changes_detects_deleted_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("exists.md"), "# Exists\ncontent").unwrap();

        let known_ids = vec![
            SourceDocId("exists.md".into()),
            SourceDocId("ghost.md".into()), // does not exist on disk
        ];

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let changes = plugin
            .fetch_changes(FetchChangesOpts {
                since: 0,
                cursor: None,
                page_size: 100,
                known_ids,
            })
            .await
            .unwrap();

        assert_eq!(changes.deleted_ids.len(), 1);
        assert_eq!(changes.deleted_ids[0].0, "ghost.md");
    }

    // ── Link extraction tests ─────────────────────────────────────────────────

    #[test]
    fn parse_links_wikilink_simple() {
        let links = parse_links("See [[PageA]] for details.");
        assert!(links.contains(&"PageA".to_string()), "got: {links:?}");
    }

    #[test]
    fn parse_links_wikilink_alias() {
        let links = parse_links("See [[PageB|some alias]] here.");
        assert!(links.contains(&"PageB".to_string()), "got: {links:?}");
        assert!(!links.contains(&"some alias".to_string()));
    }

    #[test]
    fn parse_links_markdown_relative() {
        let links = parse_links("Read [the guide](notes/guide.md) now.");
        assert!(links.contains(&"notes/guide.md".to_string()), "got: {links:?}");
    }

    #[test]
    fn parse_links_ignores_absolute_urls() {
        let links = parse_links("See [Google](https://google.com) or [local](page.md).");
        assert!(!links.iter().any(|l| l.starts_with("http")), "got: {links:?}");
        assert!(links.contains(&"page.md".to_string()));
    }

    #[test]
    fn parse_links_self_reference_excluded() {
        let links = parse_links_for_doc("See [[self]] and [[Other]].", "self");
        assert!(!links.contains(&"self".to_string()), "self-ref must be excluded: {links:?}");
        assert!(links.contains(&"Other".to_string()));
    }

    #[test]
    fn parse_links_deduplicates() {
        let links = parse_links("[[A]] and [[A]] again.");
        assert_eq!(links.iter().filter(|l| l.as_str() == "A").count(), 1);
    }

    #[tokio::test]
    async fn fetch_all_includes_links_in_metadata() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("note.md"),
            "# Note\nSee [[OtherPage]] and [guide](guide.md).",
        )
        .unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(dir.path().to_str().unwrap()));
        plugin.initialize(config, PluginSecrets::default()).await.unwrap();

        let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 100 }).await.unwrap();
        let doc = &stream.documents[0];
        let links = doc.metadata.get("links").expect("links key missing");
        let arr = links.as_array().expect("links must be array");
        let targets: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        assert!(targets.contains(&"OtherPage"), "got: {targets:?}");
        assert!(targets.contains(&"guide.md"), "got: {targets:?}");
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
