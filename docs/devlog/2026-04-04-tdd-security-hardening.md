---
title: "TDD 전체 사이클 + 보안 하드닝 (Confluence/GitHub/Agent Sidecar)"
aliases:
  - tdd-security-hardening-2026-04-04
  - TDD 보안 하드닝
  - confluence-github-agent-tdd
tags:
  - devlog
  - feature
  - security
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# TDD 전체 사이클 + 보안 하드닝 (Confluence/GitHub/Agent Sidecar)

## 배경

Phase 3 구현 이후 약 25~30% 구현 완료 상태에서 핵심 미구현 항목을 확인했다.
Autopilot TDD 모드로 5개 에이전트를 병렬 실행하여 Confluence 플러그인 통합 테스트,
GitHub 플러그인 보완, Agent Sidecar 신규 구현을 완료하고, Security Reviewer + Architect Reviewer
이중 검증을 거쳐 발견된 취약점을 즉시 수정했다.

## 변경 내용

### 주요 변경사항

#### Confluence 플러그인 — 통합 테스트 신규 (4 tests)

- `crates/plugins/confluence/tests/integration_test.rs` 신규 생성 (wiremock 기반)
- 테스트 시나리오: `fetch_all_returns_pages_from_api`, `fetch_all_paginates_with_cursor`,
  `fetch_all_returns_error_on_unauthorized`, `fetch_all_respects_page_size`
- `tests/fixtures/confluence_pages.json` fixture 추가
- 401 응답 → `PluginError::AuthRequired` 변환 수정
- `incremental_sync` 플래그를 `true` → `false`로 수정 (`fetch_changes` 미구현 명시화)

#### SSRF 취약점 수정 (HIGH) — `validate_base_url()` 추가

`crates/plugins/confluence/src/lib.rs`의 `validate_config()`에 URL 검증 추가:
- HTTPS 강제 (`http://` 거부)
- localhost, 127.0.0.1, 169.254.169.254(메타데이터 서버) 등 내부 주소 차단

#### Command Injection 방어 (MEDIUM) — URL 스키마 화이트리스트

`apps/desktop/src-tauri/src/commands/market.rs`의 `plugin_open_url` 커맨드:
- `http://` 및 `https://` 스키마만 허용, 그 외 거부

#### GitHub 플러그인 — 상태 코드 매핑 보완

`crates/plugins/github/src/lib.rs` (lines 227-234):
- 401 → `PluginError::AuthRequired`
- 403 → `PluginError::PermissionDenied`
- `expect()` → `unwrap_or_else(|_| reqwest::Client::new())` 안전 처리 (lines 54, 70)

#### Agent Sidecar 신규 구현

**`crates/agent/src/cli_detector.rs`**
- `CliKind` 변형에 `path` 필드 추가: `ClaudeCode { path }`, `GeminiCli { path }`

**`crates/agent/src/sidecar.rs` 신규**
- `SidecarManager`: tokio::process 기반 async spawn, JSONL send/recv/shutdown
- `SidecarMessage` enum (`#[serde(tag = "type")]`): `Start`, `Message`, `Cancel`, `Close`
- `AgentError` (thiserror): `CliNotFound`, `SpawnFailed`, `Protocol`

**`crates/agent/src/lib.rs`**
- `pub mod sidecar` 추가

### 영향 범위

- `crates/plugins/confluence/` — 통합 테스트 + SSRF 수정
- `crates/plugins/github/` — 상태 코드 매핑 + 안전성 보완
- `crates/agent/` — sidecar 모듈 신규
- `apps/desktop/src-tauri/src/commands/market.rs` — URL 검증

## 결과

- 커밋: `7be503a` — feat(plugin): TDD - Confluence/GitHub SSRF fix, agent sidecar, security hardening
- 테스트: **256 passed, 0 failed** (이전 246개 → 10개 증가)
- 34 files changed, 3127 insertions(+), 270 deletions(-)
- Security Reviewer: CRITICAL 0, HIGH 0 (즉시 수정), MEDIUM 1 잔존 (OAuth 메모리)
- Architect Reviewer: DocSource trait 계약 준수, Cursor 불투명성, 에러 타입 컨벤션 모두 OK

## 교훈

**보안 리뷰 → 즉시 수정 패턴이 이번 세션에서 2회 연속 반복됨** (이번: SSRF + injection, 이전: API key in URL).
외부 입력 URL을 그대로 사용하는 플러그인 패턴은 구현 단계에서 `validate_config()`에
URL 화이트리스트/블랙리스트 검증을 처음부터 포함해야 한다.
Phase 4 이후 신규 플러그인 추가 시 체크리스트로 관리할 것.

잔존 기술 부채:
- MCP server 직접 SQL (~69곳) → `crates/core/src/db/` 경유 이전 (Phase 4)
- OAuth `client_secret` 메모리 보유 (MEDIUM 보안, 향후 처리)
- Confluence `fetch_changes` 미구현 (`incremental_sync=false`로 명시화 완료)
- WASM/Extism 런타임 연동 미완성

## 관련 문서

- [[doxus — 프로젝트 개요]]
- [[플러그인 시스템 규칙]]
- [[아키텍처 원칙]]
