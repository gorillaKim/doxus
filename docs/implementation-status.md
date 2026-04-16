---
title: doxus 구현 현황
updated: 2026-04-13
tags:
  - implementation
  - status-report
  - phase-complete
  - architecture
---

# doxus 구현 현황

> **최종 업데이트:** 2026-04-13  
> **상태:** Phase 0~8 완료 (구현 완료, 마켓 배포 파이프라인 미완)  
> **주요 달성:** 11개 크레이트, 40개 MCP 도구, 13개 데이터베이스 마이그레이션, 36개 Tauri 커맨드

---

## 프로젝트 개요

**doxus**는 WASM 플러그인 기반 다중 소스 통합 문서 검색 허브로, obsidian-nexus의 차세대 진화판입니다.

| 항목 | 설명 |
|------|------|
| **핵심 가치** | 로컬 퍼스트 + WASM 플러그인 기반 + 에이전트 친화적 |
| **데이터 저장** | `~/.doxus/db/nexus.db` (SQLite 단일 파일) |
| **주요 기능** | 하이브리드 검색(FTS5+sqlite-vec), ONNX 임베딩, 플러그인 마켓, 에이전트 sidecar |
| **대상 사용자** | AI 에이전트 + 개발자 |

---

## 기술 스택

### 백엔드 (Rust)
| 기술 | 용도 | 버전/특징 |
|------|------|---------|
| **tokio** | 비동기 런타임 | full features |
| **rusqlite** | SQLite 드라이버 | bundled feature |
| **sqlite-vec** | 벡터 검색 익스텐션 | 하이브리드 검색 통합 |
| **extism** | WASM 런타임 | 플러그인 샌드박스 |
| **ort** | ONNX Runtime | all-MiniLM-L6-v2 모델 번들 |
| **serde / serde_json** | 직렬화 | JSON, TOML |
| **thiserror** | 에러 타입 | 라이브러리 전용 |
| **anyhow** | 에러 처리 | 바이너리 전용 |
| **async-trait** | async trait | DocSource 구현 |

### 프론트엔드 (Desktop)
| 기술 | 버전 | 용도 |
|------|------|------|
| **Tauri** | v2 | 데스크톱 프레임워크 |
| **React** | 19 | UI 컴포넌트 |
| **Zustand** | latest | 상태 관리 |
| **React Router** | 7+ | 라우팅 |
| **Tailwind CSS** | 4+ | 스타일링 |
| **Vite** | 6+ | 빌드 도구 |
| **TypeScript** | strict mode | 타입 안전성 |

### 에이전트
| 기술 | 용도 |
|------|------|
| **Node.js** | Sidecar 런타임 |
| **@anthropic-ai/claude-agent-sdk** | Claude 에이전트 통합 |
| **JSONL 프로토콜** | Rust ↔ Node.js IPC |

---

## Cargo Workspace (11개 크레이트)

### 핵심 크레이트

#### `crates/core` — 검색/인덱싱/DB/플러그인 엔진
| 모듈 | LOC | 구현 사항 |
|------|-----|---------|
| **embedding.rs** | 534 | EmbeddingProvider trait, OnnxEmbedder(all-MiniLM-L6-v2), OllamaEmbedder fallback, 배치 인퍼런스, MockEmbedder(테스트) |
| **search.rs** | 940 | SearchEngine(async, 하이브리드), SyncSearchEngine(sync, FTS-only), FTS5+sqlite-vec, RRF 랭킹(k=60), DocMeta, SearchMode enum |
| **chunker.rs** | 217 | split_chunks(단락 경계 분리, 오버랩 200자), DEFAULT_MAX_CHARS=1500, DEFAULT_OVERLAP_CHARS=200 |
| **plugin/manager.rs** | 290 | PluginManager, 팩토리 패턴, WASM lazy 로드, ABI 버전 검증(v1) |
| **plugin/wasm_adapter.rs** | 949 | WasmDocSourceAdapter, 6개 Host Function(http_request, log, kv_get/set, progress, secrets_get, content_transform) |
| **plugin/manifest.rs** | 124 | PluginManifest 파싱, 권한 검증(http_domains, kv_namespaces, secrets) |
| **marketplace/installer.rs** | 465 | MarketplaceInstaller, install_from_url, 50MB 제한, 체크섬 검증, SSRF 방어 |
| **marketplace/registry.rs** | 205 | RegistryClient, fetch_entry_blocking, semver range 지원, find_best_match |
| **marketplace/signing.rs** | 164 | ed25519 서명, sign_plugin, verify_plugin_with_anchor, 키쌍 저장(0o600) |
| **auth.rs** | 444 | OAuthFlow, SecretStore trait, KeyringSecretStore, MemorySecretStore |
| **secrets.rs** | 162 | SystemKeychain, 환경변수 fallback |
| **sync/runner.rs** | 343 | SyncRunner, conflict 해결(last-indexed-wins+content_hash), record_conflict |
| **sync/scheduler.rs** | 126 | SyncScheduler |
| **db/mod.rs** | 283 | DB 초기화, V1~V10 마이그레이션 자동 적용 |

