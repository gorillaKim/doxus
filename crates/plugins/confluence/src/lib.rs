pub mod html_convert;
pub mod path_utils;

#[cfg(not(target_arch = "wasm32"))]
pub mod oauth_server;

#[cfg(target_arch = "wasm32")]
use extism_pdk::*;

#[cfg(not(target_arch = "wasm32"))]
mod native_compat {
    pub type FnResult<T> = anyhow::Result<T>;
    pub type Error = anyhow::Error;
    
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct Json<T>(pub T);

    pub struct HttpRequest {
        pub url: String,
        pub method: Option<String>,
        pub headers: std::collections::HashMap<String, String>,
    }
    impl HttpRequest {
        pub fn new<S: Into<String>>(url: S) -> Self {
            Self {
                url: url.into(),
                method: None,
                headers: std::collections::HashMap::new(),
            }
        }
    }

    pub struct HttpResponse { 
        body: Vec<u8>, 
        status: u16 
    }
    impl HttpResponse {
        pub fn body(&self) -> &[u8] { &self.body }
        pub fn status_code(&self) -> u16 { self.status }
    }

    pub mod var {
        use std::collections::HashMap;
        use std::sync::Mutex;
        static MOCK_VARS: once_cell::sync::Lazy<Mutex<HashMap<String, Vec<u8>>>> = 
            once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));
            
        pub fn get(key: &str) -> super::FnResult<Option<Vec<u8>>> {
            Ok(MOCK_VARS.lock().unwrap().get(key).cloned())
        }
        pub fn set(key: &str, val: Vec<u8>) -> super::FnResult<()> {
            MOCK_VARS.lock().unwrap().insert(key.to_string(), val);
            Ok(())
        }
    }

    pub mod http {
        use once_cell::sync::Lazy;
        static CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
            reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client")
        });

        pub fn request(req: &super::HttpRequest, body: Option<Vec<u8>>) -> super::FnResult<super::HttpResponse> {
            let method = match req.method.as_deref().unwrap_or("GET") {
                "GET" => reqwest::Method::GET,
                "POST" => reqwest::Method::POST,
                "PUT" => reqwest::Method::PUT,
                "DELETE" => reqwest::Method::DELETE,
                _ => reqwest::Method::GET,
            };

            tracing::debug!("[Confluence-HTTP] Method: {}, URL: {}", method, req.url);
            
            if req.url.is_empty() || !req.url.starts_with("http") {
                return Err(super::Error::msg(format!("Invalid or empty URL for Confluence request: '{}'", req.url)).into());
            }

            let mut request_builder = CLIENT.request(method, &req.url);
            
            for (k, v) in &req.headers {
                request_builder = request_builder.header(k, v);
            }

            if let Some(b) = body {
                request_builder = request_builder.body(b);
            }

            let resp = request_builder.send().map_err(|e| super::Error::from(anyhow::anyhow!(e)))?;
            let status = resp.status().as_u16();
            let body = resp.bytes().map_err(|e| super::Error::from(anyhow::anyhow!(e)))?.to_vec();

            Ok(super::HttpResponse { body, status })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native_compat::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use doxus_plugin_sdk::{PluginError, wasm_types::*};

#[cfg(not(target_arch = "wasm32"))]
use async_trait::async_trait;

#[cfg(not(target_arch = "wasm32"))]
use doxus_plugin_sdk::{
    DocSource, FetchAllOpts, FetchChangesOpts, PluginMetadata, PluginKind, 
    Capabilities, HealthStatus, PluginConfig, PluginSecrets, RawDocument, 
    SourceDocId, ContentType as NativeContentType, DocumentStream, ChangeSet
};

macro_rules! log_d {
    ($tag:expr, $($arg:tt)*) => {
        if let Ok(state) = PluginState::load() {
            if state.debug_tags.contains($tag) {
                eprintln!($($arg)*);
            }
        }
    };
}

// ── Constants ────────────────────────────────────────────────────────────────

const STATE_VAR: &str = "__doxus_state";
const REFRESH_THRESHOLD_SECONDS: i64 = 600; // 10 minutes

// ── Host Functions ───────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    fn __doxus_set_secret(key: String, value: String);
    fn __doxus_get_secret(key: String) -> String;
    fn __doxus_get_time() -> i64;
}

#[cfg(not(target_arch = "wasm32"))]
mod native_host {
    pub unsafe fn __doxus_set_secret(_key: String, _value: String) -> Result<(), anyhow::Error> { Ok(()) }
    pub unsafe fn __doxus_get_secret(_key: String) -> Result<String, anyhow::Error> { Ok(String::new()) }
    pub unsafe fn __doxus_get_time() -> Result<i64, anyhow::Error> {
        Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64)
    }
}
#[cfg(not(target_arch = "wasm32"))]
use native_host::*;

// ── State Management ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PluginState {
    config: HashMap<String, serde_json::Value>,
    secrets: HashMap<String, String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    #[serde(default)]
    hierarchy_cache: Option<HashMap<String, (String, Option<String>)>>,
    #[serde(default)]
    last_hierarchy_fetch: Option<i64>,
    #[serde(default)]
    debug_tags: std::collections::HashSet<String>,
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

    fn get_config_string(&self, key: &str) -> Option<String> {
        self.config.get(key).and_then(|v| {
            match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
        })
    }

    fn update_hierarchy_cache(&mut self, id: &str, title: &str, parent_id: Option<&str>) {
        let cache = self.hierarchy_cache.get_or_insert_with(HashMap::new);
        cache.insert(id.to_string(), (title.to_string(), parent_id.map(|s| s.to_string())));
        self.last_hierarchy_fetch = Some(now_secs());
    }

    fn get_secret(&self, key: &str) -> Option<String> {
        if let Some(s) = self.secrets.get(key) {
            return Some(s.to_string());
        }
        let val = unsafe { __doxus_get_secret(key.to_string()).ok().unwrap_or_default() };
        if !val.is_empty() {
            return Some(val);
        }
        None
    }
}

