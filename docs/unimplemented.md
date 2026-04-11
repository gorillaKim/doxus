---
title: doxus 미구현 항목 목록
updated: 2026-04-10
---

# doxus 미구현 항목

> **Note:** 이 문서는 `UNIMPLEMENTED_ITEMS.md` (루트)를 기반으로 하되, 실제 코드 상태를 재검증하여 작성됨.
> 이전 문서에서 "미구현"으로 분류된 다수 항목이 이미 구현 완료 상태임.

## 이전 문서 대비 변경 사항 (완료된 항목)

| 항목 | 이전 상태 | 실제 상태 |
|------|----------|----------|
| McpServer embedder 연결 | "Phase 1 블로커" | **완료** — `main.rs:52-77` OnnxEmbedder 로드 + `McpServer::new(conn, embedder, plugins_dir)` |
| `doxus_index_project` | "Stub" | **완료** — `lib.rs:273-401` ObsidianPlugin fetch_all + 트랜잭션 인덱싱 |
| `doxus_sync_project` | "Stub" | **완료** — `lib.rs:1236+` fetch_changes + SyncSearchEngine 업데이트 |
| SyncScheduler 백그라운드 루프 | "spawn 없음" | **완료** — `sync_loop.rs` + `main.rs:50` spawn_sync_loop |
| `http_request` Host Function | "Stub" | **완료** — `wasm_adapter.rs:195-247` reqwest 기반 |
| `kv_get` / `kv_set` Host Function | "Unimplemented" | **완료** — `wasm_adapter.rs:152-158` |
| `progress` Host Function | "Unimplemented" | **완료** — `wasm_adapter.rs:162-165` broadcast channel |
| `doxus_resolve_alias` | "Unimplemented" | **완료** — `lib.rs:691+` |
| `doxus_inspect_document` | "Stub" | **완료** — `lib.rs:795+` |
| `doxus_plugin_info` | "Unimplemented" | **완료** — `lib.rs:1595+` |
| Obsidian `fetch_changes()` | "항상 empty" | **완료** — `obsidian/src/lib.rs:362+` mtime 기반 변경 감지 |
| Confluence `fetch_changes()` | "항상 empty" | **완료** — `confluence/src/lib.rs:401+` REST API + 삭제 감지 |
| GitHub `fetch_changes()` | "항상 empty" | **완료** — `github/src/lib.rs:572+` |
| OnnxEmbedder `embed()` | "미구현" | **완료** — `embedding.rs:98-211` 배치 추론 + mean pooling |
| Agent JSONL I/O | "미구현" | **완료** — `sidecar.rs` send/recv |
| RegistryClient | "Stub" | **완료** — `registry.rs:21+` fetch + parse |
| OAuthFlow | "미구현" | **완료** — `auth.rs:148+` authorization_url + token exchange |

---

## Phase별 우선순위

### P0: Phase 1 블로커 (즉시)

**현재 Phase 1 블로커 없음.** 핵심 엔진(검색, 인덱싱, 임베딩, 동기화)이 모두 작동 상태.

유일한 코드 TODO:
| 항목 | 파일 | 라인 | 설명 | 예상 LOC |
|------|------|------|------|----------|
| ~~레지스트리 checksum 연동~~ | `crates/mcp-server/src/lib.rs` | 1402 | `install_from_url`에 checksum 전달 — 현재 `None` 하드코딩 | (완료 2026-04-10) |

### P1: Phase 2 (플러그인 시스템)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| `secrets_get` Keychain 백엔드 | `crates/core/src/plugin/wasm_adapter.rs` | env var fallback만 구현, 테스트 8개 보강 완료 (2026-04-10), macOS Keychain 연동 미구현 | 80 |
| `content_transform` 고도화 | `crates/core/src/plugin/wasm_adapter.rs` | HTML stripping만 — 마크다운 파서 활용 미구현 | 100 |
| ~~WASM 플러그인 실제 로드~~ | `crates/core/src/plugin/manager.rs` | Extism 런타임 로드 완료, `get_source()` 구현 + path traversal 방어 | (완료 2026-04-10) |
| 플러그인 ABI 버전 검증 | `crates/core/src/plugin/manager.rs` | 매니페스트에 abi_version 정의되나 런타임 검증 없음 | 50 |

