use async_trait::async_trait;
use base64::prelude::*;
use doxus_plugin_sdk::{
    validate_base_url, Capabilities, ChangeSet, ContentType, DocSource, DocumentStream,
    FetchAllOpts, FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind,
    PluginMetadata, PluginSecrets, RawDocument, SecretValue, SourceDocId,
};
use serde::Deserialize;
use std::collections::HashMap;

// ── GitHub API response shapes ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    pub updated_at: String,
    /// pull_request field present means it's a PR, not a plain issue
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubWikiPage {
    pub title: String,
    pub content: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubDiscussion {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub html_url: String,
    pub updated_at: String,
}

// ── Cursor helpers ────────────────────────────────────────────────────────────

/// Structured cursor so fetch_all can sequence Issues → Wiki → Discussions.
/// Format: `"{source}:{page_or_cursor}"`, e.g. `"issues:1"`, `"wiki:1"`, `"discussions:abc"`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FetchCursor {
    Issues(u64),
    Wiki(u64),
    Discussions(u64),
}

impl FetchCursor {
    fn parse(s: &str) -> Option<Self> {
        let (kind, rest) = s.split_once(':')?;
        match kind {
            "issues" => rest.parse().ok().map(FetchCursor::Issues),
            "wiki" => rest.parse().ok().map(FetchCursor::Wiki),
            "discussions" => rest.parse().ok().map(FetchCursor::Discussions),
            _ => None,
        }
    }
}

impl std::fmt::Display for FetchCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchCursor::Issues(p) => write!(f, "issues:{p}"),
            FetchCursor::Wiki(p) => write!(f, "wiki:{p}"),
            FetchCursor::Discussions(p) => write!(f, "discussions:{p}"),
        }
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub base_url: String,
    pub token: Option<String>,
    pub include_closed: bool,
    /// Which sources to include (default: all three)
    pub include_wiki: bool,
    pub include_discussions: bool,
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct GitHubPlugin {
    meta: PluginMetadata,
    config: Option<GitHubConfig>,
    client: reqwest::Client,
}

impl GitHubPlugin {
    pub fn new() -> Self {
        Self {
            meta: PluginMetadata {
                id: "com.doxus.github".into(),
                name: "GitHub".into(),
                version: "0.1.0".into(),
                kind: PluginKind::External,
            },
            config: None,
            client: reqwest::ClientBuilder::new()
                .user_agent("doxus-github-plugin/0.1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub fn with_config(config: GitHubConfig) -> Self {
        Self {
            meta: PluginMetadata {
                id: "com.doxus.github".into(),
                name: "GitHub".into(),
                version: "0.1.0".into(),
                kind: PluginKind::External,
            },
            config: Some(config),
            client: reqwest::ClientBuilder::new()
                .user_agent("doxus-github-plugin/0.1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn cfg(&self) -> Result<&GitHubConfig, PluginError> {
        self.config
            .as_ref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    fn issue_to_doc(&self, issue: GitHubIssue) -> RawDocument {
        // Parse updated_at RFC3339 → unix timestamp
        let updated_at = chrono_parse_unix(&issue.updated_at);
        let mut metadata = HashMap::new();
        if let Some(cfg) = self.config.as_ref() {
            let rel_path = format!(
                "{}/{}/Issues/{}_{}.md",
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.owner),
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.repo),
                issue.number,
                doxus_plugin_sdk::path_utils::sanitize_name(&issue.title)
            );
            metadata.insert("relative_path".to_string(), serde_json::json!(rel_path));
        }

        let rel_path = metadata
            .get("relative_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        RawDocument {
            id: SourceDocId(format!("issue:{}", issue.number)),
            title: Some(issue.title),
            content: issue.body.unwrap_or_default(),
            content_type: ContentType::Markdown,
            url: Some(issue.html_url),
            metadata,
            tags: vec!["issue".into(), issue.state],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at,
            relative_path: rel_path,
        }
    }

    fn wiki_to_doc(&self, page: GitHubWikiPage) -> RawDocument {
        let slug = page
            .html_url
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .to_string();
        let mut metadata = HashMap::new();
        if let Some(cfg) = self.config.as_ref() {
            let rel_path = format!(
                "{}/{}/Wiki/{}.md",
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.owner),
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.repo),
                doxus_plugin_sdk::path_utils::sanitize_name(&page.title)
            );
            metadata.insert("relative_path".to_string(), serde_json::json!(rel_path));
        }

        let rel_path = metadata
            .get("relative_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        RawDocument {
            id: SourceDocId(format!("wiki:{slug}")),
            title: Some(page.title),
            content: page.content.unwrap_or_default(),
            content_type: ContentType::Markdown,
            url: Some(page.html_url),
            metadata,
            tags: vec!["wiki".into()],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: rel_path,
        }
    }

    fn discussion_to_doc(&self, d: GitHubDiscussion) -> RawDocument {
        let updated_at = chrono_parse_unix(&d.updated_at);
        let mut metadata = HashMap::new();
        if let Some(cfg) = self.config.as_ref() {
            let rel_path = format!(
                "{}/{}/Discussions/{}_{}.md",
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.owner),
                doxus_plugin_sdk::path_utils::sanitize_name(&cfg.repo),
                d.number,
                doxus_plugin_sdk::path_utils::sanitize_name(&d.title)
            );
            metadata.insert("relative_path".to_string(), serde_json::json!(rel_path));
        }

        let rel_path = metadata
            .get("relative_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        RawDocument {
            id: SourceDocId(format!("discussion:{}", d.number)),
            title: Some(d.title),
            content: d.body.unwrap_or_default(),
            content_type: ContentType::Markdown,
            url: Some(d.html_url),
            metadata,
            tags: vec!["discussion".into()],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at,
            relative_path: rel_path,
        }
    }