fn now_secs() -> i64 {
    unsafe { __doxus_get_time().unwrap_or(0) }
}

// ── Confluence API response shapes ────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Deserialize)]
struct ConfluenceCqlResult {
    results: Vec<ConfluencePage>,
    start: i64,
    limit: i64,
    size: i64,
    #[serde(rename = "totalSize", default)]
    total_size: Option<i64>,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Deserialize)]
struct ConfluenceAncestor {
    title: String,
}

fn default_page_type() -> String { "page".to_string() }

#[allow(dead_code)]
#[derive(Deserialize)]
struct ConfluenceLinks { webui: Option<String> }
#[allow(dead_code)]
#[derive(Deserialize)]
struct ConfluenceBody { storage: Option<ConfluenceStorage> }
#[allow(dead_code)]
#[derive(Deserialize)]
struct ConfluenceStorage { value: String }
#[allow(dead_code)]
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

// ── Write Operation Shapes ───────────────────────────────────────────────────

#[derive(Serialize)]
struct CreatePageRequestV2 {
    #[serde(rename = "spaceId")]
    space_id: String,
    title: String,
    status: String,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    body: CreatePageBodyV2,
}

#[derive(Serialize)]
struct CreatePageBodyV2 {
    representation: String,
    value: String,
}

#[derive(Serialize)]
struct UpdatePageRequestV2 {
    id: String,
    status: String,
    title: String,
    body: CreatePageBodyV2,
    version: UpdatePageVersionV2,
}

#[derive(Serialize)]
struct UpdatePageVersionV2 {
    number: i64,
    message: String,
}

#[derive(Deserialize, Serialize)]
struct CreatePageResultV2 {
    id: String,
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
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(default)]
    number: Option<i64>,
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

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

// ── Entry Points ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct InitOpts {
    config: HashMap<String, serde_json::Value>,
    secrets: HashMap<String, String>,
    #[serde(default)]
    debug_tags: Vec<String>,
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn initialize(Json(opts): Json<InitOpts>) -> FnResult<()> {
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
        debug_tags: opts.debug_tags.into_iter().collect(),
    };
    state.save()?;
    Ok(())
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_all(Json(opts): Json<FetchAllOptsWasm>) -> FnResult<Json<DocumentStreamWasm>> {
    fetch_all_impl(opts).map(Json)
}

pub(crate) fn fetch_all_impl(opts: FetchAllOptsWasm) -> FnResult<DocumentStreamWasm> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let space_key = state.get_config_string("space_key")
        .ok_or_else(|| Error::msg("space_key missing"))?;

    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;
    let root_folder_name = space_info.name.as_ref().unwrap_or(&space_info.key);
    
    let mut hierarchy = get_full_hierarchy(&mut state, &base_url, &space_info.id)?;
    
    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;
    
    let mut doc_tags = HashMap::new();
    let ancestor_id = state.get_config_string("ancestor_id");
    let (pages_results, has_next, cql_total) = if let Some(aid) = &ancestor_id {
        let cql = format!("ancestor = \"{}\" ORDER BY id ASC", aid);
        let url = format!("{base_url}/rest/api/content/search?cql={}&expand=metadata.labels&start={start}&limit={limit}", urlencoding::encode(&cql));
        log_d!("confluence", "[Confluence-Debug] CQL URL: {}", url);

        let resp = request_with_auth(&state, "GET", &url, None)?;
        let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;
        let total_hint = r.total_size.map(|t| t as u64);

        let mut details = Vec::new();
        if !r.results.is_empty() {
            // Filter only pages (exclude folder type if it appeared)
            let page_ids: Vec<String> = r.results.iter()
                .inspect(|p| log_d!("confluence", "[Confluence-Debug] - Result ID: {}, Type: {}", p.id, p.content_type))
                .filter(|p| p.content_type.to_lowercase() == "page")
                .map(|p| {
                    let tags: Vec<String> = p.metadata.as_ref()
                        .and_then(|m| m.labels.as_ref())
                        .map(|l| l.results.iter().map(|lab| lab.name.clone()).collect())
                        .unwrap_or_default();
                    doc_tags.insert(p.id.clone(), tags);
                    p.id.clone()
                })
                .collect();


            if !page_ids.is_empty() {
                let ids_str = page_ids.join(",");
                // [Doxus Fix] version 정보를 명시적으로 요청하여 updated_at(수정 날짜)이 누락되지 않게 함
                let v2_url = format!("{base_url}/api/v2/pages?id={}&body-format=storage&limit={}", ids_str, page_ids.len());
                log_d!("confluence", "[Confluence-Debug] V2 Detail URL: {}", v2_url);
                let v2_resp = request_with_auth(&state, "GET", &v2_url, None)?;
                let v2_list: ConfluenceV2ListResponse<ConfluencePageV2> = serde_json::from_slice(&v2_resp.body())?;
                log_d!("confluence", "[Confluence-Debug] Received {} V2 details", v2_list.results.len());
                details = v2_list.results;
            }
        }
        (details, r.results.len() as i64 >= r.limit, total_hint)
    } else {
        let pages_url = format!("{base_url}/api/v2/pages?spaceKey={space_key}&limit={limit}&offset={start}&body-format=storage");
        let resp = request_with_auth(&state, "GET", &pages_url, None)?;
        let r: ConfluenceV2ListResponse<ConfluencePageV2> = serde_json::from_slice(&resp.body())?;
        (r.results, r.links.next.is_some(), None)
    };

    for p in &pages_results {
        hierarchy.insert(p.id.clone(), (p.title.clone(), p.parent_id.clone()));
    }

    let next_cursor = if has_next { Some((start + limit).to_string()) } else { None };

    let mut documents: Vec<RawDocumentWasm> = pages_results.into_iter()
        .map(|p| {
            let ancestor_id = state.get_config_string("ancestor_id");
            let tags = doc_tags.remove(&p.id).unwrap_or_default();
            page_to_doc_v2(&state, &mut hierarchy, p, root_folder_name, &space_key, &base_url, ancestor_id.as_deref(), tags)
        })
        .collect();

    // [Doxus Fix] Root Ancestor 본인 추가
    if start == 0 {
        if let Some(aid) = &ancestor_id {
            // 중복 방지: 이미 목록에 있는지 확인
            if !documents.iter().any(|d| d.id == *aid) {
                if let Ok(root_raw) = fetch_document_impl(FetchDocumentOptsWasm { id: aid.clone() }) {
                    log_d!("confluence", "[Confluence-Debug] Prepending root ancestor document to the stream.");
                    documents.insert(0, root_raw);
                }
            }
        }
    }

    Ok(DocumentStreamWasm {
        documents,
        next_cursor,
        estimated_total: cql_total,
    })
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_document(Json(opts): Json<FetchDocumentOptsWasm>) -> FnResult<Json<RawDocumentWasm>> {
    fetch_document_impl(opts).map(Json)
}

