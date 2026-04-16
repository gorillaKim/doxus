pub mod html_convert;
pub mod path_utils;

use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use doxus_plugin_sdk::wasm_types::*;

macro_rules! log_debug {
    ($state:expr, $($arg:tt)*) => {
        if $state.get_config("debug") == Some("true") {
            eprintln!($($arg)*);
        }
    };
}

// ── Constants ────────────────────────────────────────────────────────────────

const STATE_VAR: &str = "__doxus_state";
const REFRESH_THRESHOLD_SECONDS: i64 = 600; // 10 minutes

// ── Host Functions ───────────────────────────────────────────────────────────

#[host_fn]
extern "ExtismHost" {
    fn __doxus_set_secret(key: String, value: String);
    fn __doxus_get_secret(key: String) -> String;
    fn __doxus_get_time() -> i64;
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
    #[serde(default)]
    hierarchy_cache: Option<HashMap<String, (String, Option<String>)>>,
    #[serde(default)]
    last_hierarchy_fetch: Option<i64>,
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

    fn get_secret(&self, key: &str) -> Option<String> {
        // 1. Try internal cache first
        if let Some(s) = self.secrets.get(key) {
            return Some(s.to_string());
        }

        // 2. Fallback to host function for on-demand fetch
        let val = unsafe { __doxus_get_secret(key.to_string()).ok().unwrap_or_default() };
        if !val.is_empty() {
            return Some(val);
        }

        None
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
    #[serde(default)]
    ancestors: Vec<ConfluenceAncestor>,
}

#[derive(Deserialize)]
struct ConfluenceAncestor {
    title: String,
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
struct ConfluenceSpace { 
    id: String,
    key: String,
    name: Option<String>,
}

// ── Confluence API v2 response shapes ──────────────────────────────────────────

#[derive(Deserialize)]
struct ConfluenceV2ListResponse<T> {
    results: Vec<T>,
    #[serde(rename = "_links")]
    links: ConfluenceV2Links,
}

#[derive(Deserialize)]
struct ConfluenceV2Links {
    next: Option<String>,
    webui: Option<String>,
}

#[derive(Deserialize)]
struct ConfluencePageV2 {
    id: String,
    title: String,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
    body: Option<ConfluenceV2Body>,
    version: Option<ConfluenceV2Version>,
    #[serde(rename = "_links")]
    links: ConfluenceV2Links,
}

#[derive(Deserialize)]
struct ConfluenceFolderV2 {
    id: String,
    title: String,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct ConfluenceV2Version {
    #[serde(rename = "createdAt")]
    created_at: String,
    number: i64,
}

#[derive(Deserialize)]
struct ConfluenceV2Body {
    storage: Option<ConfluenceV2Content>,
    atlas_doc_format: Option<ConfluenceV2Content>,
}

#[derive(Deserialize)]
struct ConfluenceV2Content {
    value: String,
}

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
        hierarchy_cache: None,
        last_hierarchy_fetch: None,
    };
    state.save()?;
    Ok(())
}

#[plugin_fn]
pub fn fetch_all(Json(opts): Json<FetchAllOptsWasm>) -> FnResult<Json<DocumentStreamWasm>> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let space_key = state.get_config("space_key").ok_or(Error::msg("space_key missing"))?.to_string();

    // 1. 스페이스 정보를 가져와서 가독성 있는 이름을 확보 (최상위 폴더명용)
    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;
    let root_folder_name = space_info.name.as_ref().unwrap_or(&space_info.key);

    // 2. 전체 계층 구조 파악을 위해 폴더 목록을 먼저 가져옴 (최적화: 실제로는 페이징 필요할 수 있음)
    // v2Folders API: GET /api/v2/spaces/{id}/folders
    // 여기서는 간단히 하기 위해 ID를 스페이스 키로 대체하거나 별도 조회 로직 필요
    // 현재 SDK 제약상 페이지 데이터와 함께 hierarchy를 구축하는 방향으로 진행
    
    log_debug!(state, "[Confluence] Starting fetch_all, space: {}", space_key);
    let mut hierarchy = get_full_hierarchy(&mut state, &base_url, &space_info.id)?;
    log_debug!(state, "[Confluence] Hierarchy built with {} entries", hierarchy.len());
    
    // 3. 페이지 조회 (v2)
    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;
    
    let ancestor_id = state.get_config("ancestor_id");
    let (pages_results, has_next) = if let Some(aid) = ancestor_id {
        let cql = format!("ancestor = \"{}\" AND type = page ORDER BY created ASC", aid);
        let url = format!("{base_url}/rest/api/content/search?cql={}&start={start}&limit={limit}", urlencoding::encode(&cql));
        let resp = request_with_auth(&state, "GET", &url, None)?;
        let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;
        
        let mut details = Vec::new();
        if !r.results.is_empty() {
            let ids = r.results.iter().map(|p| p.id.clone()).collect::<Vec<_>>().join(",");
            let v2_url = format!("{base_url}/api/v2/pages?id={}&body-format=storage", ids);
            let v2_resp = request_with_auth(&state, "GET", &v2_url, None)?;
            let v2_list: ConfluenceV2ListResponse<ConfluencePageV2> = serde_json::from_slice(&v2_resp.body())?;
            details = v2_list.results;
        }
        (details, r.size >= r.limit)
    } else {
        let pages_url = format!("{base_url}/api/v2/pages?spaceKey={space_key}&limit={limit}&offset={start}&body-format=storage");
        let resp = request_with_auth(&state, "GET", &pages_url, None)?;
        let r: ConfluenceV2ListResponse<ConfluencePageV2> = serde_json::from_slice(&resp.body())?;
        (r.results, r.links.next.is_some())
    };

    eprintln!("[Confluence] Found {} pages to sync", pages_results.len());

    // 4. 계층 지도 보완 (현재 페이지 정보도 부모가 될 수 있으므로 추가)
    for p in &pages_results {
        hierarchy.insert(p.id.clone(), (p.title.clone(), p.parent_id.clone()));
    }

    let next_cursor = if has_next { Some((start + limit).to_string()) } else { None };

    let documents = pages_results.into_iter()
        .map(|p| {
            let title = p.title.clone();
            let ancestor_id = state.get_config("ancestor_id");
            let doc = page_to_doc_v2(&state, &mut hierarchy, p, root_folder_name, &base_url, ancestor_id);
            log_debug!(state, "[Confluence]  - Converted: {}", title);
            doc
        })
        .collect();
    log_debug!(state, "[Confluence] Page sync complete");

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
    let url = format!("{base_url}/api/v2/pages/{}?body-format=storage", opts.id);
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let p: ConfluencePageV2 = serde_json::from_slice(&resp.body())?;

    // 단일 문서 조회 시에도 스페이스 정보를 가져옴
    let space_key = state.get_config("space_key").unwrap_or("Unknown").to_string();
    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;
    let root_name = space_info.name.as_ref().unwrap_or(&space_info.key);

    let mut hierarchy = get_full_hierarchy(&mut state, &base_url, &space_info.id)?;

    let ancestor_id = state.get_config("ancestor_id");
    Ok(Json(page_to_doc_v2(&state, &mut hierarchy, p, root_name, &base_url, ancestor_id)))
}

