pub mod html_convert;

use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use doxus_plugin_sdk::wasm_types::*;

// ── Constants ────────────────────────────────────────────────────────────────

const STATE_VAR: &str = "__doxus_state";
const REFRESH_THRESHOLD_SECONDS: i64 = 600; // 10 minutes

// ── Host Functions ───────────────────────────────────────────────────────────

#[host_fn]
extern "ExtismHost" {
    fn __doxus_set_secret(key: String, value: String);
}

// ── State Management ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PluginState {
    config: HashMap<String, serde_json::Value>,
    secrets: HashMap<String, String>,
    // Cached tokens from successful refreshes during this session (instance lifetime)
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
}

impl PluginState {
    fn load() -> FnResult<Self> {
        let bytes: Vec<u8> = var::get(STATE_VAR)?.ok_or(Error::msg("State not initialized"))?;
        let state: Self = serde_json::from_slice(&bytes)?;
        Ok(state)
    }

    fn save(&self) -> FnResult<()> {
        let bytes = serde_json::to_vec(self)?;
        var::set(STATE_VAR, bytes)?;
        Ok(())
    }

    fn get_config(&self, key: &str) -> Option<&str> {
        self.config.get(key).and_then(|v| v.as_str())
    }

    fn get_secret(&self, key: &str) -> Option<&str> {
        self.secrets.get(key).map(|s| s.as_str())
    }
}

// ── Confluence API response shapes ────────────────────────────────────────────

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

fn default_page_type() -> String { "page".to_string() }

#[derive(Deserialize)]
struct ConfluenceLinks { webui: Option<String> }
#[derive(Deserialize)]
struct ConfluenceBody { storage: Option<ConfluenceStorage> }
#[derive(Deserialize)]
struct ConfluenceStorage { value: String }
#[derive(Deserialize)]
struct ConfluenceVersion { when: Option<String> }
#[derive(Deserialize)]
struct ConfluencePageMetadata { labels: Option<ConfluenceLabels> }
#[derive(Deserialize)]
struct ConfluenceLabels { results: Vec<ConfluenceLabel> }
#[derive(Deserialize)]
struct ConfluenceLabel { name: String }
#[derive(Deserialize)]
struct ConfluenceSpace { key: String }

// ── OAuth ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

// ── Entry Points ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InitOpts {
    config: HashMap<String, serde_json::Value>,
    secrets: HashMap<String, String>,
}

#[plugin_fn]
pub fn initialize(Json(opts): Json<InitOpts>) -> FnResult<()> {
    // Initial tokens from secrets
    let access_token = opts.secrets.get("access_token").cloned();
    let refresh_token = opts.secrets.get("refresh_token").cloned();
    let expires_at = opts.secrets.get("expires_at").and_then(|s| s.parse::<i64>().ok());

    let state = PluginState {
        config: opts.config,
        secrets: opts.secrets,
        access_token,
        refresh_token,
        expires_at,
    };
    state.save()?;
    Ok(())
}

#[plugin_fn]
pub fn fetch_all(Json(opts): Json<FetchAllOptsWasm>) -> FnResult<Json<DocumentStreamWasm>> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;

    let cql = if let Some(ancestor_id) = state.get_config("ancestor_id") {
        format!("ancestor = \"{ancestor_id}\" ORDER BY id ASC")
    } else {
        let space_key = state.get_config("space_key").ok_or(Error::msg("space_key missing"))?;
        format!("space = \"{space_key}\" AND type = page ORDER BY id ASC")
    };

    let url = format!("{base_url}/rest/api/content/search?cql={}&expand=body.storage,version,metadata.labels,space&start={start}&limit={limit}", 
        urlencoding::encode(&cql));
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;

    let next_cursor = if r.size >= r.limit {
        Some((r.start + r.limit).to_string())
    } else {
        None
    };

    let documents = r.results.into_iter()
        .map(|p| page_to_doc(p))
        .collect();

    Ok(Json(DocumentStreamWasm {
        documents,
        next_cursor,
        estimated_total: None,
    }))
}

#[plugin_fn]
pub fn fetch_document(Json(opts): Json<FetchDocumentOptsWasm>) -> FnResult<Json<RawDocumentWasm>> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let url = format!("{base_url}/rest/api/content/{}?expand=body.storage,version,metadata.labels,space", opts.id);
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let p: ConfluencePage = serde_json::from_slice(&resp.body())?;

    Ok(Json(page_to_doc(p)))
}

#[plugin_fn]
pub fn fetch_changes(Json(opts): Json<FetchChangesOptsWasm>) -> FnResult<Json<ChangeSetWasm>> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;

    // Convert Unix timestamp to ISO 8601 for CQL
    let since_dt = chrono::DateTime::from_timestamp(opts.since, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    let since_str = since_dt.format("%Y-%m-%dT%H:%M:%S").to_string();

    let cql = if let Some(ancestor_id) = state.get_config("ancestor_id") {
        format!("ancestor = \"{ancestor_id}\" AND lastModified > \"{since_str}\" ORDER BY id ASC")
    } else {
        let space_key = state.get_config("space_key").ok_or(Error::msg("space_key missing"))?;
        format!("space = \"{space_key}\" AND type = page AND lastModified > \"{since_str}\" ORDER BY id ASC")
    };

    let url = format!("{base_url}/rest/api/content/search?cql={}&expand=body.storage,version,metadata.labels,space&start={start}&limit={limit}", 
        urlencoding::encode(&cql));
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;

    let next_cursor = if r.size >= r.limit {
        Some((r.start + r.limit).to_string())
    } else {
        None
    };

    let updated = r.results.into_iter()
        .map(|p| page_to_doc(p))
        .collect();

    Ok(Json(ChangeSetWasm {
        updated,
        deleted: vec![], // CQL search doesn't easily show deletions
        next_cursor,
    }))
}

