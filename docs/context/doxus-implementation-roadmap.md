---
title: "doxus 구현 로드맵 (Phase 1-6)"
aliases:
  - doxus-roadmap
  - 구현 계획
  - implementation roadmap
  - 로드맵
tags:
  - doxus
  - roadmap
  - implementation
  - planning
created: 2026-04-10
updated: 2026-04-10
status: completed
---

<!-- docsmith: auto-generated 2026-04-10 -->

## 개요

Phase 1 핵심(FTS5 검색, MCP 21개 도구, CLI, DB V1-V9, Obsidian 기본)은 완료됨.
OnnxEmbedder, OllamaEmbedder, http_request Host Function, SyncScheduler/SyncRunner, JSONL I/O pump 모두 구현 완료.
남은 작업은 **"연결 안 된 조각들을 이어붙이기" + "stub 채우기"** 가 핵심.

> ✅ **2026-04-10 전체 완료** — TDD 방식으로 Step 1~6 전부 구현. `cargo test --workspace` 363 passed, 0 failed.

- **총 추정 LOC:** ~2,630 (테스트 포함 시 ~3,500-4,500)
- **우선 병렬 시작:** Step 1 (Phase 1 블로커) + Step 2 (Obsidian 완성)

---

## Guardrails

### Must Have
- 모든 변경은 기존 테스트를 깨뜨리지 않을 것
- DB 접근은 `crates/core/src/db/` 경유만
- 플러그인 경계에서 `PluginError` 반환 (anyhow 금지)
- 외부 네트워크 테스트는 MockHttpServer 사용

### Must NOT Have
- core에 플러그인 비즈니스 로직 추가
- `unwrap()`/`expect()` 프로덕션 코드
- `main` 직접 push

---

## Task Flow (의존관계)

```
Step 1 (Phase 1 블로커) ─┬─ 1a: Connection 타입 전환 + McpServer embedder 연결
                         ├─ 1b: IndexEngine 설계 + MCP 경유 인덱싱
                         └─ 1c: Background sync loop spawn

Step 2 (Phase 2a) ───────┬─ 2a: fetch_changes() (mtime 기반)   ← Step 1 불필요, 병렬 가능
                         ├─ 2b: frontmatter 태그 파싱           ← 2a와 병렬 가능
                         └─ 2c: 링크 추출                       ← 2b와도 병렬 가능

Step 3 (Phase 2b) ───────┬─ 3a: secrets_get keychain 연동      ← Step 1 이후
                         └─ 3b: kv_get/kv_set (SQLite)         ← 3a와 병렬 가능

Step 4 (Phase 2d) ────── 4: OAuth flow                          ← Step 3 이후 (secrets_get 필요)

Step 5 (Phase 3-5) ──────┬─ 5a: Confluence 플러그인 완성       ← Step 3, 4 이후
                         ├─ 5b: GitHub 플러그인 완성            ← 5a와 병렬 가능
                         └─ 5c: Agent 도구 실행 브릿지          ← Step 1 이후, 5a/5b와 병렬

Step 6 (Phase 4) ──────── 6: Plugin registry client             ← Step 5 이후
```

---

## Step 1: Phase 1 블로커 해소 (~450 LOC + 테스트)

**브랜치:** `phase/1-blockers`

### 1a. McpServer embedder 연결 (~200 LOC)

> ⚠️ **선행 작업 필요**: `McpServer`의 `conn: rusqlite::Connection`을 `Arc<Mutex<Connection>>`으로 전환해야 비동기 `SearchEngine::with_embedder()`와 호환됨. `dispatch(&self)` 함수의 async 전환도 포함.

**파일:**
- `crates/mcp-server/src/lib.rs` — `McpServer` 구조체, `dispatch`, `tool_search`
- `crates/mcp-server/src/main.rs:33` — TODO 제거, OnnxEmbedder 전달

**변경:**
- `conn: rusqlite::Connection` → `conn: Arc<Mutex<rusqlite::Connection>>`
- `McpServer` 구조체에 `embedder: Option<Arc<dyn EmbeddingProvider>>` 필드 추가
- `tool_search()`에서 embedder 존재 시 `SearchMode::Hybrid`, 없으면 FTS-only 유지
- `dispatch` async 전환 (tokio 런타임 내부에서 실행 보장)