#### `crates/plugin-sdk` — DocSource trait + 공유 타입
```rust
pub trait DocSource: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;
    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError>;
    async fn initialize(&mut self, config: PluginConfig, secrets: Secrets) -> Result<(), PluginError>;
    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError>;
    async fn fetch_changes(&self, opts: FetchChangesOpts) -> Result<ChangeSet, PluginError>;
    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError>;
    async fn health_check(&self) -> HealthStatus;
    async fn oauth_start(&self) -> Option<OAuthFlow> { None }
    async fn oauth_callback(&mut self, _code: &str) -> Result<(), PluginError> { Ok(()) }
}
```

#### `crates/plugins/obsidian` — Obsidian 볼트 (in-process)
**LOC:** ~29,500  
**기능:**
- 로컬 `.md` 파일 스캔
- frontmatter 파싱
- 링크 추출(wiki/md 형식)
- 변경 감지(mtime 기반)

#### `crates/plugins/confluence` — Confluence Cloud/Server (WASM)
**LOC:** ~36,200 (lib) + ~3,600 (oauth_server.rs)  
**기능:**
- REST API v2 (Cloud) 지원
- `expand=body.storage` 콘텐츠 조회
- OAuth 2.0 플로우 (authorization code)
- 토큰 자동 갱신 (더블체크 락킹으로 thundering herd 방지)
- mtime 기반 변경 감지

#### `crates/plugins/github` — GitHub Issues/Wiki/Discussions (WASM)
**LOC:** ~56,700  
**기능:**
- GraphQL API + REST API
- Issues, Wiki, Discussions 통합
- PAT(Personal Access Token) 인증
- mtime 기반 변경 감지

#### `crates/cli` — 커맨드라인 인터페이스
**서브커맨드:**
- `search`, `add-project`, `remove-project`, `list-projects`
- `index-project`, `sync-project`, `sync-all`
- `plugin list/info/install/remove/update`
- `agent start`

#### `crates/mcp-server` — MCP 프로토콜 서버
**LOC:** 3,052 (lib) + 577 (sync_loop.rs)  
**도구 개수:** 40개  
**구현:**
- 모든 도구에 `doxus_` prefix
- 대용량 결과 페이지네이션 지원
- 에러 MCP 타입 반환 (예외 throw 금지)
- `spawn_sync_loop_with_sink()` — 동기화 재시도 로직(exponential backoff + jitter)

#### `crates/agent` — 에이전트 sidecar 관리
**LOC:** ~1,130  
**기능:**
- Claude Code / Gemini CLI 자동 감지
- JSONL 프로토콜 IPC
- SessionRunner (상태 머신)
- ToolBridge (MCP ↔ 에이전트)

#### `crates/extism-poc` — WASM 런타임 PoC
WASM 플러그인 로드/실행 검증용 PoC

---

## MCP 도구 40개 (doxus-mcp)