#[plugin_fn]
pub fn health_check() -> FnResult<String> {
    let state = PluginState::load()?;
    let base_url = get_base_url(&state)?;
    let url = format!("{base_url}/rest/api/content?limit=1");
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    if resp.status_code() == 200 {
        Ok("Healthy".into())
    } else {
        let msg = format!("Confluence returned status {}", resp.status_code());
        Err(Error::msg(msg).into())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn get_base_url(state: &PluginState) -> FnResult<String> {
    let raw = state.get_config("base_url").ok_or(Error::msg("base_url missing"))?;
    let mut url = raw.trim_end_matches('/').to_string();
    if url.contains(".atlassian.net") && !url.ends_with("/wiki") {
        url.push_str("/wiki");
    }
    Ok(url)
}

fn auth_header(state: &PluginState) -> FnResult<String> {
    if let Some(token) = &state.access_token {
        return Ok(format!("Bearer {token}"));
    }
    
    // Fallback to Basic Auth (email + api_token)
    if let (Some(email), Some(api_token)) = (state.get_config("email"), state.get_secret("api_token")) {
        let auth = format!("{email}:{api_token}");
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth);
        return Ok(format!("Basic {encoded}"));
    }

    Err(Error::msg("No authentication credentials (OAuth or API Token)").into())
}

fn request_with_auth(state: &PluginState, method: &str, url: &str, body: Option<Vec<u8>>) -> FnResult<HttpResponse> {
    let mut req = HttpRequest::new(url);
    req.method = Some(method.to_string());
    req.headers.insert("Authorization".to_string(), auth_header(state)?);
    req.headers.insert("Accept".to_string(), "application/json".to_string());
    
    let resp = http::request(&req, body)?;
    if resp.status_code() >= 400 {
        let msg = format!("HTTP {}: {}", resp.status_code(), String::from_utf8_lossy(&resp.body()));
        return Err(Error::msg(msg).into());
    }
    Ok(resp)
}

fn ensure_valid_token(state: &mut PluginState) -> FnResult<()> {
    // Note: WASM guest doesn't have system time easily. For now, we rely on the host
    // or simple exists check.
    let now: i64 = 0; 
    
    let needs_refresh = if let Some(expires_at) = state.expires_at {
        now + REFRESH_THRESHOLD_SECONDS >= expires_at
    } else {
        state.access_token.is_none() && state.refresh_token.is_some()
    };

    if needs_refresh {
        if let Some(refresh_token) = state.refresh_token.clone() {
            refresh_oauth_token(state, &refresh_token)?;
        }
    }

    Ok(())
}

fn refresh_oauth_token(state: &mut PluginState, refresh_token: &str) -> FnResult<()> {
    let client_id = state.get_config("client_id").ok_or(Error::msg("client_id missing"))?;
    let client_secret = state.get_config("client_secret").ok_or(Error::msg("client_secret missing"))?;
    
    // Allow overriding the OAuth URL for testing (default: Atlassian production)
    let oauth_base = state.get_config("oauth_base_url")
        .unwrap_or("https://auth.atlassian.com");
    let url = format!("{oauth_base}/oauth/token");
    
    let payload = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": client_id,
        "client_secret": client_secret,
        "refresh_token": refresh_token,
    });

    let mut req = HttpRequest::new(url);
    req.method = Some("POST".to_string());
    req.headers.insert("Content-Type".to_string(), "application/json".to_string());
    let body_bytes = serde_json::to_vec(&payload)?;

    let resp = http::request(&req, Some(body_bytes))?;
    if resp.status_code() != 200 {
        let msg = format!("Refresh failed ({}): {}", resp.status_code(), String::from_utf8_lossy(&resp.body()));
        return Err(Error::msg(msg).into());
    }

    let token_resp: TokenResponse = serde_json::from_slice(&resp.body())?;
    
    // Update local state
    state.access_token = Some(token_resp.access_token.clone());
    if let Some(rt) = token_resp.refresh_token {
        state.refresh_token = Some(rt);
    }
    // TODO(P2): get_time 호스트 함수 도입 후 `now + expires_in`으로 절대 시각 저장
    // 현재는 now=0 기반 비교이므로 expires_in 값 그대로 유지 (일관성 보장)
    state.expires_at = Some(token_resp.expires_in);
    state.save()?;

    // Push updated tokens to host immediately via host function.
    // Errors are propagated: if the host cannot persist the token, we treat the
    // refresh as failed so the caller can surface the problem to the user.
    unsafe {
        __doxus_set_secret("access_token".to_string(), state.access_token.clone().unwrap())?;
        if let Some(rt) = &state.refresh_token {
            __doxus_set_secret("refresh_token".to_string(), rt.clone())?;
        }
        __doxus_set_secret("expires_at".to_string(), state.expires_at.unwrap().to_string())?;
    }

    Ok(())
}

fn page_to_doc(p: ConfluencePage) -> RawDocumentWasm {
    let content = p.body.and_then(|b| b.storage).map(|s| s.value).unwrap_or_default();
    let markdown = html_convert::confluence_html_to_markdown(&content);
    
    let updated_at = p.version.and_then(|v| v.when).and_then(|w| {
        chrono::DateTime::parse_from_rfc3339(&w).ok().map(|dt| dt.timestamp())
    });

    let url = p.links.webui.map(|ui| ui);

    RawDocumentWasm {
        id: p.id,
        title: Some(p.title),
        content: markdown,
        content_type: "markdown".into(),
        url,
        metadata: HashMap::new(),
        tags: vec![],
        updated_at,
    }
}
