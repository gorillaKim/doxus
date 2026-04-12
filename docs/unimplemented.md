---
title: doxus 미구현 항목 목록
updated: 2026-04-11
---

# doxus 미구현 항목

> **Note:** 이 문서는 실제 코드 상태를 재검증하여 작성됨 (2026-04-11 재검증).
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
| RegistryClient | "Stub" | **완료** — `registry.rs:13-70` fetch + parse |
| OAuthFlow | "미구현" | **완료** — `auth.rs:148+` authorization_url + token exchange |
| 플러그인 ABI 버전 검증 | "런타임 검증 없음" | **완료** — `manager.rs` SUPPORTED_ABI_VERSION 상수 + get_source() 가드 (2026-04-11) |
| `secrets_get` Keychain 연동 | "env var fallback만" | **완료** — `secrets.rs:20-37` SystemKeychain + `wasm_adapter.rs` KeyringBackend (2026-04-11) |
| Confluence OAuth 콜백 HTTP 서버 | "oauth_start만 있음" | **완료** — `confluence/src/oauth_server.rs` OAuthCallbackServer, random port, CSRF guard (2026-04-11) |
| レジストリ checksum 연동 | "`None` 하드코딩" | **완료** — `install_from_url`에 checksum 자동 전달 (2026-04-10) |
| GitHub PAT 인증 | "PAT 처리 없음" | **완료** — `github/lib.rs:100-130` Bearer token 헤더 |
| Agent 도구 실행 브릿지 | "미연결" | **완료** — `crates/agent/src/tool_bridge.rs` ToolBridge + JSONL 라우팅 |
| Agent 세션 상태 머신 | "트래킹 없음" | **완료** — `crates/agent/src/session.rs` SessionRunner |
| ChatDrawer 에이전트 IPC | "미구현" | **완료** — `commands/agent.rs:100-130` chat_start_session + `ChatDrawer.tsx` |

---

## Phase별 우선순위

### P0: 즉시 처리

**현재 P0 블로커 없음.**

---

### P1: Phase 2 (플러그인 시스템)

**현재 P1 미구현 없음.** 모든 항목 완료.

---

### P2: Phase 3 (Confluence)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| Confluence 토큰 자동 갱신 | `crates/core/src/auth.rs` | `is_expired()` + `refresh_token()` 존재, **호출하는 곳 없음** — Confluence 플러그인 각 HTTP 요청 전 만료 체크 미수행 | 80 |

---

### P3: Phase 4-5 (마켓플레이스)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| 플러그인 마켓 UI | `apps/desktop/src/pages/MarketPage.tsx` | `MOCK_PLUGINS` 하드코딩 배열 — RegistryClient는 구현됨, UI 연결만 필요 | 200 |
| 코드 서명 자동화 | `crates/core/src/marketplace/signing.rs` | 서명 **검증**은 존재 (`verify_plugin()`), 서명 **생성** 및 CI 파이프라인 없음 | 150 |
| 플러그인 버전 해상도 | `crates/core/src/marketplace/` | `RegistryEntry.version: String` 단순 비교만 — semver range 미지원 | 100 |

---

### P4: Phase 6 (동기화 안정화)

**현재 P4 미구현 없음.** 모든 항목 완료 (2026-04-13 재검증).

| 항목 | 파일 | 실제 상태 |
|------|------|----------|
| Retry 로직 (exponential backoff) | `crates/mcp-server/src/sync_loop.rs` | **완료** — 100ms → 500ms → 2.5s 배수 증가, 30s 상한, ±10% jitter |
| Rate limit 핸들링 | `crates/mcp-server/src/sync_loop.rs` | **완료** — `RateLimited { retry_after_secs }` 수신 시 해당 초만큼 대기, MAX 300s 캡 |
| 동기화 충돌 해결 | `crates/core/src/sync/runner.rs` | **완료** — content_hash 비교 후 변경 시만 적용 (last-indexed-wins), `record_conflict()` audit_log 기록 |

---

### P5: Phase 8 (Desktop UI)

| 항목 | 파일 | 현재 상태 | 예상 LOC |
|------|------|----------|----------|
| SettingsPage 설정 영속화 | `apps/desktop/src/pages/SettingsPage.tsx` | UI만 존재, Tauri command `save_settings` 미구현 | 100 |
| WorkspacePage 백엔드 연결 | `apps/desktop/src/pages/WorkspacePage.tsx` | UI 구조 있음, 실제 Tauri command 연결 불완전 | 100 |
| DashboardPage 실시간 업데이트 | `apps/desktop/src/pages/DashboardPage.tsx` | `useEffect` 최초 1회만 실행 — Tauri 이벤트 리스너 없음 | 80 |
| `plugin_get_auth_status` IPC | `apps/desktop/src-tauri/src/commands/` | `agent_status()` 존재, 플러그인 인증 상태 전용 커맨드 없음 | 50 |

---

## MCP 도구 stub 목록

**현재 stub인 도구 없음.** 39개 도구 모두 실제 구현이 존재함.

---

## 요약

| Phase | 미구현 항목 수 | 예상 총 LOC |
|-------|--------------|------------|
| P0 (즉시) | 0 | 0 |
| P1 (플러그인) | 0 | 0 |
| P2 (Confluence) | 1 | 80 |
| P3 (마켓) | 3 | 450 |
| P4 (동기화) | 0 | 0 |
| P5 (Desktop UI) | 4 | 330 |
| **합계** | **8** | **~860** |

**총 예상 작업량:** ~1-2주 집중 개발 (풀타임 1인 기준)
