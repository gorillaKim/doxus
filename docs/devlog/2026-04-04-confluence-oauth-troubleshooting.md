---
title: "Confluence OAuth 통합 트러블슈팅 — Phase 2d"
aliases:
  - confluence-oauth-troubleshooting
  - oauth-troubleshooting-phase2d
  - Confluence OAuth 트러블슈팅
tags:
  - devlog
  - troubleshooting
  - oauth
  - tauri
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# Confluence OAuth 통합 트러블슈팅 — Phase 2d

## 배경

Phase 2d에서 Confluence OAuth 2.0 PKCE 플로우를 Tauri 데스크톱 앱에 통합하는 과정에서 macOS 환경, Tauri API 제약, Atlassian OAuth 정책이 맞물려 여러 문제가 발생했다. 각 문제의 원인과 해결 방법을 기록한다.

## 변경 내용

### 주요 변경사항

**문제 1: Deep Link dev 모드 미동작**

- **증상**: `tauri_plugin_deep_link::init()` + `register("doxus")` 등록 후에도 콜백 미수신
- **원인**: macOS에서 `.app` 번들 없이는 `LSSetDefaultHandlerForURLScheme`이 제대로 동작하지 않음. dev 모드(`cargo tauri dev`)는 번들 없이 실행되므로 커스텀 URL 스킴 등록이 무효
- **해결**: `tauri-plugin-oauth`로 전환 — localhost HTTP 서버(`http://localhost:14920`) 방식 사용. deep link 등록 코드는 향후 프로덕션 배포 시 활용을 위해 유지

---

**문제 2: Tauri 이벤트 이름에 `.` 불허**

- **증상**: `oauth-callback-com.doxus.confluence` 이름으로 `listen()` 시 "Event name must include only alphanumeric characters..." 오류
- **원인**: Tauri 이벤트 이름 규격에서 `.`(점) 문자 불허
- **해결**: `plugin_id.replace('.', "_")` 적용 → `oauth-callback-com_doxus_confluence`로 이벤트 이름 정규화

---

**문제 3: OauthConfig `response` 필드 타입 오류**

- **증상**: `response: Some("...".to_string())` → `E0308 mismatched types` 컴파일 오류
- **원인**: `OauthConfig.response` 필드 타입이 `Option<Cow<'static, str>>`
- **해결**: `Some(std::borrow::Cow::Borrowed("..."))` 사용

```rust
// 잘못된 예
response: Some("인증이 완료되었습니다.".to_string()),

// 올바른 예
response: Some(std::borrow::Cow::Borrowed("인증이 완료되었습니다. 이 창을 닫으세요.")),
```

---

**문제 4: Atlassian redirect_uri 불일치**

- **증상**: OAuth 시작 시 "앱의 콜백 URL이 유효하지 않습니다" 오류
- **원인**: `tauri-plugin-oauth` 기본 동작은 랜덤 포트 할당. Atlassian은 등록된 redirect_uri와 정확히 일치해야 하며, `http://localhost`(포트 없음)와 `http://localhost:14920`을 다른 URL로 취급
- **해결**: `OauthConfig { ports: Some(vec![14920]), .. }` 고정 포트 사용, Atlassian 앱 설정에 `http://localhost:14920` 등록

```rust
let config = OauthConfig {
    ports: Some(vec![14920]),
    response: Some(std::borrow::Cow::Borrowed("인증 완료. 이 창을 닫으세요.")),
};
```

---

**문제 5: OAuth 콜백 이벤트 미수신 (race condition)**

- **증상**: 브라우저에서 `localhost:14920?code=...` 수신 확인됐지만 앱 UI 미반응
- **원인**: `plugin_open_url`(브라우저 열기) → 이벤트 리스너 등록 순서. 사용자가 빠르게 인증을 완료하면 리스너 등록 전에 이벤트 발생 → 유실
- **해결**: 리스너 등록을 브라우저 열기보다 먼저 수행

```typescript
// 잘못된 순서 (race condition 발생 가능)
await invoke('plugin_open_url', { url: authUrl });
await listen('oauth-callback-com_doxus_confluence', handler);

// 올바른 순서
await listen('oauth-callback-com_doxus_confluence', handler);
await invoke('plugin_open_url', { url: authUrl });
```

### 영향 범위

- `apps/desktop/src-tauri/src/commands/market.rs`: OauthConfig 고정 포트, 이벤트 이름 정규화
- `apps/desktop/src/pages/MarketPage.tsx`: 리스너 등록 순서 수정

## 결과

5개 문제를 모두 해결하여 Confluence OAuth PKCE 플로우가 dev 환경에서 안정적으로 동작한다.

## 교훈

- Tauri dev 모드 ≠ 프로덕션 번들: OS 레벨 등록이 필요한 기능(커스텀 URL 스킴, 시스템 트레이 등)은 dev 모드에서 다르게 동작할 수 있음
- 외부 OAuth 제공자의 redirect_uri 검증은 엄격함 — 포트 번호 포함 정확히 일치해야 함
- 비동기 이벤트 리스너는 항상 이벤트 발생 트리거보다 먼저 등록
- Rust 타입 오류 시 `Cow<'static, str>` 패턴 기억: `String`이 아닌 `Borrowed` 래퍼 필요

## 관련 문서

- [[2026-04-04-phase2d-plugin-auth-ui]]
- [[001-oauth-localhost-over-deeplink]]
