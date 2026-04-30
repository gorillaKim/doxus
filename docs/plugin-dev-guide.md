# Doxus 플러그인 개발 가이드

doxus 플러그인은 외부 데이터 소스(Confluence, GitHub, Notion 등)로부터 문서를 가져와 통합 검색 인덱스에 넣는 역할을 한다. 이 가이드는 실제 구현된 플러그인(`confluence`, `github`, `obsidian`)의 코드를 기반으로 작성되었다.

---

## 1. 플러그인 종류 선택

| 종류 | 사용 조건 | 예시 |
|------|----------|------|
| **In-process (Rust)** | 1st-party 빌트인, 신뢰 코드, 성능 중요 | obsidian, github |
| **WASM (Extism)** | 외부 소스, 3rd-party, 격리 필요 | confluence |

- In-process 플러그인은 doxus-core 바이너리에 직접 컴파일됨
- WASM 플러그인은 `.wasm` + `manifest.toml`로 배포되며 샌드박스에서 실행됨
- `dylib`, `IPC Sidecar` 방식은 사용하지 않음

---

## 2. In-Process 플러그인 개발

### 2-1. 크레이트 생성

```
crates/plugins/my-source/
├── Cargo.toml
└── src/
    └── lib.rs
```

```toml
# Cargo.toml
[package]
name = "doxus-plugin-my-source"

[dependencies]
doxus-plugin-sdk = { path = "../../plugin-sdk" }
async-trait = { workspace = true }
reqwest = { workspace = true, features = ["rustls-tls", "json"] }
thiserror = { workspace = true }
serde = { workspace = true }
```

### 2-2. DocSource trait 구현