pub(crate) fn fetch_document_impl(opts: FetchDocumentOptsWasm) -> FnResult<RawDocumentWasm> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let url = format!("{base_url}/api/v2/pages/{}?body-format=storage", opts.id);
    log_d!("confluence", "[Confluence-Debug] Fetching document by ID: {} -> URL: {}", opts.id, url);
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let p: ConfluencePageV2 = serde_json::from_slice(&resp.body())?;

    let space_key = state.get_config_string("space_key").unwrap_or_else(|| "Unknown".to_string());
    let root_name = state.get_config_string("space_name").unwrap_or_else(|| space_key.clone());

    // [Doxus Optimize] 단일 문서 조회 시에는 전체 계층 구조를 가져오지 않습니다.
    // resolve_ancestors에서 필요한 시점에만 상단 노드를 가져오도록(Lazy fetch) 변경하여 
    // 수천 개의 페이지가 있는 스페이스에서도 즉시 응답하도록 개선합니다.
    let mut hierarchy = HashMap::new();

    let ancestor_id = state.get_config_string("ancestor_id");
    Ok(page_to_doc_v2(&state, &mut hierarchy, p, &root_name, &space_key, &base_url, ancestor_id.as_deref(), vec![]))
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_changes(Json(opts): Json<FetchChangesOptsWasm>) -> FnResult<Json<ChangeSetWasm>> {
    fetch_changes_impl(opts).map(Json)
}

pub(crate) fn fetch_changes_impl(opts: FetchChangesOptsWasm) -> FnResult<ChangeSetWasm> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let space_key = state.get_config_string("space_key")
        .ok_or_else(|| Error::msg("space_key missing"))?;

    let start = opts.cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let limit = opts.page_size as i64;
    let since_dt = chrono::DateTime::from_timestamp(opts.since, 0).unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    let now_str = since_dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    
    let mut cql = format!("space = \"{space_key}\" AND lastModified > \"{now_str}\"");
    if let Some(aid) = state.get_config_string("ancestor_id") {
        cql.push_str(&format!(" AND ancestor = \"{}\"", aid));
    }
    cql.push_str(" ORDER BY id ASC");
    
    let url = format!("{base_url}/rest/api/content/search?cql={}&start={start}&limit={limit}", urlencoding::encode(&cql));
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    let r: ConfluenceCqlResult = serde_json::from_slice(&resp.body())?;

    let next_cursor = if r.results.len() as i64 >= r.limit { Some((r.start + r.limit).to_string()) } else { None };

    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;
    let root_name = space_info.name.as_ref().unwrap_or(&space_info.key);

    let mut updated = Vec::new();
    let mut hierarchy = get_full_hierarchy(&mut state, &base_url, &space_info.id)?;
    let ancestor_id = state.get_config_string("ancestor_id");

    // [Doxus 고도화] N+1 문제를 해결하기 위한 배치(Batch) 처리 로직
    // 변경된 문서 ID들을 25개씩 묶어서 본문 내용을 한 번에 요청합니다.
    let updated_ids: Vec<String> = r.results.iter().map(|p| p.id.clone()).collect();
    
    for chunk in updated_ids.chunks(25) {
        let ids_query = chunk.join(",");
        let batch_url = format!("{base_url}/api/v2/pages?id={}&body-format=storage", ids_query);
        
        log_d!("confluence", "[Confluence-Batch] Fetching batch of {} docs", chunk.len());
        
        if let Ok(batch_resp) = request_with_auth(&state, "GET", &batch_url, None) {
            if let Ok(batch_list) = serde_json::from_slice::<ConfluenceV2ListResponse<ConfluencePageV2>>(&batch_resp.body()) {
                for p_v2 in batch_list.results {
                    // 계층 구조 캐시 업데이트
                    hierarchy.insert(p_v2.id.clone(), (p_v2.title.clone(), p_v2.parent_id.clone()));
                    
                    // 태그 정보 매칭 (v1 검색 결과에서 가져옴)
                    let tags: Vec<String> = r.results.iter()
                        .find(|v1| v1.id == p_v2.id)
                        .and_then(|v1| v1.metadata.as_ref())
                        .and_then(|m| m.labels.as_ref())
                        .map(|l| l.results.iter().map(|lab| lab.name.clone()).collect())
                        .unwrap_or_default();
                        
                    updated.push(page_to_doc_v2(&state, &mut hierarchy, p_v2, root_name, &space_key, &base_url, ancestor_id.as_deref(), tags));
                }
            }
        }
    }

    Ok(ChangeSetWasm {
        updated,
        deleted: vec![],
        next_cursor,
    })
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn health_check() -> FnResult<String> {
    health_check_impl()
}

