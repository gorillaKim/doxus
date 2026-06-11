use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SourceDocId,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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

    // Filter out code blocks before extracting tags to avoid picking up code as tags
    let mut clean_body = String::new();
    let mut in_code_block = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block {
            // Also skip inline code snippets roughly
            let char_iter = line.chars().peekable();
            let mut in_inline = false;
            for c in char_iter {
                if c == '`' {
                    in_inline = !in_inline;
                    continue;
                }
                if !in_inline {
                    clean_body.push(c);
                }
            }
            clean_body.push('\n');
        }
    }

    // Extract inline #tags from clean_body (stricter rule)
    for word in clean_body.split_whitespace() {
        if let Some(tag) = word.strip_prefix('#') {
            // Obsidian tags must start with a letter and contain only alphanumeric, '-', '_', or '/'
            if let Some(first_char) = tag.chars().next() {
                if first_char.is_alphabetic() {
                    let mut clean_tag = String::new();
                    for c in tag.chars() {
                        if c.is_alphanumeric() || c == '-' || c == '_' || c == '/' {
                            clean_tag.push(c);
                        } else {
                            break;
                        }
                    }
                    if !clean_tag.is_empty() {
                        tags.push(clean_tag);
                    }
                }
            }
        }
    }

    tags
}

/// Extract `aliases:` and `created:` / `date:` from YAML frontmatter.
fn parse_frontmatter_meta(
    fm: &str,
) -> (
    Vec<String>,
    Option<i64>,
    Option<String>,
    std::collections::HashMap<String, serde_json::Value>,
) {
    let mut aliases: Vec<String> = Vec::new();
    let mut created_at: Option<i64> = None;
    let mut title: Option<String> = None;
    let mut metadata = std::collections::HashMap::new();

    let mut lines = fm.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_string();
            let raw_value = value.trim();

            let parsed_value = if raw_value.starts_with('[') {
                // inline list: [a, b]
                let inner = raw_value.trim_start_matches('[').trim_end_matches(']');
                let items: Vec<serde_json::Value> = inner
                    .split(',')
                    .map(|s| {
                        let val = s.trim().trim_matches('"').trim_matches('\'');
                        serde_json::Value::String(val.to_string())
                    })
                    .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                    .collect();

                if key == "aliases" {
                    for item in &items {
                        if let Some(s) = item.as_str() {
                            aliases.push(s.to_string());
                        }
                    }
                }

                serde_json::Value::Array(items)
            } else if raw_value.is_empty() {
                // check for block list:
                // - item
                let mut items = Vec::new();
                while let Some(next) = lines.peek() {
                    let nt = next.trim();
                    if let Some(item_str) = nt.strip_prefix("- ") {
                        let val = item_str
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        items.push(serde_json::Value::String(val.clone()));
                        if key == "aliases" {
                            aliases.push(val);
                        }
                        lines.next();
                    } else {
                        break;
                    }
                }
                if !items.is_empty() {
                    serde_json::Value::Array(items)
                } else {
                    serde_json::Value::Null
                }
            } else {
                // single value
                let val = raw_value.trim_matches('"').trim_matches('\'');

                if key == "aliases" {
                    aliases.push(val.to_string());
                }

                // date/created extraction logic
                if (key == "created" || key == "date") && val.len() >= 10 {
                    let date_part = &val[..10];
                    let parts: Vec<&str> = date_part.split('-').collect();
                    if parts.len() == 3 {
                        if let (Ok(y), Ok(m), Ok(d)) = (
                            parts[0].parse::<i64>(),
                            parts[1].parse::<u32>(),
                            parts[2].parse::<u32>(),
                        ) {
                            let days = (y - 1970) * 365
                                + (y - 1969) / 4
                                + match m {
                                    1 => 0,
                                    2 => 31,
                                    3 => 59,
                                    4 => 90,
                                    5 => 120,
                                    6 => 151,
                                    7 => 181,
                                    8 => 212,
                                    9 => 243,
                                    10 => 273,
                                    11 => 304,
                                    _ => 334,
                                } as i64
                                + d as i64
                                - 1;
                            created_at = Some(days * 86400);
                        }
                    }
                }

                if key == "title" {
                    title = Some(val.to_string());
                }

                serde_json::Value::String(val.to_string())
            };

            if parsed_value != serde_json::Value::Null {
                metadata.insert(key, parsed_value);
            }
        }
    }
    (aliases, created_at, title, metadata)
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
    cached_paths: std::sync::Mutex<Option<(std::time::Instant, Vec<PathBuf>)>>,
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
            cached_paths: std::sync::Mutex::new(None),
        }
    }

    fn vault(&self) -> Result<&PathBuf, PluginError> {
        self.vault_path
            .as_ref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    /// Extract document title based on:
    /// 1. Frontmatter 'title' property
    /// 2. First Markdown H1-H6 header
    /// 3. File stem (filename without extension)
    fn extract_title(
        &self,
        content: &str,
        fm_title: Option<String>,
        file_path: &std::path::Path,
    ) -> Option<String> {
        let first_header = content
            .lines()
            .find(|l| l.trim().starts_with('#'))
            .map(|l| l.trim().trim_start_matches('#').trim().to_string());

        fm_title
            .filter(|s| !s.trim().is_empty())
            .or(first_header.filter(|s| !s.trim().is_empty()))
            .or_else(|| {
                file_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .filter(|s| !s.trim().is_empty())
            })
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

        // 1. Try to find title in frontmatter
        // 2. Try to find first markdown header (any level)
        // 3. Fallback to file stem
        let (aliases, fm_created_at, fm_title, mut metadata) =
            if let Some(rest) = content.strip_prefix("---") {
                let rest = rest.strip_prefix('\n').unwrap_or(rest);
                if let Some(end) = rest.find("\n---") {
                    parse_frontmatter_meta(&rest[..end])
                } else {
                    (vec![], None, None, Default::default())
                }
            } else {
                (vec![], None, None, Default::default())
            };

        let title = self.extract_title(&content, fm_title, file_path);

        let tags = parse_tags(&content);
        let links = doxus_plugin_sdk::links::LinkExtractor::extract_links(&content);
        let file_meta = file_path.metadata().ok();
        let updated_at = file_meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            });
        let fs_created_at = file_meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            });

        let created_at = fm_created_at.or(fs_created_at);

        if !links.is_empty() {
            metadata.insert(
                "links".into(),
                serde_json::Value::Array(
                    links
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
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
            links,
            created_at,
            updated_at,
            relative_path: Some(rel_path),
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
            sync_policy: doxus_plugin_sdk::SyncPolicy::Realtime(doxus_plugin_sdk::WatchOptions {
                root: self.vault_path.clone().unwrap_or_default(),
                ignore_patterns: vec![
                    ".git".to_string(),
                    ".obsidian".to_string(),
                    "node_modules".to_string(),
                ],
                extensions: vec!["md".to_string(), "txt".to_string()],
            }),
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
        let (page_paths, total, offset, page_size) = {
            let mut cache = self.cached_paths.lock().unwrap_or_else(|e| e.into_inner());
            let now = std::time::Instant::now();

            // Validate or refresh cache
            let paths = if let Some((ts, ref paths)) = *cache {
                if now.duration_since(ts) < std::time::Duration::from_secs(300) {
                    paths
                } else {
                    let new_paths = self.collect_markdown_paths(vault);
                    *cache = Some((now, new_paths));
                    cache.as_ref().map(|(_, p)| p).unwrap()
                }
            } else {
                let new_paths = self.collect_markdown_paths(vault);
                *cache = Some((now, new_paths));
                cache.as_ref().map(|(_, p)| p).unwrap()
            };

            let total = paths.len();
            let page_size = opts.page_size;
            let offset: usize = match opts.cursor {
                Some(ref c) => c.parse().unwrap_or(0),
                None => 0,
            };

            let p_paths: Vec<PathBuf> =
                paths.iter().skip(offset).take(page_size).cloned().collect();

            (p_paths, total, offset, page_size)
        };

        let mut documents = Vec::new();
        for file_path in page_paths {
            let rel_path = file_path
                .strip_prefix(vault)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();

            let file_meta = file_path.metadata().ok();
            let updated_at = file_meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });

            // For listing, we use filename as title to avoid reading file body
            let title = file_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string());

            documents.push(RawDocument {
                id: SourceDocId(rel_path.clone()),
                title,
                content: String::new(), // Lightweight
                content_type: ContentType::Markdown,
                url: Some(format!("obsidian://open?path={rel_path}")),
                metadata: HashMap::new(),
                tags: vec![],
                aliases: vec![],
                links: vec![],
                created_at: None,
                updated_at,
                relative_path: Some(rel_path),
            });
        }

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

        let file_meta = path
            .metadata()
            .map_err(|e| PluginError::NotFound(format!("{}: {e}", id.0)))?;

        let content = std::fs::read_to_string(&path)
            .map_err(|e| PluginError::Internal(format!("{}: {e}", id.0)))?;

        // 1. Try to find title in frontmatter
        // 2. Try to find first markdown header (any level)
        // 3. Fallback to file stem
        let (aliases, fm_created_at, fm_title, metadata) =
            if let Some(rest) = content.strip_prefix("---") {
                let rest = rest.strip_prefix('\n').unwrap_or(rest);
                if let Some(end) = rest.find("\n---") {
                    parse_frontmatter_meta(&rest[..end])
                } else {
                    (vec![], None, None, Default::default())
                }
            } else {
                (vec![], None, None, Default::default())
            };

        let title = self.extract_title(&content, fm_title, &path);

        let tags = parse_tags(&content);
        let links = doxus_plugin_sdk::links::LinkExtractor::extract_links(&content)
            .into_iter()
            .filter(|l| l != &id.0)
            .collect();

        let updated_at = file_meta.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        Ok(RawDocument {
            id: id.clone(),
            title,
            content,
            content_type: ContentType::Markdown,
            url: Some(format!("obsidian://open?path={}", id.0)),
            metadata,
            tags,
            aliases,
            links,
            created_at: fm_created_at,
            updated_at,
            relative_path: Some(id.0.clone()),
        })
    }

    async fn health_check(&self) -> HealthStatus {
        match &self.vault_path {
            None => HealthStatus {
                healthy: false,
                message: Some("not initialized".into()),
            },
            Some(path) => {
                if path.exists() {
                    HealthStatus {
                        healthy: true,
                        message: None,
                    }
                } else {
                    HealthStatus {
                        healthy: false,
                        message: Some(format!("vault not found: {}", path.display())),
                    }
                }
            }
        }
    }

    fn supports_write(&self) -> bool {
        true
    }

    async fn create_document(
        &self,
        title: &str,
        content: &str,
        folder: Option<&str>,
        metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<SourceDocId, PluginError> {
        let vault = self.vault()?;

        // 1. Standardize hierarchical path using SDK utility
        let segments = doxus_plugin_sdk::path_utils::parse_hierarchical_path(folder, title)?;
        let folder_segments = if segments.len() > 1 {
            &segments[..segments.len() - 1]
        } else {
            &[]
        };
        let base_title = segments.last().unwrap();

        // 2. Ensure target directory exists
        let mut target_dir = vault.clone();
        for segment in folder_segments {
            target_dir = target_dir.join(segment);
        }

        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir)
                .map_err(|e| PluginError::Internal(format!("Failed to create directory: {}", e)))?;
        }

        // 3. Resolve 'Option B' (Auto-suffixing) for Obsidian
        let mut attempts = 0;
        let final_path;

        loop {
            let current_title =
                doxus_plugin_sdk::path_utils::resolve_unique_title(base_title, attempts)?;
            attempts += 1;

            // Sanitize title for filesystem
            let safe_title = current_title.replace(
                |c: char| {
                    !c.is_alphanumeric() && c != ' ' && c != '-' && c != '_' && c != '(' && c != ')'
                },
                "",
            );
            let filename = format!("{}.md", safe_title.trim());
            let path = target_dir.join(&filename);

            if path.exists() {
                // Conflict -> suffix and retry (Option B)
                continue;
            } else {
                final_path = path;
                break;
            }
        }

        // Convert metadata to YAML frontmatter
        let final_content = if let Some(meta) = metadata {
            if !meta.is_empty() {
                let mut fm = String::from("---\n");
                for (key, val) in meta {
                    let val_str = match val {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Array(arr) => {
                            let items: Vec<String> = arr
                                .iter()
                                .map(|v| {
                                    v.as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| v.to_string())
                                })
                                .collect();
                            format!("\n  - {}", items.join("\n  - "))
                        }
                        _ => val.to_string(),
                    };
                    fm.push_str(&format!("{}: {}\n", key, val_str));
                }
                fm.push_str("---\n");
                format!("{}{}", fm, content)
            } else {
                content.to_string()
            }
        } else {
            content.to_string()
        };

        std::fs::write(&final_path, final_content)
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        let rel_path = final_path.strip_prefix(vault).unwrap_or(&final_path);
        Ok(SourceDocId(rel_path.to_string_lossy().to_string()))
    }

    async fn update_document(
        &self,
        id: &SourceDocId,
        content: Option<&str>,
        metadata: Option<&std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<(), PluginError> {
        let vault = self.vault()?;
        let path = vault.join(&id.0);

        if !path.exists() {
            return Err(PluginError::NotFound(id.0.clone()));
        }

        let current_raw =
            std::fs::read_to_string(&path).map_err(|e| PluginError::Internal(e.to_string()))?;

        // Split current file into frontmatter and body
        let (mut fm_text, mut body_text) = if let Some(rest) = current_raw.strip_prefix("---") {
            if let Some(end_idx) = rest.find("\n---") {
                let fm = &rest[..end_idx];
                let body = &rest[end_idx + 4..];
                (fm.to_string(), body.to_string())
            } else {
                ("".into(), current_raw)
            }
        } else {
            ("".into(), current_raw)
        };

        // Update body if provided
        if let Some(new_body) = content {
            body_text = new_body.to_string();
        }

        // Update metadata if provided
        if let Some(new_meta) = metadata {
            for (key, val) in new_meta {
                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Array(arr) => {
                        let mut list = String::new();
                        for v in arr {
                            let item = v
                                .as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string());
                            list.push_str(&format!("\n  - {}", item));
                        }
                        list
                    }
                    _ => val.to_string(),
                };

                let mut new_lines = Vec::new();
                let mut skipping = false;
                let mut found = false;
                for line in fm_text.lines() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with(&format!("{}:", key)) {
                        skipping = true;
                        found = true;
                        if val_str.contains('\n') {
                            new_lines.push(format!("{}:{}", key, val_str));
                        } else {
                            new_lines.push(format!("{}: {}", key, val_str));
                        }
                    } else if skipping
                        && (trimmed.starts_with("- ") || (trimmed.is_empty() && !line.is_empty()))
                    {
                        continue;
                    } else {
                        skipping = false;
                        new_lines.push(line.to_string());
                    }
                }
                if !found {
                    if val_str.contains('\n') {
                        new_lines.push(format!("{}:{}", key, val_str));
                    } else {
                        new_lines.push(format!("{}: {}", key, val_str));
                    }
                }
                fm_text = new_lines.join("\n");
            }
        }

        // Reconstruct file
        let final_content = if fm_text.is_empty() {
            body_text
        } else {
            format!("---\n{}\n---\n{}", fm_text.trim(), body_text)
        };

        std::fs::write(&path, final_content).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(())
    }

    async fn delete_document(&self, id: &SourceDocId) -> Result<(), PluginError> {
        let vault = self.vault()?;
        let path = vault.join(&id.0);

        if !path.exists() {
            return Err(PluginError::NotFound(id.0.clone()));
        }

        std::fs::remove_file(&path).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(())
    }

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        let vault = self.vault()?;

        // 1. Get total sorted paths (cached if possible)
        let all_paths = {
            let mut cache = self.cached_paths.lock().unwrap_or_else(|e| e.into_inner());
            let now = std::time::Instant::now();
            if let Some((ts, ref paths)) = *cache {
                if now.duration_since(ts) < std::time::Duration::from_secs(60) {
                    paths.clone()
                } else {
                    let new_paths = self.collect_markdown_paths(vault);
                    *cache = Some((now, new_paths.clone()));
                    new_paths
                }
            } else {
                let new_paths = self.collect_markdown_paths(vault);
                *cache = Some((now, new_paths.clone()));
                new_paths
            }
        };

        let total = all_paths.len();
        let offset: usize = match opts.cursor {
            Some(ref c) => c.parse().unwrap_or(0),
            None => 0,
        };
        let page_size = opts.page_size.min(100); // Strict limit to prevent memory spikes

        // 2. Process only the current PAGE of paths for updates
        let mut updated = Vec::new();
        let page_paths = all_paths.iter().skip(offset).take(page_size);

        for file_path in page_paths {
            let mtime = file_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });

            // Re-index if modified after 'since'
            if mtime.map(|t| t > opts.since).unwrap_or(true) {
                if let Ok(doc) = self.read_markdown_file(vault, file_path) {
                    updated.push(doc);
                }
            }
        }

        // 3. Detect deletions: ONLY during the VERY FIRST page of the sync (offset 0).
        // Build the on-disk set lazily here to avoid allocating it on every subsequent page.
        let deleted_ids: Vec<SourceDocId> = if offset == 0 && !opts.known_ids.is_empty() {
            let on_disk: HashSet<String> = all_paths
                .iter()
                .map(|p| {
                    p.strip_prefix(vault)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .to_string()
                })
                .collect();
            opts.known_ids
                .into_iter()
                .filter(|id| !on_disk.contains(&id.0))
                .collect()
        } else {
            vec![]
        };

        let next_cursor = if offset + page_size < total {
            Some((offset + page_size).to_string())
        } else {
            None
        };

        Ok(ChangeSet {
            updated,
            deleted_ids,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_check_missing_vault_is_unhealthy() {
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!("/nonexistent/vault/path_xyz"),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }

    #[tokio::test]
    async fn health_check_existing_vault_is_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 1);
        assert_eq!(stream.documents[0].id.0, "note.md");
    }

    #[tokio::test]
    async fn fetch_all_collects_full_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let content = "---
