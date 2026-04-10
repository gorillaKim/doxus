---
title: "doxus 미구현 항목 트래킹"
aliases:
  - doxus-todo
  - doxus 할일
  - 미구현 항목
tags:
  - doxus
  - todo
  - roadmap
  - unimplemented
created: 2026-04-10
updated: 2026-04-10
---

<!-- docsmith: auto-generated 2026-04-10 -->

## 개요

**생성일:** 2026-04-10
**범위:** 전체 모노레포 분석 (Rust crates + React/TypeScript 프론트엔드 + DB)

doxus는 **Phase 1 수준의 인프라**가 갖춰진 상태다. Phase 2-8의 대부분 기능은 아키텍처 설계는 완료됐으나 코드 구현은 미시작이다.

### Phase 상태 요약

| Phase | 내용 | 상태 |
|-------|------|------|
| 0 | ONNX 임베딩 PoC | **진행 중** — 모델 파일 누락 |
| 0 | Extism PoC | **진행 중** — Host Function 미완성 |
| 1 | 모노레포 스캐폴드, Core 포팅 | **완료** |
| 2a | Obsidian 플러그인 (in-process) | **부분 완료** — frontmatter 파싱 stub |
| 2b | WASM MVP | **프레임워크만** — Host Function 미완성 |
| 2c | Host Function 전체 + 보안 | **미시작** |
| 2d | OAuth 인증 플로우 | **미시작** |
| 3 | Confluence 플러그인 + Agent sidecar | **프레임워크만** |
| 4 | 플러그인 마켓 | **미시작** |
| 5 | GitHub 플러그인 | **stub** |
| 6 | 동기화 스케줄러 | **미시작** |
| 7 | 워크스페이스 + 템플릿 | **stub** |
| 8 | Desktop UI 고도화 | **백엔드 완료 대기 중** |

---

## Phase 0-1 블로커 (즉시 해결 필요)

### ONNX 임베딩 엔진 (`crates/core/src/embedding.rs`)

**구현된 것:**
- `EmbeddingProvider` trait 정의
- `OnnxEmbedder` struct (모델 로딩 포함)
- 코사인 유사도 함수

**미구현:**
| 항목 | 이유 |
|------|------|
| ONNX 모델 파일 번들 | `all-MiniLM-L6-v2.onnx` 미포함 — **Phase 0 블로커** |
| `embed()` 구현 | 텐서 처리 로직 미완성 |
| `OllamaEmbedder` fallback | 선언만 존재, 구현 없음 |
| 모델 캐싱 | 영속 임베딩 캐시 없음 |

### MCP 서버 임베더 연결 (`crates/mcp-server/src/main.rs:33`)

```rust
// TODO: Pass embedder to McpServer once it accepts an EmbeddingProvider.
```

**영향:** 임베더가 연결되기 전까지 벡터 유사도 검색은 FTS5만으로 폴백.

### Phase 0-1 작업 우선순위

| 항목 | 예상 LOC | 중요 이유 |
|------|----------|-----------|
| ONNX 모델 파일 번들 | 10 | 임베딩 프로바이더 비활성 |
| `OnnxEmbedder::embed()` 구현 | 150 | 벡터 검색 불가 |
| `McpServer`에 임베더 전달 | 50 | 벡터 유사도 비활성화 |
| IndexEngine을 API에 연결 | 200 | MCP 인덱싱 미작동 |
| 백그라운드 잡 스케줄러 추가 | 300 | 동기화/재인덱싱이 수동 전용 |

---

## Phase 2-3 핵심 기능

### MCP 서버 도구 — 39개 선언, ~20개 부분 stub

**파일:** `crates/mcp-server/src/lib.rs` (2,454줄)

**완전 구현된 도구:**
`doxus_status`, `doxus_list_projects`, `doxus_add_project`, `doxus_remove_project`, `doxus_search`, `doxus_get_document`, `doxus_get_section`, `doxus_get_metadata`, `doxus_get_toc`, `doxus_get_ranking`, `doxus_get_backlinks`, `doxus_get_links`, `doxus_find_related`, `doxus_find_path`, `doxus_get_cluster`, `doxus_create_workspace_document`, `doxus_update_workspace_document`, `doxus_delete_workspace_document`, `doxus_list_workspace_documents`, `doxus_apply_template`, `doxus_diagnose`, `doxus_system_report`