    fn add_auth(
        &self,
        builder: reqwest::RequestBuilder,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        if let Some(tok) = token {
            builder.header("Authorization", format!("Bearer {tok}"))
        } else {
            builder
        }
    }

    /// Fetch one page of issues. Returns (docs, has_more_pages).
    async fn fetch_issues_page(
        &self,
        cfg: &GitHubConfig,
        page: u64,
        page_size: usize,
    ) -> Result<(Vec<RawDocument>, bool), PluginError> {
        let state = if cfg.include_closed { "all" } else { "open" };
        let url = format!("{}/repos/{}/{}/issues", cfg.base_url, cfg.owner, cfg.repo);
        let req = self.client.get(&url).query(&[
            ("state", state),
            ("page", &page.to_string()),
            ("per_page", &page_size.to_string()),
        ]);
        let req = self.add_auth(req, cfg.token.as_deref());
        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;
        match resp.status() {
            s if s.is_success() => {}
            reqwest::StatusCode::UNAUTHORIZED => return Err(PluginError::AuthRequired),
            reqwest::StatusCode::FORBIDDEN => {
                return Err(PluginError::PermissionDenied("GitHub API".into()))
            }
            s if s == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                return Err(PluginError::RateLimited {
                    retry_after_secs: 60,
                })
            }
            s => return Err(PluginError::NetworkError(format!("HTTP {s}"))),
        }
        let issues: Vec<GitHubIssue> = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        let has_more = !issues.is_empty();
        // Filter out pull requests (GitHub issues endpoint returns both)
        let docs = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| self.issue_to_doc(i))
            .collect();
        Ok((docs, has_more))
    }

    /// Fetch one page of wiki pages (GitHub Enterprise / REST).
    async fn fetch_wiki_page(
        &self,
        cfg: &GitHubConfig,
        page: u64,
        page_size: usize,
    ) -> Result<(Vec<RawDocument>, bool), PluginError> {
        let url = format!(
            "{}/repos/{}/{}/wiki/pages",
            cfg.base_url, cfg.owner, cfg.repo
        );
        let req = self.client.get(&url).query(&[
            ("page", page.to_string().as_str()),
            ("per_page", page_size.to_string().as_str()),
        ]);
        let req = self.add_auth(req, cfg.token.as_deref());
        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;
        match resp.status() {
            s if s.is_success() => {}
            // 404 means no wiki — treat as empty, not an error
            reqwest::StatusCode::NOT_FOUND => return Ok((vec![], false)),
            reqwest::StatusCode::UNAUTHORIZED => return Err(PluginError::AuthRequired),
            s => return Err(PluginError::NetworkError(format!("HTTP {s}"))),
        }
        let pages: Vec<GitHubWikiPage> = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        let has_more = !pages.is_empty();
        let docs = pages.into_iter().map(|p| self.wiki_to_doc(p)).collect();
        Ok((docs, has_more))
    }

    /// Fetch one page of discussions via REST endpoint (non-GraphQL for testability).
    async fn fetch_discussions_page(
        &self,
        cfg: &GitHubConfig,
        page: u64,
        page_size: usize,
    ) -> Result<(Vec<RawDocument>, bool), PluginError> {
        let url = format!(
            "{}/repos/{}/{}/discussions",
            cfg.base_url, cfg.owner, cfg.repo
        );
        let req = self.client.get(&url).query(&[
            ("page", page.to_string().as_str()),
            ("per_page", page_size.to_string().as_str()),
        ]);
        let req = self.add_auth(req, cfg.token.as_deref());
        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;
        match resp.status() {
            s if s.is_success() => {}
            // 404 means discussions disabled — treat as empty
            reqwest::StatusCode::NOT_FOUND => return Ok((vec![], false)),
            reqwest::StatusCode::UNAUTHORIZED => return Err(PluginError::AuthRequired),
            s => return Err(PluginError::NetworkError(format!("HTTP {s}"))),
        }
        let discussions: Vec<GitHubDiscussion> = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        let has_more = !discussions.is_empty();
        let docs = discussions
            .into_iter()
            .map(|d| self.discussion_to_doc(d))
            .collect();
        Ok((docs, has_more))
    }

    /// Fetch issues updated since a unix timestamp, using ETag from cursor for conditional requests.
    async fn fetch_changed_issues(
        &self,
        cfg: &GitHubConfig,
        since_unix: i64,
        page: u64,
        page_size: usize,
        cursor_etag: Option<String>,
    ) -> Result<(Vec<RawDocument>, bool, Option<String>), PluginError> {
        // Convert unix ts → RFC3339 for the `since` query param
        let since_str = unix_to_rfc3339(since_unix);
        let state = if cfg.include_closed { "all" } else { "open" };
        let url = format!("{}/repos/{}/{}/issues", cfg.base_url, cfg.owner, cfg.repo);
        let mut req = self.client.get(&url).query(&[
            ("state", state),
            ("since", since_str.as_str()),
            ("page", page.to_string().as_str()),
            ("per_page", page_size.to_string().as_str()),
        ]);
        req = self.add_auth(req, cfg.token.as_deref());
        // Attach ETag from cursor for conditional GET (304 = no changes)
        if let Some(ref etag) = cursor_etag {
            req = req.header("If-None-Match", etag.as_str());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        // Extract new ETag from response before consuming body
        let new_etag = resp
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        match resp.status() {
            reqwest::StatusCode::NOT_MODIFIED => {
                // No changes since last fetch
                return Ok((vec![], false, new_etag));
            }
            s if s.is_success() => {}
            reqwest::StatusCode::UNAUTHORIZED => return Err(PluginError::AuthRequired),
            s => return Err(PluginError::NetworkError(format!("HTTP {s}"))),
        }
        let issues: Vec<GitHubIssue> = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        let has_more = !issues.is_empty();
        let docs = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| self.issue_to_doc(i))
            .collect();
        Ok((docs, has_more, new_etag))
    }
}

