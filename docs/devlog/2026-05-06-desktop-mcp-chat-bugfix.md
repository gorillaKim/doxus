---
title: "doxus 데스크톱 앱 신규 PC 버그 2종 수정 및 v0.1.5~v0.1.7 릴리즈"
aliases:
  - desktop-mcp-chat-bugfix
  - 데스크톱-MCP-채팅-버그수정
  - 2026-05-06 doxus 데브로그
tags:
  - devlog
  - troubleshooting
  - bugfix
  - tauri
  - release
agent_model: claude-sonnet-4-6
created: "2026-05-06"
updated: "2026-05-06"
---

<!-- docsmith: auto-generated 2026-05-06 -->

## 개요

신규 PC에서 재현되는 데스크톱 앱 버그 2종을 수정하고 v0.1.5, v0.1.6, v0.1.7을 순차 릴리즈했다. 첫 번째 버그는 Tauri externalBin이 바이너리에 타겟 트리플 suffix를 붙이는데 런타임 탐색 코드가 이를 고려하지 않아 MCP 서버가 묵음 실패하는 문제였다. 두 번째 버그는 release.yml에 sidecar npm ci 단계가 없어 `@anthropic-ai/claude-agent-sdk`가 DMG 번들에 포함되지 않아 인앱 채팅이 전체 불능 상태가 된 문제였다.

## 주요작업

### 신규 PC에서 MCP 서버 미실행 버그 수정 — Tauri externalBin target-triple suffix 탐색 누락 `[medium]`

- **변경 파일**: `apps/desktop/src-tauri/src/main.rs`, `Cargo.toml`, `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`, `Cargo.lock`
- **결과**: `find_doxus_mcp_bin()`을 3개 함수로 분리하고 `MacOS/` 디렉토리 prefix 스캔 방식으로 변경. 단위 테스트 4개 추가. v0.1.5 릴리즈 완료.

### 인앱 채팅 'Cannot read properties of null (reading query)' 에러 수정 — sidecar node_modules 번들 누락 `[hard]`

- **변경 파일**: `apps/desktop/src-tauri/sidecar/adapters/claude.mjs`, `.github/workflows/release.yml`, `Cargo.toml`, `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`, `Cargo.lock`
- **결과**: null 가드 + 친절한 에러 메시지 추가(v0.1.6). `pathToClaudeCodeExecutable`을 실제 경로일 때만 전달하도록 수정하여 SDK 내장 cli.js fallback 활성화(v0.1.7). `release.yml`에 sidecar npm ci 및 SDK 번들 검증 스텝 추가.

### v0.1.5 GitHub release draft 퍼블리시 `[easy]`

- **변경 파일**: (없음)
- **결과**: `gh release edit v0.1.5 --draft=false`로 퍼블리시 완료.

## 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|---------|------|---------|
| Tauri externalBin이 sidecar 바이너리에 타겟 트리플 suffix(예: `doxus-mcp-aarch64-apple-darwin`)를 붙이는데, `find_doxus_mcp_bin()`이 이를 탐색하지 않아 신규 PC에서 MCP 서버가 묵음 실패 | high | 완료 | `MacOS/` 디렉토리를 스캔하여 `doxus-mcp` prefix 매칭으로 어떤 아키텍처에서도 바이너리를 탐색하도록 리팩토링 |
| `release.yml`에 sidecar npm ci 단계가 없어 `@anthropic-ai/claude-agent-sdk`가 DMG 번들에 미포함 → `#sdk = null` → 인앱 채팅 전체 불능 | critical | 완료 | `release.yml`에 sidecar npm ci 단계 추가 및 SDK 번들 검증 스텝 추가 |
| `pathToClaudeCodeExecutable`에 시스템 claude 바이너리 경로가 없는 환경(신규 DMG 설치 사용자)에서 SDK가 cli.js fallback을 타지 못하고 실패 | high | 완료 | `cliPath`가 실제 파일시스템 경로일 때만 `pathToClaudeCodeExecutable`을 전달, 그 외에는 `undefined`로 두어 SDK 내장 cli.js(12.6MB) fallback 활성화 |
| CI에서 doxus-mcp 바이너리가 git 미추적 상태라 cargo check 실패 | medium | 미완료 | (근본 원인 미해결) |

## 배운점

- Tauri externalBin은 번들 시 바이너리명에 타겟 트리플 suffix를 자동으로 붙인다(예: `doxus-mcp` → `doxus-mcp-aarch64-apple-darwin`). 런타임 탐색 코드는 이 suffix를 반드시 고려해야 한다.
- `@anthropic-ai/claude-agent-sdk`는 `pathToClaudeCodeExecutable`을 전달하지 않으면 패키지 내 번들된 cli.js(12.6MB)를 자동으로 사용하는 fallback 경로를 가지고 있다.
- Tauri 앱 release 워크플로우에서 sidecar가 Node.js 스크립트를 포함한다면 CI의 패키징 단계 직전에 반드시 npm ci를 실행해야 한다.
- MCP SDK null 가드는 단순 에러 방어뿐 아니라 사용자에게 의미 있는 에러 메시지를 제공하는 UX 개선이기도 하다.

## 개선할점

- CI에서 doxus-mcp 바이너리가 git 미추적 상태인 근본 원인을 해결해야 한다.
- sidecar의 node_modules 번들 여부를 로컬 빌드에서도 사전 검증할 수 있는 pre-build 훅 또는 Makefile 타겟을 고려할 것.
- `find_doxus_mcp_bin()` 함수 분리와 테스트 추가는 좋은 방향이지만, 향후 Windows 지원 시 타겟 트리플 포맷이 다르므로 플랫폼별 suffix 패턴을 별도 상수로 관리할 것.
- 버그 2가 v0.1.6 → v0.1.7 두 커밋에 걸쳐 수정된 것은 null 가드와 CLI path 문제가 별개 레이어임을 초기에 파악하지 못했기 때문. 채팅 불능 시 SDK 초기화 실패와 CLI path 문제를 구분하는 진단 로그를 사전에 추가했더라면 한 번에 수정 가능했다.

## 하네스 개선 제안

<!-- rule_candidate: Tauri externalBin suffix 탐색 패턴이 이번에 처음 문제로 발견되었으며, git-workflow.md의 릴리즈 체크리스트에 이 항목이 없음 -->
**제안**: `git-workflow.md` 릴리즈 버전 bump 체크리스트에 'sidecar 바이너리 탐색 코드가 target-triple suffix를 처리하는지 확인' 항목 추가
**근거**: 신규 PC 환경에서만 재현되는 묵음 실패 버그 발생 — 개발 PC에서 테스트 시 동일 경로 바이너리가 존재해 탐색 코드 결함이 발견되지 않음

<!-- rule_candidate: Node.js sidecar를 포함한 Tauri 앱 릴리즈 시 sidecar npm ci 누락이 이번에 처음 발견되었으며 기존 체크리스트에 없음 -->
**제안**: `git-workflow.md` 릴리즈 체크리스트에 'Node.js sidecar가 있는 경우 release.yml에 sidecar npm ci 단계 포함 여부 확인' 항목 추가
**근거**: sidecar npm ci 누락으로 `@anthropic-ai/claude-agent-sdk`가 DMG 번들에서 빠져 critical 버그 발생

## 관련 문서

- [[MCP needs-auth 진단 및 v0.1.2/v0.1.3 릴리즈]]
- [[Auto Updater & Post-Update Migration — TDD 전체 구현 (Phase 1–5)]]