**부분/stub 구현:**
| 도구 | 상태 | 문제 | 라인 |
|------|------|------|------|
| `doxus_index_project` | **Stub** | "CLI 사용" 메시지만 반환, 실제 인덱싱 없음 | 254 |
| `doxus_sync_project` | **Stub** | "CLI 사용" 반환, MCP 경유 증분 동기화 없음 | ~1200 |
| `doxus_resolve_alias` | **미구현** | alias 해석 로직 없음 | ~540 |
| `doxus_inspect_document` | **Stub** | 인덱싱 상태 상세 정보 없음 | ~650 |
| `doxus_plugin_install` | **Stub** | DB 삽입만, WASM 다운로드/검증 없음 | 1134 |
| `doxus_plugin_remove` | **Stub** | DB 삭제만, 파일 정리 없음 | 1153 |
| `doxus_plugin_update` | **Stub** | 버전 문자열만 업데이트 | 1167 |
| `doxus_plugin_search` | **부분** | 로컬 DB만 검색, 마켓플레이스 API 없음 | 1185 |
| `doxus_plugin_logs` | **부분** | 로그 조회는 되나 헬스 상세 정보 없음 | 1254 |
| `doxus_plugin_info` | **미구현** | 함수 선언만 존재, 본문 없음 | ~1310 |

### 플러그인 시스템 — Extism 통합 미완성

**플러그인 매니저 (`crates/core/src/plugin/manager.rs`):**
- 플러그인 설치 (DB 쪽) — 완료
- 서명 검증 프레임워크 — 완료
- **디스크에서 실제 WASM 로딩** — 미구현, 파일명 목록만 반환

**WASM 어댑터 (`crates/core/src/plugin/wasm_adapter.rs`):**
| 기능 | 상태 | 비고 |
|------|------|------|
| Extism 초기화 | 완료 | `new()` 어댑터 생성 |
| manifest.toml 파싱 | 완료 | 권한 검증 |
| `http_request` Host Function | **Stub** | 도메인 허용목록은 작동, 실제 HTTP stub |
| `secrets_get` Host Function | **TODO** | 환경변수만, Keychain 없음 (라인 149) |
| `kv_get` / `kv_set` | **미구현** | 선언만 존재 |
| `progress` 보고 | **미구현** | 장기 인덱싱용 |
| `content_transform` | 완료 | 기본 HTML 스트리핑만 |

### 에이전트 사이드카 (`crates/agent/src/`)

**구현된 것:**
- `AgentManager` — Node.js 사이드카 스폰, 라이프사이클 관리
- `cli_detector.rs` — Claude Code / Gemini CLI 감지
- `PromptLoader` — `~/.doxus/agents/librarian/`에서 프롬프트 로드

**미구현:**
| 항목 | 문제 |
|------|------|
| JSONL 스트리밍 I/O | 프로토콜 타입은 존재, stdin/stdout 펌프 없음 |
| 도구 실행 브릿지 | 사이드카가 `doxus_*` 도구 호출, 브릿지 없음 |
| 세션 상태 머신 | start→message→result→close 추적 없음 |
| 도구 결과 주입 | MCP 결과를 사이드카로 전달 안 됨 |

### 인증 & 시크릿 (`crates/core/src/auth.rs`)

| 기능 | 상태 |
|------|------|
| OAuth 플로우 정의 | 완료 — `OAuthFlow` struct 존재 |
| `SecretStore` trait | 완료 — 정의됨 |
| 메모리 백엔드 (테스트용) | 완료 — `MemorySecretStore` 작동 |
| **Keychain 백엔드 (프로덕션)** | **Stub** — `security` / `keyring` 크레이트 없음 |
| OAuth 콜백 핸들러 | **미구현** |
| 토큰 갱신 로직 | **미구현** |
| 세션 영속성 | **미구현** |

**영향:** 플러그인 인증이 환경변수에만 의존, 안전한 자격증명 저장소 없음.

### Obsidian 플러그인 (`crates/plugins/obsidian/src/lib.rs`)

| 기능 | 상태 |
|------|------|
| 볼트 스캐닝 | 완료 |
| 문서 파싱 | 완료 |
| **Frontmatter 추출** | **Stub** — 태그 파싱 없음 (라인 192: `tags: vec![]`) |
| **`fetch_changes()`** | **Stub** — 항상 빈 값 반환 (라인 214) |
| **링크 추출** | **미구현** — 역링크/정방향 링크 추출 없음 |

### Confluence 플러그인 (`crates/plugins/confluence/src/lib.rs`)

