---
title: "sqlite-vec 통합 + WASM 브릿지 검증 + MCP lib.rs 추출"
aliases:
  - sqlite-vec-wasm-bridge-mcp-extraction
  - sqlite-vec 통합 devlog
  - wasm 브릿지 검증
tags:
  - devlog
  - tdd
  - search
  - wasm
  - refactoring
created: "2026-04-10"
updated: "2026-04-10"
---

<!-- docsmith: auto-generated 2026-04-10 -->

# sqlite-vec 통합 + WASM 브릿지 검증 + MCP lib.rs 추출

## 배경

doxus의 핵심 가치인 하이브리드 검색(FTS5 + 벡터 RRF)을 실제로 작동시키기 위해 세 가지 작업을 TDD 방식으로 완료했다 (커밋: bbf9376).

1. **sqlite-vec 통합** — `SearchEngine`이 벡터 유사도 검색을 실제로 수행할 수 있도록
2. **WASM call_wasm 브릿지 검증** — `WasmDocSourceAdapter`의 스텁 메서드를 실제 WASM 라운드트립으로 전환
3. **mcp-server lib.rs 추출** — 2523줄 `main.rs` 단일 파일을 테스트 가능한 구조로 분리

## 변경 내용

### Task 1: sqlite-vec 통합 + SearchEngine async 전환

#### 주요 변경사항

- `sqlite-vec = "0.1.9"` crate 추가. `build.rs + cc` 자동 처리로 수동 `.c` 파일 불필요.
- `sqlite3_auto_extension()` + `Once` guard로 프로세스 전역 extension 등록. `load_extension()` 대신 정적 링크 환경에 올바른 방법.
- `chunk_embeddings` vec0 테이블 (384-dim FLOAT) DDL을 MIGRATIONS 배열 밖에서 별도 실행 → 버전 번호 보호.
- V4 no-op placeholder를 MIGRATIONS 배열 index 3에 삽입 → V5가 version 4로 기록되는 버그 방지.
- `SearchEngine` 구조체: `&'a Connection` → `Arc<Mutex<Connection>>` + `Arc<dyn EmbeddingProvider>`.
- `SyncSearchEngine<'a>` 래퍼 추가 → 기존 모든 호출부 하위 호환 유지 (zero-change).
- `vector_search_async()`: KNN 쿼리 `WHERE embedding MATCH ?1 ORDER BY distance LIMIT k`.
- `rrf_merge()`: FTS + vector 결과 RRF(k=60) 합산.
- `index_document()`: SHA-256 `content_hash`, binary blob 임베딩 INSERT.
- Hybrid search: `tokio::join!` 제거 → sequential (단일 Mutex 안전성).
- 에러 전파: `filter_map(|r| r.ok())` → `collect::<Result<Vec<_>>>()?`.

#### 기술 발견

- sqlite-vec 공식 Rust API는 binary blob(`embedding.as_bytes()`) 바인딩이 정식 방식.
- `sqlite3_auto_extension()`이 `load_extension(path)` 보다 정적 링크 환경에서 올바른 방법.
- `tokio::join!`을 단일 `Mutex<Connection>` 환경에서 쓰면 parallelism이 없으므로 sequential이 더 명확하다.

### Task 2: WASM call_wasm 브릿지 검증

#### 주요 변경사항

- `crates/plugin-sdk/src/wasm_types.rs` 신규 생성 — WASM 경계 직렬화 mirror 타입 정의.
- `fetch_all` / `fetch_document` / `fetch_changes` / `health_check` 스텁 → 실제 `call_wasm()` 연결.
- `call_wasm`의 `#[allow(dead_code)]` 제거.
- 최소 WASM test fixture: `extism-pdk` 기반 `wasm32-unknown-unknown` 크레이트.
- 통합 테스트 `fetch_all_calls_wasm_export` — 실제 WASM 라운드트립 검증.

### Task 3: mcp-server lib.rs 추출

#### 주요 변경사항

- `McpServer` + `dispatch_tool()` + 37개 도구 메서드 + 단위 테스트 → `lib.rs`로 이동.
- `main.rs` 2523줄 → 57줄 (DB 경로 결정 + stdin JSONL 루프만 보존).

### 영향 범위