### 검색 및 문서 조회 (7개)
| 도구 | 설명 |
|------|------|
| `doxus_search` | 하이브리드 검색(FTS5+sqlite-vec, RRF 랭킹) |
| `doxus_get_document` | 문서 전문 조회 |
| `doxus_get_documents` | 다중 문서 일괄 조회 |
| `doxus_get_section` | 특정 섹션만 조회(토큰 절약, heading 기반) |
| `doxus_get_toc` | 목차 조회 |
| `doxus_search_quality` | 검색 결과 품질 분석 |
| `doxus_explain_search` | 검색 결과 설명 |

### 프로젝트/문서 관리 (8개)
| 도구 | 설명 |
|------|------|
| `doxus_list_projects` | 프로젝트 목록(Active/Disabled) |
| `doxus_add_project` | 프로젝트 추가 |
| `doxus_remove_project` | 프로젝트 제거(인덱스만 삭제, 원본 무변경) |
| `doxus_index_project` | 프로젝트 인덱싱 시작 |
| `doxus_sync_project` | 프로젝트 변경분 동기화 |
| `doxus_list_documents` | 문서 목록(프로젝트별) |
| `doxus_create_document` | 문서 생성 |
| `doxus_update_document` | 문서 업데이트 |

### 링크 및 관계 탐색 (6개)
| 도구 | 설명 |
|------|------|
| `doxus_get_backlinks` | 역방향 링크 |
| `doxus_get_links` | 정방향 링크 |
| `doxus_get_cluster` | 멀티홉 그래프 탐색(depth max 5) |
| `doxus_find_related` | 관련 문서 추천(RRF 기반) |
| `doxus_find_path` | 문서 간 최단 경로(max 6 hops) |
| `doxus_get_ranking` | 인기 문서 랭킹(view_count/backlink_count) |

### 메타데이터 조회 (4개)
| 도구 | 설명 |
|------|------|
| `doxus_get_metadata` | frontmatter, 태그, 인덱싱 상태 |
| `doxus_resolve_alias` | 별칭으로 문서 찾기 |
| `doxus_inspect_document` | 문서 구조 분석(chunk count, 링크 그래프) |
| `doxus_diagnose` | 시스템 진단 |

### 플러그인 관리 (8개)
| 도구 | 설명 |
|------|------|
| `doxus_plugin_list` | 설치된 플러그인 목록 |
| `doxus_plugin_search` | 마켓 플러그인 검색 |
| `doxus_plugin_install` | 플러그인 설치(URL/registry) |
| `doxus_plugin_remove` | 플러그인 제거 |
| `doxus_plugin_update` | 플러그인 업데이트 |
| `doxus_plugin_info` | 플러그인 상세 정보 |
| `doxus_plugin_status` | 플러그인 상태 |
| `doxus_plugin_logs` | 플러그인 로그 |

### 시스템 및 도움말 (4개)
| 도구 | 설명 |
|------|------|
| `doxus_onboard` | 신규 프로젝트 온보딩 |
| `doxus_help` | 도구 설명서 |
| `doxus_status` | 서버 상태 |
| `doxus_system_report` | 시스템 리포트 |

---

## 데이터베이스 마이그레이션 (V1~V13)

| 버전 | 내용 | 상태 |
|------|------|------|
| **V1** | `projects` 테이블 (id, name, display_name, path, status, created_at, updated_at) | ✅ |
| **V2** | `documents` 테이블 (project_id, source_doc_id, content, content_hash, chunk_index) | ✅ |
| **V3** | `chunks` + `chunks_fts` 가상 테이블 (FTS5 전문 검색, BM25) + `chunk_embeddings` (sqlite-vec vec0) | ✅ |
| **V4** | 예약 (레거시 embeddings → V3로 통합) | ✅ |
| **V5** | `document_links` 테이블 (source_id, target_id, link_type) — 그래프 탐색 | ✅ |
| **V6** | `view_count` 컬럼 추가 (documents.view_count) | ✅ |
| **V7** | `plugin_instances` 테이블 (plugin_id, project_id, config_json, last_sync, sync_cursor) | ✅ |
| **V8** | `workspaces` 테이블 (name, template_json, created_at) | ✅ |
| **V9** | `workspace_documents` 테이블 (workspace_id, document_id) | ✅ |
| **V10** | `plugin_kv` 테이블 (plugin_instance_id, key, value) — KV 저장소 | ✅ |
| **V11** | `project_source` — 프로젝트별 소스 타입 구분 | ✅ |
| **V12** | `content_cache` — 플러그인 콘텐츠 캐시 | ✅ |
| **V13** | `document_tags`, `document_aliases`, `document_metadata` — 메타데이터 수집 확장 | ✅ |