| 기능 | 상태 |
|------|------|
| OAuth 플로우 | **미구현** |
| REST API 클라이언트 | **Stub** — 실제 HTTP 호출 없음 |
| `fetch_all()` | **Stub** — `vec![]` 반환 |
| `fetch_changes()` | **Stub** — 빈 값 반환 |

**영향:** Confluence 수집 불가. Phase 3 블로커.

### Phase 2-4 핵심 기능 우선순위

| 항목 | 예상 LOC | Phase | 중요 이유 |
|------|----------|-------|-----------|
| Extism Host Function 완성 | 400 | 2b | 플러그인이 HTTP/시크릿 호출 불가 |
| OAuth 플로우 구현 | 250 | 2d | 외부 플러그인 인증 불가 |
| 플러그인 레지스트리 클라이언트 | 200 | 4 | 마켓플레이스 없음 |
| 증분 동기화 로직 | 300 | 6 | 매번 전체 재인덱싱 |
| Confluence/GitHub 플러그인 | 600 | 3-5 | 데이터 소스 수집 불가 |

---

## Phase 4-8 중장기

### 마켓플레이스 (`crates/core/src/marketplace/`)

| 모듈 | 상태 | 비고 |
|------|------|------|
| `registry.rs` | **Stub** | 레지스트리 클라이언트 없음 |
| `signing.rs` | **부분** | 서명 검증은 작동, 레지스트리 조회 없음 |
| `installer.rs` | **Stub** | .wasm 파일 다운로드 안 됨 |

**완전 미구현:**
- 플러그인 마켓플레이스 UI (현재 로컬 DB만 검색)
- 레지스트리 API 클라이언트 (GitHub / Cloudflare Workers)
- 플러그인 코드 서명 자동화
- 플러그인 버전 해석
- 의존성 관리

### GitHub 플러그인 (`crates/plugins/github/src/lib.rs`)

| 기능 | 상태 |
|------|------|
| REST API 클라이언트 | **Stub** — GitHub API 호출 없음 |
| Issues/Wiki/Discussions 수집 | **미구현** |
| 인증 | **미구현** — 토큰 처리 없음 |
| `fetch_changes()` | **Stub** — 빈 값 반환 |

**영향:** GitHub 수집 불가. Phase 5 블로커.

### 동기화 엔진 (`crates/core/src/sync/`)

**존재하는 것:**
- `SyncJob` struct (재스케줄 로직 포함)
- `SyncScheduler` 스켈레톤 (기본 3600초 간격)

**핵심 미구현:**
| 컴포넌트 | 상태 |
|----------|------|
| 증분 동기화 감지 | **Stub** — 항상 빈 ChangeSet 반환 |
| 델타 추적 | **미구현** — 타임스탬프 기반 변경 감지 없음 |
| Cursor 영속성 | **미구현** — DB의 `sync_cursor` 미사용 |
| 오류 시 재시도 | **미구현** — 지수 백오프 없음 |
| 백그라운드 태스크 러너 | **미구현** — 스케줄러가 실제로 실행되지 않음 |
| Rate limit 처리 | **미구현** — `retry_after` 무시 |

**영향:** 프로젝트가 한 번만 인덱싱됨. 매 트리거마다 전체 재인덱싱.

### Desktop 앱 (`apps/desktop/src/`)

**페이지 상태:**
| 페이지 | 상태 | 비고 |
|--------|------|------|
| DashboardPage.tsx | **부분** | 통계 표시, 실시간 업데이트 없음 |
| SearchPage.tsx | **작동** | `doxus_search` MCP 도구 사용 |
| ProjectsPage.tsx | **부분** | 목록 작동, enable/disable stub |
| SettingsPage.tsx | **Stub** | 설정 영속성 없음 |
| WorkspacePage.tsx | **TODO** | 라인 23: "TODO 목록" 선언만 있음 |
| MarketPage.tsx | **Mock** | 하드코딩된 `MOCK_PLUGINS` 배열, 실제 마켓플레이스 없음 |

**ChatDrawer (에이전트 채팅):**
| 기능 | 상태 |
|------|------|
| 에이전트 세션 시작 | **Stub** — `invoke('agent_session_start')` 미연결 |
| 메시지 스트리밍 | **미구현** — WebSocket/청크 처리 없음 |
| 도구 사용 UI | **미구현** — 도구 호출 표시 없음 |
| 세션 히스토리 | **Stub** — 스토어 사용하나 영속성 없음 |