status: in-progress
category: research
author: \"John Doe\"
priority: 5
aliases: [alias1, alias2]
---
# Note
body context";
        std::fs::write(dir.path().join("meta.md"), content).unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 1);
        let doc = plugin
            .fetch_document(&stream.documents[0].id)
            .await
            .unwrap();

        assert_eq!(
            doc.metadata.get("status").unwrap().as_str().unwrap(),
            "in-progress"
        );
        assert_eq!(
            doc.metadata.get("category").unwrap().as_str().unwrap(),
            "research"
        );
        assert_eq!(
            doc.metadata.get("author").unwrap().as_str().unwrap(),
            "John Doe"
        );
        assert_eq!(doc.metadata.get("priority").unwrap().as_str().unwrap(), "5");

        // Aliases check
        assert!(doc.aliases.contains(&"alias1".to_string()));
        assert!(doc.aliases.contains(&"alias2".to_string()));
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
        config
            .fields
            .insert("path".into(), serde_json::json!("/nonexistent/vault"));
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 2);

        let mut docs = Vec::new();
        for d in &stream.documents {
            docs.push(plugin.fetch_document(&d.id).await.unwrap());
        }
        assert!(docs.iter().any(|d| d.title.as_deref() == Some("My Note")));
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        let doc = plugin
            .fetch_document(&stream.documents[0].id)
            .await
            .unwrap();
        assert_eq!(doc.tags, vec!["rust", "doxus"]);
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        let doc = plugin
            .fetch_document(&stream.documents[0].id)
            .await
            .unwrap();
        assert_eq!(doc.tags, vec!["alpha", "beta"]);
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        let doc = plugin
            .fetch_document(&stream.documents[0].id)
            .await
            .unwrap();
        let tags = &doc.tags;
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"doxus".to_string()));
    }

    #[tokio::test]
    async fn no_frontmatter_no_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plain.md"), "# Plain\nNo frontmatter here.").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        // First page: items 0..10
        let page1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 10,
            })
            .await
            .unwrap();
        assert_eq!(page1.documents.len(), 10);
        assert!(page1.next_cursor.is_some());

        // Second page: items 10..20
        let page2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: page1.next_cursor.clone(),
                page_size: 10,
            })
            .await
            .unwrap();
        assert_eq!(page2.documents.len(), 10);

        // The two pages must be disjoint
        let ids1: std::collections::HashSet<_> = page1.documents.iter().map(|d| &d.id.0).collect();
        let ids2: std::collections::HashSet<_> = page2.documents.iter().map(|d| &d.id.0).collect();
        assert!(
            ids1.is_disjoint(&ids2),
            "pages overlap: {ids1:?} ∩ {ids2:?}"
        );

        // estimated_total should reflect the full vault
        assert_eq!(page1.estimated_total, Some(100));

        // Verify cursor arithmetic: last page has no next_cursor
        let last_cursor = Some("90".to_string());
        let last_page = plugin
            .fetch_all(FetchAllOpts {
                cursor: last_cursor,
                page_size: 10,
            })
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

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
        let links =
            doxus_plugin_sdk::links::LinkExtractor::extract_links("See [[PageA]] for details.");
        assert!(links.contains(&"PageA".to_string()), "got: {links:?}");
    }

    #[test]
    fn parse_links_wikilink_alias() {
        let links =
            doxus_plugin_sdk::links::LinkExtractor::extract_links("See [[PageB|some alias]] here.");
        assert!(links.contains(&"PageB".to_string()), "got: {links:?}");
        assert!(!links.contains(&"some alias".to_string()));
    }

    #[test]
    fn parse_links_markdown_relative() {
        let links = doxus_plugin_sdk::links::LinkExtractor::extract_links(
            "Read [the guide](notes/guide.md) now.",
        );
        assert!(
            links.contains(&"notes/guide.md".to_string()),
            "got: {links:?}"
        );
    }

    #[test]
    fn parse_links_ignores_absolute_urls() {
        let links = doxus_plugin_sdk::links::LinkExtractor::extract_links(
            "Check [Google](https://google.com) and [[Local]].",
        );
        assert!(!links.contains(&"https://google.com".to_string()));
        assert!(links.contains(&"Local".to_string()));
    }

    #[test]
    fn parse_links_self_reference_excluded() {
        // LinkExtractor is raw, filtering happens in fetch_document or similar.
        // For the test purpose, we verify the filter logic we implemented.
        let current_id = SourceDocId("self".into());
        let links =
            doxus_plugin_sdk::links::LinkExtractor::extract_links("See [[self]] and [[Other]].")
                .into_iter()
                .filter(|l| l != &current_id.0)
                .collect::<Vec<_>>();

        assert!(
            !links.contains(&"self".to_string()),
            "self-ref must be excluded: {links:?}"
        );
        assert!(links.contains(&"Other".to_string()));
    }

    #[test]
    fn parse_links_deduplicates() {
        let links = doxus_plugin_sdk::links::LinkExtractor::extract_links("[[A]] and [[A]] again.");
        assert_eq!(
            links.iter().filter(|l: &&String| l.as_str() == "A").count(),
            1
        );
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
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 100,
            })
            .await
            .unwrap();
        let doc = plugin
            .fetch_document(&stream.documents[0].id)
            .await
            .unwrap();
        let links = &doc.links;
        assert!(links.contains(&"OtherPage".to_string()), "got: {links:?}");
        assert!(links.contains(&"guide.md".to_string()), "got: {links:?}");
    }

    #[tokio::test]
    async fn fetch_all_pagination() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(
                dir.path().join(format!("note{i}.md")),
                format!("# Note {i}"),
            )
            .unwrap();
        }

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let page1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 3,
            })
            .await
            .unwrap();
        assert_eq!(page1.documents.len(), 3);
        assert!(page1.next_cursor.is_some());

        let page2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: page1.next_cursor,
                page_size: 3,
            })
            .await
            .unwrap();
        assert_eq!(page2.documents.len(), 2);
        assert!(page2.next_cursor.is_none());
    }

    #[tokio::test]
    async fn supports_write_is_true() {
        let plugin = ObsidianPlugin::new();
        assert!(plugin.supports_write());
    }

    #[tokio::test]
    async fn create_document_success() {
        let dir = tempfile::tempdir().unwrap();
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let id = plugin
            .create_document("New Note", "# New Note\nBody content", None, None)
            .await
            .unwrap();
        assert_eq!(id.0, "New Note.md");

        let path = dir.path().join("New Note.md");
        assert!(path.exists());
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Body content"));
    }

    #[tokio::test]
    async fn create_document_conflict_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Existing.md"), "already here").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let id = plugin
            .create_document("Existing", "new content", None, None)
            .await
            .unwrap();
        assert_eq!(id.0, "Existing (1).md");
        assert!(dir.path().join("Existing (1).md").exists());
    }

    #[tokio::test]
    async fn create_document_with_metadata_creates_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let mut metadata = HashMap::new();
        metadata.insert("tags".into(), serde_json::json!(["rust", "testing"]));
        metadata.insert("status".into(), serde_json::json!("draft"));

        let id = plugin
            .create_document("Metadata Test", "Body here", None, Some(&metadata))
            .await
            .unwrap();
        assert_eq!(id.0, "Metadata Test.md");

        let path = dir.path().join("Metadata Test.md");
        let content = std::fs::read_to_string(path).unwrap();

        assert!(
            content.starts_with("---\n"),
            "Must start with frontmatter delimiter"
        );
        assert!(
            content.contains("status: draft"),
            "Must contain status metadata"
        );
        // Body must appear after frontmatter
        assert!(content.contains("Body here"), "Must contain body content");
    }
    #[tokio::test]
    async fn update_document_success() {
        let dir = tempfile::tempdir().unwrap();
        let note_path = dir.path().join("Note.md");
        std::fs::write(&note_path, "---\ntags: [old]\n---\n# Note\nOld body").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let mut new_meta = std::collections::HashMap::new();
        new_meta.insert("tags".into(), serde_json::json!(["new", "tags"]));
        new_meta.insert("status".into(), serde_json::json!("updated"));

        plugin
            .update_document(
                &SourceDocId("Note.md".into()),
                Some("# Note\nNew body"),
                Some(&new_meta),
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&note_path).unwrap();
        assert!(content.contains("New body"));
        assert!(content.contains("status: updated"));
        assert!(content.contains("- new"));
        assert!(content.contains("- tags"));
        assert!(!content.contains("- old"));
    }

    #[tokio::test]
    async fn delete_document_success() {
        let dir = tempfile::tempdir().unwrap();
        let note_path = dir.path().join("DeleteMe.md");
        std::fs::write(&note_path, "to be deleted").unwrap();

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert(
            "path".into(),
            serde_json::json!(dir.path().to_str().unwrap()),
        );
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        plugin
            .delete_document(&SourceDocId("DeleteMe.md".into()))
            .await
            .unwrap();
        assert!(!note_path.exists());
    }
}
