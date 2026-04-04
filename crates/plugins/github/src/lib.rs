use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SecretValue, SourceDocId,
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
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub base_url: String,
    pub token: Option<String>,
    pub include_closed: bool,
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
        RawDocument {
            id: SourceDocId(issue.number.to_string()),
            title: Some(issue.title),
            content: issue.body.unwrap_or_default(),
            content_type: ContentType::Markdown,
            url: Some(issue.html_url),
            metadata: HashMap::new(),
            tags: vec![issue.state],
            updated_at: None,
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
            incremental_sync: false,
            oauth: false,
            native_search: false,
        }
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
        let include_closed = config
            .fields
            .get("include_closed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
        });
        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let cfg = self.cfg()?;
        let page: u64 = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(1);
        let state = if cfg.include_closed { "all" } else { "open" };
        let url = format!(
            "{}/repos/{}/{}/issues",
            cfg.base_url, cfg.owner, cfg.repo
        );
        let req = self.client.get(&url).query(&[
            ("state", state),
            ("page", &page.to_string()),
            ("per_page", &opts.page_size.to_string()),
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
            s => return Err(PluginError::NetworkError(format!("HTTP {s}"))),
        }
        let issues: Vec<GitHubIssue> = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        let next_cursor = if issues.is_empty() {
            None
        } else {
            Some((page + 1).to_string())
        };
        let documents = issues.into_iter().map(|i| self.issue_to_doc(i)).collect();
        Ok(DocumentStream {
            documents,
            next_cursor,
            estimated_total: None,
        })
    }

    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Ok(ChangeSet {
            updated: vec![],
            deleted_ids: vec![],
            next_cursor: None,
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        let cfg = self.cfg()?;
        // Validate: id must be a valid u64 (prevents path traversal)
        let _num: u64 = id
            .0
            .parse()
            .map_err(|_| PluginError::NotFound(format!("invalid document id: {}", id.0)))?;
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            cfg.base_url, cfg.owner, cfg.repo, id.0
        );
        let req = self.client.get(&url);
        let req = self.add_auth(req, cfg.token.as_deref());
        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PluginError::NotFound(id.0.clone()));
        }
        if !resp.status().is_success() {
            return Err(PluginError::NetworkError(format!(
                "HTTP {}",
                resp.status()
            )));
        }
        let issue: GitHubIssue = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(self.issue_to_doc(issue))
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

/// Validates that base_url is HTTPS and not a private/loopback address.
/// Rejects http://, file://, and private hostnames to prevent SSRF.
fn validate_base_url(url: &str) -> Result<(), PluginError> {
    if !url.starts_with("https://") {
        return Err(PluginError::ConfigInvalid(
            "base_url must use HTTPS".into(),
        ));
    }
    // Extract host from https://host/...
    let host = url
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let blocked = ["localhost", "127.0.0.1", "::1", "0.0.0.0", "169.254.169.254"];
    if blocked.contains(&host) || host.ends_with(".local") || host.is_empty() {
        return Err(PluginError::ConfigInvalid(format!(
            "base_url host is not allowed: {host}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    fn make_plugin(server: &MockServer) -> GitHubPlugin {
        GitHubPlugin::with_config(GitHubConfig {
            owner: "foo".into(),
            repo: "bar".into(),
            base_url: server.uri(),
            token: None,
            include_closed: false,
        })
    }

    fn make_plugin_with_token(server: &MockServer, token: &str) -> GitHubPlugin {
        GitHubPlugin::with_config(GitHubConfig {
            owner: "foo".into(),
            repo: "bar".into(),
            base_url: server.uri(),
            token: Some(token.to_string()),
            include_closed: false,
        })
    }

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

        let plugin = make_plugin(&server);
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
        assert_eq!(stream.next_cursor, Some("2".to_string()));
    }

    #[tokio::test]
    async fn fetch_issues_empty_page_ends_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: Some("3".into()),
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
        let doc = plugin
            .fetch_document(&SourceDocId("42".into()))
            .await
            .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Bug"));
        assert_eq!(doc.content, "Details");
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
        assert!(matches!(err, PluginError::ConfigInvalid(_)));
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
        // Missing both owner and repo
        let config = PluginConfig::default();
        let result = plugin.validate_config(&config).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn validate_config_accepts_valid_config() {
        let plugin = GitHubPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("owner".into(), serde_json::json!("myorg"));
        config.fields.insert("repo".into(), serde_json::json!("myrepo"));
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
        let result = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 50 }).await;
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

        let mut plugin = GitHubPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("owner".into(), serde_json::json!("owner"));
        config.fields.insert("repo".into(), serde_json::json!("repo"));
        config.fields.insert("base_url".into(), serde_json::json!(server.uri()));
        plugin
            .initialize(config, PluginSecrets::default())
            .await
            .unwrap();

        let stream = plugin
            .fetch_all(FetchAllOpts { cursor: None, page_size: 50 })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 2);
    }
}