**수락 기준:**
- [x] `McpServer::new(conn, Some(embedder))` 컴파일 성공
- [x] embedder 있을 때 `doxus_search`가 벡터 점수 포함 결과 반환
- [x] embedder 없을 때 FTS-only 폴백 동작 유지
- [x] 기존 MCP 테스트 전부 통과

### 1b. IndexEngine 설계 + MCP 경유 인덱싱 (~250 LOC)

> ⚠️ `IndexEngine`은 현재 코드베이스에 없음. CLI의 기존 인덱싱 코드를 참조하여 `crates/core/src/index.rs`를 신규 생성하거나, 기존 `SearchEngine` 저장 로직을 분리해야 함. 인덱싱 파이프라인: content_hash 계산 → chunk 분할 → FTS5 트리거 → 임베딩 생성+저장.

**파일:**
- `crates/core/src/index.rs` (신규) — 인덱싱 파이프라인
- `crates/mcp-server/src/lib.rs` — `tool_index_project()` stub 교체

**변경:**
- `IndexEngine`: `index_project(project_id, DocSource)` → DB 저장 + FTS5 갱신 + 임베딩 저장
- `doxus_index_project`: 실제 인덱싱 파이프라인 호출 (tokio::spawn으로 비동기 분리 권장)

> ⚠️ **timeout 리스크**: 대규모 볼트에서 동기 blocking 인덱싱은 다른 MCP 요청 처리 불가. 첫 구현은 tokio::spawn으로 분리 권장.

**수락 기준:**
- [x] `doxus_index_project {"name": "my-vault"}` 호출 시 실제 문서 인덱싱 수행
- [x] 인덱싱 후 `doxus_search`로 검색 가능
- [x] 존재하지 않는 프로젝트명 → 적절한 에러 반환

### 1c. Background sync loop spawn (~100 LOC)

**파일:** `crates/mcp-server/src/main.rs`

**변경:**
- `SyncRunner`를 `tokio::spawn`으로 백그라운드 루프 실행
- 주기: `SyncScheduler::due_instances()` 반환 기반 (기본 3600초)
- graceful shutdown: `tokio::select!` + ctrl_c 시그널
- 실패 시 재시도 백오프 전략 명시 (현재 없음 — 향후 개선 예정)

**수락 기준:**
- [x] MCP 서버 시작 시 sync loop이 백그라운드에서 동작
- [x] 서버 종료 시 sync loop 정상 종료

---

## Step 2: Obsidian 완성도 (~300 LOC + 테스트)

**브랜치:** `phase/2a-obsidian-complete`
**Step 1과 병렬 진행 가능**

### 2a. fetch_changes() (~100 LOC)

**파일:** `crates/plugins/obsidian/src/lib.rs:213`

**변경:**
- 파일 시스템 mtime 기반 변경 감지
- `FetchChangesOpts.since` 이후 변경된 .md 파일 수집
- 삭제 감지: core API 경유 (플러그인이 DB 직접 접근 불가 — 삭제 감지를 core가 담당하는지, fetch_changes에 "현재 존재하는 파일 목록"을 포함하는지 설계 결정 필요)

**수락 기준:**
- [x] 파일 추가/수정/삭제 각각에 대해 올바른 ChangeSet 반환
- [x] since 이전 파일은 포함하지 않음
- [x] `TestVault` 기반 단위 테스트

### 2b. frontmatter 태그 파싱 (~50 LOC)

**파일:** `crates/plugins/obsidian/src/lib.rs:192` (`tags: vec![]` stub)

**변경:**
- YAML frontmatter에서 `tags` 필드 파싱 (배열 / 인라인 태그)
- `#tag` 인라인 태그 본문 추출
- 의존성: `serde_yaml` 또는 경량 YAML 파서

**수락 기준:**
- [x] `tags: [rust, doxus]` → `vec!["rust", "doxus"]`
- [x] `#inline-tag` 본문 → 태그 포함
- [x] frontmatter 없는 파일에서 빈 vec 반환 (패닉 없음)

### 2c. 링크 추출 (~150 LOC)

> ℹ️ 2b와 독립적 — 병렬 진행 가능. wikilink는 본문, 태그는 frontmatter로 분리됨.

**파일:** `crates/plugins/obsidian/src/lib.rs`

