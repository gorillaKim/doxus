---
title: "Phase 2d — 플러그인 인증 설정 UI 및 OAuth 플로우 구현"
aliases:
  - phase2d-plugin-auth-ui
  - plugin-auth-ui
  - 플러그인 인증 UI
  - Phase 2d 인증
tags:
  - devlog
  - feature
  - oauth
  - tauri
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# Phase 2d — 플러그인 인증 설정 UI 및 OAuth 플로우 구현

## 배경

Phase 2d는 외부 플러그인(Confluence, GitHub 등)의 인증 흐름을 앱 내에서 완결하는 것이 목표다. 플러그인마다 인증 방식이 다르므로(`none` / `api_token` / `oauth`), 각 방식에 맞는 UI를 동적으로 렌더링하고, OAuth의 경우 PKCE 기반 콜백 수신까지 처리해야 했다. 기존 `config_schema`(프로젝트 추가용)와 인증 스키마를 분리하여 관심사를 명확히 구분하는 설계가 선행 과제였다.

## 변경 내용

### 주요 변경사항

**플러그인 인증 스키마 분리**
- `auth_type` + `auth_schema` 개념 도입 — `config_schema`(프로젝트 추가용)와 분리
- 각 플러그인이 자신의 인증 방식을 선언: `none` / `api_token` / `oauth`

**MarketPage UI 개선**
- `PluginSettingsModal` 컴포넌트 — `auth_type`에 따라 다른 UI 렌더링
- 설치된 플러그인 카드에 "설정" 버튼 추가
- 인증 상태 뱃지: ● 인증됨 (emerald) / ○ 미인증 (yellow)

**OAuth 2.0 PKCE 플로우 (Confluence)**
- `tauri-plugin-deep-link`: `doxus://` 커스텀 URL 스킴 등록 (향후 프로덕션용)
- `tauri-plugin-oauth`: localhost HTTP 서버 기반 OAuth 콜백 수신 (포트 14920 고정)
- Confluence OAuth 2.0 PKCE flow — `client_secret` 불필요, public app으로 등록
- Atlassian 필요 스코프: `read:confluence-content.all`, `read:confluence-space.summary`, `offline_access`

**신규 Tauri 커맨드**
- `plugin_start_oauth(plugin_id, client_id)` — PKCE pair 생성, OAuth 서버 시작, `auth_url` 반환
- `plugin_oauth_exchange(plugin_id, code)` — code → access_token 교환, keychain 저장
- `plugin_validate_config(plugin_id, config_fields)` — Confluence/GitHub 연결 헬스체크
- `plugin_open_url(url)` — 기본 브라우저에서 URL 열기

**상태 관리**
- `usePluginStore` Zustand 스토어 신규 생성 — 플러그인별 인증 상태 중앙 관리
- `AppState`에 `oauth_pending: Mutex<HashMap<String, OAuthPending>>` 추가

### 영향 범위

| 파일 | 변경 내용 |
|------|-----------|
| `apps/desktop/src-tauri/Cargo.toml` | tauri-plugin-deep-link, tauri-plugin-oauth, sha2, base64, rand, reqwest 추가 |
| `apps/desktop/src-tauri/tauri.conf.json` | deep-link 플러그인 설정 추가 |
| `apps/desktop/src-tauri/src/state.rs` | `OAuthPending` 구조체, `oauth_pending` 필드 추가 |
| `apps/desktop/src-tauri/src/commands/market.rs` | 4개 신규 커맨드 + `auth_type`/`auth_schema` 필드 |
| `apps/desktop/src-tauri/src/main.rs` | 플러그인 등록 + 커맨드 등록 |
| `apps/desktop/src/stores/usePluginStore.ts` | 신규 Zustand 스토어 생성 |
| `apps/desktop/src/pages/MarketPage.tsx` | `PluginSettingsModal`, 카드 뱃지, `onAuthChange` 연동 |

## 결과

- 플러그인별 인증 방식을 선언적으로 정의하고, UI가 이를 자동으로 렌더링하는 구조 완성
- Confluence OAuth 2.0 PKCE 콜백 수신부터 keychain 저장까지 전체 플로우 구현
- 인증 상태가 `usePluginStore`로 중앙화되어 카드 뱃지와 설정 모달이 일관된 상태를 공유

## 교훈

- `auth_schema`와 `config_schema` 분리 원칙: 인증(자격증명)과 프로젝트 설정(URL, 공간 키 등)은 별도 스키마로 관리해야 재사용성이 높아짐
- macOS dev 모드에서 deep link가 동작하지 않는 제약 → `tauri-plugin-oauth` localhost 서버 방식이 현실적인 대안
- OAuth 콜백 이벤트 race condition: 브라우저 열기 전에 리스너를 먼저 등록해야 이벤트 유실 방지
- Atlassian OAuth는 정확한 redirect_uri 매칭 필요 → 랜덤 포트 사용 불가, 14920 고정

## 관련 문서

- [[2026-04-04-confluence-oauth-troubleshooting]]
- [[001-oauth-localhost-over-deeplink]]
- [[2026-04-04-doxus-phase0-3-implementation]]