pub(crate) fn health_check_impl() -> FnResult<String> {
    let state = PluginState::load()?;
    let base_url = get_base_url(&state)?;
    let url = format!("{base_url}/rest/api/content?limit=1");
    
    let resp = request_with_auth(&state, "GET", &url, None)?;
    if resp.status_code() == 200 {
        Ok("Healthy".into())
    } else {
        let msg = format!("Confluence returned status {}", resp.status_code());
        Ok(msg)
    }
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn create_document(Json(opts): Json<CreateDocumentOptsWasm>) -> FnResult<Json<CreateDocumentResultWasm>> {
    create_document_impl(opts).map(Json)
}

pub(crate) fn create_document_impl(opts: CreateDocumentOptsWasm) -> FnResult<CreateDocumentResultWasm> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let space_key = state.get_config("space_key")
        .ok_or_else(|| Error::msg("space_key missing"))?
        .to_string();

    let space_url = format!("{base_url}/api/v2/spaces?keys={space_key}");
    let space_resp = request_with_auth(&state, "GET", &space_url, None)?;
    let space_list: ConfluenceV2ListResponse<ConfluenceSpace> = serde_json::from_slice(&space_resp.body())?;
    let space_info = space_list.results.first().ok_or(Error::msg("Space not found"))?;

    let segments = doxus_plugin_sdk::path_utils::parse_hierarchical_path(opts.folder.as_deref(), &opts.title)?;
    
    let config_ancestor_id = state.get_config("ancestor_id").map(|s| s.to_string());
    let mut current_parent_id = opts.metadata.get("parent_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| state.get_config("default_parent_id").map(|s| s.to_string()))
        .or_else(|| config_ancestor_id);

    let mut final_title = String::new();
    let mut final_id = String::new();

    for (i, segment) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        
        if is_last {
            let (id, actual_title) = get_or_create_page_v2(
                &mut state,
                &base_url,
                &space_info.id,
                segment,
                current_parent_id.as_deref(),
            )?;
            final_id = id;
            final_title = actual_title;
        } else {
            let (id, _) = get_or_create_page_v2(
                &mut state,
                &base_url,
                &space_info.id,
                segment,
                current_parent_id.as_deref(),
            )?;
            current_parent_id = Some(id);
        }
    }

    let html_content = html_convert::markdown_to_html(&opts.content);
    let update_body = CreatePageBodyV2 {
        representation: "storage".to_string(),
        value: html_content,
    };

    let get_url = format!("{base_url}/api/v2/pages/{final_id}");
    let get_resp = request_with_auth(&state, "GET", &get_url, None)?;
    let current_page: ConfluencePageV2 = serde_json::from_slice(&get_resp.body())?;
    let current_version = current_page.version.and_then(|v| v.number).unwrap_or(1);

    let req_body = UpdatePageRequestV2 {
        id: final_id.clone(),
        status: "current".to_string(),
        title: final_title.clone(),
        body: update_body,
        version: UpdatePageVersionV2 {
            number: current_version + 1,
            message: "Updated via doxus standardized flow".to_string(),
        },
    };

    let update_url = format!("{base_url}/api/v2/pages/{final_id}");
    let _resp = request_with_auth(&state, "PUT", &update_url, Some(serde_json::to_vec(&req_body)?))?;

    state.update_hierarchy_cache(&final_id, &final_title, current_parent_id.as_deref());
    state.save()?;

    Ok(CreateDocumentResultWasm { id: final_id })
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn update_document(Json(opts): Json<UpdateDocumentOptsWasm>) -> FnResult<()> {
    update_document_impl(opts)
}

pub(crate) fn update_document_impl(opts: UpdateDocumentOptsWasm) -> FnResult<()> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;

    let get_url = format!("{base_url}/api/v2/pages/{}", opts.id);
    let get_resp = request_with_auth(&state, "GET", &get_url, None)?;
    let current_page: ConfluencePageV2 = serde_json::from_slice(&get_resp.body())?;
    let current_version = current_page.version.and_then(|v| v.number).unwrap_or(1);

    let html_content = if let Some(content) = opts.content {
        html_convert::markdown_to_html(&content)
    } else {
        return Err(Error::msg("Updating without new content not yet supported").into());
    };

    let req_body = UpdatePageRequestV2 {
        id: opts.id.clone(),
        status: "current".to_string(),
        title: current_page.title,
        body: CreatePageBodyV2 {
            representation: "storage".to_string(),
            value: html_content,
        },
        version: UpdatePageVersionV2 {
            number: current_version + 1,
            message: "Updated via doxus".to_string(),
        },
    };

    let update_url = format!("{base_url}/api/v2/pages/{}", opts.id);
    let _resp = request_with_auth(&state, "PUT", &update_url, Some(serde_json::to_vec(&req_body)?))?;

    Ok(())
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn delete_document(Json(opts): Json<DeleteDocumentOptsWasm>) -> FnResult<()> {
    delete_document_impl(opts)
}

pub(crate) fn delete_document_impl(opts: DeleteDocumentOptsWasm) -> FnResult<()> {
    let mut state = PluginState::load()?;
    ensure_valid_token(&mut state)?;

    let base_url = get_base_url(&state)?;
    let delete_url = format!("{base_url}/api/v2/pages/{}", opts.id);
    
    let _resp = request_with_auth(&state, "DELETE", &delete_url, None)?;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn get_base_url(state: &PluginState) -> FnResult<String> {
    let raw = state.get_config("base_url").ok_or_else(|| Error::msg("base_url missing in plugin configuration"))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::msg("base_url is empty. Please check your project configuration.").into());
    }
    
    let mut url = trimmed.trim_end_matches('/').to_string();
    if url.contains(".atlassian.net") && !url.ends_with("/wiki") {
        url.push_str("/wiki");
    }
    Ok(url)
}

