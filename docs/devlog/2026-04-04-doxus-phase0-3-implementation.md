---
title: "doxus Phase 0~3 TDD 구현 세션"
aliases:
  - doxus-phase0-3
  - doxus-phase0-3-implementation
  - doxus 구현 세션
  - doxus Phase 0~3
tags:
  - devlog
  - implementation
  - rust
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# doxus Phase 0~3 TDD 구현 세션

## 배경

doxus는 obsidian-nexus의 차세대 진화판으로, WASM 플러그인 기반 다중 소스 통합 문서 검색 허브다. 로컬 퍼스트 + 에이전트 친화적 설계를 핵심으로 하며, 이 세션에서는 autopilot + team agents를 활용해 Phase 0부터 Phase 3까지 TDD 방식으로 전체 기반 구현을 완료했다.

기존 obsidian-nexus의 FTS5 + 벡터 하이브리드 검색 엔진을 계승하면서, ONNX 내장 임베딩과 Extism WASM 플러그인 시스템을 새롭게 도입하는 것이 이번 구현의 핵심 과제였다.

## 변경 내용

### 주요 변경사항

**Phase 0-A: ONNX 임베딩 PoC**
- `EmbeddingProvider` trait 정의 (async, Send+Sync, thiserror 기반 EmbeddingError)
- `OnnxEmbedder`: all-MiniLM-L6-v2.onnx (86MB) + tokenizer.json 번들, 384차원 벡터 출력
- 배치 인퍼런스: TensorRef 기반 mean pooling + L2 normalize
- 코사인 유사도 검증, 13개 테스트 (unit 11개 + ONNX inference 2개 `#[ignore]`)
- `ort = "2.0.0-rc.12"` with `download-binaries` feature 사용

**Phase 0-B: Extism WASM PoC**
- `Plugin`: Send=YES, Sync=NO 확정
- 최종 아키텍처 패턴: `Arc<Mutex<Plugin>> + tokio::spawn_blocking`
- extism 1.21.0 (번들 Wasmtime 41.0.4) 통합 검증

**Phase 1: Cargo Workspace + Core 포팅**
- workspace members: core, plugin-sdk, plugins/obsidian, cli, mcp-server, agent, extism-poc
- DB 마이그레이션 V1~V8 (V4 vec0는 extension 로드 후 별도 적용)
- `SearchEngine`: FTS5 + BM25 검색, `SearchQuery` 빌더 패턴
- doxus-cli: `project add/list/remove/enable/disable`, `index`, `search`, `status` 커맨드
- MCP 서버: 37개 `docnx_*` 도구 (3개 구현, 나머지 stub)

**Phase 2a: plugin-sdk + Obsidian 플러그인**
- `DocSource` trait: async_trait, Send+Sync, optional OAuth 메서드
- 공유 타입: PluginError, RawDocument, FetchAllOpts, DocumentStream
- `ObsidianPlugin`: walkdir로 `.md` 파일 읽기, 숨김 경로 필터링 (상대 경로 기준)
- 페이지네이션: cursor = offset 문자열 (opaque)
- 버그 수정: 절대 경로 컴포넌트 기준 hidden dir 필터 → 상대 경로 기준으로 수정

**Phase 2b: WasmDocSourceAdapter**
- `Arc<Mutex<Plugin>> + tokio::spawn_blocking` 패턴 구현
- `call_wasm<I: Serialize, O: DeserializeOwned>()` 제네릭 헬퍼
- `DocSource` trait 구현 (WASM 함수 없으면 빈 결과 반환)

**Phase 2c: Host Function 인프라**
- `PluginManifest`: ABI 버전 검증, `http_domains` 화이트리스트
- `KvStore`: `Arc<RwLock<HashMap>>` 플러그인 전용 KV 저장소
- `WasmDocSourceAdapter`에 `kv_get` / `kv_set` / `is_http_allowed` 메서드 추가

**Phase 2d: Auth 추상화**
- `SecretStore` trait: get / set / delete
- `MemorySecretStore`: 테스트 / CI용 인메모리 구현
- `OAuthFlow` 타입: auth_url, state, redirect_uri