impl Default for GitHubPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocSource for GitHubPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: true,
            oauth: true,
            native_search: false,
            sync_policy: doxus_plugin_sdk::SyncPolicy::Interval { seconds: 7200 },
        }
    }

    fn guide(&self) -> Option<&'static str> {
        Some(include_str!("../GUIDE.md"))
    }

    fn supports_write(&self) -> bool {
        true
    }

    async fn create_document(
        &self,
        title: &str,
        content: &str,
        folder: Option<&str>,
        _metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<SourceDocId, PluginError> {
        let cfg = self.cfg()?;
        if cfg.token.is_none() {
            return Err(PluginError::AuthRequired);
        }

        // 1. Standardize hierarchical path using SDK utility
        let segments = doxus_plugin_sdk::path_utils::parse_hierarchical_path(folder, title)?;
        let folder_part = if segments.len() > 1 {
            segments[..segments.len() - 1].join("/")
        } else {
            "".to_string()
        };
        let base_title = segments.last().unwrap();

        let mut attempts = 0;
        let final_path;

        // 2. Resolve 'Option B' (Auto-suffixing) for GitHub
        loop {
            let current_title = doxus_plugin_sdk::path_utils::resolve_unique_title(base_title, attempts)?;
            attempts += 1;

            let path = if folder_part.is_empty() {
                format!("{}.md", current_title)
            } else {
                format!("{}/{}.md", folder_part, current_title)
            };

            // Check if exists
            let url = format!(
                "{}/repos/{}/{}/contents/{}",
                cfg.base_url, cfg.owner, cfg.repo, path
            );
            let req = self.client.get(&url);
            let req = self.add_auth(req, cfg.token.as_deref());
            let resp = req
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;

            if resp.status().is_success() {
                // Already exists -> suffix and retry (Option B)
                continue;
            } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
                // Good to go
                final_path = path;
                break;
            } else {
                let status = resp.status();
                return Err(PluginError::NetworkError(format!(
                    "Unexpected status checking existence: HTTP {}",
                    status
                )));
            }
        }

        // 3. Create Final Page
        let url = format!(
            "{}/repos/{}/{}/contents/{}",
            cfg.base_url, cfg.owner, cfg.repo, final_path
        );
        let encoded_content = BASE64_STANDARD.encode(content);

        let body = serde_json::json!({
            "message": format!("Create document: {}", title),
            "content": encoded_content,
        });

        let req = self.client.put(&url).json(&body);
        let req = self.add_auth(req, cfg.token.as_deref());

        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(PluginError::NetworkError(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        Ok(SourceDocId(final_path))
    }

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError> {
        for field in ["owner", "repo"] {
            match config.fields.get(field).and_then(|v| v.as_str()) {
                None | Some("") => {
                    return Err(PluginError::ConfigInvalid(format!(
                        "missing required field: {field}"
                    )))
                }
                Some(val) => {
                    // Prevent URL path injection via owner/repo containing '/' or '..'
                    if val.contains('/') || val.contains("..") {
                        return Err(PluginError::ConfigInvalid(format!(
                            "invalid characters in {field}: {val}"
                        )));
                    }
                }
            }
        }
        // SSRF protection: validate base_url if provided
        if let Some(base_url) = config.fields.get("base_url").and_then(|v| v.as_str()) {
            validate_base_url(base_url)?;
        }
        Ok(())
    }

    async fn initialize(
        &mut self,
        config: PluginConfig,
        secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        let owner = config
            .fields
            .get("owner")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| PluginError::ConfigInvalid("missing owner".into()))?;
        let repo = config
            .fields
            .get("repo")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| PluginError::ConfigInvalid("missing repo".into()))?;
        let base_url = config
            .fields
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://api.github.com")
            .trim_end_matches('/')
            .to_string();
        validate_base_url(&base_url)?;
        let include_closed = config
            .fields
            .get("include_closed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_wiki = config
            .fields
            .get("include_wiki")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_discussions = config
            .fields
            .get("include_discussions")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let token = secrets
            .fields
            .get("token")
            .map(|sv| match sv {
                SecretValue::Text(t) => t.clone(),
                SecretValue::Token { value, .. } => value.clone(),
            })
            .or_else(|| {
                config
                    .fields
                    .get("token")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

        self.config = Some(GitHubConfig {
            owner,
            repo,
            base_url,
            token,
            include_closed,
            include_wiki,
            include_discussions,
        });
        Ok(())
    }

    /// Fetch all documents across Issues, Wiki pages, and Discussions.
    ///
    /// Cursor format: `"{source}:{page}"` where source is `issues`, `wiki`, or `discussions`.
    /// Sources are iterated in order: issues first, then wiki, then discussions.
    /// A `None` cursor starts at `issues:1`.
    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let cfg = self.cfg()?;

        let cursor = opts
            .cursor
            .as_deref()
            .and_then(FetchCursor::parse)
            .unwrap_or(FetchCursor::Issues(1));

        match cursor {
            FetchCursor::Issues(page) => {
                let (docs, has_more) = self.fetch_issues_page(cfg, page, opts.page_size).await?;
                let next_cursor = if has_more {
                    Some(FetchCursor::Issues(page + 1).to_string())
                } else if cfg.include_wiki {
                    // Issues exhausted — move to wiki
                    Some(FetchCursor::Wiki(1).to_string())
                } else if cfg.include_discussions {
                    Some(FetchCursor::Discussions(1).to_string())
                } else {
                    None
                };
                Ok(DocumentStream {
                    documents: docs,
                    next_cursor,
                    estimated_total: None,
                })
            }
            FetchCursor::Wiki(page) => {
                let (docs, has_more) = self.fetch_wiki_page(cfg, page, opts.page_size).await?;
                let next_cursor = if has_more {
                    Some(FetchCursor::Wiki(page + 1).to_string())
                } else if cfg.include_discussions {
                    // Wiki exhausted — move to discussions
                    Some(FetchCursor::Discussions(1).to_string())
                } else {
                    None
                };
                Ok(DocumentStream {
                    documents: docs,
                    next_cursor,
                    estimated_total: None,
                })
            }
            FetchCursor::Discussions(page) => {
                let (docs, has_more) = self
                    .fetch_discussions_page(cfg, page, opts.page_size)
                    .await?;
                let next_cursor = if has_more {
                    Some(FetchCursor::Discussions(page + 1).to_string())
                } else {
                    None
                };
                Ok(DocumentStream {
                    documents: docs,
                    next_cursor,
                    estimated_total: None,
                })
            }
        }
    }

    /// Incremental sync: fetch issues updated since `opts.since` (unix timestamp).
    /// Uses ETag embedded in cursor for conditional requests (304 = no changes).
    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        let cfg = self.cfg()?;
        let (page, cursor_etag) = opts
            .cursor
            .as_deref()
            .map(parse_changes_cursor)
            .unwrap_or((1, None));

        let (docs, has_more, new_etag) = self
            .fetch_changed_issues(cfg, opts.since, page, opts.page_size, cursor_etag)
            .await?;

        // Build next_cursor for further pages, embedding the new ETag so the
        // next call can send If-None-Match without mutating &self.
        let next_cursor = if has_more {
            let etag_part = new_etag.as_deref().unwrap_or("");
            Some(format!("changes:{}|{}", page + 1, etag_part))
        } else {
            None
        };

        // Detect deletions: issues known to caller but not in updated set
        // We only detect deletions if this is a single-page response (no next_cursor)
        // since we can't know the full result set across pages.
        let deleted_ids = if next_cursor.is_none() && !opts.known_ids.is_empty() {
            let updated_ids: std::collections::HashSet<&str> =
                docs.iter().map(|d| d.id.0.as_str()).collect();
            // Check each known issue ID against the API
            // For now, conservatively report no deletions (requires separate API calls)
            // to avoid false positives. Full deletion detection requires HEAD requests.
            let _ = updated_ids;
            vec![]
        } else {
            vec![]
        };

        // new_etag is already embedded in next_cursor above; no mutation of &self needed.

        Ok(ChangeSet {
            updated: docs,
            deleted_ids,
            next_cursor,
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        let cfg = self.cfg()?;

        // Dispatch by id prefix
        if let Some(number_str) = id.0.strip_prefix("issue:") {
            let _: u64 = number_str
                .parse()
                .map_err(|_| PluginError::NotFound(format!("invalid document id: {}", id.0)))?;
            let url = format!(
                "{}/repos/{}/{}/issues/{}",
                cfg.base_url, cfg.owner, cfg.repo, number_str
            );
            let req = self.add_auth(self.client.get(&url), cfg.token.as_deref());
            let resp = req
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::NotFound(id.0.clone()));
            }
            if !resp.status().is_success() {
                return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
            }
            let issue: GitHubIssue = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            return Ok(self.issue_to_doc(issue));
        }

        if let Some(slug) = id.0.strip_prefix("wiki:") {
            // Sanitize slug
            if slug.contains('/') || slug.contains("..") {
                return Err(PluginError::NotFound(format!(
                    "invalid wiki slug: {}",
                    id.0
                )));
            }
            let url = format!(
                "{}/repos/{}/{}/wiki/pages/{}",
                cfg.base_url, cfg.owner, cfg.repo, slug
            );
            let req = self.add_auth(self.client.get(&url), cfg.token.as_deref());
            let resp = req
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::NotFound(id.0.clone()));
            }
            if !resp.status().is_success() {
                return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
            }
            let page: GitHubWikiPage = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            return Ok(self.wiki_to_doc(page));
        }

        if let Some(number_str) = id.0.strip_prefix("discussion:") {
            let _: u64 = number_str
                .parse()
                .map_err(|_| PluginError::NotFound(format!("invalid document id: {}", id.0)))?;
            let url = format!(
                "{}/repos/{}/{}/discussions/{}",
                cfg.base_url, cfg.owner, cfg.repo, number_str
            );
            let req = self.add_auth(self.client.get(&url), cfg.token.as_deref());
            let resp = req
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::NotFound(id.0.clone()));
            }
            if !resp.status().is_success() {
                return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
            }
            let d: GitHubDiscussion = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            return Ok(self.discussion_to_doc(d));
        }

        // Legacy support: bare numeric IDs treated as issues
        if id.0.parse::<u64>().is_ok() {
            let url = format!(
                "{}/repos/{}/{}/issues/{}",
                cfg.base_url, cfg.owner, cfg.repo, id.0
            );
            let req = self.add_auth(self.client.get(&url), cfg.token.as_deref());
            let resp = req
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::NotFound(id.0.clone()));
            }
            if !resp.status().is_success() {
                return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
            }
            let issue: GitHubIssue = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            return Ok(self.issue_to_doc(issue));
        }

        Err(PluginError::NotFound(format!(
            "unrecognized document id format: {}",
            id.0
        )))
    }

    async fn health_check(&self) -> HealthStatus {
        let cfg = match self.config.as_ref() {
            Some(c) => c,
            None => {
                return HealthStatus {
                    healthy: false,
                    message: Some("plugin not initialized".into()),
                }
            }
        };
        let url = format!("{}/repos/{}/{}", cfg.base_url, cfg.owner, cfg.repo);
        let req = self.client.get(&url);
        let req = self.add_auth(req, cfg.token.as_deref());
        match req.send().await {
            Ok(resp) if resp.status().is_success() => HealthStatus {
                healthy: true,
                message: None,
            },
            Ok(resp) => HealthStatus {
                healthy: false,
                message: Some(format!("HTTP {}", resp.status())),
            },
            Err(e) => HealthStatus {
                healthy: false,
                message: Some(e.to_string()),
            },
        }
    }
}