fn auth_header(state: &PluginState) -> FnResult<String> {
    if let Some(token) = &state.access_token {
        return Ok(format!("Bearer {token}"));
    }
    
    let api_token_opt = state.get_secret("confluence_api_token")
        .or_else(|| state.get_secret("api_token"));

    if let (Some(email), Some(api_token)) = (state.get_config("email"), api_token_opt) {
        let auth = format!("{}:{}", email.trim(), api_token.trim());
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth);
        return Ok(format!("Basic {encoded}"));
    }

    Err(Error::msg("No authentication credentials (OAuth or API Token)").into())
}

/// 429 응답 시 exponential backoff으로 최대 3회 재시도합니다.
/// __doxus_get_time (초 단위)을 이용한 busy-wait 딜레이를 사용합니다.
fn request_with_auth(state: &PluginState, method: &str, url: &str, body: Option<Vec<u8>>) -> FnResult<HttpResponse> {
    const MAX_RETRIES: u32 = 3;
    const BACKOFF_SECS: [i64; 3] = [2, 4, 8];

    let auth = auth_header(state)?;

    for attempt in 0..=MAX_RETRIES {
        let mut req = HttpRequest::new(url);
        req.method = Some(method.to_string());
        req.headers.insert("Authorization".to_string(), auth.clone());
        req.headers.insert("Accept".to_string(), "application/json".to_string());
        if body.is_some() {
            req.headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        let resp = http::request(&req, body.clone()).map_err(|e| {
            log_d!("confluence", "[Confluence-Debug] HTTP REQUEST FAILED: {} -> {}", url, e);
            e
        })?;

        if resp.status_code() == 429 {
            if attempt < MAX_RETRIES {
                let wait = BACKOFF_SECS[attempt as usize];
                log_d!("confluence", "[Confluence-RateLimit] 429 received, waiting {}s before retry {}/{}", wait, attempt + 1, MAX_RETRIES);
                busy_wait_secs(wait);
                continue;
            }
            return Err(Error::msg(format!("HTTP 429: rate limited after {} retries", MAX_RETRIES)).into());
        }

        if resp.status_code() >= 400 {
            let msg = format!("HTTP {}: {}", resp.status_code(), String::from_utf8_lossy(&resp.body()));
            return Err(Error::msg(msg).into());
        }

        return Ok(resp);
    }

    unreachable!()
}

/// __doxus_get_time(초 단위)을 이용한 busy-wait 딜레이
fn busy_wait_secs(secs: i64) {
    let start = unsafe { __doxus_get_time().unwrap_or(0) };
    loop {
        let now = unsafe { __doxus_get_time().unwrap_or(0) };
        if now >= start + secs {
            break;
        }
    }
}

fn ensure_valid_token(state: &mut PluginState) -> FnResult<()> {
    let now: i64 = unsafe { __doxus_get_time().unwrap_or(0) }; 
    
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
    
    state.access_token = Some(token_resp.access_token.clone());
    if let Some(rt) = token_resp.refresh_token {
        state.refresh_token = Some(rt);
    }
    state.expires_at = Some(token_resp.expires_in);
    state.save()?;

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
    space_key: &str,
    base_url: &str,
    stop_id: Option<&str>,
    tags: Vec<String>,
) -> RawDocumentWasm {
    let mut storage_content = p.body.as_ref()
        .and_then(|b| b.atlas_doc_format.as_ref().or(b.storage.as_ref()))
        .map(|s| s.value.as_str())
        .unwrap_or_default()
        .to_string();
    
    // [Doxus Fix] 본문 누락에 의한 builder error 방지 (Resilience)
    if storage_content.trim().is_empty() {
        storage_content = format!("<p><em>본문 내용을 읽을 수 없거나 비어있는 문서입니다. (ID: {})</em></p>", p.id);
    }
    
    log_d!("confluence:doc", "[Confluence-Doc-Debug] Page ID: {}, Storage Content Length: {}", p.id, storage_content.len());
    
    let markdown = html_convert::confluence_html_to_markdown(&storage_content);
    log_d!("confluence:doc", "[Confluence-Doc-Debug] Generated Markdown Length: {}", markdown.len());

    let (mut ancestor_titles, root_name) = resolve_ancestors(state, base_url, &p.id, hierarchy, stop_id);
    
    // [Doxus Fix] 최상위 계층 중복 제거
    // 1. ancestor_titles의 첫 번째 요소가 스페이스 이름/키와 일치하면 제거
    if !ancestor_titles.is_empty() {
        let first = &ancestor_titles[0];
        if first == space_name || first == space_key || first == "Project" {
            ancestor_titles.remove(0);
        }
    }
    // 2. 만약 여전히 리스트가 있고, 첫 번째 요소가 root_name(stop_id 페이지 제목)과 일치하면 제거
    if !ancestor_titles.is_empty() && !root_name.is_empty() && ancestor_titles[0] == root_name {
        ancestor_titles.remove(0);
    }
    
    let final_root_name = if !root_name.is_empty() {
        "".to_string() 
    } else if let Some(_) = stop_id {
        if !ancestor_titles.is_empty() {
             ancestor_titles.remove(0)
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };

    let is_parent = hierarchy.values().any(|(_, parent_id)| parent_id.as_deref() == Some(&p.id));
    let relative_path = path_utils::build_relative_path(&final_root_name, &ancestor_titles, &p.title, is_parent);
    log_d!("confluence:doc", "[Confluence-Doc-Debug] Final Result - Title: {}, Path: {}", p.title, relative_path);

    // TIP: 인덱싱 최적화를 위해 updated_at을 제공하는 것이 좋습니다.
    // 업데이트 시간이 없을 경우 코어에서 매번 전체 인덱싱을 수행하게 됩니다.
    let updated_at = p.version.as_ref().and_then(|v| {
        v.created_at.as_ref()
            .or(v.created_at.as_ref()) // Fallback (API 버전에 따른 필드명 차이 대비)
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.timestamp())
    });

    let url = p.links.webui.map(|ui| format!("{}{}", base_url, ui));

    let mut metadata = HashMap::new();
    if let Some(sid) = &p.space_id {
        metadata.insert("space_id".to_string(), serde_json::json!(sid));
    }
    metadata.insert("space_name".to_string(), serde_json::json!(space_name));
    metadata.insert("space_key".to_string(), serde_json::json!(space_key));
    metadata.insert("plugin".to_string(), serde_json::json!("confluence"));

    RawDocumentWasm {
        id: p.id,
        title: Some(p.title),
        content: markdown,
        content_type: "markdown".into(),
        url,
        metadata,
        tags,
        links: vec![],
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
    let now: i64 = unsafe { __doxus_get_time().unwrap_or(0) };
    const CACHE_TTL_SECONDS: i64 = 3600;

    if let (Some(cache), Some(last_fetch)) = (&state.hierarchy_cache, state.last_hierarchy_fetch) {
        if now < last_fetch + CACHE_TTL_SECONDS {
            return Ok(cache.clone());
        }
    }

    let mut hierarchy = HashMap::new();
    let mut pages_url = format!("{base_url}/api/v2/spaces/{space_id}/pages?limit=250");
    let mut pages_count = 0;
    loop {
        pages_count += 1;
        if pages_count > 100 { break; }
        
        if let Ok(resp) = request_with_auth(state, "GET", &pages_url, None) {
            if let Ok(list) = serde_json::from_slice::<ConfluenceV2ListResponse<ConfluencePageV2>>(&resp.body()) {
                let current_batch_size = list.results.len();
                for p in list.results {
                    hierarchy.insert(p.id, (p.title, p.parent_id));
                }
                
                if let Some(next) = list.links.next {
                    pages_url = if next.starts_with("http") { next } else { format!("{}{}", base_url, next) };
                } else if current_batch_size >= 250 {
                   pages_url = format!("{base_url}/api/v2/spaces/{space_id}/pages?limit=250&offset={}", pages_count * 250);
                } else { break; }
            } else { break; }
        } else { break; }
    }

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
    let url = format!("{base_url}/api/v2/pages/{id}");
    if let Ok(resp) = request_with_auth(state, "GET", &url, None) {
        if let Ok(p) = serde_json::from_slice::<ConfluencePageV2>(&resp.body()) {
            return Some((p.title, p.parent_id));
        }
    }
    let url = format!("{base_url}/api/v2/folders/{id}");
    if let Ok(resp) = request_with_auth(state, "GET", &url, None) {
        if let Ok(f) = serde_json::from_slice::<ConfluenceFolderV2>(&resp.body()) {
            return Some((f.title, f.parent_id));
        }
    }
    None
}

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

    log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] Resolving ancestors for: {}, stop_id: {:?}", current_id, stop_id);

    let mut depth = 0;
    loop {
        depth += 1;
        if depth > 50 { 
            log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] - Depth limit (>50) reached. Breaking loop.");
            break; 
        }

        let (title, parent_id) = if let Some(info) = hierarchy.get(&cursor) {
            info.clone()
        } else {
            if let Some(info) = fetch_page_info(state, base_url, &cursor) {
                hierarchy.insert(cursor.clone(), info.clone());
                info
            } else {
                log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] - Could not fetch info for cursor: {}. Breaking loop.", cursor);
                break;
            }
        };

        if let Some(sid) = stop_id {
            if cursor.trim() == sid.trim() {
                log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] - Found stop_id match: {} ({})", title, cursor);
                root_title = title.clone();
                break;
            }
        }

        if let Some(pid) = parent_id {
            if pid.is_empty() || pid == cursor { // Cycle/Root check
                if stop_id.is_none() {
                    root_title = title.clone();
                    // Do not add root-level title to ancestors to avoid redundant top folder
                }
                break;
            }
            if pid == cursor { break; }
            
            // Not root yet, add to ancestors
            log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] - Adding to chain: {} ({})", title, cursor);
            ancestors.push(title.clone());
            
            cursor = pid;
        } else {
            if stop_id.is_none() {
                root_title = title.clone();
                // Do not add root-level title to ancestors to avoid redundant top folder
            }
            break;
        }
    }

    if !ancestors.is_empty() {
        // Current page's own title is always added first, remove it
        ancestors.remove(0);
    }
    ancestors.reverse();
    
    log_d!("confluence:ancestors", "[Confluence-Ancestors-Debug] - Final ancestors list: {:?}", ancestors);
    (ancestors, root_title)
}