**Phase 3: Confluence 플러그인 + Agent Sidecar**
- `ConfluencePlugin`: reqwest 기반, wiremock TDD 테스트
- Confluence REST API: content 목록 페이지네이션, health_check
- `ContentType::Html` plugin-sdk에 추가
- Agent sidecar: `CliKind` (ClaudeCode / GeminiCli / None), `detect_cli()`
- JSONL 프로토콜: `HostMessage` + `AgentMessage` (serde tagged enums)
- 환경변수 테스트 race condition 방지: `static ENV_LOCK: Mutex<()>` 도입

**보안 QA 수정 (code-reviewer + security-reviewer)**
- **CRITICAL — SSRF**: `url.contains(suffix)` 방식의 도메인 화이트리스트 우회 가능 → `url::Url::parse()`로 host만 비교, wildcard는 `host.ends_with(".suffix")`로 수정, 테스트 6개 추가
- **HIGH — 경로 주입**: `fetch_document`에서 `SourceDocId`를 URL에 직접 삽입 → ID 문자 검증 추가 (alphanumeric + hyphen + underscore만 허용)
- **MEDIUM — 락 패닉**: `MemorySecretStore` `RwLock::unwrap()` → `map_err(|_| AuthError::Keychain("lock poisoned"))`으로 교체

### 영향 범위

- `crates/core`: SearchEngine, DB 마이그레이션, EmbeddingProvider
- `crates/plugin-sdk`: DocSource trait, 공유 타입 전체
- `crates/plugins/obsidian`: ObsidianPlugin 구현
- `crates/plugins/confluence` (신규): ConfluencePlugin
- `crates/cli`: 전체 CLI 커맨드
- `crates/mcp-server`: 37개 도구 스캐폴드
- `crates/agent`: CliKind 감지, JSONL 프로토콜

## 결과

- 총 **68개 테스트**, 0 실패, 2 ignored (ONNX 모델 파일 필요)
- 보안 수정 완료: SSRF 우회 / 경로 주입 / 락 패닉 3건
- 커밋: `e2154c8` (Phase 2b~3 구현), `14f22ff` (보안 수정)
- Phase 0~3 기반 구현 완료, Phase 4 (플러그인 마켓) 준비 상태

## 교훈

- **ONNX 버전 이슈**: `ort = "2"` 지정 시 crates.io에서 찾지 못함. rc 버전은 반드시 전체 버전 문자열(`"2.0.0-rc.12"`)로 고정해야 한다.
- **WASM Send+Sync**: Extism `Plugin`은 Send=YES, Sync=NO. 멀티스레드 환경에서는 `Arc<Mutex<Plugin>> + tokio::spawn_blocking` 패턴이 유일한 안전한 해법이다. Phase 0-B PoC에서 이를 먼저 확정한 것이 Phase 2b 설계에 결정적이었다.
- **도메인 화이트리스트 보안**: 단순 문자열 `contains` 검사는 쿼리 파라미터나 경로에도 매칭돼 SSRF 우회가 가능하다. URL을 파싱한 뒤 host 컴포넌트만 비교해야 한다. 화이트리스트 구현은 반드시 negative 테스트(우회 시도)를 함께 작성할 것.
- **환경변수 테스트 격리**: `std::env::set_var`는 프로세스 전역 상태를 변경하므로 병렬 테스트에서 race condition이 발생한다. `static Mutex`로 직렬화하거나, 환경변수 대신 의존성 주입(DI) 패턴으로 설계하는 것이 근본적인 해결책이다.
- **ndarray 버전 충돌**: ort 내부에서 ndarray 0.17을 사용하는데 Cargo.toml에 0.16을 명시하면 충돌한다. ort가 re-export하는 버전을 직접 사용하거나 standalone 의존성을 제거해야 한다.

---

## 2026-04-04 세션 2 — Phase 4~8 구현 완료

### 배경

이전 세션에서 Phase 0~3 기반 구현을 완료한 뒤, 남은 미구현 트랙 5개(Track A~E)를 우선순위 순서로 TDD 방식으로 구현했다.

### 구현 내용

**Track B (CLI) — 2순위**
- `plugin list/status` 서브커맨드 추가 (source_instances 테이블 조회)
- `workspace list/create` 서브커맨드 추가 (workspaces V8 테이블)
- 통합 테스트 8개 (TempDir + doxus_core::db::open)

**Track A (MCP) — 1순위**
- `McpServer` struct 도입 (dispatch_tool을 free fn → method로 리팩토링)
- DB 연결: rusqlite::Connection 보유
- 13개 도구 실제 구현: list_projects, add_project, remove_project, search, get_document, get_section, get_metadata, list_documents, get_backlinks, get_links, plugin_list, diagnose, system_report
- 테스트 17개