```rust
use async_trait::async_trait;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, ContentType, DocSource, DocumentStream, FetchAllOpts,
    FetchChangesOpts, HealthStatus, PluginConfig, PluginError, PluginMetadata, RawDocument,
    SecretValue, SourceDocId, SyncPolicy, PluginKind, PluginSecrets,
};
use std::collections::HashMap;

pub struct MySourcePlugin {
    meta: PluginMetadata,
    config: Option<MyConfig>,
    client: reqwest::Client,
}

struct MyConfig {
    base_url: String,
    token: Option<String>,
}

impl MySourcePlugin {
    pub fn new() -> Self {
        Self {
            meta: PluginMetadata {
                id: "com.example.my-source".to_string(),
                name: "My Source".to_string(),  // 필드명은 name (display_name 아님)
                version: "0.1.0".to_string(),
                kind: PluginKind::External,
            },
            config: None,
            client: reqwest::Client::new(),
        }
    }

    fn cfg(&self) -> Result<&MyConfig, PluginError> {
        self.config.as_ref().ok_or_else(|| PluginError::Internal("not initialized".into()))
    }
}

#[async_trait]
impl DocSource for MySourcePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: false,
            oauth: false,
            native_search: false,
            sync_policy: SyncPolicy::Interval { seconds: 3600 },
        }
    }

    fn guide(&self) -> Option<&'static str> {
        Some("base_url과 token을 설정하세요.")
    }

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError> {
        let url = config.fields.get("base_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PluginError::ConfigInvalid("missing 'base_url'".into()))?;

        // SSRF 방지: 반드시 호출할 것
        doxus_plugin_sdk::validate_base_url(url)?;
        Ok(())
    }

    async fn initialize(
        &mut self,
        config: PluginConfig,
        secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        let base_url = config.fields.get("base_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PluginError::ConfigInvalid("missing 'base_url'".into()))?
            .to_string();

        doxus_plugin_sdk::validate_base_url(&base_url)?;

        let token = secrets.fields.get("api_token")
            .and_then(|v| match v {
                SecretValue::Text(s) => Some(s.clone()),
                SecretValue::Token { value, .. } => Some(value.clone()),
            });

        self.config = Some(MyConfig { base_url, token });
        Ok(())
    }

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let cfg = self.cfg()?;
        let page: u64 = opts.cursor.as_ref()
            .and_then(|c| c.parse().ok())
            .unwrap_or(1);

        let url = format!("{}/api/documents?page={}&limit={}", cfg.base_url, page, opts.page_size);
        let mut req = self.client.get(&url);
        if let Some(tok) = &cfg.token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }

        let resp = req.send().await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        if resp.status() == 401 { return Err(PluginError::AuthExpired); }
        if !resp.status().is_success() {
            return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
        }

        let data: serde_json::Value = resp.json().await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        let items = data["items"].as_array().cloned().unwrap_or_default();
        let has_more = data["has_more"].as_bool().unwrap_or(false);

        let documents: Vec<RawDocument> = items.into_iter().map(|item| {
            // updated_at이 없으면 created_at으로 fallback — 인덱싱 파이프라인 필수 패턴
            let updated_at = item["updated_at"].as_i64()
                .or_else(|| item["created_at"].as_i64());

            RawDocument {
                id: SourceDocId(item["id"].as_str().unwrap_or_default().to_string()),
                title: item["title"].as_str().map(|s| s.to_string()),
                content: item["content"].as_str().unwrap_or_default().to_string(),
                content_type: ContentType::Markdown,
                url: item["url"].as_str().map(|s| s.to_string()),
                metadata: HashMap::new(),
                tags: vec![],
                aliases: vec![],
                links: vec![],
                created_at: item["created_at"].as_i64(),
                updated_at,
                relative_path: item["path"].as_str().map(|s| s.to_string()),
            }
        }).collect();

        Ok(DocumentStream {
            documents,
            next_cursor: if has_more { Some((page + 1).to_string()) } else { None },
            estimated_total: data["total"].as_u64(),
        })
    }

    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        // 증분 동기화를 지원하지 않으면 빈 ChangeSet 반환
        Ok(ChangeSet {
            updated: vec![],
            deleted_ids: vec![],
            next_cursor: None,
        })
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
        let cfg = self.cfg()?;
        let url = format!("{}/api/documents/{}", cfg.base_url, id.0);

        let mut req = self.client.get(&url);
        if let Some(tok) = &cfg.token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }

        let resp = req.send().await
            .map_err(|e| PluginError::NetworkError(e.to_string()))?;

        if resp.status() == 404 { return Err(PluginError::NotFound(id.0.clone())); }
        if !resp.status().is_success() {
            return Err(PluginError::NetworkError(format!("HTTP {}", resp.status())));
        }

        let item: serde_json::Value = resp.json().await
            .map_err(|e| PluginError::Internal(e.to_string()))?;

        Ok(RawDocument {
            id: id.clone(),
            title: item["title"].as_str().map(|s| s.to_string()),
            content: item["content"].as_str().unwrap_or_default().to_string(),
            content_type: ContentType::Markdown,
            url: item["url"].as_str().map(|s| s.to_string()),
            metadata: HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: item["created_at"].as_i64(),
            updated_at: item["updated_at"].as_i64().or_else(|| item["created_at"].as_i64()),
            relative_path: item["path"].as_str().map(|s| s.to_string()),
        })
    }

    async fn health_check(&self) -> HealthStatus {
        let cfg = match self.cfg() {
            Ok(c) => c,
            Err(_) => return HealthStatus { healthy: false, message: Some("not initialized".into()) },
        };

        let mut req = self.client.get(&format!("{}/api/health", cfg.base_url));
        if let Some(tok) = &cfg.token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => HealthStatus { healthy: true, message: None },
            Ok(resp) => HealthStatus { healthy: false, message: Some(format!("HTTP {}", resp.status())) },
            Err(e) => HealthStatus { healthy: false, message: Some(e.to_string()) },
        }
    }
}
```

### 2-3. PluginManager에 등록

앱 초기화 코드(`apps/desktop/src-tauri/src/state.rs` 등)에서:

```rust
// register_factory의 실제 시그니처:
// pub fn register_factory<F>(&mut self, plugin_id: &str, factory: F)
// where F: Fn() -> Box<dyn DocSource + Send + Sync> + Send + Sync + 'static
plugin_manager.register_factory("com.example.my-source", || {
    Box::new(MySourcePlugin::new()) as Box<dyn DocSource + Send + Sync>
});
```