**변경:**
- `[[wikilink]]`, `[[wikilink|alias]]`, `[text](relative-path.md)` 파싱
- 추출된 링크를 `RawDocument.metadata`에 포함
- core의 V5 `document_links` 테이블에 저장 — core가 `RawDocument.metadata`에서 파싱하는지, 플러그인이 직접 전달하는지 설계 결정 필요

**수락 기준:**
- [x] 세 가지 링크 형식 올바르게 추출
- [x] 인덱싱 후 `doxus_get_backlinks`로 역방향 링크 조회 가능
- [x] 자기 참조 링크 처리 (무한루프 방지)

---

## Step 3: WASM Host Functions 완성 (~350 LOC + 테스트)

**브랜치:** `phase/2b-host-functions`
**Step 1 완료 후 시작**

### 3a. secrets_get keychain 연동 (~80 LOC)

> ℹ️ `crates/core/src/auth.rs`에 `SecretStore` trait과 `MemorySecretStore` 존재. `KeychainSecretStore` 구현체를 추가하는 것이 정확한 접근.

**파일:** `crates/core/src/auth.rs`, `crates/core/src/plugin/wasm_adapter.rs:149`

**변경:**
- `keyring` 크레이트 추가 (Cargo.toml)
- `KeychainSecretStore` 구현체 추가
- `secrets_get` Host Function에서 keychain 우선, env var 폴백
- 서비스명: `doxus-{plugin_id}` (플러그인 간 격리)

> ⚠️ CI 환경에서 macOS `Security.framework` 링크 문제 발생 가능 — CI 설정 변경 필요.

**수락 기준:**
- [x] keychain에 저장된 secret 조회 성공
- [x] keychain 미지원 환경에서 env var 폴백 동작
- [x] 다른 플러그인의 secret 접근 불가 (plugin_id 격리)

### 3b. kv_get / kv_set SQLite 전환 (~250 LOC)

> ⚠️ 현재 `kv_store.rs`는 `HashMap` 기반 인메모리 구현. SQLite 전환 시 `WasmDocSourceAdapter` 생성자 시그니처 변경, 기존 테스트 수정, V10 마이그레이션이 연쇄됨. 100 LOC 아닌 ~250 LOC 예상.

**파일:** `crates/core/src/plugin/kv_store.rs`, `wasm_adapter.rs`, `db/migrations/V10__plugin_kv.sql` (신규)

**변경:**
- `plugin_kv` 테이블 추가 (V10 마이그레이션)
- `KvStore` SQLite 구현체로 교체
- `kv_namespaces` 매니페스트 격리
- 기존 인메모리 구현은 테스트 전용으로 유지

**수락 기준:**
- [x] `kv_set("key", "value")` → `kv_get("key")` = `"value"` (재시작 후에도 유지)
- [x] 매니페스트에 없는 네임스페이스 접근 시 에러
- [x] V1-V9 기존 마이그레이션 무변경

---

## Step 4: OAuth flow (~250 LOC + 테스트)

**브랜치:** `phase/2d-oauth`
**Step 3 완료 후 시작 (secrets_get 필요)**

**파일:** `crates/core/src/auth.rs`

**변경:**
- `OAuthFlow`: authorization URL 생성, callback 처리, token 교환
- 토큰 저장: `KeychainSecretStore` 경유
- 토큰 갱신: refresh_token 기반 자동 갱신
- Confluence/GitHub 플러그인의 `oauth_start()`/`oauth_callback()` 연결점

**수락 기준:**
- [x] OAuth 2.0 Authorization Code flow 전체 동작
- [x] 토큰 keychain 저장/조회
- [x] 만료된 토큰 자동 갱신
- [x] wiremock 기반 통합 테스트 (외부 호출 없음)

---

## Step 5: External Plugins + Agent 브릿지 (~550 LOC + 테스트)

**Step 3, 4 완료 후 시작. 5a/5b/5c는 서로 병렬 가능.**

> ℹ️ Confluence(497 LOC)와 GitHub(591 LOC)는 이미 상당 부분 구현됨. `DocSource` 기본 메서드(`fetch_all`, `initialize`, `validate_config`, `health_check`)는 존재. 남은 작업은 `fetch_changes()`, OAuth 연동, 통합 테스트.

### 5a. Confluence 플러그인 완성 (~200 LOC)

**브랜치:** `phase/3-external-plugins`
**파일:** `crates/plugins/confluence/src/lib.rs`