**Track C (Desktop) — 3순위**
- `AppState { conn: Mutex<Connection>, plugin_manager: PluginManager }` 완성
- IPC 커맨드 4개: search_documents, list_projects, market_list_installed, get_workspaces
- tauri.conf.json 추가, `[[bin]] test=false` 설정
- 테스트 1개

**Track D (Agent) — 4순위**
- `cli_detector.rs`: CliKind enum + detect_cli() (env var + PATH)
- `manager.rs`: AgentManager (start/stop/is_running + Drop)
- `protocol.rs`: HostMessage/AgentMessage 직렬화 타입
- 테스트 13개

**Track E (RegistryClient HTTP) — 5순위**
- `RegistryClient::fetch_entries()` async 구현 (reqwest)
- wiremock 테스트 3개 (성공, HTTP 500, trailing slash)

### Phase 4 보안 검토 (security-reviewer + code-reviewer 에이전트)

발견된 이슈와 수정:
- **CRITICAL**: `RegistryClient::new()` 에서 `expect()` → `Result<Self, RegistryError>` 반환으로 변경
- **HIGH**: `install_from_bytes()` pub → `pub(crate)` 제한 (서명 우회 방지)
- **HIGH**: MCP server `db::open().expect()` → `?` 연산자로 대체
- **MEDIUM**: GitHub plugin `base_url` SSRF (문서화, 향후 allowlist 추가 예정)

### 미구현 갭 추가 구현 (설계 대비 점검 후)

**Priority 1: http_request Host Function** (`wasm_adapter.rs`)
- `HttpRequest` / `HttpResponse` 타입 정의
- `http_request()` async 메서드: URL 파싱 → 도메인 allowlist 검사 (SSRF) → reqwest 실행
- 와일드카드 도메인 지원 (`*.atlassian.net`)
- `new_with_domains()` 생성자 추가
- 테스트 6개

**Priority 3: SyncDb — DB 연동** (`sync/db.rs`)
- `mark_synced(instance_id, cursor)`: source_instances.last_synced 업데이트
- `due_instances(interval_secs)`: 동기화 대상 조회 (비활성 프로젝트 제외)
- `get_cursor(instance_id)`: cursor 조회
- 테스트 7개

**Priority 2: Node.js Agent Sidecar** (`crates/agent/sidecar/`)
- `sidecar.js`: JSONL stdio bridge (start/message/cancel/close 처리)
- `package.json`: node >=18, `node --test` 연동
- `sidecar.test.js`: 5개 테스트 (init, start, cancel, invalid JSON)
- `default_sidecar_path()`: 프로덕션(`~/.doxus/agents/`) / 개발(`CARGO_MANIFEST_DIR`) 폴백

**Priority 4+5: React UI + Zustand Stores** (`apps/desktop/src/`)
- `useSearchStore`: query/hits + invoke('search_documents')
- `useProjectStore`: projects + invoke('list_projects')
- `useChatStore`: drawer open/close + message history
- `SearchPage`: 검색 폼 + 결과 카드 (score, 프로젝트 뱃지, snippet)
- `ProjectsPage`: 프로젝트 목록 + active/disabled 뱃지
- `ChatDrawer`: 우측 고정 오버레이 (w-96), 역할별 말풍선

### 최종 테스트 현황

- Rust: 94 passed (core) + 8 CLI + 17 MCP + 1 Desktop + 13 Agent = **133+ passed**
- JavaScript: 5 passed (sidecar)
- 0 failures, 0 errors

### 커밋

- `dd17ede`: Track B+A+C+D+E 전체 구현
- `8f4c38c`: 보안 검토 수정
- `0a76402`: http_request + SyncDb + sidecar + React UI

### 남은 항목

- React App.tsx 라우터 연결 (SearchPage/ProjectsPage 미등록)
- Desktop package.json / Vite 빌드 설정
- Agent sidecar Claude API 실제 연동 (현재 echo stub)
- MCP 나머지 24개 도구 실제 구현

---

## 2026-04-04 세션 3 — 보안 강화 + 프론트엔드 스캐폴드 + 남은 트랙 완료

### 배경

