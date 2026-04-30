# 플러그인 시스템 규칙

## 핵심 원칙

"문서 소스가 뭐든, core의 검색 엔진은 하나."

- 모든 문서 소스는 `DocSource` trait을 구현해야 함
- 외부 플러그인은 반드시 WASM (Extism) 샌드박스로 실행
- 플러그인 크래시는 앱 전체에 영향을 주지 않아야 함

## 플러그인 종류 선택 기준

| 방식 | 사용 조건 |
|------|----------|
| in-process (Rust) | 1st-party 빌트인만 (obsidian) — 성능 중요, 완전 신뢰 |
| WASM (Extism) | 외부 소스, 3rd-party, 격리 필요 (confluence, github, 마켓 플러그인) |

dylib / IPC Sidecar는 사용하지 않음.

## DocSource Trait 인터페이스

```rust
#[async_trait]
pub trait DocSource: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;
    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError>;
    async fn initialize(&mut self, config: PluginConfig, secrets: Secrets) -> Result<(), PluginError>;
    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError>;
    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError>;
    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError>;
    async fn health_check(&self) -> HealthStatus;
    // OAuth 플로우 지원 (선택적)
    async fn oauth_start(&self) -> Option<OAuthFlow> { None }
    async fn oauth_callback(&mut self, _code: &str) -> Result<(), PluginError> { Ok(()) }
}
```

## WASM Host Function 목록

WASM 플러그인이 사용할 수 있는 Host Function (core가 제공):

| 함수 | 설명 |
|------|------|
| `http_request` | 외부 HTTP 요청 (허용 도메인만) |
| `log` | 구조화 로그 출력 |
| `kv_get` / `kv_set` | 플러그인 전용 KV 저장소 |
| `progress` | 인덱싱 진행률 보고 |
| `secrets_get` | Keychain에서 자격증명 조회 |
| `content_transform` | core의 마크다운 파서 활용 |

Host Function 외의 시스템 접근은 WASM 샌드박스에 의해 차단됨.

## 페이지네이션 규칙

- cursor는 **opaque string** — 플러그인 내부 구현에 의존
- core는 cursor 내용을 파싱하거나 조작하지 않음
- `fetch_all`에서 cursor가 `None`이면 첫 페이지, 반환된 cursor가 `None`이면 마지막 페이지

```rust
pub struct FetchAllOpts {
    pub cursor: Option<String>,  // opaque
    pub page_size: usize,
}

pub struct DocumentStream {
    pub documents: Vec<RawDocument>,
    pub next_cursor: Option<String>,  // None = 끝
}
```

## 에러 처리

모든 플러그인 에러는 `PluginError` enum으로 통일:

```rust
pub enum PluginError {
    ConfigInvalid(String),
    AuthRequired,
    AuthExpired,
    NetworkError(String),
    RateLimited { retry_after_secs: u64 },
    NotFound(String),
    PermissionDenied(String),
    Internal(String),
}
```

`anyhow::Error`를 플러그인 경계에서 반환하지 않음.

## 플러그인 매니페스트 (WASM)

```toml
[plugin]
id = "com.doxus.confluence"
version = "1.0.0"
abi_version = 1
display_name = "Confluence"

[permissions]
http_domains = ["*.atlassian.net", "your-server.com"]
kv_namespaces = ["confluence"]
secrets = ["api_token", "base_url"]
```

매니페스트에 없는 도메인으로의 http_request는 런타임에 거부됨.

## PluginManager 규칙

- `PluginManager`는 `crates/core/src/plugin/` 에 위치
- 플러그인 로드는 lazy (첫 사용 시)
- 동일 플러그인 ID의 다중 인스턴스 허용 (source_instances 테이블로 구분)
- 플러그인 업데이트 시 기존 인덱스 무효화 여부는 ABI 버전으로 결정

## WASM 빌드 타겟

플러그인 빌드 타겟은 `wasm32-wasip1` 사용. `wasm32-wasi`는 지원 중단됨.

```bash
cargo build --target wasm32-wasip1 --release
```

> **근거**: 2026-04-27 devlog — `wasm32-wasi` 타겟 지원 중단으로 빌드 실패, `wasm32-wasip1`으로 교체.

## 외부 API 응답 필드 안전 처리

외부 플러그인(Confluence 등)의 API 응답은 선택적 필드가 누락될 수 있다.
`updated_at`, `created_at` 같은 타임스탬프 필드는 반드시 fallback을 명시해야 한다.
fallback 없이 unwrap하면 해당 문서가 인덱싱 파이프라인 진입 자체에서 실패한다.

```rust
// 올바른 예 — updated_at 없으면 created_at, 그것도 없으면 현재 시각
let updated_at = doc.updated_at
    .or(doc.created_at)
    .unwrap_or_else(|| current_time_secs());

// 잘못된 예 — updated_at 없는 문서에서 패닉 또는 인덱싱 실패
let updated_at = doc.updated_at.unwrap();
```

## 외부 API 다중 버전 지원

V1/V2처럼 다중 버전을 지원하는 API는 파라미터 직렬화 형식이 버전별로 다르다.
HTTP 요청 빌더에 버전별 분기를 명시하고, 각 버전을 독립적으로 검증할 것.

```rust
match self.api_version {
    ApiVersion::V1 => builder.query(&[("limit", limit.to_string())]),
    ApiVersion::V2 => builder.query(&[("page-size", limit.to_string())]),
}
```

페이지네이션 루프에서 빈 페이지 반환 시 **명시적 `break`** 로 탈출해야 한다.
`next_cursor`에만 의존하면 일부 API가 빈 결과와 함께 cursor를 계속 반환하는 경우 무한 루프가 발생한다.

> **근거**: 2026-04-27 devlog — Confluence V2 API limit 파라미터 버그 + 빈 페이지 무한루프 버그.