---

## 3. WASM 플러그인 개발

WASM 플러그인은 Extism PDK를 사용해 JSON 직렬화 경계를 통해 doxus-core와 통신한다.

### 3-1. 크레이트 생성

```
crates/plugins/my-wasm-plugin/
├── Cargo.toml
├── my-wasm-plugin.manifest.toml
└── src/
    └── lib.rs
```

```toml
# Cargo.toml
[package]
name = "doxus-plugin-my-wasm"

[lib]
crate-type = ["cdylib", "rlib"]  # cdylib = .wasm 출력, rlib = 네이티브 테스트용

[dependencies]
doxus-plugin-sdk = { path = "../../plugin-sdk", default-features = false }  # native feature 제거
extism-pdk = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 네이티브 빌드(테스트)에서만 사용 — WASM 환경에서는 extism-pdk의 http::request 사용
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
reqwest = { version = "0.12", features = ["blocking", "json"] }
tokio = { version = "1", features = ["rt", "macros"] }  # #[tokio::test] 용
serde_json = "1"
```

### 3-2. 매니페스트 파일

```toml
# my-wasm-plugin.manifest.toml
plugin_id = "com.example.my-wasm"
display_name = "My WASM Plugin"
version = "0.1.0"
abi_version = 1

# 이 목록에 없는 도메인으로의 HTTP 요청은 런타임에 차단됨
http_domains = ["api.example.com", "*.example.org"]

# 허용된 KV 네임스페이스 (plugin_id + namespace + key로 격리됨)
kv_namespaces = ["settings", "cache"]

# 허용된 시크릿 키 (__doxus_get_secret으로 조회 가능한 항목만 명시)
secrets = ["api_token", "refresh_token", "expires_at"]
```

### 3-3. 플러그인 상태 관리

WASM 함수 간에는 메모리가 공유되지 않으므로, `extism_pdk::var`를 통해 상태를 직렬화해서 저장한다.

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const STATE_VAR: &str = "plugin_state";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct PluginState {
    config: HashMap<String, serde_json::Value>,
    secrets: HashMap<String, String>,
    // 캐시가 필요한 데이터 (예: 계층 구조, 토큰 만료 시간 등)
    token_expires_at: Option<i64>,
}