- `crates/core`: `db/mod.rs`, `search.rs` (SearchEngine 구조 변경)
- `crates/plugin-sdk`: `wasm_types.rs` 신규, `wasm_adapter.rs` 스텁 제거
- `crates/mcp-server`: `lib.rs` 신규, `main.rs` 대폭 축소
- `crates/core/src/db/migrations/`: V4 placeholder 삽입

## 결과

- 271 passed, 0 failed (3 ignored — 모델 파일/fixture 의존).
- Architect: CONDITIONAL APPROVE → 4개 이슈 수정 후 통과.
- Security Reviewer: pre-existing SSRF in `market.rs` 확인 (이번 범위 외, 별도 추적).
- Code Reviewer: CRITICAL/HIGH 이슈 모두 수정 후 통과.
- `mcp-server`가 단위 테스트 가능한 구조로 전환됨.
- WASM 라운드트립이 실제 extism 호출로 검증됨.

## 교훈

| 문제 | 해결 |
|------|------|
| sqlite-vec 공식 crate 없다고 가정 → build.rs 수동 계획 | 조사 결과 `sqlite-vec = "0.1.9"` crate 존재, 자동 처리 확인 |
| V4 migration 배열 삽입 시 V5가 version 4로 기록 | V4 no-op placeholder 삽입으로 버전 번호 보호 |
| `tokio::join!`이 shared Mutex로 parallelism 없음 | sequential로 변경, 의도를 명확히 |
| `content_hash`에 content 전체 저장 | SHA-256 해시로 수정 |
| test_plugin/target/ 디렉토리 git 스테이징 | .gitignore 추가 후 unstage |

잔여 항목:
- `market.rs:plugin_validate_config` SSRF — pre-existing, 별도 이슈로 추적 필요.
- `reqwest::Client` per-request in WASM adapter — MINOR, 다음 이터레이션에서 개선.

## 관련 문서

- [[doxus-agent-chat-ipc-indexing]]
- [[architecture]]

---

## 2026-04-10 - Session 2

코드 리뷰 이슈 수정 + 남은 작업 계획 수립 + PR #1-4 구현 (커밋: 721b357, 061728f).

### 작업 1: 코드 리뷰 이슈 수정 (commit 721b357)

Session 1 코드 리뷰에서 나온 6개 이슈를 TDD 방식으로 수정했다.

#### 수정 내용

| 등급 | 위치 | 이슈 | 해결 |
|------|------|------|------|
| CRITICAL | `search.rs` | `project_ids` SQL injection — `format!()` 인터폴레이션 | `?N` 파라미터화 + `params_from_iter` 교체 (fts_search_sync, vector_search_sync, search_simple 3곳) |
| HIGH | `mcp-server/lib.rs:52` | `pub conn` 노출 | `conn` private으로 캡슐화 |
| HIGH | `db/mod.rs:71-76` | `transmute` 안전성 근거 부재 | `// SAFETY:` 주석 추가 |
| HIGH | `wasm_adapter.rs` | `reqwest::Client` per-request 생성 | 구조체 필드로 이동, 30s timeout 설정 |
| MEDIUM | `wasm_adapter.rs` | PATCH 메서드 미지원 | `http_request` host function에 PATCH 추가 |
| MEDIUM | `search.rs` | `rrf_merge` eager clone | Entry API로 불필요한 clone 제거 |

Security review 결과: APPROVED (0 critical/high/medium 잔존).

### 작업 2: 남은 작업 계획 수립

`.omc/plans/doxus-remaining-tasks.md` 생성. 4개 PR로 분류:

| PR | 내용 | 규모 |
|----|------|------|
| PR#1 | SearchEngine API 정리 | Small |
| PR#2 | SSRF 보안 수정 | Small |
| PR#3 | Host Functions 3종 | Medium |
| PR#4 | OnnxEmbedder 활성화 | Medium |

### 작업 3: PR #1-4 구현 및 검증 (commit 061728f)

#### PR #1 — SearchEngine API 정리 (`crates/core/src/search.rs`)

- `SyncSearchEngine::from_conn()` 추가 — primary constructor, `SearchEngine::new`는 위임.
- `make_async_engine()` 테스트 헬퍼 추출 — 3개 async 테스트의 ~80줄 중복 제거.

#### PR #2 — SSRF 수정 (`apps/desktop/src-tauri/src/commands/market.rs`)

