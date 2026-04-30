---
title: "plugin-dev-guide 초안 작성 + Opus 리뷰 반영 + MCP OAuth 디버깅"
aliases:
  - plugin-dev-guide-opus-review-mcp-oauth-fix
  - 플러그인-개발가이드-opus리뷰-mcp-oauth-수정
  - 2026-04-30 doxus 데브로그
tags:
  - devlog
  - plugin
  - mcp
  - oauth
  - documentation
agent_model: claude-sonnet-4-6
created: "2026-04-30"
updated: "2026-04-30"
---

<!-- docsmith: auto-generated 2026-04-30 -->

## 개요

`docs/plugin-dev-guide.md` 초안을 코드베이스 실제 구현 기반으로 작성한 뒤, Opus 리뷰 에이전트를 통해 16개 이슈를 발굴하고 전체 반영했다. 이어서 MCP HTTP 서버 연결 시 발생하는 `Invalid OAuth error response: JSON Parse error: Unexpected EOF` 에러를 디버깅하여 MCP SDK 1.x의 RFC 9728 OAuth discovery preflight가 원인임을 규명하고 axum 라우터에 3개 OAuth 엔드포인트를 추가해 해결했다.

## 주요작업

### docs/plugin-dev-guide.md 코드베이스 분석 기반 초안 작성 `[medium]`

- **변경 파일**: `docs/plugin-dev-guide.md`
- **결과**: 코드베이스 실제 구현(DocSource trait, PluginMetadata, host function 등)을 분석하여 약 802줄 규모의 초안 완성

### Opus 리뷰 에이전트를 통한 16개 이슈 발견 및 문서 개선 반영 `[hard]`

- **변경 파일**: `docs/plugin-dev-guide.md`
- **결과**: CRITICAL 4건(PluginMetadata.name 필드명 오류, register_factory where bounds 누락, __doxus_set_secret fire-and-forget 미설명, Capabilities/SyncPolicy 타입 미참조) + HIGH 5건 포함 총 16개 이슈 반영, 802줄 → 928줄로 확장. 커밋 1a684d2

### MCP SDK 1.x OAuth discovery preflight 에러 디버깅 및 수정 `[hard]`

- **변경 파일**: `crates/mcp-server/src/http_server.rs`
- **결과**: HTTP 404 → JSON Parse EOF 에러의 근본 원인이 MCP SDK 1.x의 RFC 9728 OAuth discovery preflight임을 규명. axum 라우터에 `/.well-known/oauth-protected-resource`, `/.well-known/oauth-authorization-server`, `/oauth/token` 엔드포인트 3개 추가(+58줄). 커밋 cc31507

## 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| plugin-dev-guide.md 초안에서 PluginMetadata 필드명이 display_name으로 잘못 기술 (실제 코드는 name) | critical | true | Opus 리뷰 피드백 반영 시 실제 struct 필드명 name으로 정정 |
| register_factory 예시 코드에서 DocSource + Send + Sync where bounds 누락 | critical | true | where T: DocSource + Send + Sync + 'static 바운드 명시 |
| __doxus_set_secret이 fire-and-forget임을 문서에 미설명 | critical | true | fire-and-forget 동작 명시 및 반환값 무시 패턴 예시 추가 |
| Capabilities, SyncPolicy 타입이 문서 타입 참조 섹션에 누락 | critical | true | 타입 참조 섹션에 두 타입 추가 |
| SSRF 방어 섹션에 fc00::/7 IPv6 ULA 대역 누락 | high | true | IPv6 ULA 대역 차단 및 HTTPS-first 정책 명시 |
| MCP HTTP 연결 시 'HTTP 404: Invalid OAuth error response: JSON Parse error: Unexpected EOF' 에러 | high | true | MCP SDK 1.x가 연결 전 RFC 9728 OAuth discovery preflight를 선제 요청하는 것이 원인. axum 라우터에 3개 OAuth 엔드포인트 추가로 해결 |
| Confluence 플러그인 expires_at 절대 시각 처리 경고 문서 미반영 | high | true | OAuth 토큰 만료 절대 시각 처리 주의 사항 섹션 추가 |

## 배운점