### P2: Phase 3 (Confluence)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| Confluence OAuth 콜백 핸들러 | `crates/plugins/confluence/src/lib.rs` | `oauth_start` 완료, HTTP 콜백 서버 미구현 | 100 |
| Confluence 토큰 자동 갱신 | `crates/core/src/auth.rs` | `is_expired()` 존재, 자동 갱신 로직 없음 | 80 |

### P3: Phase 4-5 (마켓플레이스 + GitHub)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| 플러그인 마켓 UI | `apps/desktop/src/pages/MarketPage.tsx` | `MOCK_PLUGINS` 하드코딩 배열 사용 | 200 |
| 레지스트리 API 연동 (MarketPage) | `apps/desktop/src/pages/MarketPage.tsx:311` | invoke 실패 시 MOCK_PLUGINS fallback | 50 |
| 코드 서명 자동화 | `crates/core/src/marketplace/signing.rs` | 서명 검증 존재, 서명 생성/CI 파이프라인 없음 | 150 |
| 플러그인 버전 해상도 | `crates/core/src/marketplace/` | 레지스트리에서 latest 버전만 — semver range 미지원 | 100 |
| GitHub 플러그인 인증 | `crates/plugins/github/src/lib.rs` | PAT 토큰 처리 없음 | 80 |

### P4: Phase 6 (동기화)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| Retry 로직 (exponential backoff) | `crates/mcp-server/src/sync_loop.rs` | 실패 시 로그만, 재시도 없음 | 80 |
| Rate limit 핸들링 | `crates/mcp-server/src/sync_loop.rs` | `retry_after_secs` 미참조 | 40 |
| 동기화 충돌 해결 | 미존재 | 동시 수정 시 정책 없음 (last-write-wins도 미구현) | 150 |

### P5: Phase 8 (Desktop UI)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| SettingsPage 설정 영속화 | `apps/desktop/src/pages/SettingsPage.tsx` | UI만 존재, 저장 로직 없음 | 100 |
| WorkspacePage 기능 구현 | `apps/desktop/src/pages/WorkspacePage.tsx` | TODO 플레이스홀더 | 200 |
| DashboardPage 실시간 업데이트 | `apps/desktop/src/pages/DashboardPage.tsx` | 정적 통계만 표시 | 80 |
| `plugin_get_auth_status` IPC | `apps/desktop/src-tauri/` | 프론트엔드에서 호출하나 백엔드 stub | 50 |

### P6: 최후 처리 (Agent)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| Agent 도구 실행 브릿지 | `crates/agent/src/sidecar.rs` | JSONL I/O 완료, doxus_* 도구 호출 미연결 | 150 |
| Agent 세션 상태 머신 | `crates/agent/src/` | start/message/result/close 트래킹 없음 | 200 |
| ChatDrawer 에이전트 연결 | `apps/desktop/src/components/layout/ChatDrawer.tsx` | `agent_session_start` IPC 미구현 | 150 |

## MCP 도구 stub 목록

**현재 stub인 도구 없음.** 39개 도구 모두 실제 구현이 존재함.

유일한 제한:
- `doxus_plugin_install`: DB 기록 + WASM 다운로드 동작하나, 레지스트리 checksum 미전달 (`lib.rs:1402`)
- `doxus_plugin_search`: 로컬 DB 검색만 — 원격 레지스트리 통합 검색은 Phase 4

## 요약

| Phase | 미구현 항목 수 | 예상 총 LOC |
|-------|--------------|------------|
| P0 (즉시) | 0 | 0 |
| P1 (플러그인) | 3 | 230 |
| P2 (Confluence) | 2 | 180 |
| P3 (마켓) | 5 | 580 |
| P4 (동기화) | 3 | 270 |
| P5 (Desktop UI) | 4 | 430 |
| P6 (Agent — 최후) | 3 | 500 |
| **합계** | **20** | **~2,190** |

**총 예상 작업량:** ~4-6주 집중 개발 (풀타임 1인 기준)