#[plugin_fn]
pub fn fetch_changes(Json(opts): Json<FetchChangesOptsWasm>) -> FnResult<Json<ChangeSetWasm>> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let space_key = state.get_config("space_key").ok_or(Error::msg("space_key missing"))?.to_string();

    // v2에서는 lastModified 대신 CQL을 직접적으로 사용하기 어려우므로 
    // 우선 v1 API의 검색을 사용하여 ID 목록을 가져온 뒤 v2로 상세 정보를 채우거나,
    // 전체 v2 페이지 목록에서 필터링하는 방식을 취해야 함.
    // 여기서는 일관성을 위해 v1 검색 후 v2 변환 방식을 사용 (현실적인 타협점)
    
    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;
    let since_dt = chrono::DateTime::from_timestamp(opts.since, 0).unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    let now_str = since_dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    
    let mut cql = format!("space = \"{space_key}\" AND type = page AND lastModified > \"{now_str}\"");
    if let Some(aid) = state.get_config("ancestor_id") {
        cql.push_str(&format!(" AND ancestor = \"{}\"", aid));
    }
    cql.push_str(" ORDER BY id ASC");
    
    let url = format!("{base_url}/rest/api/content/search?cql={}&start={start}&limit={limit}", urlencoding::encode(&cql));
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;

    let next_cursor = if r.size >= r.limit { Some((r.start + r.limit).to_string()) } else { None };

    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;
    let root_name = space_info.name.as_ref().unwrap_or(&space_info.key);

    // 상세 정보 및 계층 구조 복원
    let mut updated = Vec::new();
    let mut hierarchy = get_full_hierarchy(&mut state, &base_url, &space_info.id)?;

    let ancestor_id = state.get_config("ancestor_id");

    for v1_page in r.results {
        // 상세 정보 및 계층 구조 복원
        let v2_url = format!("{base_url}/api/v2/pages/{}?body-format=storage", v1_page.id);
        if let Ok(v2_resp) = request_with_auth(&state, "GET", &v2_url, None) {
             if let Ok(p_v2) = serde_json::from_slice::<ConfluencePageV2>(&v2_resp.body()) {
                 hierarchy.insert(p_v2.id.clone(), (p_v2.title.clone(), p_v2.parent_id.clone()));
                 updated.push(page_to_doc_v2(&state, &mut hierarchy, p_v2, root_name, &base_url, ancestor_id));
             }
        }
    }

    Ok(Json(ChangeSetWasm {
        updated,
        deleted: vec![],
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
    let api_token_opt = state.get_secret("confluence_api_token")
        .or_else(|| state.get_secret("api_token"));

    if let (Some(email), Some(api_token)) = (state.get_config("email"), api_token_opt) {
        let auth = format!("{email}:{api_token}");
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth);
        return Ok(format!("Basic {encoded}"));
    }

    Err(Error::msg("No authentication credentials (OAuth or API Token)").into())
}

fn request_with_auth(state: &PluginState, method: &str, url: &str, body: Option<Vec<u8>>) -> FnResult<HttpResponse> {
    let auth = auth_header(state)?;
    let auth_type = if auth.starts_with("Bearer") { "Bearer" } else { "Basic" };
    
    // WASM 인스턴스 로그 전송 (debug=true 일 때만 출력)
    log_debug!(state, "[Wasm-Network] {} request to: {}", method, url);
    log_debug!(state, "[Wasm-Network] Auth using: {}", auth_type);

    let mut req = HttpRequest::new(url);
    req.method = Some(method.to_string());
    req.headers.insert("Authorization".to_string(), auth);
    req.headers.insert("Accept".to_string(), "application/json".to_string());
    
    let resp = http::request(&req, body)?;
    if resp.status_code() >= 400 {
        let msg = format!("HTTP {}: {}", resp.status_code(), String::from_utf8_lossy(&resp.body()));
        eprintln!("[Wasm-Network] ERROR: {}", msg);
        return Err(Error::msg(msg).into());
    }
    Ok(resp)
}

fn ensure_valid_token(state: &mut PluginState) -> FnResult<()> {
    // Note: WASM guest doesn't have system time easily. For now, we rely on the host
    // or simple exists check.
    let now: i64 = unsafe { __doxus_get_time().ok().unwrap_or(0) }; 
    
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

fn page_to_doc_v2(
    state: &PluginState,
    hierarchy: &mut HashMap<String, (String, Option<String>)>,
    p: ConfluencePageV2, 
    space_name: &str,
    base_url: &str,
    stop_id: Option<&str>
) -> RawDocumentWasm {
    let storage_content = p.body.as_ref()
        .and_then(|b| b.atlas_doc_format.as_ref().or(b.storage.as_ref()))
        .map(|s| s.value.as_str())
        .unwrap_or_default();
    
    let markdown = html_convert::confluence_html_to_markdown(storage_content);
    
    let (ancestor_titles, root_name) = resolve_ancestors(state, base_url, &p.id, hierarchy, stop_id);
    
    // 만약 stop_id가 지정되었는데 못찾았다면, 최소한 스페이스명을 루트로 쓰지 않음 (평면화 방지)
    let final_root_name = if !root_name.is_empty() {
        root_name
    } else if stop_id.is_some() {
        "".to_string() 
    } else {
        space_name.to_string()
    };

    let relative_path = path_utils::build_relative_path(&final_root_name, &ancestor_titles, &p.title);

    let updated_at = p.version.as_ref().and_then(|v| {
        chrono::DateTime::parse_from_rfc3339(&v.created_at).ok().map(|dt| dt.timestamp())
    });

    // v2 webui link는 "/pages/..." 형태이므로 base_url과 조합
    let url = p.links.webui.map(|ui| format!("{}{}", base_url, ui));

    let mut metadata = HashMap::new();
    metadata.insert("debug_check".to_string(), serde_json::json!("OK-PATH-FIXED"));
    
    if let Some(sid) = &p.space_id {
        metadata.insert("space_id".to_string(), serde_json::json!(sid));
    }

    RawDocumentWasm {
        id: p.id,
        title: Some(p.title),
        content: markdown,
        content_type: "markdown".into(),
        url,
        metadata,
        tags: vec![],
        created_at: updated_at,
        updated_at,
        relative_path: Some(relative_path),
    }
}

fn get_full_hierarchy(
    state: &mut PluginState, 
    base_url: &str, 
    space_id: &str
) -> FnResult<HashMap<String, (String, Option<String>)>> {
    let now: i64 = unsafe { __doxus_get_time().ok().unwrap_or(0) };
    const CACHE_TTL_SECONDS: i64 = 3600; // 1 hour

    if let (Some(cache), Some(last_fetch)) = (&state.hierarchy_cache, state.last_hierarchy_fetch) {
        if now < last_fetch + CACHE_TTL_SECONDS {
            log_debug!(state, "[Confluence] Using cached hierarchy ({} entries)", cache.len());
            return Ok(cache.clone());
        }
    }

    let mut hierarchy = HashMap::new();
    
    // 1. Fetch Pages (Standard hierarchy)
    let mut pages_url = format!("{base_url}/api/v2/spaces/{space_id}/pages?limit=250");
    let mut pages_count = 0;
    loop {
        pages_count += 1;
        if pages_count > 100 {
            log_debug!(state, "[Confluence] Pages hierarchy fetch reached safety limit (100 pages)");
            break;
        }
        
        log_debug!(state, "[Confluence] Fetching pages hierarchy page {}, url: {}", pages_count, pages_url);
        if let Ok(resp) = request_with_auth(state, "GET", &pages_url, None) {
            if let Ok(list) = serde_json::from_slice::<ConfluenceV2ListResponse<ConfluencePageV2>>(&resp.body()) {
                let current_batch_size = list.results.len();
                for p in list.results {
                    hierarchy.insert(p.id, (p.title, p.parent_id));
                }
                
                if let Some(next) = list.links.next {
                    pages_url = if next.starts_with("http") { next } else { format!("{}{}", base_url, next) };
                } else if current_batch_size >= 250 {
                   // Fallback if links.next is missing but results are full
                   pages_url = format!("{base_url}/api/v2/spaces/{space_id}/pages?limit=250&offset={}", pages_count * 250);
                } else {
                    break;
                }
            } else { break; }
        } else { break; }
    }

    // 2. Fetch Folders (For premium plans/new types if any)
    let mut folders_url = format!("{base_url}/api/v2/spaces/{space_id}/folders?limit=250");
    let mut folders_count = 0;
    loop {
        folders_count += 1;
        if folders_count > 100 { break; }
        if let Ok(resp) = request_with_auth(state, "GET", &folders_url, None) {
            if let Ok(list) = serde_json::from_slice::<ConfluenceV2ListResponse<ConfluenceFolderV2>>(&resp.body()) {
                for f in list.results {
                    hierarchy.insert(f.id, (f.title, f.parent_id));
                }
                if let Some(next) = list.links.next {
                    folders_url = if next.starts_with("http") { next } else { format!("{}{}", base_url, next) };
                } else { break; }
            } else { break; }
        } else { break; }
    }

    state.hierarchy_cache = Some(hierarchy.clone());
    state.last_hierarchy_fetch = Some(now);
    state.save()?;

    Ok(hierarchy)
}

fn fetch_page_info(
    state: &PluginState,
    base_url: &str,
    id: &str
) -> Option<(String, Option<String>)> {
    log_debug!(state, "[Confluence] Fetching missing parent info for ID: {}", id);
    let url = format!("{base_url}/api/v2/pages/{id}");
    if let Ok(resp) = request_with_auth(state, "GET", &url, None) {
        if let Ok(p) = serde_json::from_slice::<ConfluencePageV2>(&resp.body()) {
            return Some((p.title, p.parent_id));
        }
    }
    // Fallback to folders if page not found
    let url = format!("{base_url}/api/v2/folders/{id}");
    if let Ok(resp) = request_with_auth(state, "GET", &url, None) {
        if let Ok(f) = serde_json::from_slice::<ConfluenceFolderV2>(&resp.body()) {
            return Some((f.title, f.parent_id));
        }
    }
    None
}

/// 조상 경로를 추적합니다. 캐시에 없는 경우 API호출로 보완합니다.
fn resolve_ancestors(
    state: &PluginState,
    base_url: &str,
    current_id: &str, 
    hierarchy: &mut HashMap<String, (String, Option<String>)>,
    stop_id: Option<&str>
) -> (Vec<String>, String) {
    let mut ancestors = Vec::new();
    let mut cursor = current_id.to_string();
    let mut root_title = String::new();

    loop {
        let (title, parent_id) = if let Some(info) = hierarchy.get(&cursor) {
            info.clone()
        } else {
            // 캐시에 없으면 API로 직접 가져옴
            if let Some(info) = fetch_page_info(state, base_url, &cursor) {
                hierarchy.insert(cursor.clone(), info.clone());
                info
            } else {
                break;
            }
        };

        if let Some(sid) = stop_id {
            if cursor == sid {
                root_title = title.clone();
                break;
            }
        }

        ancestors.push(title.clone());
        if let Some(pid) = parent_id {
            let next_pid = pid.clone();
            if next_pid == cursor { break; }
            cursor = next_pid;
        } else {
            if stop_id.is_none() {
                root_title = title.clone();
            }
            break;
        }
    }

    if !ancestors.is_empty() {
        ancestors.remove(0);
    }
    ancestors.reverse();
    (ancestors, root_title)
}