fn get_or_create_page_v2(
    state: &mut PluginState,
    base_url: &str,
    space_id: &str,
    title: &str,
    parent_id: Option<&str>,
) -> FnResult<(String, String)> {
    let mut attempts = 0;
    let mut current_title = title.to_string();

    loop {
        attempts += 1;
        if attempts > 10 {
            return Err(Error::msg(format!("Failed to find a unique title for '{}' after 10 attempts", title)).into());
        }

        let search_url = format!("{base_url}/api/v2/pages?spaceId={space_id}&title={}", urlencoding::encode(&current_title));
        let resp = request_with_auth(state, "GET", &search_url, None)?;
        let list: ConfluenceV2ListResponse<ConfluencePageV2> = serde_json::from_slice(&resp.body())?;
        
        if !list.results.is_empty() {
            for p in &list.results {
                if p.parent_id.as_deref() == parent_id {
                    state.update_hierarchy_cache(&p.id, &p.title, p.parent_id.as_deref());
                    return Ok((p.id.clone(), current_title));
                }
            }
            current_title = format!("{} ({})", title, attempts);
            continue;
        }

        let req_body = CreatePageRequestV2 {
            space_id: space_id.to_string(),
            title: current_title.clone(),
            status: "current".to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            body: CreatePageBodyV2 {
                representation: "storage".to_string(),
                value: "<p />".to_string(), 
            },
        };
        
        let create_url = format!("{base_url}/api/v2/pages");
        let resp = request_with_auth(state, "POST", &create_url, Some(serde_json::to_vec(&req_body)?))?;
        let result: CreatePageResultV2 = serde_json::from_slice(&resp.body())?;
        return Ok((result.id, current_title));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct ConfluencePlugin {
    pub base_url: String,
    pub space_key: String,
    pub token: String,
    pub email: String,
    pub ancestor_id: Option<String>,
    pub oauth_token: Option<doxus_core::auth::OAuthToken>,
    pub oauth_config: Option<doxus_core::auth::OAuthConfig>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ConfluencePlugin {
    pub fn new() -> Self {
        Self {
            base_url: String::new(),
            space_key: String::new(),
            token: "test-token".to_string(),
            email: String::new(),
            ancestor_id: None,
            oauth_token: None,
            oauth_config: None,
        }
    }

    pub fn set_test_config(&mut self, base_url: String, space_key: String, token: String) {
        self.base_url = base_url;
        self.space_key = space_key;
        self.token = token;
    }

    pub fn set_test_ancestor_config(&mut self, base_url: String, ancestor_id: String, token: String) {
        self.base_url = base_url;
        self.ancestor_id = Some(ancestor_id);
        self.token = token;
        self.space_key = "TEAM".to_string();
    }

    pub fn set_test_oauth_config(&mut self, config: doxus_core::auth::OAuthConfig, token: Option<doxus_core::auth::OAuthToken>) {
        self.oauth_config = Some(config);
        self.oauth_token = token;
    }

    fn setup_state(&self) -> Result<(), PluginError> {
        let mut config = HashMap::new();
        config.insert("base_url".to_string(), serde_json::json!(self.base_url));
        config.insert("space_key".to_string(), serde_json::json!(self.space_key));
        config.insert("email".to_string(), serde_json::json!(self.email));
        if let Some(aid) = &self.ancestor_id {
            config.insert("ancestor_id".to_string(), serde_json::json!(aid));
        }

        let mut secrets = HashMap::new();
        if let Some(oauth) = &self.oauth_token {
            secrets.insert("access_token".to_string(), oauth.access_token.clone());
        } else {
            // Standard API Token
            secrets.insert("api_token".to_string(), self.token.clone());
        }

        if let Some(oauth_cfg) = &self.oauth_config {
            config.insert("oauth_config".to_string(), serde_json::to_value(oauth_cfg).unwrap());
        }

        let state = PluginState {
            config,
            secrets,
            access_token: self.oauth_token.as_ref().map(|t| t.access_token.clone()),
            refresh_token: self.oauth_token.as_ref().and_then(|t| t.refresh_token.clone()),
            expires_at: self.oauth_token.as_ref().and_then(|t| t.expires_at.map(|e| e as i64)),
            hierarchy_cache: None,
            last_hierarchy_fetch: None,
            debug_tags: std::collections::HashSet::new(),
        };
        
        let bytes = serde_json::to_vec(&state).map_err(|e| PluginError::Internal(e.to_string()))?;
        var::set(STATE_VAR, bytes).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl DocSource for ConfluencePlugin {
    fn metadata(&self) -> &PluginMetadata {
        use std::sync::OnceLock;
        static META: OnceLock<PluginMetadata> = OnceLock::new();
        META.get_or_init(|| PluginMetadata {
            id: "confluence".to_string(),
            name: "Confluence".to_string(),
            version: "0.1.0".to_string(),
            kind: PluginKind::External,
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: true,
            oauth: true,
            native_search: false,
            sync_policy: doxus_plugin_sdk::SyncPolicy::Interval { seconds: 7200 },
        }
    }

    async fn validate_config(&self, _config: &PluginConfig) -> Result<(), PluginError> {
        Ok(())
    }

    fn guide(&self) -> Option<&'static str> {
        Some(include_str!("../GUIDE.md"))
    }

    async fn initialize(&mut self, config: PluginConfig, secrets: PluginSecrets) -> Result<(), PluginError> {
        self.base_url = config.fields.get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        self.space_key = config.fields.get("space_key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        self.ancestor_id = config.fields.get("ancestor_id")
            .and_then(|v| {
                match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                }
            })
            .filter(|s| !s.is_empty());
        self.email = config.fields.get("email")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        
        // Secrets mapping
        if let Some(sv) = secrets.fields.get("confluence_api_token").or_else(|| secrets.fields.get("api_token")) {
            match sv {
                doxus_plugin_sdk::SecretValue::Text(t) => self.token = t.clone(),
                doxus_plugin_sdk::SecretValue::Token { value, .. } => self.token = value.clone(),
            }
        }
        
        // OAuth handling
        if let Some(oauth_cfg_val) = config.fields.get("oauth_config") {
            self.oauth_config = serde_json::from_value(oauth_cfg_val.clone()).ok();
        }

        // OAuth token handles
        if let Some(sv) = secrets.fields.values().next() {
            if let doxus_plugin_sdk::SecretValue::Token { value, expires_at, refresh_token } = sv {
                self.oauth_token = Some(doxus_core::auth::OAuthToken {
                    access_token: value.clone(),
                    refresh_token: refresh_token.clone(),
                    expires_at: expires_at.map(|e| e as u64),
                });
            }
        }
        
        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            this.setup_state()?;
            let res = fetch_all_impl(FetchAllOptsWasm { 
                cursor: opts.cursor.clone(), 
                page_size: opts.page_size 
            }).map_err(|e| PluginError::Internal(e.to_string()))?;
            
            Ok(DocumentStream {
                documents: res.documents.into_iter().map(wasm_to_native_doc).collect(),
                next_cursor: res.next_cursor,
                estimated_total: res.estimated_total,
            })
        }).await.map_err(|e| PluginError::Internal(e.to_string()))?
    }

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            this.setup_state()?;
            let res = fetch_changes_impl(FetchChangesOptsWasm {
                since: opts.since,
                cursor: opts.cursor.clone(),
                page_size: opts.page_size,
            }).map_err(|e| PluginError::Internal(e.to_string()))?;

            Ok(ChangeSet {
                updated: res.updated.into_iter().map(wasm_to_native_doc).collect(),
                deleted_ids: res.deleted.into_iter().map(SourceDocId).collect(),
                next_cursor: res.next_cursor,
            })
        }).await.map_err(|e| PluginError::Internal(e.to_string()))?
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        let this = self.clone();
        let doc_id = id.0.clone();
        tokio::task::spawn_blocking(move || {
            this.setup_state()?;
            let res = fetch_document_impl(FetchDocumentOptsWasm { id: doc_id })
                .map_err(|e| PluginError::Internal(e.to_string()))?;
            Ok(wasm_to_native_doc(res))
        }).await.map_err(|e| PluginError::Internal(e.to_string()))?
    }

    async fn health_check(&self) -> HealthStatus {
        if let Err(e) = self.setup_state() {
            return HealthStatus { healthy: false, message: Some(format!("Failed to setup test state: {}", e)) };
        }
        let res = health_check_impl();
        match res {
            Ok(msg) => HealthStatus { healthy: true, message: Some(msg) },
            Err(e) => HealthStatus { healthy: false, message: Some(e.to_string()) },
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
        self.setup_state()?;
        let res = create_document_impl(CreateDocumentOptsWasm {
            title: title.to_string(),
            content: content.to_string(),
            folder: folder.map(String::from),
            metadata: metadata.cloned().unwrap_or_default(),
        }).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(SourceDocId(res.id))
    }

    async fn update_document(
        &self,
        _id: &SourceDocId,
        _content: Option<&str>,
        _metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(), PluginError> {
        self.setup_state()?;
        update_document_impl(UpdateDocumentOptsWasm {
            id: _id.0.clone(),
            content: _content.map(String::from),
            metadata: _metadata.cloned(),
        }).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(())
    }

    async fn delete_document(&self, _id: &SourceDocId) -> Result<(), PluginError> {
        self.setup_state()?;
        delete_document_impl(DeleteDocumentOptsWasm {
            id: _id.0.clone(),
        }).map_err(|e| PluginError::Internal(e.to_string()))?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wasm_to_native_doc(w: RawDocumentWasm) -> RawDocument {
    RawDocument {
        id: SourceDocId(w.id),
        title: w.title,
        content: w.content,
        content_type: match w.content_type.as_str() {
            "html" => NativeContentType::Html,
            "plain_text" => NativeContentType::PlainText,
            _ => NativeContentType::Markdown,
        },
        url: w.url,
        metadata: w.metadata,
        tags: w.tags,
        aliases: vec![],
        links: vec![],
        created_at: w.created_at,
        updated_at: w.updated_at,
        relative_path: w.relative_path,
    }
}
