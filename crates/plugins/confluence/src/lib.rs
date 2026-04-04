use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata,
    PluginSecrets, RawDocument, SecretValue, SourceDocId,
};
use serde::Deserialize;
use std::collections::HashMap;

// ── Confluence API response shapes ────────────────────────────────────────────

#[derive(Deserialize)]
struct ConfluencePageList {
    results: Vec<ConfluencePage>,
    start: i64,
    limit: i64,
    size: i64,
}

#[derive(Deserialize)]
struct ConfluencePage {
    id: String,
    title: String,
    #[serde(rename = "_links")]
    links: ConfluenceLinks,
    body: Option<ConfluenceBody>,
}

#[derive(Deserialize)]
struct ConfluenceLinks {
    #[serde(rename = "webui")]
    web_ui: Option<String>,
}

#[derive(Deserialize)]
struct ConfluenceBody {
    storage: Option<ConfluenceStorage>,
}

#[derive(Deserialize)]
struct ConfluenceStorage {
    value: String,
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct ConfluencePlugin {
    meta: PluginMetadata,
    base_url: Option<String>,
    api_token: Option<String>,
    space_key: Option<String>,
    client: reqwest::Client,
}

impl ConfluencePlugin {
    pub fn new() -> Self {
        Self {
            meta: PluginMetadata {
                id: "com.doxus.confluence".into(),
                name: "Confluence".into(),
                version: "0.1.0".into(),
                kind: PluginKind::External,
            },
            base_url: None,
            api_token: None,
            space_key: None,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn base_url(&self) -> Result<&str, PluginError> {
        self.base_url
            .as_deref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    fn api_token(&self) -> Result<&str, PluginError> {
        self.api_token
            .as_deref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    fn space_key(&self) -> Result<&str, PluginError> {
        self.space_key
            .as_deref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    fn page_to_doc(&self, page: ConfluencePage) -> Result<RawDocument, PluginError> {
        let base_url = self.base_url()?;
        let content = page
            .body
            .and_then(|b| b.storage)
            .map(|s| s.value)
            .unwrap_or_default();
        let url = page
            .links
            .web_ui
            .map(|path| format!("{base_url}{path}"));
        Ok(RawDocument {
            id: SourceDocId(page.id),
            title: Some(page.title),
            content,
            content_type: ContentType::PlainText,
            url,
            metadata: HashMap::new(),
            tags: vec![],
            updated_at: None,
        })
    }
}

impl Default for ConfluencePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocSource for ConfluencePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: false, // fetch_changes not yet implemented
            oauth: false,
            native_search: false,
        }
    }

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError> {
        for field in ["base_url", "space_key", "api_token"] {
            if !config.fields.contains_key(field) {
                return Err(PluginError::ConfigInvalid(format!(
                    "missing required field: {field}"
                )));
            }
        }
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
        self.base_url = config
            .fields
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string());
        self.space_key = config
            .fields
            .get("space_key")
            .and_then(|v| v.as_str())
            .map(String::from);

        // api_token from secrets, fallback to config field
        self.api_token = secrets
            .fields
            .get("api_token")
            .map(|sv| match sv {
                SecretValue::Text(t) => t.clone(),
                SecretValue::Token { value, .. } => value.clone(),
            })
            .or_else(|| {
                config
                    .fields
                    .get("api_token")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let base_url = self.base_url()?;
        let api_token = self.api_token()?;
        let space_key = self.space_key()?;

        let start: i64 = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let limit = opts.page_size as i64;

        let url = format!("{base_url}/rest/api/content");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("spaceKey", space_key),
                ("type", "page"),
                ("start", &start.to_string()),
                ("limit", &limit.to_string()),
            ])
            .header("Authorization", format!("Bearer {api_token}"))
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(PluginError::AuthRequired);
        }
        if !resp.status().is_success() {
            return Err(PluginError::NetworkError(format!(
                "HTTP {}",
                resp.status()
            )));
        }

        let page_list: ConfluencePageList = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        let next_cursor = if page_list.size >= page_list.limit {
            Some((page_list.start + page_list.limit).to_string())
        } else {
            None
        };

        let documents: Result<Vec<_>, _> = page_list
            .results
            .into_iter()
            .map(|p| self.page_to_doc(p))
            .collect();

        Ok(DocumentStream {
            documents: documents?,
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
        let base_url = self.base_url()?;
        let api_token = self.api_token()?;

        // Validate ID to prevent path traversal: Confluence page IDs are alphanumeric
        if !id.0.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(PluginError::NotFound(format!("invalid document id: {}", id.0)));
        }

        let url = format!("{base_url}/rest/api/content/{}", id.0);
        let resp = self
            .client
            .get(&url)
            .query(&[("expand", "body.storage")])
            .header("Authorization", format!("Bearer {api_token}"))
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

        let page: ConfluencePage = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        self.page_to_doc(page)
    }

    async fn health_check(&self) -> HealthStatus {
        let (base_url, api_token, space_key) = match (
            self.base_url.as_deref(),
            self.api_token.as_deref(),
            self.space_key.as_deref(),
        ) {
            (Some(b), Some(t), Some(s)) => (b, t, s),
            _ => {
                return HealthStatus {
                    healthy: false,
                    message: Some("plugin not initialized".into()),
                }
            }
        };

        let url = format!("{base_url}/rest/api/space/{space_key}");
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {api_token}"))
            .send()
            .await
        {
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

fn validate_base_url(url: &str) -> Result<(), PluginError> {
    if !url.starts_with("https://") {
        return Err(PluginError::ConfigInvalid(
            "base_url must use HTTPS".into(),
        ));
    }
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
    use doxus_plugin_sdk::{FetchAllOpts, PluginConfig, PluginSecrets};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn make_plugin(server: &MockServer) -> ConfluencePlugin {
        let mut plugin = ConfluencePlugin::new();
        let mut config = PluginConfig::default();
        config
            .fields
            .insert("base_url".into(), serde_json::json!(server.uri()));
        config
            .fields
            .insert("space_key".into(), serde_json::json!("TEST"));
        let mut secrets = PluginSecrets::default();
        secrets
            .fields
            .insert("api_token".into(), SecretValue::Text("test-token".into()));
        plugin.initialize(config, secrets).await.unwrap();
        plugin
    }

    #[tokio::test]
    async fn validate_config_requires_base_url() {
        let plugin = ConfluencePlugin::new();
        let result = plugin.validate_config(&PluginConfig::default()).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn validate_config_requires_space_key() {
        let plugin = ConfluencePlugin::new();
        let mut config = PluginConfig::default();
        config
            .fields
            .insert("base_url".into(), serde_json::json!("http://example.com"));
        config
            .fields
            .insert("api_token".into(), serde_json::json!("tok"));
        // missing space_key
        let result = plugin.validate_config(&config).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))));
    }

    #[tokio::test]
    async fn fetch_all_returns_pages() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "results": [
                {"id": "123", "title": "Test Page", "_links": {"webui": "/wiki/test"}, "body": null}
            ],
            "start": 0, "limit": 50, "size": 1
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server).await;
        let stream = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 50,
            })
            .await
            .unwrap();
        assert_eq!(stream.documents.len(), 1);
        assert_eq!(stream.documents[0].title.as_deref(), Some("Test Page"));
    }

    #[tokio::test]
    async fn fetch_all_paginates() {
        let server = MockServer::start().await;
        let page1 = serde_json::json!({"results": [{"id": "1", "title": "A", "_links": {"webui": ""}, "body": null}], "start": 0, "limit": 1, "size": 1});
        let page2 = serde_json::json!({"results": [{"id": "2", "title": "B", "_links": {"webui": ""}, "body": null}], "start": 1, "limit": 1, "size": 1});

        Mock::given(method("GET"))
            .and(path("/rest/api/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rest/api/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server).await;
        let p1 = plugin
            .fetch_all(FetchAllOpts {
                cursor: None,
                page_size: 1,
            })
            .await
            .unwrap();
        assert_eq!(p1.documents.len(), 1);
        // size (1) >= limit (1) → has next cursor
        assert!(p1.next_cursor.is_some());

        let p2 = plugin
            .fetch_all(FetchAllOpts {
                cursor: p1.next_cursor,
                page_size: 1,
            })
            .await
            .unwrap();
        assert_eq!(p2.documents.len(), 1);
        assert_eq!(p2.documents[0].title.as_deref(), Some("B"));
    }

    #[tokio::test]
    async fn health_check_healthy_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/space/TEST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&serde_json::json!({"key": "TEST"})),
            )
            .mount(&server)
            .await;

        let plugin = make_plugin(&server).await;
        let status = plugin.health_check().await;
        assert!(status.healthy);
    }

    #[tokio::test]
    async fn health_check_unhealthy_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/space/TEST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server).await;
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }
}