- `validate_base_url()` 함수 신규 작성:
  - https-only 강제.
  - IPv4 private range 차단: `127.x`, `10.x`, `172.16-31.x`, `192.168.x`.
  - IPv6 강화: link-local (`fe80::/10`), unique-local (`fc00::/7`), IPv4-mapped (`::ffff:x`) 모두 차단.
- `plugin_open_url` 기존 bare scheme check → `validate_base_url()` 호출로 교체.

#### PR #3 — Host Functions (`crates/core/src/plugin/wasm_adapter.rs`)

- `progress(current, total)`: `broadcast::Sender<ProgressEvent>` 연결.
- `secrets_get(key)`: manifest.secrets allowlist 검증 + `[a-zA-Z0-9_]` 문자 제한 (env var 스텁).
- `content_transform(raw)`: HTML 태그 스트리핑, 리터럴 `>` 보존 수정.
- `from_bytes()` 시그니처에 `progress_tx: Option<broadcast::Sender<ProgressEvent>>` 추가.

#### PR #4 — OnnxEmbedder 활성화 (`scripts/download-model.sh`)

- `all-MiniLM-L6-v2` 다운로드 스크립트 생성 (HuggingFace, idempotent, `ERR` trap으로 부분 다운로드 정리).
- mcp-server 시작 시 OnnxEmbedder probe: 모델 있으면 `info` 로그, 없으면 `warn` + 스크립트 안내.
- embedding 테스트 `#[ignore]` 이유 문서화.

### 발견된 문제와 해결

| 문제 | 해결 |
|------|------|
| `secrets_get` key를 manifest allowlist 검증 없이 사용 | manifest.secrets 필드 대조 + 특수문자 차단 |
| `::ffff:192.168.1.1` IPv4-mapped 형태로 private IP 우회 가능 | `to_ipv4_mapped()` + link-local/unique-local 모두 차단 |
| `download-model.sh` curl 실패 시 partial 파일 잔존 | `trap ERR` 추가로 부분 다운로드 정리 |
| `content_transform`에서 리터럴 `>` 소실 | `'>' if in_tag =>` 분기 외 `'>' =>` fallback 추가 |

### 미구현 항목 인벤토리

`UNIMPLEMENTED_ITEMS.md` (399줄) 생성. 15개 카테고리 분류.

주요 발견:

| 카테고리 | 항목 |
|----------|------|
| Phase 0-1 블로커 | `OnnxEmbedder::embed()` 미완성, 임베더 McpServer 미연결 |
| Phase 2-3 | Confluence/GitHub `fetch_all()` 모두 `vec![]` 스텁 |
| Obsidian 플러그인 | frontmatter tags 미추출 (`tags: vec![]`) |
| CLI | `panic!()` 3곳 (lines 563, 579, 581) |
| Desktop | Agent ChatDrawer JSONL 펌프 미연결 |

### 최종 결과