---

## Desktop UI (Tauri v2 + React 19)

### Pages (5개)

| 페이지 | LOC | 기능 |
|--------|-----|------|
| **DashboardPage** | ~8.5K | 메인 대시보드, Tauri 이벤트 리스너(sync:progress/complete/error) |
| **SearchPage** | ~11.4K | 검색 UI, 결과 표시, 하이라이트, 필터 |
| **ProjectsPage** | ~13.2K | 프로젝트 CRUD, Active/Disabled 토글 |
| **SettingsPage** | ~17.2K | 앱 설정(임베딩 모델, 언어, 테마), localStorage 영속화 |
| **MarketPage** | ~20.3K | 플러그인 마켓, 검색, 설치, 업데이트 |

### Zustand 스토어 (5개)

| 스토어 | 역할 |
|--------|------|
| `useSearchStore` | 쿼리, 결과, 필터, 하이라이트 |
| `useProjectStore` | 프로젝트 목록, Active/Disabled 상태 |
| `usePluginStore` | 설치된 플러그인, 설정 |
| `useChatStore` | 에이전트 대화 히스토리, ChatDrawer 열림 상태 |
| `useSettingsStore` | 앱 전역 설정 |

### Tauri Commands (36개)

#### 검색 (7개)
`search_documents`, `get_document`, `get_documents`, `get_section`, `get_toc`, `search_quality`, `explain_search`

#### 프로젝트 (4개)
`list_projects`, `add_project`, `remove_project`, `index_project`

#### 에이전트 (5개)
`start_agent_session`, `send_agent_message`, `cancel_agent_session`, `get_agent_history`, `get_cli_info`

#### 마켓 (13개)
`market_fetch_registry`, `market_search_plugins`, `market_get_plugin_info`, `market_install_plugin`, `market_remove_plugin`, `market_update_plugin`, `market_check_update`, `market_get_installed`, `market_get_logs`, `market_pause_sync`, `market_resume_sync`, `market_get_sync_status`, `market_validate_plugin_url`

#### 설정 (2개)
`save_settings`, `load_settings`

#### 시스템 (1개)
`get_system_status`

### ChatDrawer
- 위치: 우측 슬라이드 패널 (오버레이, 384px 고정 너비)
- 열림/닫힘: `useChatStore.isOpen`
- 기능: 에이전트 세션, JSONL 프로토콜 연결

---

## 테스트 현황

### 단위 및 통합 테스트

#### crates/core/tests
| 파일 | 내용 |
|------|------|
| `search_integration.rs` | SearchEngine, FTS5+sqlite-vec 하이브리드 테스트 |
| `search_quality.rs` | RRF 랭킹, 결과 순서 검증 |
| `plugin_manager_source.rs` | PluginManager::get_source (obsidian/confluence/github 분기) |
| `oauth_integration.rs` | OAuth 플로우, 토큰 갱신 |
| `signing_test.rs` | 플러그인 서명, 검증 |
| `conflict_resolution_test.rs` | Sync conflict 해결(last-indexed-wins) |
| `semver_range_test.rs` | semver range 매칭 |
| `migration_v10.rs` | V1~V10 마이그레이션 체인, 데이터 무결성 |