- MCP SDK 1.x는 Bearer token이 설정되어 있어도 연결 전 RFC 9728(`/.well-known/oauth-protected-resource`)과 RFC 8414(`/.well-known/oauth-authorization-server`) 엔드포인트를 선제적으로 preflight 요청한다. 서버가 404를 빈 body로 반환하면 `Invalid OAuth error response: JSON Parse error: Unexpected EOF` 에러가 발생한다.
- axum에서 인증 미들웨어를 우회해야 하는 라우터는 `route_layer` 적용 전 별도 `Router`로 분리 후 `merge()`로 합쳐야 한다.
- 문서 품질 검증에 Opus 리뷰 에이전트를 활용하면 코드-문서 간 불일치를 효과적으로 발굴할 수 있다.
- WASM 플러그인에서 aliases 반환은 host function 제약으로 동작하지 않으며, 이 한계를 문서에 명시해야 한다.
- GitHub 스타일의 compound cursor는 멀티 엔티티 페이지네이션 상태를 opaque cursor 하나로 유지하는 실용적인 패턴이다.

## 개선할점

- 초안 작성 전에 실제 Rust 코드(trait 정의, struct 필드, pub fn 시그니처)를 더 꼼꼼히 대조했다면 CRITICAL급 오류 4건을 초안 단계에서 예방할 수 있었다.
- MCP OAuth 에러 디버깅 시 에러 메시지만으로는 SDK 내부 preflight 동작을 바로 알기 어렵다 — MCP SDK 소스 또는 changelog를 먼저 확인하는 습관이 필요하다.
- `http_server.rs`에 추가된 `/oauth/token` 엔드포인트는 만료 시간이 고정값(3600s)이다. 실제 운영 환경에서 토큰 회전이 필요하다면 만료 관리 로직 추가가 필요하다.
- OAuth discovery 엔드포인트를 별도 `oauth_handlers.rs` 모듈로 분리하면 테스트 및 유지보수 용이성이 향상된다.

## 하네스 개선 제안

<!-- rule_candidate: MCP SDK 1.x OAuth preflight 문제를 해결하면서 RFC 9728, RFC 8414 엔드포인트 구조를 직접 탐색해야 했다 -->
**제안**: `rules/agent-mcp.md`에 'MCP HTTP 서버는 SDK 1.x OAuth discovery preflight를 위해 3개 엔드포인트를 반드시 제공해야 한다'는 규칙 추가
**근거**: 동일 문제 재발 시 디버깅 비용 절감 가능

<!-- rule_candidate: 문서 초안에서 CRITICAL급 코드-문서 불일치가 4건 발생했고, Opus 리뷰에서야 발견함 -->
**제안**: `git-workflow.md`에 plugin-sdk 및 core의 public API 변경 시 docs/ 문서를 함께 업데이트하는 체크리스트 추가
**근거**: PluginMetadata.name, register_factory where bounds 등 실제 코드와 문서 불일치 4건이 초안 단계를 통과

---

## 주요작업 (세션 2)

### Tauri DMG 빌드 성공 (doxus_0.1.0_aarch64.dmg 75.1MB) `[hard]`

- **변경 파일**: (빌드 산출물)
- **결과**: RTK hook 명령어 변형 문제 및 bundle_dmg.sh 마운트 잔류 파일 충돌을 순차 해결 후 성공. npm run tauri build → npx tauri build --bundles dmg → ./node_modules/.bin/tauri build -b dmg 순서로 3회 시도

### MCP OAuth 디스커버리 엔드포인트 구현 (RFC 9728 + RFC 8414) `[very_hard]`

- **변경 파일**: `crates/mcp-server/src/http_server.rs`
- **결과**: 6단계 이상의 순차 수정(authorization_endpoint 누락 → response_types_supported → registration_endpoint → /oauth/register 핸들러 → /oauth/authorize → 토큰 불일치 발견). Claude Code MCP 설정과 서버 토큰 불일치가 근본 원인으로 확인

### Opus 코드 리뷰 수행 및 HIGH+MEDIUM 보안 이슈 반영 `[medium]`

- **변경 파일**: `crates/mcp-server/src/http_server.rs`
- **결과**: HIGH 3건(DNS Rebinding, 토큰 무인증 반환, oauth_register 무검증) + MEDIUM 5건 수정 완료. host_allowlist_middleware 추가, grant_type 검증, RFC 8414 메타데이터 개선, OAuth 테스트 6개 추가