세션 2에서 남긴 미구현 항목(MCP 24개 도구, Agent Claude API 연동, Desktop 빌드 설정, SyncScheduler/Runner, 워크스페이스 템플릿)을 완료하고, 추가 보안 QA에서 발견된 CRITICAL/HIGH/MEDIUM 이슈를 수정했다.

### 구현 내용

#### 작업 1: 보안 강화 (commit: 33b21f1)

**SHA-256으로 콘텐츠 해시 교체** (`crates/cli/src/main.rs`)
- 기존: `DefaultHasher` (비암호학적 해시) — 콘텐츠 해시 충돌 가능성
- 변경: `sha2::Sha256` + `hex::encode`로 32바이트 hex digest 반환
- Cargo.toml workspace에 `sha2 = "0.10"`, `hex = "0.4"` 추가

**공개키 핀닝** (`crates/core/src/plugin/manager.rs`)
- 기존: `install_signed()`가 `SignedPlugin.public_key`만 신뢰 (플러그인 자기 신고)
- 변경: `entry.public_key_hex`와 `plugin.public_key` 일치 여부 검증 후 ED25519 서명 검증
- 신규 테스트: `install_signed_rejects_key_mismatch`
- 기존 테스트 업데이트: `make_pinned_entry()` 헬퍼 추가

**GitHub SSRF 방어** (`crates/plugins/github/src/lib.rs`)
- `validate_base_url()` 추가: HTTPS 필수, localhost / 127.0.0.1 / 169.254.169.254 / .local 차단
- `validate_config()`에서 `base_url` 필드 검증 호출
- 신규 테스트 4개: https 허용, http 거부, localhost 거부, 링크로컬 거부

**`AgentManager::is_running()` 수정** (`crates/agent/src/manager.rs`)
- 기존: `self.process.is_some()` — 이미 종료된 프로세스도 running으로 오인
- 변경: `child.try_wait()`로 실제 프로세스 생존 확인
- 서명 변경: `&self` → `&mut self` (try_wait이 &mut Child 필요)

#### 작업 2: 프론트엔드 스캐폴드 (commit: 33b21f1)

**Desktop 빌드 문제 해결:**
- `build.rs` 누락 → `tauri_build::build()` 추가
- PNG 아이콘 채널 오류: RGB(3ch) → RGBA(4ch) Python 스크립트로 재생성
- 결과: `cargo build -p doxus-desktop` 성공

**신규 파일:**
- `apps/desktop/package.json` (react-router-dom 7, zustand 5, tauri api 2)
- `apps/desktop/vite.config.ts`, `tsconfig.json`
- `apps/desktop/src/App.tsx` — BrowserRouter + Routes (search / projects / workspace / market)
- `apps/desktop/src/main.tsx` — ReactDOM.createRoot
- `apps/desktop/src/components/layout/AppShell.tsx` — Sidebar + NavLink + ChatDrawer

#### 작업 3: MCP 24개 도구 추가 구현 (commit: 1b5729a)

`crates/mcp-server/src/main.rs`에 21개 도구 추가 구현:
- **그래프**: find_related (FTS 기반), find_path (BFS with recursive CTE), get_cluster (멀티홉)
- **동기화**: sync_project (source_instances cursor 상태 반환)
- **플러그인**: plugin_install / remove / update / search / status / logs / info (V7 테이블)
- **워크스페이스**: workspace_documents CRUD + apply_template (V8 테이블)
- **진단**: explain_search (BM25 term frequency 분석)
- MCP 테스트: 17 → 38개

#### 작업 4: SyncScheduler + SyncRunner (commit: 1b5729a)

신규 파일:
- `crates/core/src/sync/scheduler.rs`: `SyncScheduler::due_instances()` → SyncDb 위임
- `crates/core/src/sync/runner.rs`: `SyncRunner<S: DocSource>::run_once()` → fetch_changes + mark_synced
- sync 테스트: 7 → 20개

#### 작업 5: Agent Sidecar Claude API 연동 (commit: 1b5729a)

`crates/agent/sidecar/sidecar.js` 교체:
- ESM 방식으로 `@anthropic-ai/sdk` import
- streaming 방식: `client.messages.stream()` → content_block_delta 이벤트로 text 청크 전송
- 세션별 대화 히스토리: `sessions = new Map(session_id → messages[])`
- `ANTHROPIC_API_KEY` 없으면 echo fallback 자동 전환
- cancel 수신 시 in-flight stream 중단 플래그
- sidecar 테스트: 5 → 9개