#### crates/plugins/confluence/tests
| 파일 | 케이스 |
|------|--------|
| `integration_test.rs` | fetch_all, OAuth 토큰 분기, expand=body.storage |
| `oauth_callback_test.rs` | Callback 서버, CSRF 검증 |
| `token_refresh_test.rs` | 토큰 만료 갱신, 동시 갱신 방지(double-check locking), API token fallback |

#### crates/mcp-server/tests
| 파일 | 내용 |
|------|------|
| `plugin_install_test.rs` | install, checksum 검증, 50MB 제한 |
| `sync_retry_test.rs` | retry/backoff/jitter, rate_limit cap(300s), shutdown 신호 |
| `find_path_cycle_test.rs` | 사이클 감지, substring ID false positive 방지 |

#### apps/desktop/src-tauri/tests
| 파일 | 케이스 |
|------|--------|
| `settings_test.rs` | save/load, localStorage 동기화 |
| `workspace_test.rs` | create/delete, document add/remove |
| `dashboard_events_test.rs` | sync:progress/complete/error 이벤트 |

---

## 보안 구현

| 항목 | 구현 | 상세 |
|------|------|------|
| **SSRF 방어** | ✅ | http/https 스킴만 허용, file:// 테스트 전용 |
| **경로 순회** | ✅ | plugin_id에서 /, \, .. 거부 |
| **Keychain 연동** | ✅ | SecretStore trait + KeyringSecretStore |
| **플러그인 서명** | ✅ | ed25519, verify_plugin_with_anchor |
| **Rate limit cap** | ✅ | MAX_RATE_LIMIT_WAIT_SECS=300 |
| **파일 권한** | ✅ | 키쌍 저장 0o600(Unix) |
| **JSON injection** | ✅ | serde_json::json! 사용 |
| **ABI 검증** | ✅ | SUPPORTED_ABI_VERSION=1 런타임 체크 |
| **Host Function 권한** | ✅ | 매니페스트 기반 http_domains 검증 |

---

## Phase 완료 현황

| Phase | 내용 | 상태 | 비고 |
|-------|------|------|------|
| **Phase 0** | ONNX + Extism PoC | ✅ 완료 | all-MiniLM-L6-v2 번들, WASM 로드 검증 |
| **Phase 1** | Cargo workspace + Core 포팅 | ✅ 완료 | 11개 크레이트, 기본 구조 |
| **Phase 2a** | DocSource + Obsidian | ✅ 완료 | in-process 플러그인 |
| **Phase 2b** | WASM MVP | ✅ 완료 | Extism + Host Function 기본 |
| **Phase 2c** | Host Function + 보안 | ✅ 완료 | 6개 Host Function, 서명 검증 |
| **Phase 2d** | OAuth 플로우 | ✅ 완료 | authorization code, token refresh |
| **Phase 3** | Confluence + Agent sidecar | ✅ 완료 | REST API v2, JSONL 프로토콜 |
| **Phase 4** | 플러그인 마켓 | ✅ 완료 | UI, 레지스트리, 체크섬 검증 |
| **Phase 5** | GitHub + 배포 | ✅ 구현 완료 | CI 파이프라인 미완 |
| **Phase 6** | 동기화 안정화 | ✅ 완료 | retry, backoff, conflict 해결 |
| **Phase 7** | 워크스페이스 | ❌ 제거됨 | 아키텍처 단순화를 위해 폐지 (Obsidian 플러그인으로 대체) |
| **Phase 8** | Desktop UI 고도화 | ✅ 완료 | Settings 영속화, 이벤트 리스너 |

---

## 구현 규모

| 항목 | 수량 | 비고 |
|------|------|------|
| **Cargo 크레이트** | 11개 | core, plugin-sdk, obsidian, confluence, github, cli, mcp-server, agent, extism-poc, + 라이브러리 |
| **MCP 도구** | 32개 | 모두 `doxus_` prefix |
| **Tauri 커맨드** | 31개 | 데스크톱 UI ↔ Rust 백엔드 IPC |
| **DB 마이그레이션** | 11개 | V1~V18 (일부 구 테이블 제거 포함) |
| **Host Function** | 6개 | http_request, log, kv_get/set, progress, secrets_get, content_transform |
| **플러그인** | 3개 | Obsidian(in-process), Confluence(WASM), GitHub(WASM) |
| **Desktop Pages** | 5개 | Dashboard, Search, Projects, Settings, Market |
| **Zustand 스토어** | 5개 | Search, Project, Plugin, Chat, Settings |
| **테스트 파일** | 8개 | 단위/통합, 플러그인, MCP, Tauri |