**Tauri IPC 커맨드:**
| 커맨드 | 상태 |
|--------|------|
| `search_documents` | 작동 |
| `add_project` | 작동 |
| `plugin_get_auth_status` | **Stub** — 항상 `false` 반환 |
| `plugin_set_auth_*` | **미구현** |
| `agent_session_start` | **미구현** |
| `workspace_apply_template` | **Stub** — 템플릿 하이드레이션 없음 |

### 관측성 & 로깅 (`crates/core/src/observability.rs`)

| 기능 | 상태 |
|------|------|
| 트레이싱 구독자 초기화 | 완료 |
| 감사 이벤트 타입 | 완료 (IndexStart, IndexComplete, PluginError, SyncStart, SyncComplete) |
| **감사 로그 조회** | **미구현** |
| **성능 메트릭** | **미구현** |
| **오류 집계** | **미구현** |

### 하위 우선순위 (폴리시)

| 항목 | 예상 LOC | Phase |
|------|----------|-------|
| 에이전트 ChatDrawer JSONL | 200 | 3 |
| 워크스페이스 템플릿 | 150 | 7 |
| Keychain 통합 | 80 | 2d |

---

## 코드 품질 이슈

### CLI panic 처리 (`crates/cli/src/main.rs`)

라인 563, 579, 581에 `panic!()` 사용 중:

```rust
_ => panic!("expected Search command"),  // 라인 563
_ => panic!("expected Add action"),      // 라인 579
_ => panic!("expected Project command"), // 라인 581
```

적절한 에러 처리로 교체 필요.

### CLI 미구현 커맨드

| 커맨드 | 문제 |
|--------|------|
| `doxus plugin install <url>` | Commands enum 누락 |
| `doxus plugin remove/update` | Commands enum 누락 |
| `doxus workspace delete` | Commands enum 누락 |
| `doxus sync` | Commands enum 누락 |
| `doxus agent start` | Commands enum 누락 |

### 데이터베이스 스키마 — 선언됐으나 미사용 테이블

| 테이블 | 용도 | 상태 |
|--------|------|------|
| `plugins` | 플러그인 메타데이터 | 부분 사용 |
| `source_instances` | 플러그인 프로젝트별 설정 | sync runner가 미사용 |
| `workspace_documents` | 워크스페이스 노트 | MCP 도구는 있음, desktop 통합 없음 |
| `workspace_templates` | 재사용 가능 문서 템플릿 | 앱 stub |
| `plugin_logs` | 플러그인 런타임 로그 | 테이블 존재, 로그 싱크 없음 |
| `session_tokens` | OAuth 토큰 | 미생성 — Keychain 없음 |

**인덱싱 갭:**
- FTS5 트리거 (검색 인덱스 자동 업데이트) 없음
- `sqlite-vec` 익스텐션 로드 실패 시 무음 처리
- 시작 시 스키마 검증 없음

### Plugin SDK (`crates/plugin-sdk/src/lib.rs`)

| 항목 | 상태 |
|------|------|
| `DocSource` trait | 완료 (모든 메서드 포함) |
| `PluginMetadata` | 완료 |
| `RawDocument` | 완료 |
| `PluginError` enum | 완료 |
| 테스트 플러그인 fixture | **Stub** — 빈 vec 반환 (라인 21: `documents: vec![]`) |

---

## 빠른 참조 테이블

어디서 시작할지:

| 목표 | 수정할 파일 |
|------|------------|
| 벡터 검색 | `crates/core/src/embedding.rs` → `OnnxEmbedder::embed()` 완성 |
| 플러그인 설치/업데이트 | `crates/mcp-server/src/lib.rs:1134+` → WASM 다운로드 구현 |
| 증분 동기화 | `crates/core/src/sync/runner.rs` → `fetch_changes()` 구현 |
| 에이전트 채팅 | `apps/desktop/src/components/layout/ChatDrawer.tsx` + `crates/agent/src/sidecar.rs` |
| Confluence 플러그인 | `crates/plugins/confluence/src/lib.rs` → Confluence REST API 구현 |
| OAuth | `crates/core/src/auth.rs` + plugin SDK oauth 메서드 |
| 백그라운드 동기화 | `crates/core/src/sync/scheduler.rs` → async 루프 스폰 |
| 워크스페이스 템플릿 | `crates/mcp-server/src/lib.rs:1462+` → 템플릿 하이드레이션 |

---

**총 예상 작업량:** 전체 MVP까지 집중 개발 3-4개월
**분석 기준일:** 2026-04-10

## 관련 문서

- [[architecture]]
- [[plugin-system]]
- [[agent-mcp]]
- [[database]]