#### 작업 6: Handlebars 워크스페이스 템플릿 (commit: 1b5729a)

신규: `crates/core/src/workspace/template.rs`
- `TemplateEngine::register()`, `render()`, `with_builtins()`
- 내장 템플릿 5개: note, meeting, decision, journal, retrospective
- `handlebars = "6"` 추가
- workspace 테스트: 6 → 14개

### 트러블슈팅

| 문제 | 원인 | 해결 |
|------|------|------|
| `cargo build -p doxus-desktop` proc macro panic | `build.rs` 누락 | `tauri_build::build()` 추가 |
| icon.png is not RGBA | Python으로 RGB PNG 생성 | `color_type=6` (RGBA) 재생성 |
| `is_running()` 컴파일 오류 | `&self` → `&mut self` 변경 후 테스트에서 `mut` 없음 | `let mut mgr` 선언으로 수정 |
| 워크트리 브랜치에 커밋 없음 | isolation:worktree 에이전트가 uncommitted 상태 | git status 확인 후 메인 repo에서 직접 커밋 |

### 최종 테스트 현황

- Rust: 147 → **182 passed** (35개 신규)
- JavaScript (sidecar): 5 → **9 passed**
- 커밋: `33b21f1` (보안 강화 + Desktop 빌드), `1b5729a` (MCP 24개 + Sync + Sidecar + Template)

### 남은 항목

- Desktop Tailwind 스타일 완성 + MarketPage 실제 연동
- MCP 워크트리 에이전트 uncommitted 상태 잔존 주의

### 교훈

- **공개키 핀닝**: 플러그인 자기 서명만 검증하는 것은 불충분하다. 레지스트리 entry의 `public_key_hex`와 플러그인 번들의 공개키를 교차 검증해야 신뢰 체인이 완성된다.
- **프로세스 생존 확인**: `Option<Child>.is_some()`은 프로세스 종료를 감지하지 못한다. `try_wait()`로 실제 exit status를 확인해야 하며, 이는 `&mut self`를 요구하므로 API 서명 변경이 수반된다.
- **RGBA vs RGB 아이콘**: Tauri는 icon.png가 반드시 RGBA(4채널)여야 한다. PIL/Pillow로 생성 시 `mode='RGBA'`를 명시하거나 `convert('RGBA')` 후 저장해야 한다.
- **워크트리 에이전트 격리**: 멀티 에이전트 워크트리 환경에서 하위 에이전트가 uncommitted 상태로 종료될 수 있다. 오케스트레이터는 완료 후 반드시 각 워크트리의 git status를 확인하고 커밋을 통합해야 한다.

---

## 세션 5 — 2026-04-04

### 배경

세션 3에서 남긴 항목(SearchEngine 실제 구현, MCP 테스트 보강, Desktop 빌드 환경, Tailwind 스타일, MarketPage 연동)을 완료하고, Desktop UI를 전면 재설계하여 프로덕션 수준의 완성도로 끌어올렸다.

### 구현 내용

#### 1. 미구현 항목 TDD 구현

**SearchEngine** (`crates/core/src/search.rs`)
- `index_document()`: documents + FTS5 가상 테이블 upsert
- `search_simple()`: FTS5 쿼리 + RRF 스코어링 + project filter
- 신규 테스트 4개 (empty result, finds doc, ranks by RRF, project filter) → **7 tests total**

**MCP 핸들러** (`crates/mcp-server/src/main.rs`)
- +8 tests: get_document, add_project, remove_project, status, index_project
- **46 tests total**

**Desktop IPC 정렬**
- `SearchHit` TypeScript 타입을 Rust 백엔드 JSON shape와 정렬
- `App.tsx` default import 방식 수정 (named export 충돌 해결)

**전체: 195 Rust tests 통과**

#### 2. Desktop 앱 개발 모드 실행 환경 구성

| 문제 | 원인 | 해결 |
|------|------|------|
| `npm run tauri dev` → 404 | index.html 없음 | `apps/desktop/index.html` 생성 (Vite entry point) |
| Tailwind 스타일 미적용 | CSS 파일 없음, plugin 미설치 | `@tailwindcss/vite` 플러그인 + `src/index.css` 생성 |
| `tauri dev` 시 Vite 미시작 | `beforeDevCommand` 없음 | `tauri.conf.json`에 `devUrl` + `beforeDevCommand` 추가 |
| WorkspacePage/MarketPage import 에러 | named export인데 default import | `App.tsx` import 방식 수정 |
| MarketPage `filter is not a function` | `invoke` 반환이 `{plugins:[...]}` 객체 | 응답 shape 자동 감지 + MOCK_PLUGINS fallback 추가 |

