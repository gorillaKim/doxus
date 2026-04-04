# Rust 코딩 컨벤션

## 크레이트 역할 분리

| 크레이트 | 허용 | 금지 |
|---------|------|------|
| `core` | 검색, 인덱싱, DB, 플러그인 관리, 임베딩 | 플러그인 비즈니스 로직, HTTP 직접 요청 |
| `plugin-sdk` | DocSource trait, 공유 타입, PluginError | core 의존성 |
| `plugins/*` | plugin-sdk 구현 | core 내부 직접 접근 |
| `cli` | 사용자 인터페이스, 커맨드 파싱 | 검색 로직 직접 구현 |
| `mcp-server` | MCP 프로토콜, 도구 라우팅 | 검색 로직 직접 구현 |
| `agent` | CLI 감지, sidecar 관리 | 검색 직접 호출 |

## 에러 처리

- 라이브러리 크레이트 (`core`, `plugin-sdk`, `plugins/*`): `thiserror` 사용
- 바이너리 크레이트 (`cli`, `mcp-server`): `anyhow` 사용
- 플러그인 경계에서는 `PluginError`로 반드시 변환
- `unwrap()` / `expect()` 는 테스트 코드에서만 허용

```rust
// 올바른 예
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("embedding failed: {0}")]
    Embedding(String),
}

// 잘못된 예 — 라이브러리에서 anyhow 반환
pub fn search(...) -> anyhow::Result<Vec<Hit>> { ... }
```

## async 사용 패턴

- `DocSource` trait은 `#[async_trait]` 필수
- Tauri 커맨드는 `#[tauri::command]` + `async fn`
- tokio 런타임은 바이너리 진입점에서 한 번만 생성
- `block_on`은 async 컨텍스트 내에서 사용하지 않음

```rust
#[async_trait]
impl DocSource for ObsidianPlugin {
    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        // ...
    }
}
```

## 모듈 visibility

- 크레이트 외부로 노출이 필요한 것만 `pub`
- 같은 크레이트 내 모듈 간 공유는 `pub(crate)`
- 테스트 헬퍼는 `#[cfg(test)]` 모듈 또는 별도 `tests/` 디렉토리

## 임베딩 엔진 규칙

```rust
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    fn dimension(&self) -> usize;
}

// 구현체
// - OnnxEmbedder: 내장 (기본값, all-MiniLM-L6-v2)
// - OllamaEmbedder: 선택적 fallback (Ollama 서버 필요)
```

- 기본 임베더는 `OnnxEmbedder` (ONNX Runtime, 외부 서버 불필요)
- 배치 인퍼런스 필수 (문서 1개씩 embed 호출 금지)
- 모델 파일은 `crates/core/models/` 에 번들

## 의존성 규칙

- SQLite: `rusqlite` (bundled feature)
- 벡터 검색: `sqlite-vec` (extension)
- WASM 런타임: `extism`
- ONNX Runtime: `ort`
- HTTP 클라이언트: `reqwest` (rustls-tls, native-tls 금지)
- 직렬화: `serde` + `serde_json`
- 비동기: `tokio` (full features)
- 에러: `thiserror` (lib) / `anyhow` (bin)

## 파일/모듈 구조 예시 (core)

```
crates/core/src/
├── lib.rs
├── search.rs           # SearchEngine (FTS5 + sqlite-vec)
├── index_engine.rs     # 인덱싱 파이프라인
├── embedding.rs        # EmbeddingProvider trait + OnnxEmbedder
├── db/
│   ├── mod.rs
│   ├── migrations/     # V1.sql ~ V8.sql
│   └── schema.rs
├── plugin/
│   ├── mod.rs
│   ├── manager.rs      # PluginManager
│   └── wasm_adapter.rs # WasmDocSourceAdapter
└── workspace/
    ├── mod.rs
    └── template.rs
```