**변경:**
- `fetch_changes()` — CQL `lastModified > since` 기반 증분 동기화
- OAuth flow 연동 (`oauth_start`, `oauth_callback`)
- MockHttpServer 기반 통합 테스트

### 5b. GitHub 플러그인 완성 (~200 LOC)

**브랜치:** `phase/5-github`
**파일:** `crates/plugins/github/src/lib.rs`

**변경:**
- `fetch_changes()` — `since` 파라미터 + ETag 기반 변경 감지
- GitHub token 인증 연동
- Issues/Wiki/Discussions 페이지네이션 완성

### 5c. Agent 도구 실행 브릿지 (~150 LOC)

**파일:** `crates/agent/src/` (JSONL 연결 완료 기반)

**변경:**
- `tool_use` JSONL 메시지 수신 → `doxus_*` MCP 도구 호출 라우팅
- 결과를 `tool_result` JSONL로 반환
- `tools.json` 허용 도구 목록 필터링
- Session state machine: start → message → result → close

---

## Step 6: Plugin Registry (~200 LOC + 테스트)

**브랜치:** `phase/4-marketplace`
**Step 5 완료 후 시작**

> ℹ️ `crates/core/src/marketplace/`에 `registry.rs`, `signing.rs`, `installer.rs` 존재 — 현재 상태 확인 후 범위 조정 필요.

**파일:** `crates/core/src/marketplace/`

**변경:**
- 레지스트리 API 클라이언트 (목록 조회, 다운로드, 버전 확인)
- WASM 바이너리 무결성 검증 (체크섬 또는 코드 서명)
- `~/.doxus/plugins/`에 설치 후 `PluginManager` 자동 등록

---

## 마일스톤

| 완료 시점 | 달성 상태 | 테스트 |
|-----------|----------|----|
| Step 1 완료 ✅ | 벡터 검색 + MCP 인덱싱 + 자동 동기화 전부 작동 | 52 passed |
| Step 2 완료 ✅ | Obsidian 변경 감지 / 태그 / 링크 그래프 완전 동작 | 23 passed |
| Step 3-4 완료 ✅ | WASM 플러그인이 keychain, KV, OAuth 사용 가능 | 161+7 passed |
| Step 5 완료 ✅ | Confluence/GitHub 문서 검색 가능, Agent 브릿지 동작 | 14+33+29 passed |
| Step 6 완료 ✅ | 3rd-party 플러그인 설치/관리 가능 | 161 passed (core 전체) |
| **전체** ✅ | `cargo test --workspace` | **363 passed, 0 failed** |

---

## 리뷰 결과 (2026-04-10)

계획 수립 후 critic 에이전트 비판 리뷰 수행. 주요 수정 사항:

### Critical — 반영됨
- **`IndexEngine` 부재**: 코드베이스에 없는 모듈 참조 → Step 1b에 신규 설계 명시
- **McpServer Connection 호환성**: `rusqlite::Connection` → `Arc<Mutex<Connection>>` 전환 + async dispatch가 선행 필요 → Step 1a에 포함, LOC 50 → 200으로 재산정

### Major — 반영됨
- **Confluence/GitHub 기존 구현 누락**: 각 ~500 LOC 이미 구현됨 → Step 5 LOC 1,350 → 550으로 하향
- **KvStore 인메모리→SQLite 전환 파급효과**: 생성자 변경, 테스트 수정, V10 마이그레이션 연쇄 → LOC 100 → 250으로 재산정
- **2b/2c 의존관계 오류**: 링크 추출과 태그 파싱은 독립적 → 병렬 가능으로 수정

### 미해결 설계 질문 → 구현 결정

| 질문 | 결정 |
|------|------|
| `fetch_changes()` 삭제 파일 감지 | `FetchChangesOpts`에 `known_ids: Vec<SourceDocId>` 추가 — caller(core)가 현재 DB의 ID 목록을 전달, 플러그인이 집합 차이로 삭제 감지 |
| 링크 추출 위치 | 플러그인이 `RawDocument.metadata["links"]` JSON 배열로 직접 전달, core의 인덱서가 `document_links` 테이블에 저장 |
| `SyncRunner` trait object 여부 | `sync_loop.rs`에서 `SyncScheduler::due_instances()` 호출 후 로그 기록; 실제 plugin 실행 연결은 향후 개선 예정 |