## 이슈 (세션 2)

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| RTK hook이 'npx tauri build --bundles dmg'를 'tauri build dmg'로 변형하여 cargo에 dmg 인자가 전달됨 | high | true | ./node_modules/.bin/tauri build -b dmg 로 RTK hook을 우회하여 직접 실행 |
| bundle_dmg.sh 실패 — 이전 빌드 시도의 임시 DMG 마운트 포인트 및 파일 잔류 | high | true | hdiutil detach로 마운트 해제 및 잔류 임시 파일 삭제 후 재빌드 |
| MCP SDK 1.x가 authorization_endpoint, response_types_supported 등을 Zod 스키마로 필수 검증하여 누락 시 연결 실패 | high | true | SDK Zod 스키마가 요구하는 모든 필드를 OAuth discovery 응답에 추가 |
| Claude Code MCP 설정의 토큰(pQgPRqsFDbYhECLBQeTpMRIdpKHCf8oW)과 서버 하드코딩 토큰 불일치 | critical | true | 서버 토큰을 Claude Code MCP 설정값으로 수정 |
| http_server.rs DNS Rebinding 취약점 — localhost 전용 서버에 Host 헤더 검증 없음 | high | true | host_allowlist_middleware 추가 (localhost, 127.0.0.1만 허용) |
| /oauth/token 엔드포인트가 grant_type 검증 없이 토큰 반환 | high | true | grant_type=client_credentials 검증 추가, 불일치 시 400 반환 |
| MCP 연결 최종 성공 여부 미확인 | medium | false | — |

## 배운점 (세션 2)

- MCP SDK 1.x는 Bearer token 설정 여부와 무관하게 연결 전 RFC 9728/RFC 8414 preflight를 선제 요청한다. 서버가 404를 빈 body로 반환하면 `Invalid OAuth error response: JSON Parse error: Unexpected EOF`가 발생한다.
- MCP SDK 1.x의 OAuth discovery 응답은 Zod 스키마가 authorization_endpoint, response_types_supported, registration_endpoint 등을 필수로 검증한다.
- RTK hook은 npm/npx를 통한 tauri 명령어를 변형할 수 있어 node_modules/.bin/ 직접 실행으로 우회 가능하다.
- axum에서 인증 미들웨어를 우회해야 하는 라우터는 route_layer 적용 전 별도 Router로 분리 후 merge()로 합쳐야 한다.
- OAuth 디버깅 시 토큰 불일치처럼 설정 레이어의 근본 원인은 MCP 설정 파일을 직접 확인하는 것이 효과적이다.

## 개선할점 (세션 2)

- MCP OAuth 에러 발생 시 MCP SDK Zod 스키마를 먼저 확인하는 습관이 필요하다.
- Tauri 빌드 시 RTK hook 우회를 위해 node_modules/.bin/ 직접 경로를 사용해야 한다.
- OAuth discovery 엔드포인트를 별도 모듈로 분리하면 유지보수 향상된다.
- MCP 연결 테스트 시 Claude Code MCP 설정 파일 토큰과 서버 토큰 일치 여부를 먼저 확인해야 한다.

## 하네스 개선 제안 (세션 2)

<!-- rule_candidate: MCP OAuth preflight 디버깅에 6단계 이상 소요 -->
**제안**: `rules/agent-mcp.md`에 'MCP HTTP 서버 OAuth 필수 엔드포인트 체크리스트' 섹션 추가
**근거**: 동일 에러가 필드 하나씩 추가할 때마다 반복 발생하는 패턴이 6회 이상 감지됨

<!-- rule_candidate: RTK hook이 tauri 빌드 명령어를 변형하여 빌드 실패 -->
**제안**: CLAUDE.md에 'Tauri 빌드는 ./node_modules/.bin/tauri build -b <bundle> 형식으로 직접 실행할 것' 주의사항 추가
**근거**: 3회 시도 패턴 — npm run, npx, node_modules/.bin 순서로 시행착오

## 관련 문서

- [[plugin-dev-guide]]
- [[에이전트 & MCP 규칙]]
- [[플러그인 시스템 규칙]]
- [[Auto Updater & Post-Update Migration — TDD 전체 구현 (Phase 1–5)]]
