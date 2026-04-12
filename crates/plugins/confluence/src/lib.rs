pub mod html_convert;
pub mod oauth_server;

use async_trait::async_trait;
use doxus_core::auth::{OAuthConfig, OAuthFlow, OAuthToken};
use doxus_plugin_sdk::{
    validate_base_url, Capabilities, ChangeSet, ContentType, DocSource, DocumentStream,
    FetchAllOpts, FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginKind,
    PluginMetadata, PluginSecrets, RawDocument, SecretValue, SourceDocId,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Confluence API response shapes ────────────────────────────────────────────

#[derive(Deserialize)]
struct ConfluencePageList {
    results: Vec<ConfluencePage>,
    start: i64,
    limit: i64,
    size: i64,
}

#[derive(Deserialize)]
struct ConfluenceCqlResult {
    results: Vec<ConfluencePage>,
    start: i64,
    limit: i64,
    size: i64,
}

#[derive(Deserialize)]
struct ConfluencePage {
    id: String,
    title: String,
    #[serde(rename = "type", default = "default_page_type")]
    content_type: String,
    #[serde(rename = "_links")]
    links: ConfluenceLinks,
    body: Option<ConfluenceBody>,
    version: Option<ConfluenceVersion>,
    metadata: Option<ConfluencePageMetadata>,
    space: Option<ConfluenceSpace>,
}

fn default_page_type() -> String {
    "page".to_string()
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

#[derive(Deserialize)]
struct ConfluenceVersion {
    when: Option<String>,
}

#[derive(Deserialize)]
struct ConfluencePageMetadata {
    labels: Option<ConfluenceLabels>,
}

#[derive(Deserialize)]
struct ConfluenceLabels {
    results: Vec<ConfluenceLabel>,
}

#[derive(Deserialize)]
struct ConfluenceLabel {
    name: String,
}

#[derive(Deserialize)]
struct ConfluenceSpace {
    key: String,
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct ConfluencePlugin {
    meta: PluginMetadata,
    base_url: Option<String>,
    api_token: Option<String>,
    /// 이메일이 설정된 경우 Basic auth(email:token) 사용, 없으면 Bearer 사용
    email: Option<String>,
    space_key: Option<String>,
    ancestor_id: Option<String>,
    oauth_config: Option<OAuthConfig>,
    oauth_token: Arc<RwLock<Option<OAuthToken>>>,
    /// Pending state value generated during oauth_start, used to validate oauth_exchange.
    oauth_pending_state: std::sync::Mutex<Option<String>>,
    /// OAuth callback server started during oauth_start.
    oauth_server: std::sync::Mutex<Option<oauth_server::OAuthCallbackServer>>,
    client: reqwest::Client,
}

fn build_oauth_urls(base_url: &str) -> (String, String) {
    if base_url.contains(".atlassian.net") {
        (
            "https://auth.atlassian.com/authorize".to_string(),
            "https://auth.atlassian.com/oauth/token".to_string(),
        )
    } else {
        (
            format!("{base_url}/rest/oauth2/latest/authorize"),
            format!("{base_url}/rest/oauth2/latest/token"),
        )
    }
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
            email: None,
            space_key: None,
            ancestor_id: None,
            oauth_config: None,
            oauth_token: Arc::new(RwLock::new(None)),
            oauth_pending_state: std::sync::Mutex::new(None),
            oauth_server: std::sync::Mutex::new(None),
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

    /// Authorization 헤더 값 반환.
    /// email이 있으면 Basic auth(email:token), 없으면 Bearer token.
    fn auth_header(&self) -> Result<String, PluginError> {
        let token = self.api_token()?;
        if let Some(email) = &self.email {
            use base64::Engine as _;
            let credentials = base64::engine::general_purpose::STANDARD
                .encode(format!("{email}:{token}"));
            Ok(format!("Basic {credentials}"))
        } else {
            Ok(format!("Bearer {token}"))
        }
    }

    fn space_key(&self) -> Result<&str, PluginError> {
        self.space_key
            .as_deref()
            .ok_or_else(|| PluginError::Internal("plugin not initialized".into()))
    }

    /// HTTP 응답 상태 코드를 PluginError로 변환.
    /// 성공(2xx)이면 Ok(())를 반환하고, 오류면 적절한 PluginError를 반환.
    fn check_status(status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) -> Result<(), PluginError> {
        if status.is_success() {
            return Ok(());
        }
        match status.as_u16() {
            401 => Err(PluginError::AuthRequired),
            403 => Err(PluginError::PermissionDenied("insufficient permissions".into())),
            404 => Err(PluginError::NotFound("resource not found".into())),
            429 => {
                let retry_after = headers
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(60);
                Err(PluginError::RateLimited { retry_after_secs: retry_after })
            }
            _ => Err(PluginError::NetworkError(format!("HTTP {}", status))),
        }
    }

    fn page_to_doc(&self, page: ConfluencePage) -> Result<RawDocument, PluginError> {
        let base_url = self.base_url()?;
        let raw_content = page
            .body
            .and_then(|b| b.storage)
            .map(|s| s.value)
            .unwrap_or_default();
        let content = html_convert::confluence_html_to_markdown(&raw_content);
        let url = page
            .links
            .web_ui
            .map(|path| format!("{base_url}{path}"));

        // updated_at: version.when → Unix timestamp
        let updated_at = page
            .version
            .and_then(|v| v.when)
            .and_then(|when| chrono::DateTime::parse_from_rfc3339(&when).ok())
            .map(|dt| dt.timestamp());

        // metadata: space_key
        let mut metadata = HashMap::new();
        if let Some(space) = page.space {
            metadata.insert("space_key".to_string(), serde_json::Value::String(space.key));
        }

        // tags: Confluence labels
        let tags = page
            .metadata
            .and_then(|m| m.labels)
            .map(|l| l.results.into_iter().map(|label| label.name).collect())
            .unwrap_or_default();

        Ok(RawDocument {
            id: SourceDocId(page.id),
            title: Some(page.title),
            content,
            content_type: ContentType::Markdown,
            url,
            metadata,
            tags,
            aliases: vec![],
            created_at: None,
            updated_at,
        })
    }
}

impl Default for ConfluencePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfluencePlugin {
    /// Configure plugin fields directly, bypassing SSRF validation.
    /// Intended for testing with local HTTP mock servers (e.g. wiremock).
    /// Do not use in production paths.
    /// Fetch all page IDs currently in the space (used for deletion detection).
    /// Paginates until all pages are collected.
    async fn fetch_all_space_ids(
        &self,
        base_url: &str,
        auth_header: &str,
        space_key: &str,
    ) -> Result<std::collections::HashSet<String>, PluginError> {
        let mut ids = std::collections::HashSet::new();
        let mut start: i64 = 0;
        let limit: i64 = 200;
        loop {
            let cql = format!("space = \"{space_key}\" AND type = page ORDER BY id ASC");
            let resp = self
                .client
                .get(&format!("{base_url}/rest/api/content/search"))
                .query(&[
                    ("cql", cql.as_str()),
                    ("start", &start.to_string()),
                    ("limit", &limit.to_string()),
                ])
                .header("Authorization", auth_header)
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;

            Self::check_status(resp.status(), resp.headers())?;

            let result: ConfluenceCqlResult = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;

            let count = result.results.len() as i64;
            for page in result.results {
                ids.insert(page.id);
            }

            if count < limit {
                break;
            }
            start += limit;
        }
        Ok(ids)
    }

    async fn fetch_all_ancestor_ids(
        &self,
        base_url: &str,
        auth_header: &str,
        ancestor_id: &str,
    ) -> Result<std::collections::HashSet<String>, PluginError> {
        let mut ids = std::collections::HashSet::new();
        let mut start: i64 = 0;
        let limit: i64 = 200;
        loop {
            // type 필터 없이 조회 — folder ancestor에서도 하위 페이지 ID를 수집하기 위함
            let cql = format!("ancestor = \"{ancestor_id}\" ORDER BY id ASC");
            let resp = self
                .client
                .get(&format!("{base_url}/rest/api/content/search"))
                .query(&[
                    ("cql", cql.as_str()),
                    ("start", &start.to_string()),
                    ("limit", &limit.to_string()),
                ])
                .header("Authorization", auth_header)
                .send()
                .await
                .map_err(|e| PluginError::NetworkError(e.to_string()))?;

            Self::check_status(resp.status(), resp.headers())?;

            let result: ConfluenceCqlResult = resp
                .json()
                .await
                .map_err(|e| PluginError::Internal(e.to_string()))?;

            let count = result.results.len() as i64;
            for page in result.results {
                // folder 타입은 인덱싱 대상이 아니므로 ID 수집에서도 제외
                if page.content_type == "page" {
                    ids.insert(page.id);
                }
            }

            if count < limit {
                break;
            }
            start += limit;
        }
        Ok(ids)
    }

    #[doc(hidden)]
    pub fn set_test_config(&mut self, base_url: String, space_key: String, api_token: String) {
        self.base_url = Some(base_url);
        self.space_key = Some(space_key);
        self.api_token = Some(api_token);
    }

    #[doc(hidden)]
    pub fn set_test_ancestor_config(&mut self, base_url: String, ancestor_id: String, api_token: String) {
        self.base_url = Some(base_url);
        self.ancestor_id = Some(ancestor_id);
        self.api_token = Some(api_token);
    }

    /// Configure OAuth for testing without going through `initialize`.
    /// Sets the oauth_config and (optionally) an initial oauth_token.
    #[doc(hidden)]
    pub fn set_test_oauth_config(
        &mut self,
        config: OAuthConfig,
        token: Option<OAuthToken>,
    ) {
        self.oauth_config = Some(config);
        // Replace the shared lock with a new one containing the supplied token.
        self.oauth_token = Arc::new(RwLock::new(token));
    }

    /// Ensures the OAuth token is valid, refreshing it if expired.
    /// Returns immediately (Ok) when api_token auth is in use (no oauth_config).
    async fn ensure_valid_token(&self) -> Result<(), PluginError> {
        // If no OAuth config, this plugin uses api_token auth — skip.
        let oauth_config = match &self.oauth_config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };

        // Fast path: read lock — token present and not expired.
        {
            let guard = self.oauth_token.read().await;
            match &*guard {
                Some(tok) if !tok.is_expired() => return Ok(()),
                _ => {} // fall through to refresh
            }
        }

        // Slow path: write lock with double-checked locking to prevent thundering herd.
        let mut guard = self.oauth_token.write().await;
        // Re-check inside write lock.
        match &*guard {
            Some(tok) if !tok.is_expired() => return Ok(()),
            _ => {}
        }

        // Need to refresh. Extract refresh_token before calling async method.
        let refresh_tok = guard
            .as_ref()
            .and_then(|t| t.refresh_token.clone())
            .ok_or(PluginError::AuthRequired)?;

        let flow = OAuthFlow::new(oauth_config);
        let new_token = flow
            .refresh_token(&refresh_tok)
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        *guard = Some(new_token);
        Ok(())
    }

    /// Wait for the OAuth callback that was set up during `oauth_start`.
    /// Calls `oauth_exchange` internally with the received code and state.
    pub async fn wait_oauth_callback(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<(), PluginError> {
        let server = self
            .oauth_server
            .lock()
            .map_err(|_| PluginError::Internal("oauth_server lock poisoned".into()))?
            .take()
            .ok_or_else(|| PluginError::Internal("no OAuth server started; call oauth_start first".into()))?;

        let expected_state = self
            .oauth_pending_state
            .lock()
            .map_err(|_| PluginError::Internal("oauth_pending_state lock poisoned".into()))?
            .clone()
            .ok_or_else(|| PluginError::Internal("no pending OAuth state; call oauth_start first".into()))?;

        let (code, state) = server.wait_for_callback(timeout, &expected_state).await?;
        self.oauth_exchange(&code, &state).await
    }
}

#[async_trait]
impl DocSource for ConfluencePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: true,
            oauth: self.oauth_config.is_some(),
            native_search: false,
        }
    }

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError> {
        for field in ["base_url", "api_token"] {
            if !config.fields.contains_key(field) {
                return Err(PluginError::ConfigInvalid(format!(
                    "missing required field: {field}"
                )));
            }
        }
        let has_space_key = config.fields.get("space_key").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
        let has_ancestor_id = config.fields.get("ancestor_id").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
        if !has_space_key && !has_ancestor_id {
            return Err(PluginError::ConfigInvalid(
                "either space_key or ancestor_id must be provided".to_string(),
            ));
        }
        if let Some(base_url) = config.fields.get("base_url").and_then(|v| v.as_str()) {
            validate_base_url(base_url)?;
        }
        if let Some(space_key) = config.fields.get("space_key").and_then(|v| v.as_str()) {
            if !space_key.is_empty() && !space_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '~') {
                return Err(PluginError::ConfigInvalid(
                    "space_key must contain only alphanumeric characters, underscores, hyphens, or tildes".to_string(),
                ));
            }
        }
        if let Some(ancestor_id) = config.fields.get("ancestor_id").and_then(|v| v.as_str()) {
            if !ancestor_id.is_empty() && !ancestor_id.chars().all(|c| c.is_ascii_digit()) {
                return Err(PluginError::ConfigInvalid(
                    "ancestor_id must be a numeric content ID".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn initialize(
        &mut self,
        config: PluginConfig,
        secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        let raw_base_url = config
            .fields
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_end_matches('/');
        validate_base_url(raw_base_url)?;
        // Atlassian Cloud: Confluence REST API lives under /wiki. Auto-append if missing.
        let normalized_base_url = if raw_base_url.contains(".atlassian.net")
            && !raw_base_url.ends_with("/wiki")
        {
            format!("{raw_base_url}/wiki")
        } else {
            raw_base_url.to_string()
        };
        self.base_url = if normalized_base_url.is_empty() {
            None
        } else {
            Some(normalized_base_url.clone())
        };
        let raw_base_url = normalized_base_url.as_str();
        self.email = config
            .fields
            .get("email")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        self.space_key = config
            .fields
            .get("space_key")
            .and_then(|v| v.as_str())
            .map(String::from);
        self.ancestor_id = config
            .fields
            .get("ancestor_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
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

        // OAuth config — optional; only set when client_id + client_secret are provided
        if let (Some(client_id), Some(client_secret)) = (
            config.fields.get("client_id").and_then(|v| v.as_str()),
            config.fields.get("client_secret").and_then(|v| v.as_str()),
        ) {
            let (auth_url, token_url) = build_oauth_urls(raw_base_url);
            self.oauth_config = Some(OAuthConfig {
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                redirect_uri: config
                    .fields
                    .get("redirect_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http://localhost:8080/callback")
                    .to_string(),
                auth_url,
                token_url,
                scopes: vec!["read:confluence-content.all".to_string()],
            });
        }

        Ok(())
    }

    async fn oauth_start(&self) -> Option<String> {
        let config = self.oauth_config.as_ref()?;

        // Start callback server and build redirect_uri from the dynamically assigned port.
        let server = oauth_server::OAuthCallbackServer::start().await.ok()?;
        let port = server.local_addr().ok()?.port();
        let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

        // Store server for later use in wait_oauth_callback.
        if let Ok(mut guard) = self.oauth_server.lock() {
            *guard = Some(server);
        }

        let mut config_with_redirect = config.clone();
        config_with_redirect.redirect_uri = redirect_uri;

        let flow = OAuthFlow::new(config_with_redirect);
        // Generate a cryptographically random state for CSRF protection.
        let state = {
            let mut bytes = [0u8; 16];
            getrandom::getrandom(&mut bytes).ok()?;
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        let url = flow.authorization_url(&state).ok()?;
        // Store state for CSRF validation in oauth_exchange.
        if let Ok(mut guard) = self.oauth_pending_state.lock() {
            *guard = Some(state);
        }
        Some(url)
    }

    async fn oauth_exchange(&mut self, code: &str, state: &str) -> Result<(), PluginError> {
        let config = self
            .oauth_config
            .as_ref()
            .ok_or(PluginError::AuthRequired)?;
        // Validate state to prevent CSRF attacks.
        let expected = self
            .oauth_pending_state
            .lock()
            .map_err(|_| PluginError::Internal("state lock poisoned".into()))?
            .take()
            .ok_or(PluginError::AuthRequired)?;
        if expected != state {
            return Err(PluginError::AuthRequired);
        }
        let flow = OAuthFlow::new(config.clone());
        let token = flow
            .exchange_code(code, state, &expected)
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;
        *self.oauth_token.write().await = Some(token);
        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        self.ensure_valid_token().await?;
        let base_url = self.base_url()?;
        let api_token = self.api_token()?;

        let start: i64 = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let limit = opts.page_size as i64;

        // ancestor_id 모드의 첫 페이지: ancestor 페이지 자체도 포함
        let ancestor_self_doc: Option<RawDocument> = if self.ancestor_id.is_some() && start == 0 {
            let ancestor_id = self.ancestor_id.as_deref().unwrap();
            let url = format!("{base_url}/rest/api/content/{ancestor_id}");
            if let Ok(resp) = self
                .client
                .get(&url)
                .query(&[("expand", "body.storage,version,metadata.labels,space")])
                .header("Authorization", self.auth_header()?)
                .send()
                .await
            {
                if resp.status().is_success() {
                    resp.json::<ConfluencePage>().await.ok().and_then(|p| self.page_to_doc(p).ok())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // CQL 통합: ancestor 모드와 space 모드 모두 CQL search API 사용
        // - ancestor: /descendant/page는 Cloud에서 404 발생, space path에 ~가 포함된 경우도 404
        // - CQL은 두 경우 모두 처리 가능
        let cql = if let Some(ancestor_id) = &self.ancestor_id {
            format!("ancestor = \"{ancestor_id}\" ORDER BY id ASC")
        } else {
            let space_key = self.space_key()?;
            format!("space = \"{space_key}\" AND type = page ORDER BY id ASC")
        };
        let url = format!("{base_url}/rest/api/content/search");
        eprintln!("[confluence:fetch_all] CQL={cql}");
        eprintln!("[confluence:fetch_all] URL={url} start={start}");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("cql", cql.as_str()),
                ("expand", "body.storage,version,metadata.labels,space"),
                ("start", &start.to_string()),
                ("limit", &limit.to_string()),
            ])
            .header("Authorization", self.auth_header()?)
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body_bytes = resp.bytes().await.map_err(|e| PluginError::Internal(e.to_string()))?;
        eprintln!("[confluence:fetch_all] status={status} body={}", String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(500)]));
        Self::check_status(status, &headers)?;
        let r: ConfluenceCqlResult = serde_json::from_slice(&body_bytes).map_err(|e| PluginError::Internal(e.to_string()))?;
        eprintln!("[confluence:fetch_all] parsed: results={} size={}", r.results.len(), r.size);
        let (results, size, limit_val, start_val) = (r.results, r.size, r.limit, r.start);

        let next_cursor = if size >= limit_val {
            Some((start_val + limit_val).to_string())
        } else {
            None
        };

        let mut documents: Vec<RawDocument> = results
            .into_iter()
            .filter(|p| p.content_type == "page")
            .map(|p| self.page_to_doc(p))
            .collect::<Result<_, _>>()?;

        // ancestor 페이지 자체를 맨 앞에 prepend (첫 페이지, start==0 일 때만)
        if let Some(self_doc) = ancestor_self_doc {
            documents.insert(0, self_doc);
        }

        Ok(DocumentStream {
            documents,
            next_cursor,
            estimated_total: None,
        })
    }

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        self.ensure_valid_token().await?;
        let base_url = self.base_url()?;
        let api_token = self.api_token()?;

        // Convert Unix timestamp (seconds) to ISO 8601 date string for CQL
        let since_dt = chrono::DateTime::from_timestamp(opts.since, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
        let since_str = since_dt.format("%Y-%m-%dT%H:%M:%S").to_string();

        let start: i64 = opts
            .cursor
            .as_deref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let limit = opts.page_size as i64;

        let cql = if let Some(ancestor_id) = &self.ancestor_id {
            format!(
                "ancestor = \"{ancestor_id}\" AND lastModified >= \"{since_str}\" ORDER BY lastModified ASC"
            )
        } else {
            let space_key = self.space_key()?;
            format!(
                "space = \"{space_key}\" AND lastModified >= \"{since_str}\" ORDER BY lastModified ASC"
            )
        };

        let url = format!("{base_url}/rest/api/content/search");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("cql", cql.as_str()),
                ("expand", "body.storage,version,metadata.labels,space"),
                ("start", &start.to_string()),
                ("limit", &limit.to_string()),
            ])
            .header("Authorization", self.auth_header()?)
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        Self::check_status(resp.status(), resp.headers())?;

        let cql_result: ConfluenceCqlResult = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        let next_cursor = if cql_result.size >= cql_result.limit {
            Some((cql_result.start + cql_result.limit).to_string())
        } else {
            None
        };

        let updated: Result<Vec<_>, _> = cql_result
            .results
            .into_iter()
            .filter(|p| p.content_type == "page")
            .map(|p| self.page_to_doc(p))
            .collect();

        // Detect deletions only when pagination is complete (final page) and
        // known_ids were supplied.  We query the full space to get *all* current
        // page IDs and compute the set difference — comparing only against the
        // CQL change-result would cause false positives for unmodified documents.
        let deleted_ids = if next_cursor.is_none() && !opts.known_ids.is_empty() {
            let all_current_ids = if let Some(ancestor_id) = &self.ancestor_id {
                self.fetch_all_ancestor_ids(base_url, &self.auth_header()?, ancestor_id).await?
            } else {
                let space_key = self.space_key()?;
                self.fetch_all_space_ids(base_url, &self.auth_header()?, space_key).await?
            };
            opts.known_ids
                .into_iter()
                .filter(|id| !all_current_ids.contains(&id.0))
                .collect()
        } else {
            vec![]
        };

        Ok(ChangeSet {
            updated: updated?,
            deleted_ids,
            next_cursor,
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        self.ensure_valid_token().await?;
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
            .query(&[("expand", "body.storage,version,metadata.labels,space")])
            .header("Authorization", self.auth_header()?)
            .send()
            .await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        Self::check_status(resp.status(), resp.headers())?;

        let page: ConfluencePage = resp
            .json()
            .await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        self.page_to_doc(page)
    }

    async fn health_check(&self) -> HealthStatus {
        let (base_url, api_token) = match (
            self.base_url.as_deref(),
            self.api_token.as_deref(),
        ) {
            (Some(b), Some(t)) => (b, t),
            _ => {
                return HealthStatus {
                    healthy: false,
                    message: Some("plugin not initialized".into()),
                }
            }
        };

        let url = if let Some(ancestor_id) = &self.ancestor_id {
            format!("{base_url}/rest/api/content/{ancestor_id}")
        } else if let Some(space_key) = &self.space_key {
            format!("{base_url}/rest/api/space/{space_key}")
        } else {
            return HealthStatus {
                healthy: false,
                message: Some("neither space_key nor ancestor_id is configured".into()),
            };
        };

        match self
            .client
            .get(&url)
            .header("Authorization", self.auth_header().unwrap_or_default())
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


#[cfg(test)]
mod tests {
    use super::*;
    use doxus_plugin_sdk::{FetchAllOpts, PluginConfig};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_build_oauth_urls_cloud() {
        let (auth, token) = build_oauth_urls("https://mycompany.atlassian.net/wiki");
        assert_eq!(auth, "https://auth.atlassian.com/authorize");
        assert_eq!(token, "https://auth.atlassian.com/oauth/token");
    }

    #[test]
    fn test_build_oauth_urls_server() {
        let (auth, token) = build_oauth_urls("https://confluence.corp.com");
        assert_eq!(auth, "https://confluence.corp.com/rest/oauth2/latest/authorize");
        assert_eq!(token, "https://confluence.corp.com/rest/oauth2/latest/token");
    }

    #[test]
    fn test_build_oauth_urls_cloud_no_trailing_slash() {
        let (auth, _) = build_oauth_urls("https://foo.atlassian.net");
        assert_eq!(auth, "https://auth.atlassian.com/authorize");
    }

    fn make_plugin(server: &MockServer) -> ConfluencePlugin {
        // Set fields directly to bypass SSRF validation (wiremock uses HTTP localhost).
        // Fetch-behavior tests are orthogonal to SSRF; SSRF is tested via validate_config/initialize.
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some(server.uri().trim_end_matches('/').to_string());
        plugin.space_key = Some("TEST".to_string());
        plugin.api_token = Some("test-token".to_string());
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
    async fn validate_config_accepts_numeric_ancestor_id() {
        let plugin = ConfluencePlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("base_url".into(), serde_json::json!("https://example.atlassian.net/wiki"));
        config.fields.insert("api_token".into(), serde_json::json!("tok"));
        config.fields.insert("ancestor_id".into(), serde_json::json!("4667998225"));
        let result = plugin.validate_config(&config).await;
        assert!(result.is_ok(), "numeric ancestor_id should be valid");
    }

    #[tokio::test]
    async fn validate_config_rejects_non_numeric_ancestor_id() {
        let plugin = ConfluencePlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("base_url".into(), serde_json::json!("https://example.atlassian.net/wiki"));
        config.fields.insert("api_token".into(), serde_json::json!("tok"));
        // CQL injection attempt
        config.fields.insert("ancestor_id".into(), serde_json::json!("123\" OR \"1\"=\"1"));
        let result = plugin.validate_config(&config).await;
        assert!(matches!(result, Err(PluginError::ConfigInvalid(_))), "non-numeric ancestor_id should be rejected");
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

        let plugin = make_plugin(&server);
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

        let plugin = make_plugin(&server);
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

    // ── fetch_changes deletion detection tests ────────────────────────────────

    fn make_fetch_changes_opts(
        server: &MockServer,
        known_ids: Vec<&str>,
        since: i64,
    ) -> (ConfluencePlugin, doxus_plugin_sdk::FetchChangesOpts) {
        let plugin = make_plugin(server);
        let opts = doxus_plugin_sdk::FetchChangesOpts {
            since,
            cursor: None,
            page_size: 50,
            known_ids: known_ids
                .into_iter()
                .map(|s| SourceDocId(s.to_string()))
                .collect(),
        };
        (plugin, opts)
    }

    /// Regression: unmodified documents must NOT be reported as deleted.
    /// The CQL `lastModified >= since` result only contains recently changed pages;
    /// a page absent from that result is not necessarily deleted.
    #[tokio::test]
    async fn fetch_changes_does_not_false_positive_unmodified_docs_as_deleted() {
        let server = MockServer::start().await;

        // CQL change query returns only doc "101" (recently modified)
        let changes_body = serde_json::json!({
            "results": [
                {"id": "101", "title": "Modified Page", "_links": {"webui": "/wiki/101"}, "body": null}
            ],
            "start": 0, "limit": 50, "size": 1
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content/search"))
            .and(wiremock::matchers::query_param_contains("cql", "lastModified"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&changes_body))
            .mount(&server)
            .await;

        // Full-space query returns BOTH "101" and "102" (both still exist)
        let all_body = serde_json::json!({
            "results": [
                {"id": "101", "title": "Modified Page", "_links": {"webui": ""}, "body": null},
                {"id": "102", "title": "Unmodified Page", "_links": {"webui": ""}, "body": null}
            ],
            "start": 0, "limit": 200, "size": 2
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content/search"))
            .and(wiremock::matchers::query_param_contains("cql", "type = page"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&all_body))
            .mount(&server)
            .await;

        let (plugin, opts) = make_fetch_changes_opts(&server, vec!["101", "102"], 0);
        let changeset = plugin.fetch_changes(opts).await.unwrap();

        // "102" is unmodified but still exists — must NOT appear in deleted_ids
        assert!(
            changeset.deleted_ids.is_empty(),
            "unmodified doc '102' must not be false-positively reported as deleted, got: {:?}",
            changeset.deleted_ids
        );
        assert_eq!(changeset.updated.len(), 1);
    }

    /// A document in known_ids that is absent from the full-space query IS truly deleted.
    #[tokio::test]
    async fn fetch_changes_detects_truly_deleted_doc() {
        let server = MockServer::start().await;

        // CQL change query returns doc "101"
        let changes_body = serde_json::json!({
            "results": [
                {"id": "101", "title": "Modified Page", "_links": {"webui": ""}, "body": null}
            ],
            "start": 0, "limit": 50, "size": 1
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content/search"))
            .and(wiremock::matchers::query_param_contains("cql", "lastModified"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&changes_body))
            .mount(&server)
            .await;

        // Full-space query only returns "101" — "999" is gone
        let all_body = serde_json::json!({
            "results": [
                {"id": "101", "title": "Modified Page", "_links": {"webui": ""}, "body": null}
            ],
            "start": 0, "limit": 200, "size": 1
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content/search"))
            .and(wiremock::matchers::query_param_contains("cql", "type = page"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&all_body))
            .mount(&server)
            .await;

        let (plugin, opts) = make_fetch_changes_opts(&server, vec!["101", "999"], 0);
        let changeset = plugin.fetch_changes(opts).await.unwrap();

        assert_eq!(changeset.deleted_ids.len(), 1);
        assert_eq!(changeset.deleted_ids[0].0, "999");
    }

    /// When known_ids is empty, deletion detection is skipped (no extra API call).
    #[tokio::test]
    async fn fetch_changes_skips_deletion_when_no_known_ids() {
        let server = MockServer::start().await;

        let changes_body = serde_json::json!({
            "results": [],
            "start": 0, "limit": 50, "size": 0
        });
        Mock::given(method("GET"))
            .and(path("/rest/api/content/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&changes_body))
            .mount(&server)
            .await;

        let (plugin, opts) = make_fetch_changes_opts(&server, vec![], 0);
        let changeset = plugin.fetch_changes(opts).await.unwrap();

        assert!(changeset.deleted_ids.is_empty());
        // Only 1 request should have been made (no full-space query)
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
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

        let plugin = make_plugin(&server);
        let status = plugin.health_check().await;
        assert!(status.healthy);
    }

    // ── OAuth tests ───────────────────────────────────────────────────────────

    fn make_oauth_plugin(server: &MockServer, with_oauth: bool) -> ConfluencePlugin {
        // Use direct field assignment to bypass SSRF validation (wiremock uses HTTP localhost).
        // This mirrors the pattern used by make_plugin() for fetch tests.
        let base_url = server.uri().trim_end_matches('/').to_string();
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some(base_url.clone());
        plugin.space_key = Some("TEST".to_string());
        plugin.api_token = Some("tok".to_string());
        if with_oauth {
            plugin.oauth_config = Some(OAuthConfig {
                client_id: "my-client-id".to_string(),
                client_secret: "my-secret".to_string(),
                redirect_uri: "http://localhost/cb".to_string(),
                auth_url: format!("{base_url}/oauth2/authorize"),
                token_url: format!("{base_url}/oauth2/token"),
                scopes: vec!["read:confluence-content.all".to_string()],
            });
        }
        plugin
    }

    #[tokio::test]
    async fn oauth_start_returns_url_when_config_provided() {
        let server = MockServer::start().await;
        let plugin = make_oauth_plugin(&server, true);

        let url = plugin.oauth_start().await;
        assert!(url.is_some(), "expected auth URL");
        let url = url.unwrap();
        assert!(url.contains("oauth2/authorize"), "got: {url}");
        assert!(url.contains("my-client-id"), "got: {url}");
    }

    #[tokio::test]
    async fn oauth_start_returns_none_without_oauth_config() {
        let server = MockServer::start().await;
        let plugin = make_oauth_plugin(&server, false);

        let url = plugin.oauth_start().await;
        assert!(url.is_none(), "expected None without OAuth config");
    }

    #[tokio::test]
    async fn oauth_exchange_stores_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "test-access-token",
                    "token_type": "bearer",
                    "expires_in": 3600,
                })),
            )
            .mount(&server)
            .await;

        let mut plugin = make_oauth_plugin(&server, true);
        // Must call oauth_start first to generate and store the pending state.
        let auth_url = plugin.oauth_start().await.expect("expected auth URL");
        // Extract the state value from the generated URL query parameter.
        let state = auth_url
            .split("state=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .expect("state param in auth URL");
        plugin.oauth_exchange("auth-code", state).await.unwrap();

        let token_guard = plugin.oauth_token.read().await;
        assert!(token_guard.is_some(), "token should be stored after exchange");
        assert_eq!(
            token_guard.as_ref().unwrap().access_token,
            "test-access-token"
        );
    }

    #[tokio::test]
    async fn oauth_exchange_errors_without_oauth_config() {
        let server = MockServer::start().await;
        let mut plugin = make_oauth_plugin(&server, false);

        let result = plugin.oauth_exchange("code", "state").await;
        assert!(matches!(result, Err(PluginError::AuthRequired)));
    }

    #[tokio::test]
    async fn capabilities_oauth_true_when_config_present() {
        let server = MockServer::start().await;
        let plugin = make_oauth_plugin(&server, true);
        assert!(plugin.capabilities().oauth);
    }

    #[tokio::test]
    async fn capabilities_oauth_false_without_config() {
        let server = MockServer::start().await;
        let plugin = make_oauth_plugin(&server, false);
        assert!(!plugin.capabilities().oauth);
    }

    #[tokio::test]
    async fn health_check_unhealthy_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/space/TEST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let plugin = make_plugin(&server);
        let status = plugin.health_check().await;
        assert!(!status.healthy);
    }

    #[test]
    fn test_page_to_doc_html_converted_to_markdown() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "1".into(),
            title: "Test".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: Some(ConfluenceBody {
                storage: Some(ConfluenceStorage {
                    value: "<p>Hello <strong>world</strong></p>".into(),
                }),
            }),
            version: None,
            metadata: None,
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert!(matches!(doc.content_type, ContentType::Markdown));
        assert!(doc.content.contains("**world**"), "expected bold markdown, got: {}", doc.content);
    }

    #[test]
    fn test_page_to_doc_empty_body_is_markdown() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "2".into(),
            title: "Empty".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: None,
            version: None,
            metadata: None,
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert!(matches!(doc.content_type, ContentType::Markdown));
        assert!(doc.content.is_empty() || !doc.content.contains('<'));
    }

    #[test]
    fn test_page_to_doc_confluence_macro_code_block() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let html = r#"<ac:structured-macro ac:name="code"><ac:parameter ac:name="language">rust</ac:parameter><ac:plain-text-body><![CDATA[fn main() {}]]></ac:plain-text-body></ac:structured-macro>"#;
        let page = ConfluencePage {
            id: "3".into(),
            title: "Code".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: Some(ConfluenceBody {
                storage: Some(ConfluenceStorage { value: html.into() }),
            }),
            version: None,
            metadata: None,
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert!(matches!(doc.content_type, ContentType::Markdown));
        assert!(!doc.content.contains("<ac:"), "raw ac: tags should not appear: {}", doc.content);
    }

    #[test]
    fn test_page_to_doc_updated_at_from_version() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "1".into(),
            title: "Test".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: None,
            version: Some(ConfluenceVersion {
                when: Some("2024-01-15T10:30:00.000Z".into()),
            }),
            metadata: None,
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert_eq!(doc.updated_at, Some(1705314600));
    }

    #[test]
    fn test_page_to_doc_updated_at_none_when_missing() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "2".into(),
            title: "Test".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: None,
            version: None,
            metadata: None,
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert_eq!(doc.updated_at, None);
    }

    #[test]
    fn test_page_to_doc_space_key_in_metadata() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "3".into(),
            title: "Test".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: None,
            version: None,
            metadata: None,
            space: Some(ConfluenceSpace { key: "ENG".into() }),
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert_eq!(doc.metadata.get("space_key").and_then(|v| v.as_str()), Some("ENG"));
    }

    #[test]
    fn test_page_to_doc_labels_as_tags() {
        let mut plugin = ConfluencePlugin::new();
        plugin.base_url = Some("https://example.atlassian.net/wiki".into());
        let page = ConfluencePage {
            id: "4".into(),
            title: "Test".into(),
            content_type: "page".into(),
            links: ConfluenceLinks { web_ui: None },
            body: None,
            version: None,
            metadata: Some(ConfluencePageMetadata {
                labels: Some(ConfluenceLabels {
                    results: vec![
                        ConfluenceLabel { name: "architecture".into() },
                        ConfluenceLabel { name: "draft".into() },
                    ],
                }),
            }),
            space: None,
        };
        let doc = plugin.page_to_doc(page).unwrap();
        assert!(doc.tags.contains(&"architecture".to_string()));
        assert!(doc.tags.contains(&"draft".to_string()));
    }
}