변경 파일:
- `apps/desktop/index.html` (신규)
- `apps/desktop/src/index.css` (신규, `@import "tailwindcss"`)
- `apps/desktop/vite.config.ts`: `@tailwindcss/vite` 플러그인 추가
- `apps/desktop/src-tauri/tauri.conf.json`: `devUrl: "http://localhost:1420"`, `beforeDevCommand: "npm run dev"`

#### 3. Desktop UI 전체 구현

**DashboardPage** (`src/pages/DashboardPage.tsx`, 신규)
- 통계 카드 3개: 프로젝트 수 / 문서 수 / 마지막 동기화
- 최근 검색 히스토리 목록
- 빠른 액션 버튼 (Add Project, Sync All, Open Chat)

**ProjectsPage 재설계**
- 기존 인라인 `AddProjectForm` → 소스 타입별 모달 방식으로 전면 재설계
- 소스 타입 선택: Obsidian / Confluence / GitHub

**WorkspacePage** (`src/pages/WorkspacePage.tsx`, 신규)
- Documents / Templates 탭 구조
- `NewDocModal`: 제목 + 템플릿 선택
- 내장 템플릿 5개 (note, meeting, decision, journal, retrospective)

**MarketPage** (`src/pages/MarketPage.tsx`, 신규)
- 플러그인 카드 + trust 뱃지 + 검색/필터
- Install 토글 버튼
- 내장 플러그인(obsidian/confluence/github) "Built-in" 뱃지 + "Included" 레이블 (Uninstall 버튼 없음)

**ChatDrawer 멀티 세션 아키텍처**
- 세션 목록 사이드바 + 세션별 대화 히스토리
- 프로바이더 선택: Claude / Gemini
- 모델 드롭다운: Sonnet / Opus / Haiku (Claude), Gemini Pro / Flash (Gemini)

**AppShell 네비게이션**
- Dashboard 항목 추가

#### 4. 플러그인별 프로젝트 추가 폼

**Obsidian**
- `@tauri-apps/plugin-dialog`로 폴더 직접 선택 (Browse 버튼)
- `path`에서 `name` 자동 추출

**Confluence**
- 필드: Base URL, Space Key, API Token, Email

**GitHub**
- 필드: owner/repo, PAT
- 소스 체크박스: Issues / Wiki / Discussions

인프라:
- `tauri-plugin-dialog` Cargo.toml 추가
- `src-tauri/capabilities/default.json` 신규 생성

#### 5. Rust `market_list_installed` 커맨드 수정

내장 플러그인 3개(obsidian / confluence / github)를 항상 반환하도록 수정:
- `builtin: true`, `installed: true` 필드 추가
- UI에서 Built-in 뱃지 표시, Uninstall 버튼 숨김 처리

### 커밋 이력

| 커밋 | 내용 |
|------|------|
| `94a70e6` | feat(core/desktop/mcp): SearchEngine FTS5+RRF, MCP +8 tests, Desktop IPC wiring |
| `0017b80` | feat(desktop): Tailwind CSS v4, index.html, devUrl + beforeDevCommand |
| `ceb831b` | feat(desktop): Dashboard, Projects form, Workspace, Market, Chat sessions |
| `566efd6` | feat(desktop): plugin-aware project form, folder picker, MarketPage fix |
| `d1bc4c4` | feat(desktop/market): show built-in plugins, Built-in badge, Included label |

### 최종 테스트 현황

- Rust: **195 tests passed**, 0 failures
- MCP: 38 → **46 tests**
- SearchEngine: 3 → **7 tests**

### 교훈

