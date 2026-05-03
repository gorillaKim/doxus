---
title: "MCP needs-auth 진단 및 v0.1.2/v0.1.3 릴리즈"
aliases:
  - mcp-needs-auth-diagnosis-v013-release
  - MCP-인증-진단-v013-릴리즈
  - 2026-05-03 doxus 데브로그
tags:
  - devlog
  - mcp
  - troubleshooting
  - release
agent_model: claude-sonnet-4-6
created: "2026-05-03"
updated: "2026-05-03"
---

<!-- docsmith: auto-generated 2026-05-03 -->

## 개요

MCP HTTP 서버가 모든 요청에 401 needs-auth를 반환하는 문제를 진단했다. 원인은 구버전 doxus 바이너리가 다른 bridge token으로 백그라운드에서 실행 중인 상태였다. 프로세스 교체 후 정상화를 확인하고 v0.1.2 릴리즈를 진행했으나, `apps/desktop/src-tauri/Cargo.toml`의 버전 하드코딩 누락으로 Tauri 업데이터 오판정이 발생하여 v0.1.3을 긴급 재릴리즈했다.

## 주요작업

### MCP HTTP 서버 401 needs-auth 원인 진단 및 해결 `[hard]`

- **변경 파일**: `crates/mcp-server/src/http_server.rs`
- **결과**: 구버전 doxus 바이너리(Thu 10PM 빌드)가 다른 bridge token으로 실행 중이었음을 확인. 프로세스 kill → 재빌드(OAuth 엔드포인트 제거 반영) → 올바른 토큰으로 재시작 후 38개 도구 정상 응답 확인. `oauth-protected-resource` → 404, Bearer 인증 → 200 검증 완료.

### v0.1.2 릴리즈 — 버전 bump, DMG 빌드, GitHub 릴리즈 `[medium]`

- **변경 파일**: `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json`, `Cargo.lock`
- **결과**: 0.1.1 → 0.1.2 버전 bump 완료. DMG 빌드, tar.gz 서명, latest.json 생성, GitHub 릴리즈 성공. 단, `apps/desktop/src-tauri/Cargo.toml`의 version이 0.1.1로 하드코딩된 버그를 포함한 채 릴리즈됨.

### v0.1.3 릴리즈 — src-tauri/Cargo.toml 버전 mismatch 수정 `[medium]`

- **변경 파일**: `Cargo.toml`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json`, `Cargo.lock`
- **결과**: `apps/desktop/src-tauri/Cargo.toml`의 `version = '0.1.1'` 하드코딩을 `version.workspace = true`로 교체. `env!(CARGO_PKG_VERSION)`이 workspace 버전을 올바르게 반영하게 되어 Tauri 업데이터의 0.1.2 == 0.1.2 오판정 해소. 0.1.3 DMG 재빌드 및 릴리즈 완료.

## 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|---------|------|---------|
| 구버전 doxus-mcp 바이너리(Thu 10PM 빌드)가 백그라운드에서 다른 bridge token으로 계속 실행 중 — 신규 빌드 바이너리의 토큰과 mismatch로 모든 MCP 요청 401 반환 | critical | 완료 | 구버전 프로세스 kill, 최신 빌드로 재시작하여 토큰 일치시킴 |
| v0.1.2 릴리즈 시 `apps/desktop/src-tauri/Cargo.toml` 버전 누락으로 0.1.1 하드코딩 잔존 — `env!(CARGO_PKG_VERSION)`이 0.1.1을 반환하고 `tauri.conf.json`은 0.1.2를 반환해, Tauri 업데이터가 '최신버전'으로 오표시 | high | 완료 | v0.1.3에서 `version.workspace = true`로 교체하고 전체 버전을 0.1.3으로 bump하여 재릴리즈 |

## 배운점

- Tauri 프로젝트에서 workspace `Cargo.toml`과 `apps/desktop/src-tauri/Cargo.toml`의 버전을 별도 관리하면 mismatch 발생 위험이 있음 — `version.workspace = true`로 단일 소스 관리가 필수다.
- doxus-mcp처럼 정적 Bearer 토큰으로 인증하는 서버는, 이전 빌드 프로세스가 살아있을 때 토큰 불일치로 전체 인증이 실패한다. 배포 전 동일 포트를 점유한 구버전 프로세스 여부를 반드시 확인해야 한다.
- MCP SDK 1.x는 `/.well-known/oauth-protected-resource`를 선제적으로 요청한다. 해당 엔드포인트가 없으면 SDK가 에러를 낸 뒤 연결을 포기한다.
- OAuth 관련 엔드포인트를 추가했다가 오히려 SDK가 OAuth 루프에 빠지는 역효과가 발생했다. 최종적으로 해당 엔드포인트를 제거하는 것이 올바른 해법이었다.

## 개선할점

- 릴리즈 체크리스트에 'workspace 하위 모든 `Cargo.toml`의 version 필드가 workspace 위임인지 확인' 항목 추가 필요.
- doxus-mcp 시작 스크립트에 동일 포트 선점 프로세스 자동 감지 및 경고 로직 추가 고려.
- 릴리즈 자동화 스크립트 부재 — release 스킬 또는 `cargo-release` + GitHub Actions로 자동화 권장.

## 하네스 개선 제안

<!-- rule_candidate: 릴리즈 시 4개 파일 수동 수정 중 src-tauri/Cargo.toml 누락으로 버그 릴리즈 발생 -->
**제안**: `git-workflow.md`에 '릴리즈 버전 bump 체크리스트' 섹션 추가
**근거**: v0.1.2에서 누락 → v0.1.3 긴급 재릴리즈 필요

<!-- skill_candidate: 버전 bump → cargo build → DMG 빌드 → tar.gz 서명 → latest.json → gh release create 순서를 두 번 연속 수동 실행 -->
**제안**: release 스킬 도입으로 단일 커맨드 자동화
**근거**: v0.1.2, v0.1.3 두 번 연속 동일 절차 반복

## 관련 문서

- [[plugin-dev-guide 초안 작성 + Opus 리뷰 반영 + MCP OAuth 디버깅]]
- [[doxus_index_project 버그 4건 수정 (TDD)]]