impl PluginState {
    fn load() -> FnResult<Self> {
        let bytes: Vec<u8> = var::get(STATE_VAR)?
            .ok_or_else(|| Error::msg("state not initialized — call initialize first"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save(&self) -> FnResult<()> {
        var::set(STATE_VAR, serde_json::to_vec(self)?)?;
        Ok(())
    }

    fn get_config_str(&self, key: &str) -> Option<&str> {
        self.config.get(key)?.as_str()
    }

    fn get_secret(&self, key: &str) -> Option<String> {
        // 1. 초기화 시 넘겨받은 secrets에서 조회
        if let Some(s) = self.secrets.get(key) {
            if !s.is_empty() { return Some(s.clone()); }
        }
        // 2. 이후 갱신된 경우 host function으로 조회
        let val = unsafe { __doxus_get_secret(key.to_string()).ok().unwrap_or_default() };
        if !val.is_empty() { Some(val) } else { None }
    }
}
```

### 3-4. Host Functions 선언

ABI v1에서 WASM 플러그인이 호출 가능한 host function은 아래 3종뿐이다. `.claude/rules/plugin-system.md`에 나열된 `kv_get/kv_set`, `progress`, `content_transform`은 현재 host에 미등록 상태이므로 호출 불가.

| Host Function | 역할 |
|---|---|
| `__doxus_set_secret(key, value)` | 키체인에 시크릿 영구 저장 (예: 갱신된 OAuth 토큰) |
| `__doxus_get_secret(key) → String` | 저장된 시크릿 조회 |
| `__doxus_get_time() → i64` | 현재 Unix timestamp (WASM 환경에는 시스템 시간 API 없음) |

HTTP 요청은 Extism PDK의 `http::request()`를 사용한다 — `http_domains` 허용목록에 따라 샌드박스에서 제어됨.

```rust
// WASM 환경에서만 host function 사용 가능
#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    // __doxus_set_secret은 반환값이 없는 fire-and-forget
    // extism_pdk #[host_fn] 매크로가 자동으로 Result<(), Error>로 감싸줌
    fn __doxus_set_secret(key: String, value: String);
    fn __doxus_get_secret(key: String) -> String;
    fn __doxus_get_time() -> i64;
}

// 네이티브 빌드(테스트)용 stub — WASM 시그니처와 동일하게 유지
#[cfg(not(target_arch = "wasm32"))]
unsafe fn __doxus_get_secret(_key: String) -> Result<String, Error> {
    Ok(String::new())
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn __doxus_set_secret(_key: String, _value: String) -> Result<(), Error> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn __doxus_get_time() -> Result<i64, Error> {
    use std::time::{SystemTime, UNIX_EPOCH};
    Ok(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64)
}
```

### 3-5. 엔트리포인트 구현

`#[plugin_fn]`은 WASM 빌드 시에만 필요하므로 `cfg_attr`로 조건부 적용한다.

```rust
use doxus_plugin_sdk::wasm_types::*;

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn initialize(Json(opts): Json<InitOpts>) -> FnResult<()> {
    let state = PluginState {
        config: opts.config,
        secrets: opts.secrets,
        token_expires_at: None,
    };
    state.save()
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_all(Json(opts): Json<FetchAllOptsWasm>) -> FnResult<Json<DocumentStreamWasm>> {
    fetch_all_impl(opts).map(Json)
}

// 실제 로직은 pub(crate) fn으로 분리 — 네이티브 테스트에서 직접 호출 가능
pub(crate) fn fetch_all_impl(opts: FetchAllOptsWasm) -> FnResult<DocumentStreamWasm> {
    let state = PluginState::load()?;
    let base_url = state.get_config_str("base_url")
        .ok_or_else(|| Error::msg("missing base_url"))?;
    let token = state.get_secret("api_token")
        .ok_or_else(|| Error::msg("missing api_token"))?;

    let start: i64 = opts.cursor.as_ref()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);

    let url = format!("{}/api/documents?start={}&limit={}", base_url, start, opts.page_size);
    let resp = http_get_with_token(&url, &token)?;

    let data: serde_json::Value = serde_json::from_slice(&resp.body())?;
    let items = data["items"].as_array().cloned().unwrap_or_default();
    let has_more = data["has_more"].as_bool().unwrap_or(false);

    let documents: Vec<RawDocumentWasm> = items.into_iter().map(|item| {
        RawDocumentWasm {
            id: item["id"].as_str().unwrap_or_default().to_string(),
            title: item["title"].as_str().map(|s| s.to_string()),
            content: item["content"].as_str().unwrap_or_default().to_string(),
            content_type: "markdown".to_string(),
            url: item["url"].as_str().map(|s| s.to_string()),
            metadata: HashMap::new(),
            tags: vec![],
            // updated_at 없으면 created_at fallback
            created_at: item["created_at"].as_i64(),
            updated_at: item["updated_at"].as_i64()
                .or_else(|| item["created_at"].as_i64()),
            relative_path: item["path"].as_str().map(|s| s.to_string()),
            links: vec![],
        }
    }).collect();

    // 빈 페이지 방어 — cursor만으로 루프 탈출 판단하지 않음
    let next_cursor = if has_more && !documents.is_empty() {
        Some((start + opts.page_size as i64).to_string())
    } else {
        None
    };

    Ok(DocumentStreamWasm {
        documents,
        next_cursor,
        estimated_total: data["total"].as_u64(),
    })
}

// fetch_changes: capabilities().incremental_sync = false이면 구현하지 않아도 됨
// 단, 빈 ChangeSet 반환 대신 에러를 반환해야 core가 전체 재인덱싱으로 fallback할 수 있다
// capabilities().incremental_sync = true인데 이 함수가 빈 결과를 반환하면 변경 사항 누락 위험
#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_changes(_input: Vec<u8>) -> FnResult<Vec<u8>> {
    Err(Error::msg("incremental sync not supported"))
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn fetch_document(Json(opts): Json<FetchDocumentOptsWasm>) -> FnResult<Json<RawDocumentWasm>> {
    let state = PluginState::load()?;
    let base_url = state.get_config_str("base_url").ok_or_else(|| Error::msg("missing base_url"))?;
    let token = state.get_secret("api_token").ok_or_else(|| Error::msg("missing api_token"))?;

    let url = format!("{}/api/documents/{}", base_url, opts.id);
    let resp = http_get_with_token(&url, &token)?;
    let item: serde_json::Value = serde_json::from_slice(&resp.body())?;

    Ok(Json(RawDocumentWasm {
        id: opts.id,
        title: item["title"].as_str().map(|s| s.to_string()),
        content: item["content"].as_str().unwrap_or_default().to_string(),
        content_type: "markdown".to_string(),
        url: item["url"].as_str().map(|s| s.to_string()),
        metadata: HashMap::new(),
        tags: vec![],
        created_at: item["created_at"].as_i64(),
        updated_at: item["updated_at"].as_i64().or_else(|| item["created_at"].as_i64()),
        relative_path: item["path"].as_str().map(|s| s.to_string()),
        links: vec![],
    }))
}

#[cfg_attr(target_arch = "wasm32", plugin_fn)]
pub fn health_check() -> FnResult<String> {
    let state = PluginState::load()?;
    let base_url = state.get_config_str("base_url").ok_or_else(|| Error::msg("missing base_url"))?;
    let token = state.get_secret("api_token").ok_or_else(|| Error::msg("missing api_token"))?;

    let resp = http_get_with_token(&format!("{}/api/health", base_url), &token)?;
    if resp.status_code() == 200 {
        Ok("OK".into())
    } else {
        Err(Error::msg(format!("HTTP {}", resp.status_code())))
    }
}
```

### 3-6. HTTP 헬퍼

WASM 환경에서는 `extism_pdk::http::request`를 사용하고, 네이티브 테스트에서는 `reqwest::blocking`을 사용한다.

```rust
#[cfg(target_arch = "wasm32")]
fn http_get_with_token(url: &str, token: &str) -> FnResult<HttpResponse> {
    let mut req = HttpRequest::new(url);
    req.method = Some("GET".to_string());
    req.headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    http::request(&req, None::<Vec<u8>>)
}

#[cfg(not(target_arch = "wasm32"))]
fn http_get_with_token(url: &str, token: &str) -> FnResult<NativeHttpResponse> {
    let client = reqwest::blocking::Client::new();
    let resp = client.get(url)
        .header("Authorization", format!("Bearer {}", token))
        .send()?;
    let status = resp.status().as_u16();
    let body = resp.bytes()?.to_vec();
    Ok(NativeHttpResponse { status, body })
}

// 네이티브 테스트용 응답 타입 stub
#[cfg(not(target_arch = "wasm32"))]
struct NativeHttpResponse { status: u16, body: Vec<u8> }
#[cfg(not(target_arch = "wasm32"))]
impl NativeHttpResponse {
    fn status_code(&self) -> u16 { self.status }
    fn body(&self) -> &[u8] { &self.body }
}
```

### 3-7. 토큰 갱신 패턴 (OAuth)

```rust
fn ensure_valid_token(state: &mut PluginState) -> FnResult<()> {
    let now = unsafe { __doxus_get_time()? };
    let expires_at = state.token_expires_at.unwrap_or(0);

    // 만료 5분 전에 갱신
    if expires_at > now + 300 {
        return Ok(());
    }

    let refresh_token = state.get_secret("refresh_token")
        .ok_or_else(|| Error::msg("no refresh_token"))?;
    let base_url = state.get_config_str("base_url").ok_or_else(|| Error::msg("missing base_url"))?;

    let body = serde_json::json!({ "grant_type": "refresh_token", "refresh_token": refresh_token });
    let mut req = HttpRequest::new(&format!("{}/oauth/token", base_url));
    req.method = Some("POST".to_string());
    req.headers.insert("Content-Type".to_string(), "application/json".to_string());
    let resp = http::request(&req, Some(serde_json::to_vec(&body)?))?;

    let data: serde_json::Value = serde_json::from_slice(&resp.body())?;
    let new_token = data["access_token"].as_str().ok_or_else(|| Error::msg("no access_token in response"))?;
    let new_expires_in = data["expires_in"].as_i64().unwrap_or(3600);

    // host function으로 영구 저장
    unsafe { __doxus_set_secret("access_token".to_string(), new_token.to_string())? };

    // now + expires_in = 절대 만료 시각
    // ⚠️ 주의: Confluence 플러그인 reference 코드는 `state.expires_at = Some(token_resp.expires_in)`으로
    //   상대 초(초 단위 유효기간)를 절대 시각으로 착각해 저장하는 버그가 있다.
    //   항상 `now + expires_in` 형태로 저장할 것.
    state.token_expires_at = Some(now + new_expires_in);
    state.save()?;
    Ok(())
}
```

### 3-8. OAuth 플로우 지원 (In-process 전용)

OAuth가 필요한 플러그인은 `capabilities().oauth = true`로 선언하고 두 메서드를 구현한다.

```rust
// oauth_start: 사용자를 리디렉션할 인증 URL을 반환
async fn oauth_start(&self) -> Option<String> {
    let client_id = "...";
    let redirect_uri = "doxus://oauth/callback";
    Some(format!(
        "https://auth.example.com/oauth/authorize?client_id={}&redirect_uri={}&response_type=code",
        client_id, redirect_uri
    ))
}

// oauth_exchange: 인증 코드와 state를 받아 액세스 토큰 교환 후 저장
async fn oauth_exchange(&mut self, code: &str, state: &str) -> Result<(), PluginError> {
    let resp = self.client
        .post("https://auth.example.com/oauth/token")
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": "doxus://oauth/callback",
        }))
        .send().await
        .map_err(|e| PluginError::NetworkError(e.to_string()))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| PluginError::Internal(e.to_string()))?;

    let token = data["access_token"].as_str()
        .ok_or_else(|| PluginError::Internal("no access_token".into()))?;
    let expires_in = data["expires_in"].as_i64().unwrap_or(3600);

    // SecretValue::Token으로 만료 시각 포함 저장
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    // config에 저장하거나 self 필드에 유지
    // (WASM 플러그인은 __doxus_set_secret으로 저장)
    Ok(())
}
```

> WASM 플러그인에서 OAuth는 현재 지원되지 않는다. `oauth_start`/`oauth_exchange`는 In-process trait 전용이다.

---

## 4. 핵심 타입 레퍼런스

### RawDocument (In-process)

| 필드 | 타입 | 설명 |
|------|------|------|
| `id` | `SourceDocId(String)` | 플러그인이 발급한 원본 ID (opaque) |
| `title` | `Option<String>` | 문서 제목 |
| `content` | `String` | 본문 (마크다운 권장) |
| `content_type` | `ContentType` | `Markdown` \| `PlainText` \| `Html` |
| `url` | `Option<String>` | 원본 URL |
| `metadata` | `HashMap<String, Value>` | 임의 메타데이터 |
| `tags` | `Vec<String>` | 태그 목록 |
| `aliases` | `Vec<String>` | 별칭 (검색 시 매칭됨) |
| `links` | `Vec<String>` | 참조 링크 (위키링크, URL 등) |
| `created_at` | `Option<i64>` | Unix timestamp |
| `updated_at` | `Option<i64>` | Unix timestamp — **없으면 created_at fallback 필수** |
| `relative_path` | `Option<String>` | 폴더 계층 경로 (검색 UI 표시용) |

### RawDocumentWasm (WASM)

`RawDocument`와 동일 필드이나 `content_type`이 문자열 (`"markdown"`, `"plain_text"`, `"html"`).

> **주의**: WASM 경계에서 `aliases` 필드는 지원되지 않는다. `RawDocumentWasm`에 없으므로 host가 자동으로 `vec![]`로 채운다. aliases가 필요하면 In-process 플러그인을 사용해야 한다.

### Capabilities / SyncPolicy

```rust
pub struct Capabilities {
    pub incremental_sync: bool,   // fetch_changes 지원 여부
    pub oauth: bool,              // oauth_start/oauth_exchange 지원 여부
    pub native_search: bool,      // 소스 자체 검색 API 사용 여부 (미구현)
    pub sync_policy: SyncPolicy,
}

pub enum SyncPolicy {
    Realtime(WatchOptions),          // 파일시스템 감시 (obsidian 전용)
    Interval { seconds: u64 },       // 주기적 동기화
    OnFocus,                         // 앱 포커스 시 동기화
    Manual,                          // 사용자 직접 트리거
}
```

`capabilities()`는 `DocSource` trait의 **필수 메서드**다 (default 구현 없음).

### FetchAllOpts / DocumentStream

```rust
pub struct FetchAllOpts {
    pub cursor: Option<String>,   // opaque — core가 파싱하지 않음
    pub page_size: usize,
}

pub struct DocumentStream {
    pub documents: Vec<RawDocument>,
    pub next_cursor: Option<String>,   // None = 마지막 페이지
    pub estimated_total: Option<u64>,
}
```

**cursor 설계 원칙**: core는 cursor 내용을 파싱하지 않는다. 어떤 형식도 가능하다. 복잡한 소스는 복합 cursor를 쓴다:

```rust
// GitHub 플러그인 패턴 — 여러 소스를 순서대로 순회할 때
enum FetchCursor {
    Issues(u64),       // "issues:1"
    Wiki(u64),         // "wiki:3"
    Discussions(u64),  // "discussions:2"
}

impl FetchCursor {
    fn to_string(&self) -> String {
        match self {
            Self::Issues(p) => format!("issues:{p}"),
            Self::Wiki(p) => format!("wiki:{p}"),
            Self::Discussions(p) => format!("discussions:{p}"),
        }
    }
    fn parse(s: &str) -> Option<Self> {
        let (kind, num) = s.split_once(':')?;
        let p: u64 = num.parse().ok()?;
        match kind {
            "issues" => Some(Self::Issues(p)),
            "wiki" => Some(Self::Wiki(p)),
            "discussions" => Some(Self::Discussions(p)),
            _ => None,
        }
    }
}
```

### PluginError

```rust
pub enum PluginError {
    ConfigInvalid(String),
    AuthRequired,        // 초기 인증 필요
    AuthExpired,         // 토큰 만료 (자동 갱신 불가 시)
    NetworkError(String),
    RateLimited { retry_after_secs: u64 },
    NotFound(String),
    PermissionDenied(String),
    Internal(String),
}
```

`anyhow::Error`를 플러그인 경계에서 반환하지 않는다.

---

## 5. 보안 규칙

### SSRF 방지

외부 URL을 config로 받는 플러그인은 반드시 `validate_base_url`을 호출해야 한다.

```rust
// plugin-sdk에서 제공
doxus_plugin_sdk::validate_base_url(&url)?;
```

첫 번째 게이트는 **HTTPS 강제**다. `http://`로 시작하면 URL 파싱 단계에서 즉시 차단된다.

아래 주소도 차단된다:
- Loopback: `127.0.0.0/8`, `::1`
- RFC 1918: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- Link-local / AWS metadata: `169.254.0.0/16`, `fe80::/10`
- IPv6 unique-local: `fc00::/7` (fd00:: 포함)
- Hostname: `localhost`, `*.local`

### WASM 도메인 허용목록

매니페스트에 `http_domains`를 명시하지 않으면 모든 HTTP 요청이 차단된다. 와일드카드 지원: `*.example.com`.

---

## 6. 인덱싱 파이프라인 주의사항

### updated_at fallback

```rust
// 반드시 fallback 제공 — 없으면 인덱싱 파이프라인 진입 실패
let updated_at = item.updated_at
    .or(item.created_at)
    .unwrap_or_else(|| current_time_secs());
```

### 빈 페이지 무한루프 방어

```rust
// cursor만으로 판단하면 일부 API가 빈 결과 + cursor를 함께 반환해 무한루프 발생
let next_cursor = if has_more && !documents.is_empty() {
    Some(next_offset.to_string())
} else {
    None  // 빈 페이지면 cursor 관계없이 종료
};
```

### 배치 내 메타데이터 저장 분리

```rust
// 임베딩 실패로 continue해도 documents 레코드는 반드시 저장
for doc in docs {
    save_document_record(&doc)?;           // 항상 실행
    if let Ok(embeddings) = generate_embeddings(&doc) {
        if !embeddings.is_empty() {
            save_embeddings(&doc.id, &embeddings)?;
        }
    }
}
```

---

## 7. 빌드 및 배포

### In-process 플러그인

`crates/core`에서 의존성 추가 후 `register_factory` 호출만 하면 된다.

### WASM 플러그인

```bash
# 빌드 타겟은 wasm32-wasip1 (wasm32-wasi는 지원 중단됨)
rustup target add wasm32-wasip1
cargo build --target wasm32-wasip1 --release -p doxus-plugin-my-wasm

# 출력: target/wasm32-wasip1/release/doxus_plugin_my_wasm.wasm
```

배포:

```bash
# ~/.doxus/plugins/ 아래에 .wasm + manifest.toml 쌍으로 배치
cp target/wasm32-wasip1/release/doxus_plugin_my_wasm.wasm ~/.doxus/plugins/com.example.my-wasm.wasm
cp my-wasm-plugin.manifest.toml ~/.doxus/plugins/com.example.my-wasm.manifest.toml
```

PluginManager는 `{plugin_id}.wasm` + `{plugin_id}.manifest.toml` 쌍을 자동 인식한다.

---

## 8. 테스트

### In-process 플러그인 단위 테스트

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    #[tokio::test]
    async fn test_fetch_all() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/documents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "1", "title": "Doc", "content": "body", "created_at": 1234567890}],
                "has_more": false,
                "total": 1
            })))
            .mount(&server).await;

        let mut plugin = MySourcePlugin::new();
        plugin.initialize(
            PluginConfig { fields: [("base_url".into(), server.uri().into())].into() },
            PluginSecrets { fields: HashMap::new() },
        ).await.unwrap();

        let result = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 50 }).await.unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.next_cursor.is_none());
    }
}
```

### WASM 플러그인 네이티브 테스트

`rlib` 타겟 덕분에 WASM으로 빌드하지 않고도 로직을 테스트할 수 있다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_all_impl() {
        // PluginState를 직접 세팅한 뒤 fetch_all_impl() 호출
        let state = PluginState {
            config: [("base_url".into(), "http://test-server".into())].into(),
            secrets: [("api_token".into(), "test-token".into())].into(),
            ..Default::default()
        };
        // var::set으로 state 세팅 후 테스트
    }
}
```

---

## 9. 체크리스트

개발 완료 전 아래 항목을 확인한다:

- [ ] `validate_base_url` 호출 (외부 URL을 config로 받는 경우)
- [ ] `PluginMetadata` 필드명 `name` 사용 (`display_name` 아님)
- [ ] `capabilities()` 구현 — 필수 메서드, default 없음
- [ ] `capabilities().incremental_sync`와 `fetch_changes` 구현 일관성 확인
- [ ] `updated_at` 없을 때 `created_at` fallback 처리
- [ ] 빈 페이지 반환 시 루프 탈출 (`!documents.is_empty()` 조건)
- [ ] `PluginError` 타입으로 반환 (anyhow 미사용)
- [ ] 토큰 만료 시각 저장 시 `now + expires_in` 형태로 절대 시각 사용
- [ ] 매니페스트 `http_domains`에 실제 호출 도메인 명시
- [ ] 매니페스트 `secrets`에 사용하는 키 명시
- [ ] `health_check` 구현
- [ ] WASM 빌드 타겟 `wasm32-wasip1` 사용 (`wasm32-wasi` 아님)