- **Vite + Tauri dev 환경**: `index.html`이 프로젝트 루트에 없으면 Vite가 404를 반환한다. `tauri.conf.json`의 `beforeDevCommand`와 `devUrl`을 명시적으로 설정해야 `npm run tauri dev` 한 번으로 Vite와 Tauri가 함께 기동된다.
- **Tailwind CSS v4 플러그인 방식**: v4부터 `tailwind.config.ts` 없이 `@tailwindcss/vite` 플러그인으로 설정한다. `src/index.css`에 `@import "tailwindcss"` 한 줄이면 충분하다.
- **invoke 반환 shape 방어**: Tauri `invoke` 결과는 Rust 타입에 따라 배열 또는 래핑 객체로 직렬화될 수 있다. 프론트엔드에서 `Array.isArray()` 분기로 양쪽을 처리하거나, Rust 커맨드가 항상 배열을 직접 반환하도록 통일하는 것이 안전하다.
- **내장 플러그인 표시 일관성**: 마켓 페이지에서 빌트인 플러그인을 숨기면 사용자가 설치 상태를 파악하기 어렵다. "Built-in" 뱃지와 "Included" 레이블로 존재를 명시하되 Uninstall을 비활성화하는 패턴이 UX상 더 명확하다.

---

## 세션 6 — 2026-04-04

### 작업 요약

**요구사항 4가지 구현 (UI 개선)**

1. **전체 UI 한글화**
   - AppShell 사이드바, SearchPage, DashboardPage, ChatDrawer, WorkspacePage, MarketPage, ProjectsPage 모두 한글로 전환
   - 날짜 포맷 `ko-KR` 로케일로 변경

2. **검색 화면 문서 프리뷰**
   - 좌우 분할 패널 구조: 좌측 결과 목록 (`w-80`) + 우측 마크다운 프리뷰
   - 결과 선택 시 프리뷰 패널 표시, X 버튼으로 닫기

3. **react-markdown 도입**
   - 검색 프리뷰: snippet / content 마크다운 렌더링
   - ChatDrawer 어시스턴트 메시지: 마크다운 렌더링
   - `prose-invert` 다크 테마 스타일 적용

4. **설정 페이지 신규 (`/settings`)**
   - 앱 / DB / MCP / CLI / 에이전트 상태 배지 (ok / warn / error / unknown)
   - DB 연결 테스트, MCP 포트(7700) 연결 테스트 버튼
   - `get_system_status` Tauri 커맨드 추가 (DB 경로, CLI 존재 여부, MCP 포트 확인)
   - 개발 도구 버튼 (DB 재인덱싱, 검색 엔진 상태, 플러그인 로그)

**Autopilot + Team Agent 5트랙 병렬 구현 (TDD)**

| 트랙 | 내용 | 테스트 수 |
|------|------|----------|
| Track A | Desktop IPC 커맨드 (add_project, toggle_project_status, list_workspace_documents, create_workspace_document) | 5 |
| Track B | doxus-plugin-sdk (DocSource trait, RawDocument, PluginError) + doxus-plugin-obsidian 실구현 | 15 |
| Track C | EmbeddingProvider trait + OllamaEmbedder (Ollama HTTP API) + MockEmbedder | 16 |
| Track D | GitHub 플러그인 workspace 등록 + CLI clap 파싱 테스트 | 15 + 11 |
| Track E | Keychain 추상화 (SecretStore trait, SystemKeychain, MemorySecretStore) | 6 |

### 문제 및 해결

- **keyring feature 이름 오류**: `linux-native-sync-secret-service` → `sync-secret-service` (Track E 에이전트가 잘못된 feature 명 사용, 수동 수정)
- **WorkspacePage/MarketPage default import**: 이전 세션 이슈와 동일한 패턴, 한글화 과정에서 유지
- **SearchPage Hit 타입**: `useSearchStore`의 `SearchHit`과 `SearchPage` 내부 `Hit` 인터페이스를 분리하여 타입 안전성 확보

### 설계 결정

- **Keychain**: `keyring` v3 크레이트 선택 — macOS apple-native + Linux sync-secret-service 동시 지원
- **검색 프리뷰**: 별도 문서 fetch 없이 snippet으로 기본 프리뷰 제공. `content` 필드가 있을 경우 전체 마크다운 렌더링으로 전환
- **설정 페이지 DevButton**: 미구현 커맨드는 "미구현" 메시지를 2초 후 초기화하는 UX 적용

### 결과

- 전체 테스트: **235개 통과, 0 실패**
- 커밋: `f85253f` (UI 개선), `672ce00` (Track A/B/C), `0263e92` (Track D/E)
- workspace 멤버: `crates/plugins/github` 추가됨

## 관련 문서

- [[doxus 아키텍처 설계]]
- [[obsidian-nexus]]