- 218 tests pass, 3 ignored.
- 커밋: 721b357 (코드 리뷰 이슈 수정), 061728f (PR #1-4 구현).

---

## 2026-04-10 (세션 3) — 로드맵 전 항목 TDD 구현 + 코드리뷰 수정

### 작업 배경

이전 세션에서 작성한 `docs/context/doxus-implementation-roadmap.md` (Step 1~6, 13개 항목)를 토대로 TDD 방식으로 전체 구현 진행.

### 구현 완료 (Step 1~6)

**Step 1 — Phase 1 블로커**
- `McpServer`에 `Option<Arc<dyn EmbeddingProvider>>` 필드 추가. embedder 있으면 `SearchMode::Hybrid`, 없으면 FTS fallback
- `tool_index_project()` stub → 실제 ObsidianPlugin + 단일 SQLite 트랜잭션 인덱싱
- `crates/mcp-server/src/sync_loop.rs` 신규: `tokio::spawn` 백그라운드 sync loop, `watch::channel` graceful shutdown

**Step 2 — Obsidian 완성**
- `fetch_changes()`: walkdir mtime 기반, `FetchChangesOpts.known_ids`로 삭제 감지
- frontmatter 태그 파싱 (`tags: [a, b]`, block list, `#inline-tag`)
- 링크 추출 (`[[wikilink]]`, `[[wikilink|alias]]`, `[text](path.md)`) → `metadata["links"]`
- `fetch_all` lazy 페이지네이션: 경로 목록만 먼저 수집 후 해당 페이지 파일만 읽기

**Step 3 — Host Functions**
- `secrets_get`: `keyring` crate, `doxus-{plugin_id}` 서비스명, env var fallback
- `kv_get/kv_set`: in-memory HashMap → SQLite `plugin_kv` 테이블 (V10 마이그레이션), namespace 격리

**Step 4 — OAuth**
- `OAuthFlow`: authorization URL, code exchange, refresh token, `is_expired()` (30s 버퍼)
- `OAuthError::InvalidUrl`, `OAuthError::StateMismatch` CSRF 보호
- wiremock 기반 7개 통합 테스트

**Step 5 — External Plugins + Agent**
- Confluence: CQL `lastModified >= since` 증분 동기화, cursor 기반 페이지네이션
- GitHub: Issues + Wiki + Discussions, `FetchCursor` enum으로 소스 순서 관리, ETag cursor 인코딩
- `crates/agent/src/tool_bridge.rs` 신규: `tool_use` JSONL → `doxus_*` 허용 도구 라우팅

**Step 6 — Plugin Registry**
- `crates/core/src/plugin/registry.rs` 신규: `list_plugins()`, `download_and_install()`, SHA-256 체크섬 검증

### 코드리뷰 수정 (리뷰어: oh-my-claudecode:code-reviewer opus)

Critical/High 이슈 발견 후 즉시 수정:

| 이슈 | 수정 |
|------|------|
| `auth.rs` `.expect()` 패닉 | `authorization_url()` → `Result<String, OAuthError>` |
| SSRF 블록리스트 불완전 | `plugin-sdk`에 `validate_base_url()` 통합, RFC 1918 전체 범위 |
| `kv_store` `.lock().unwrap()` | `KvError::LockPoisoned` 전파 |
| Obsidian 전체 볼트 메모리 로드 | 경로 목록만 수집 후 해당 페이지만 읽기 |
| 인덱싱 트랜잭션 누락 | `BEGIN`/`COMMIT`/`ROLLBACK` 래핑 |
| GitHub ETag `let _ =` 폐기 | cursor 문자열에 `changes:{page}|{etag}` 인코딩 |
| Confluence 삭제 오탐 | 전체 space 조회 후 집합 차이로 삭제 계산 |
| 수제 RFC3339 파서 버그 | `chrono` 교체, 음수 타임존 수정 |

### 설계 결정

- **삭제 감지 방식**: `FetchChangesOpts`에 `known_ids` 추가, caller(core)가 DB ID 목록 전달 → 플러그인이 집합 차이로 감지. 플러그인이 DB 직접 접근하는 대신 파라미터로 받는 패턴 선택
- **ETag 저장**: `&self` immutable 제약으로 필드 저장 불가 → cursor 문자열에 인코딩 (`changes:{page}|{etag}`)
- **validate_base_url 위치**: github/confluence 중복 → `plugin-sdk` 공통 함수로 이동

### 결과

```
cargo test --workspace: 387 passed, 0 failed (+24개 신규 테스트)
커밋: 7e28220
```

### 남은 작업

- SyncRunner ↔ PluginManager 실제 연결 (현재 로그만)
- `document_links` 테이블 인덱서 연결
- `ToolBridge` ↔ `McpServer` dispatcher 배선
- Desktop UI (Phase 8)

---

## 2026-04-10 (세션 4) — TDD autopilot: sync_loop/tool_sync/agent 배선 + 보안 수정

### 작업 배경

세션 3에서 남긴 4개 미완성 항목 + critic 리뷰에서 발견된 pagination 버그를 TDD 방식으로 구현. autopilot으로 8개 태스크 완료 (commit: `c8b10a1`, 409 tests passing).

### 구현 완료

**A-1 — sync_loop + PluginManager.get_source() 연결**
- `PluginManager::get_source(plugin_id: &str) -> Option<Box<dyn DocSource + Send + Sync>>` 추가
  - `"com.doxus.obsidian"` → `Box::new(ObsidianPlugin::new())`, 나머지 → `None`
- `spawn_sync_loop()` 시그니처에 `Arc<PluginManager>` 추가
- 루프: `config_json` 파싱 → `initialize()` → `fetch_changes()` → `mark_synced()`
- `DueInstance` 구조체에 `config_json: String` 필드 추가, SQL 업데이트
- 잘못된 `config_json`은 warn + continue (패닉 없음)

**A-2 — tool_index_project 페이지네이션 수정**
- `next_cursor` 루프 추가: 단일 `fetch_all()` 호출에서 전체 페이지 순회
- 루프마다 `document_links` 삽입: `metadata["links"]` JSON 배열 → `document_links` 테이블

**A-3 — tool_sync_project 풀 구현**
- 기존 `// TODO` 스텁 → 실제 증분 동기화 구현
- `ObsidianPlugin` 초기화 → `fetch_changes()` → 트랜잭션 묶음 upsert + delete + `mark_synced()`
- `sync_cursor` NULL 처리: `unwrap_or("")` → `Option<&str>` 정확한 NULL 저장

**B-1 — CLI 플러그인 커맨드 (install/remove/update)**
- `PluginAction::Install { plugin_id }` → DB `INSERT OR IGNORE` (레지스트리 다운로드는 향후 연결)
- `PluginAction::Remove { plugin_id }` → `DELETE FROM plugins` + `.wasm` 파일 삭제
- `PluginAction::Update { plugin_id }` → 버전 업데이트 스텁 (MVP)

**B-2 — SessionRunner (SidecarManager + ToolBridge 연결)**
- `crates/agent/src/session.rs` 신규 파일
- `SessionRunner { sidecar: SidecarManager, bridge: ToolBridge }`
- `process_one()`: JSONL 라인 수신 → `ToolBridge.handle_line()` 먼저 시도 → tool_use면 결과 전송, 아니면 `AgentMessage` 파싱해서 반환
- `AgentMessage::ToolUse { id, name, input }` variant 추가
- `SidecarManager::recv_raw() / send_raw()` 로우 바이트 메서드 추가

**C-1 — OAuth CSRF 수정 (Confluence)**
- `oauth_pending_state: Option<String>` → `std::sync::Mutex<Option<String>>`
- `oauth_start(&self)`: state 생성 → Mutex에 저장 → auth URL 반환 (interior mutability)
- `oauth_exchange(&mut self, code, state)`: Mutex에서 expected state 취득 → 불일치 시 `OAuthError::StateMismatch`
- 기존 테스트: `oauth_start()` 먼저 호출 후 URL query param에서 state 추출로 수정

**C-2 — 보안 수정 (CQL injection + path traversal)**
- `validate_config`: `space_key` 영숫자 전용 검증 (`[a-zA-Z0-9_-]`) → CQL 인젝션 차단
- `tool_plugin_remove`: `plugin_id` 문자셋 검증 + canonical path check → path traversal 차단
- `PluginMetadata::capabilities()`: `oauth: true` 하드코딩 → `oauth: self.oauth_config.is_some()` 동적 반환

### 발견된 문제와 해결

| 문제 | 해결 |
|------|------|
| `oauth_exchange` 테스트에서 `oauth_start()` 없이 직접 호출 → `AuthRequired` | 테스트를 `oauth_start()` → URL state 추출 → `oauth_exchange()` 순서로 수정 |
| `make_oauth_plugin(&server, false).await` 컴파일 오류 (sync 함수에 await) | `.await` 제거 |
| `cargo test -p doxus-plugin-confluence` 0 tests | `--lib` 플래그 필요 |
| sync_cursor NULL 저장 오류 (`""` 빈 문자열 저장) | `Option<&str>` 타입으로 NULL 정확히 저장 |

### 설계 결정

- **`oauth_start(&self)` 불변 참조에서 state 저장**: `&mut self` 대신 `Mutex<Option<String>>` interior mutability 사용 — trait 시그니처 `&self` 유지
- **`Box<dyn DocSource>` 직접 호출**: `SyncRunner<S: DocSource>` 제네릭 래퍼 대신 직접 `fetch_changes()` 호출 — object-safety 우회 불필요

### 결과

```
cargo test --workspace: 409 passed, 0 failed
커밋: c8b10a1
```

### 남은 작업

- Desktop UI (Phase 8): ChatDrawer 에이전트 연결, MarketPage 실제 데이터, SettingsPage 저장
- `doxus plugin install`: 레지스트리 API 실제 다운로드 미연결
- `PluginAction::Update`: 버전 스텁, 실제 업데이트 로직 미구현
- `SidecarMessage` / `HostMessage` 중복 타입 정리 (Low priority)