// ── Cursor helpers (changes) ──────────────────────────────────────────────────

/// Parse a changes cursor string into (page, etag).
/// Format: `"changes:{page}"` (legacy) or `"changes:{page}|{etag}"`.
fn parse_changes_cursor(cursor: &str) -> (u64, Option<String>) {
    let rest = match cursor.strip_prefix("changes:") {
        Some(r) => r,
        None => return (1, None),
    };
    if let Some((page_str, etag)) = rest.split_once('|') {
        let page = page_str.parse().unwrap_or(1);
        let etag = if etag.is_empty() {
            None
        } else {
            Some(etag.to_string())
        };
        (page, etag)
    } else {
        (rest.parse().unwrap_or(1), None)
    }
}

// ── Time helpers ──────────────────────────────────────────────────────────────

use chrono::{DateTime, TimeZone, Utc};

/// Parse RFC3339/ISO8601 string to unix timestamp (seconds).
/// Returns None if parsing fails rather than panicking.
fn chrono_parse_unix(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

/// Convert unix timestamp (seconds) to RFC3339 string for the GitHub `since` parameter.
fn unix_to_rfc3339(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn make_issue(number: u64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": title,
            "body": format!("Body of issue {number}"),
            "state": "open",
            "html_url": format!("https://github.com/foo/bar/issues/{number}"),
            "updated_at": "2026-01-01T00:00:00Z"
        })
    }

    fn make_wiki_page(title: &str, slug: &str) -> serde_json::Value {
        serde_json::json!({
            "title": title,
            "content": format!("# {title}\n\nContent for {title}."),
            "html_url": format!("https://github.com/foo/bar/wiki/{slug}")
        })
    }

    fn make_discussion(number: u64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": title,
            "body": format!("Discussion body {number}"),
            "html_url": format!("https://github.com/foo/bar/discussions/{number}"),
            "updated_at": "2026-01-01T00:00:00Z"
        })
    }

    fn make_plugin(server: &MockServer) -> GitHubPlugin {
        GitHubPlugin::with_config(GitHubConfig {
            owner: "foo".into(),
            repo: "bar".into(),
            base_url: server.uri(),
            token: None,
            include_closed: false,
            include_wiki: true,
            include_discussions: true,
        })
    }

    fn make_plugin_with_token(server: &MockServer, token: &str) -> GitHubPlugin {
        GitHubPlugin::with_config(GitHubConfig {
            owner: "foo".into(),
            repo: "bar".into(),
            base_url: server.uri(),
            token: Some(token.to_string()),
            include_closed: false,
            include_wiki: true,
            include_discussions: true,
        })
    }

    fn make_plugin_issues_only(server: &MockServer) -> GitHubPlugin {
        GitHubPlugin::with_config(GitHubConfig {
            owner: "foo".into(),
            repo: "bar".into(),
            base_url: server.uri(),
            token: None,
            include_closed: false,
            include_wiki: false,
            include_discussions: false,
        })
    }

    // ── Original tests (must not break) ───────────────────────────────────────

    #[tokio::test]
    async fn fetch_issues_returns_documents() {
        let server = MockServer::start().await;
        let body = serde_json::json!([make_issue(1, "First"), make_issue(2, "Second")]);
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(query_param("state", "open"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin_issues_only(&server);
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 2);
        assert_eq!(stream.documents[0].title.as_deref(), Some("First"));
        assert_eq!(stream.documents[1].title.as_deref(), Some("Second"));
        // has_more=true → next_cursor is issues:2
        assert_eq!(stream.next_cursor, Some("issues:2".to_string()));
    }

    #[tokio::test]
    async fn fetch_issues_empty_page_ends_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
            .mount(&server)
            .await;

        let plugin = make_plugin_issues_only(&server);
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: Some("issues:3".into()),
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 0);
        assert_eq!(stream.next_cursor, None);
    }

    #[tokio::test]
    async fn fetch_document_returns_issue_body() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "number": 42,
            "title": "Bug",
            "body": "Details",
            "state": "open",
            "html_url": "https://github.com/foo/bar/issues/42",
            "updated_at": "2026-01-01T00:00:00Z"
        });
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        // New id format: "issue:42"
        let doc = plugin
            .fetch_document(&SourceDocId("issue:42".into()))
            .await
            .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Bug"));
        assert_eq!(doc.content, "Details");
    }

    #[tokio::test]
    async fn fetch_document_legacy_numeric_id() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "number": 42,
            "title": "Legacy",
            "body": "Old format",
            "state": "open",
            "html_url": "https://github.com/foo/bar/issues/42",
            "updated_at": "2026-01-01T00:00:00Z"
        });
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        // Legacy bare numeric ID
        let doc = plugin
            .fetch_document(&SourceDocId("42".into()))
            .await
            .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Legacy"));
    }

    #[tokio::test]
    async fn fetch_document_rejects_non_numeric_id() {
        let server = MockServer::start().await;
        let plugin = make_plugin(&server);
        let result = plugin
            .fetch_document(&SourceDocId("../etc/passwd".into()))
            .await;
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[tokio::test]
    async fn auth_header_is_sent_when_token_present() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&serde_json::json!([make_issue(1, "Authed")])),
            )
            .mount(&server)
            .await;

        let plugin = make_plugin_with_token(&server, "test-token");
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 1);
    }

    #[test]
    fn validate_base_url_accepts_https_public_host() {
        assert!(validate_base_url("https://api.github.com").is_ok());
        assert!(validate_base_url("https://github.mycompany.com").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_http() {
        let err = validate_base_url("http://api.github.com").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn validate_base_url_rejects_localhost() {
        assert!(validate_base_url("https://localhost/api").is_err());
        assert!(validate_base_url("https://127.0.0.1/api").is_err());
    }

    #[test]
    fn validate_base_url_rejects_link_local() {
        assert!(validate_base_url("https://169.254.169.254/latest/meta-data").is_err());
    }

    // ── TDD tests ─────────────────────────────────────────────────────────────

    #[test]
    fn github_plugin_metadata_is_correct() {
        let plugin = GitHubPlugin::new();
        let meta = plugin.metadata();
        assert_eq!(meta.id, "com.doxus.github");
        assert_eq!(meta.name, "GitHub");
    }

    #[tokio::test]
    async fn validate_config_requires_repo() {
        let plugin = GitHubPlugin::new();
        let config = PluginConfig::default();
        let result = plugin.validate_config(&config).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn validate_config_accepts_valid_config() {
        let plugin = GitHubPlugin::new();
        let mut config = PluginConfig::default();
        config
            .fields
            .insert("owner".into(), serde_json::json!("myorg"));
        config
            .fields
            .insert("repo".into(), serde_json::json!("myrepo"));
        let result = plugin.validate_config(&config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn health_check_before_init_is_unhealthy() {
        let plugin = GitHubPlugin::new();
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }

    #[tokio::test]
    async fn fetch_all_before_init_returns_error() {
        let plugin = GitHubPlugin::new();
        let result = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await;
        assert!(matches!(result, Err(PluginError::Internal(_))));
    }

    #[tokio::test]
    async fn fetch_all_with_mock_server() {
        let server = MockServer::start().await;
        let body = serde_json::json!([make_issue(1, "Issue One"), make_issue(2, "Issue Two")]);
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/issues"))
            .and(query_param("state", "open"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        // Use with_config directly to bypass SSRF validation (wiremock uses HTTP localhost).
        let plugin = GitHubPlugin::with_config(GitHubConfig {
            owner: "owner".into(),
            repo: "repo".into(),
            base_url: server.uri(),
            token: None,
            include_closed: false,
            include_wiki: false,
            include_discussions: false,
        });

        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 2);
    }

    // ── New: Wiki tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn fetch_all_wiki_pages() {
        let server = MockServer::start().await;
        // Issues page 1 returns empty → moves to wiki
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
            .mount(&server)
            .await;

        let wiki_body = serde_json::json!([
            make_wiki_page("Home", "Home"),
            make_wiki_page("Setup", "Setup")
        ]);
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/wiki/pages"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&wiki_body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        // First call: issues (empty) → transition to wiki
        let stream1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream1.documents.len(), 0);
        assert_eq!(stream1.next_cursor, Some("wiki:1".to_string()));

        // Second call: wiki page 1
        let stream2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: stream1.next_cursor,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream2.documents.len(), 2);
        assert_eq!(stream2.documents[0].title.as_deref(), Some("Home"));
        assert_eq!(stream2.documents[1].title.as_deref(), Some("Setup"));
        // wiki docs have "wiki" tag
        assert!(stream2.documents[0].tags.contains(&"wiki".to_string()));
        // wiki id format: "wiki:{slug}"
        assert!(stream2.documents[0].id.0.starts_with("wiki:"));
    }

    #[tokio::test]
    async fn fetch_all_wiki_404_moves_to_discussions() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/wiki/pages"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let disc_body = serde_json::json!([make_discussion(1, "First Discussion")]);
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/discussions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&disc_body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        // Step 1: issues empty → wiki:1
        let s1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s1.next_cursor, Some("wiki:1".to_string()));

        // Step 2: wiki 404 (empty) → discussions:1
        let s2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: s1.next_cursor,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s2.documents.len(), 0);
        assert_eq!(s2.next_cursor, Some("discussions:1".to_string()));

        // Step 3: discussions page 1
        let s3 = plugin
            .fetch_all(FetchAllOpts {
                cursor: s2.next_cursor,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s3.documents.len(), 1);
        assert_eq!(s3.documents[0].title.as_deref(), Some("First Discussion"));
    }

    // ── New: Discussions tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn fetch_all_discussions_pagination() {
        let server = MockServer::start().await;
        let page1 = serde_json::json!([make_discussion(1, "Alpha"), make_discussion(2, "Beta")]);
        let page2 = serde_json::json!([make_discussion(3, "Gamma")]);
        let page3 = serde_json::json!([]);

        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/discussions"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/discussions"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/discussions"))
            .and(query_param("page", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page3))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);

        let s1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: Some("discussions:1".into()),
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s1.documents.len(), 2);
        assert_eq!(s1.next_cursor, Some("discussions:2".to_string()));

        let s2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: s1.next_cursor,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s2.documents.len(), 1);
        assert_eq!(s2.documents[0].title.as_deref(), Some("Gamma"));
        assert_eq!(s2.next_cursor, Some("discussions:3".to_string()));

        let s3 = plugin
            .fetch_all(FetchAllOpts {
                cursor: s2.next_cursor,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(s3.documents.len(), 0);
        assert_eq!(s3.next_cursor, None);
    }

    #[tokio::test]
    async fn fetch_document_wiki_page() {
        let server = MockServer::start().await;
        let body = make_wiki_page("Installation", "Installation");
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/wiki/pages/Installation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let doc = plugin
            .fetch_document(&SourceDocId("wiki:Installation".into()))
            .await
            .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Installation"));
        assert!(doc.tags.contains(&"wiki".to_string()));
    }

    #[tokio::test]
    async fn fetch_document_discussion() {
        let server = MockServer::start().await;
        let body = make_discussion(5, "Feature Request");
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/discussions/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let doc = plugin
            .fetch_document(&SourceDocId("discussion:5".into()))
            .await
            .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Feature Request"));
        assert!(doc.tags.contains(&"discussion".to_string()));
        assert!(doc.id.0.starts_with("discussion:"));
    }

    #[tokio::test]
    async fn fetch_document_wiki_rejects_path_traversal() {
        let server = MockServer::start().await;
        let plugin = make_plugin(&server);
        let result = plugin
            .fetch_document(&SourceDocId("wiki:../../../etc/passwd".into()))
            .await;
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    // ── New: fetch_changes tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn fetch_changes_returns_updated_issues() {
        let server = MockServer::start().await;
        let body = serde_json::json!([
            make_issue(10, "Updated Issue"),
            make_issue(11, "Another Update")
        ]);
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(query_param("since", "2026-01-01T00:00:00Z"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let changes = plugin
            .fetch_changes(FetchChangesOpts {
                since: 1767225600, // 2026-01-01T00:00:00Z
                cursor: None,
                page_size: 50,
                known_ids: vec![],
            })
            .await
            .unwrap();
        assert_eq!(changes.updated.len(), 2);
        assert_eq!(changes.updated[0].title.as_deref(), Some("Updated Issue"));
        // Two results returned → has_more=true → next page cursor (with empty etag part)
        assert_eq!(changes.next_cursor, Some("changes:2|".to_string()));
    }

    #[tokio::test]
    async fn fetch_changes_pagination() {
        let server = MockServer::start().await;
        let page1 = serde_json::json!([make_issue(10, "Page1 Issue")]);
        let page2 = serde_json::json!([]);
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let c1 = plugin
            .fetch_changes(FetchChangesOpts {
                since: 0,
                cursor: None,
                page_size: 50,
                known_ids: vec![],
            })
            .await
            .unwrap();
        assert_eq!(c1.updated.len(), 1);
        assert!(c1
            .next_cursor
            .as_deref()
            .map(|s| s.starts_with("changes:2|"))
            .unwrap_or(false));

        let c2 = plugin
            .fetch_changes(FetchChangesOpts {
                since: 0,
                cursor: c1.next_cursor,
                page_size: 50,
                known_ids: vec![],
            })
            .await
            .unwrap();
        assert_eq!(c2.updated.len(), 0);
        assert_eq!(c2.next_cursor, None);
    }

    #[tokio::test]
    async fn fetch_changes_304_not_modified() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .and(header("If-None-Match", "\"abc123\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        // Pass ETag via cursor (encoded as "changes:1|\"abc123\"")
        let plugin = make_plugin(&server);
        let changes = plugin
            .fetch_changes(FetchChangesOpts {
                since: 0,
                cursor: Some("changes:1|\"abc123\"".to_string()),
                page_size: 50,
                known_ids: vec![],
            })
            .await
            .unwrap();
        assert_eq!(changes.updated.len(), 0);
        assert_eq!(changes.next_cursor, None);
    }

    #[tokio::test]
    async fn fetch_changes_before_init_returns_error() {
        let plugin = GitHubPlugin::new();
        let result = plugin
            .fetch_changes(FetchChangesOpts {
                since: 0,
                cursor: None,
                page_size: 50,
                known_ids: vec![],
            })
            .await;
        assert!(matches!(result, Err(PluginError::Internal(_))));
    }

    // ── New: cursor parsing ────────────────────────────────────────────────────

    #[test]
    fn cursor_roundtrip() {
        let cases = [
            FetchCursor::Issues(1),
            FetchCursor::Issues(99),
            FetchCursor::Wiki(1),
            FetchCursor::Wiki(5),
            FetchCursor::Discussions(1),
            FetchCursor::Discussions(3),
        ];
        for c in &cases {
            let s = c.to_string();
            let parsed = FetchCursor::parse(&s).expect("should parse");
            assert_eq!(*c, parsed);
        }
    }

    #[test]
    fn cursor_invalid_returns_none() {
        assert!(FetchCursor::parse("bogus").is_none());
        assert!(FetchCursor::parse("issues:abc").is_none());
        assert!(FetchCursor::parse("").is_none());
    }

    // ── New: time helpers ──────────────────────────────────────────────────────

    #[test]
    fn unix_to_rfc3339_epoch() {
        assert_eq!(unix_to_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn unix_to_rfc3339_known_date() {
        // 2025-01-01T00:00:00Z = 1735689600
        assert_eq!(unix_to_rfc3339(1735689600), "2025-01-01T00:00:00Z");
        // 2026-01-01T00:00:00Z = 1767225600
        assert_eq!(unix_to_rfc3339(1767225600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn parse_rfc3339_roundtrip() {
        let ts: i64 = 1767225600;
        let s = unix_to_rfc3339(ts);
        let back = chrono_parse_unix(&s).expect("should parse");
        assert_eq!(back, ts);
    }

    #[test]
    fn parse_rfc3339_negative_timezone() {
        // "2026-01-01T00:00:00-05:00" = 2026-01-01T05:00:00Z = 1767243600
        let ts = chrono_parse_unix("2026-01-01T00:00:00-05:00").expect("should parse negative tz");
        assert_eq!(ts, 1767225600 + 5 * 3600);
    }

    #[test]
    fn parse_rfc3339_positive_timezone() {
        // "2026-01-01T02:00:00+02:00" = 2026-01-01T00:00:00Z = 1767225600
        let ts = chrono_parse_unix("2026-01-01T02:00:00+02:00").expect("should parse positive tz");
        assert_eq!(ts, 1767225600);
    }

    #[test]
    fn parse_rfc3339_returns_none_on_invalid() {
        assert!(chrono_parse_unix("not-a-date").is_none());
        assert!(chrono_parse_unix("").is_none());
    }

    // ── New: capabilities ─────────────────────────────────────────────────────

    #[test]
    fn capabilities_incremental_sync_enabled() {
        let plugin = GitHubPlugin::new();
        let caps = plugin.capabilities();
        assert!(caps.incremental_sync);
    }

    // ── New: issues only mode ─────────────────────────────────────────────────

    #[tokio::test]
    async fn fetch_all_issues_only_mode_no_wiki_or_discussions() {
        let server = MockServer::start().await;
        // Empty issues — should NOT transition to wiki or discussions
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
            .mount(&server)
            .await;

        let plugin = make_plugin_issues_only(&server);
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 0);
        assert_eq!(stream.next_cursor, None);
    }

    #[test]
    fn test_issue_relative_path() {
        let mut plugin = GitHubPlugin::new();
        plugin.config = Some(GitHubConfig {
            owner: "owner".into(),
            repo: "repo".into(),
            base_url: "https://api.github.com".into(),
            token: None,
            include_closed: false,
            include_wiki: true,
            include_discussions: true,
        });
        let issue = GitHubIssue {
            number: 123,
            title: "Test Issue: Hello/World".into(),
            body: Some("Body".into()),
            state: "open".into(),
            html_url: "https://github.com/owner/repo/issues/123".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            pull_request: None,
        };
        let doc = plugin.issue_to_doc(issue);
        let rel_path = doc.metadata.get("relative_path").and_then(|v| v.as_str());
        // Expected: owner/repo/Issues/123_Test Issue_ Hello_World.md
        assert_eq!(
            rel_path,
            Some("owner/repo/Issues/123_Test Issue_ Hello_World.md")
        );
    }

    #[test]
    fn test_wiki_relative_path() {
        let mut plugin = GitHubPlugin::new();
        plugin.config = Some(GitHubConfig {
            owner: "owner".into(),
            repo: "repo".into(),
            base_url: "https://api.github.com".into(),
            token: None,
            include_closed: false,
            include_wiki: true,
            include_discussions: true,
        });
        let page = GitHubWikiPage {
            title: "Wiki Page? Title: Good".into(),
            content: Some("Content".into()),
            html_url: "https://github.com/owner/repo/wiki/Wiki-Page".into(),
        };
        let doc = plugin.wiki_to_doc(page);
        let rel_path = doc.metadata.get("relative_path").and_then(|v| v.as_str());
        // Expected: owner/repo/Wiki/Wiki Page_ Title_ Good.md
        assert_eq!(rel_path, Some("owner/repo/Wiki/Wiki Page_ Title_ Good.md"));
    }

    #[test]
    fn test_discussion_relative_path() {
        let mut plugin = GitHubPlugin::new();
        plugin.config = Some(GitHubConfig {
            owner: "owner".into(),
            repo: "repo".into(),
            base_url: "https://api.github.com".into(),
            token: None,
            include_closed: false,
            include_wiki: true,
            include_discussions: true,
        });
        let discussion = GitHubDiscussion {
            number: 456,
            title: "Discussion <Title> | Rules".into(),
            body: Some("Body".into()),
            html_url: "https://github.com/owner/repo/discussions/456".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let doc = plugin.discussion_to_doc(discussion);
        let rel_path = doc.metadata.get("relative_path").and_then(|v| v.as_str());
        // Expected: owner/repo/Discussions/456_Discussion _Title_ _ Rules.md
        assert_eq!(
            rel_path,
            Some("owner/repo/Discussions/456_Discussion _Title_ _ Rules.md")
        );
    }
}
