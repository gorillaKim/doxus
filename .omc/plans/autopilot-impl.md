# doxus Phase 4~8 Implementation Plan (TDD)

## Context

Phase 0~3 완료 (68 tests passing). 남은 Phase 4~8을 TDD 방식으로 구현한다.
각 Phase는 Red → Green → Refactor 순서를 따른다.

---

## Phase 4: Plugin Marketplace

### 4a — Code Signing & Registry Client (`crates/core/src/marketplace/`)

**TDD 순서:**
1. `SignedPlugin` 구조체 + ED25519 서명 검증 테스트 (실패)
2. `RegistryClient` — GitHub Releases JSON 파싱 테스트 (실패)
3. `PluginInstaller` — 다운로드 + 검증 + `~/.doxus/plugins/` 저장 테스트 (실패)
4. 구현 → 테스트 통과

**핵심 타입:**
```rust
pub struct SignedPlugin {
    pub manifest: PluginManifest,
    pub wasm_bytes: Vec<u8>,
    pub signature: [u8; 64],
    pub public_key: [u8; 32],
}
pub struct RegistryEntry {
    pub plugin_id: String,
    pub version: String,
    pub download_url: String,
    pub checksum_sha256: String,
    pub public_key_hex: String,
}
pub struct RegistryClient { pub registry_url: String }
```

**의존성 추가:**
- `ed25519-dalek = "2"` (서명 검증)
- `sha2 = "0.10"` (체크섬)

### 4b — PluginManager CRUD + MCP Tools

**TDD 순서:**
1. `PluginManager::install/uninstall/list/update` 테스트
2. `docnx_market_search`, `docnx_market_install`, `docnx_market_list_installed` MCP 도구 테스트
3. 구현 → 테스트 통과

---

## Phase 5: GitHub Plugin

### 5a — GitHub Issues/Wiki/Discussions WASM Plugin (`crates/plugins/github/`)

**TDD 순서 (wiremock):**
1. GitHub REST API `/repos/{owner}/{repo}/issues` 파싱 테스트
2. `fetch_document` — 이슈 단건 조회 테스트
3. `fetch_all` — 페이지네이션 테스트
4. 인증 헤더 (`Authorization: Bearer {token}`) 테스트
5. 구현 → 테스트 통과

**핵심 구조:**
```rust
pub struct GitHubPlugin {
    config: GitHubConfig,
    client: reqwest::Client,
}
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub include_issues: bool,
    pub include_discussions: bool,
}
```

---

## Phase 6: Sync Scheduler + Observability

### 6a — Background Sync Scheduler (`crates/core/src/sync/`)

**TDD 순서:**
1. `SyncJob` 스케줄 계산 테스트 (interval, next_run_at)
2. `SyncScheduler::tick()` — 만료된 job 반환 테스트
3. `SyncScheduler::register/cancel` 테스트
4. 구현

**핵심 타입:**
```rust
pub struct SyncJob {
    pub source_instance_id: i64,
    pub interval_secs: u64,
    pub next_run_at: std::time::Instant,
}
pub struct SyncScheduler {
    jobs: Vec<SyncJob>,
}
impl SyncScheduler {
    pub fn tick(&mut self) -> Vec<i64>; // 실행할 instance ids
    pub fn register(&mut self, job: SyncJob);
    pub fn cancel(&mut self, source_instance_id: i64);
}
```

### 6b — Observability (tracing + structured logs)

**추가 사항:**
- `tracing` + `tracing-subscriber` 크레이트
- `audit_log` 테이블 write 헬퍼
- `IndexProgress` 이벤트 (Tauri emit 연결)

---

## Phase 7: Workspace + Template Management

### 7a — Workspace (`crates/core/src/workspace/`)

**DB V8 마이그레이션:**
```sql
CREATE TABLE IF NOT EXISTS workspaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    project_ids TEXT NOT NULL DEFAULT '[]', -- JSON array of project ids
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS workspace_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);
```

**TDD 순서:**
1. `WorkspaceRepo::create/get/list/delete` 테스트 (TestDb)
2. `WorkspaceRepo::add_project/remove_project` 테스트
3. `TemplateRepo::create/apply` 테스트
4. 구현

---

## Phase 8: Desktop UI 고도화

### 8a — Tauri IPC Commands for Phase 4~7 features

새 IPC 커맨드:
- `market_search(query: String) -> Vec<RegistryEntry>`
- `market_install(plugin_id: String) -> Result<(), String>`
- `list_workspaces() -> Vec<WorkspaceSummary>`
- `create_workspace(req: CreateWorkspaceReq) -> WorkspaceSummary`
- `get_sync_status() -> Vec<SyncJobStatus>`

**TDD**: 각 커맨드에 대한 단위 테스트 + mock AppState

### 8b — React UI Stubs

파일만 생성 (전체 UI는 scope out):
- `apps/desktop/src/pages/MarketPage.tsx` — 플러그인 마켓 목록 UI stub
- `apps/desktop/src/pages/WorkspacePage.tsx` — 워크스페이스 관리 stub
- `apps/desktop/src/stores/useMarketStore.ts`
- `apps/desktop/src/stores/useWorkspaceStore.ts`

---

## 실행 전략

### Team 병렬화

| 팀 | 담당 Phase | 병렬 여부 |
|----|-----------|----------|
| Team A | Phase 4a (Code Signing + Registry) | 독립 |
| Team B | Phase 4b (PluginManager + MCP) | 4a 완료 후 |
| Team C | Phase 5 (GitHub Plugin) | Phase 4와 병렬 |
| Team D | Phase 6 (Sync + Observability) | Phase 4~5와 병렬 |
| Team E | Phase 7 (Workspace + Templates) | Phase 6과 병렬 |
| Team F | Phase 8 (Desktop IPC + UI stubs) | Phase 7 완료 후 |

### TDD 규칙

1. 각 기능마다 **테스트 먼저** 작성 → `cargo test` 실패 확인
2. 최소 구현으로 테스트 통과
3. `cargo clippy -- -D warnings` 통과 필수
4. `cargo fmt` 적용