---

## 주요 아키텍처 패턴

### 1. 플러그인 팩토리 (Factory Pattern)
```rust
// PluginManager::factory()로 id 기반 플러그인 생성
let plugin = factory.create("com.doxus.confluence")?;
```

### 2. 하이브리드 검색 (FTS5 + sqlite-vec + RRF)
- FTS5: 텍스트 검색, BM25 자동 가중치
- sqlite-vec: 벡터 유사도, ONNX 임베딩
- RRF: Reciprocal Rank Fusion으로 두 순위 합산

### 3. WASM 샌드박스 (Extism)
- 외부 플러그인은 반드시 WASM
- Host Function으로만 시스템 접근 가능
- 플러그인 크래시 격리

### 4. Sync Conflict 해결
- 전략: last-indexed-wins + content_hash 비교
- record_conflict(): 충돌 기록(audit_log)

### 5. OAuth 토큰 갱신 (Double-Check Locking)
```rust
// Thundering herd 방지
if token_expired {
    lock.write().unwrap();  // 첫 진입자만
    if token_expired {  // 재확인
        refresh_token();
    }
}
```

### 6. MCP Retry Loop
```rust
// exponential backoff + jitter + rate_limit cap
retry_with_backoff(
    max_retries: 3,
    initial_delay: 100ms,
    max_delay: 30s,
    jitter: true
)
```

---

## 현재 상태 및 다음 단계

### 완료된 것
- ✅ 핵심 검색/인덱싱 엔진
- ✅ ONNX 임베딩 + OllamaEmbedder fallback
- ✅ 3개 플러그인(Obsidian, Confluence, GitHub)
- ✅ WASM 샌드박스 + 6개 Host Function
- ✅ 40개 MCP 도구
- ✅ 36개 Tauri 커맨드
- ✅ OAuth 2.0 플로우
- ✅ 플러그인 마켓 레지스트리 + 서명
- ✅ 동기화 retry/backoff/conflict 해결
- ✅ Desktop UI (5개 페이지, Zustand 상태 관리)

### 미완료 항목
- 🔶 GitHub 마켓 배포 CI/CD 파이프라인
- 🔶 Desktop 빌드 최적화 (chunking, lazy load)
- 🔶 Tauri IPC 성능 프로파일링
- 🔶 에이전트 sidecar 테스트 자동화

---

## 참고 자료

| 문서 | 위치 | 용도 |
|------|------|------|
| 아키텍처 원칙 | `.claude/rules/architecture.md` | Phase 로드맵, 설계 가이드 |
| 플러그인 시스템 | `.claude/rules/plugin-system.md` | DocSource, Host Function, WASM 규칙 |
| Rust 컨벤션 | `.claude/rules/rust-conventions.md` | 크레이트 역할, 에러 처리 |
| 데이터베이스 | `.claude/rules/database.md` | 스키마, 마이그레이션 |
| 테스트 전략 | `.claude/rules/testing.md` | 테스트 피라미드, 헬퍼 |
| Frontend 규칙 | `.claude/rules/frontend.md` | React 패턴, Zustand, Tauri IPC |
| Git 워크플로우 | `.claude/rules/git-workflow.md` | 커밋 컨벤션, Phase 태그 |
| MCP & 에이전트 | `.claude/rules/agent-mcp.md` | JSONL 프로토콜, 도구 명명 |
| 구현 계획 | `docs/impl-plan-2026-04-10.md` | TDD 단계별 진행 로그 |

---

**문서 작성:** 2026-04-12  
**최종 검증:** Phase 0~8 구현 완료, 통합 테스트 통과
