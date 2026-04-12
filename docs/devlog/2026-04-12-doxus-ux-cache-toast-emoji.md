---
title: "doxus UX 개선 — 캐시 토스트, 플러그인 이모지 시스템, MarketPage 인증 폼 접힘"
aliases:
  - doxus-ux-cache-toast-emoji
  - doxus UX 토스트 이모지
  - 2026-04-12 doxus 데브로그
tags:
  - devlog
  - feature
  - ux
  - frontend
  - tauri
  - react
  - zustand
created: "2026-04-12"
updated: "2026-04-12"
---

<!-- docsmith: auto-generated 2026-04-12 -->

# doxus UX 개선 — 캐시 토스트, 플러그인 이모지 시스템, MarketPage 인증 폼 접힘

## 배경

데스크톱 앱의 사용성을 높이기 위해 여러 UX 개선 작업을 일괄 진행했다.
Rust 런타임 패닉(`no reactor running`) 버그를 수정하고,
캐시 cleanup 알림 · 검색 새로고침 피드백 · 플러그인 이모지 커스터마이징 기능을 추가했다.

## 변경 내용

### 주요 변경사항

#### 1. tokio::spawn 패닉 수정 (`main.rs`)

`tauri::Builder` 시작 전에 `tokio::spawn`을 호출해 `no reactor running` 패닉이 발생했다.
`tauri::async_runtime::spawn`으로 교체하고 `.setup()` 클로저 안으로 이동했다.
`conn_arc`를 setup 클로저에 `move` 캡처하도록 수정하고,
`emit` 메서드를 사용하기 위해 `use tauri::Emitter;` import를 추가했다.

#### 2. 캐시 cleanup 토스트 (`main.rs` + `App.tsx`)

30분 스케줄러에서 만료 캐시 제거 후 `app_handle.emit("cache:cleanup", { count: N })`을 emit한다.
`App.tsx`에 `listen("cache:cleanup")` 전역 리스너를 추가해 우측 하단에 4초 토스트를 표시한다.
스타일은 기존 `ProjectsPage`의 `fixed bottom-6 right-6` 패턴을 그대로 활용했다.

#### 3. SearchPage 새로고침 토스트 (`SearchPage.tsx`)

`fetchPreview(doc, forceRefresh=true)` 완료 시 "최신 콘텐츠로 업데이트됨" 토스트를 3초 표시한다.
`refreshToast` state와 `useRef` 타이머로 구현했다.

#### 4. MarketPage 인증 폼 접힘 UX (`MarketPage.tsx`)

인증 완료 상태에서 모달을 열어도 인증 폼이 항상 표시돼 TTL 설정 접근이 불편했다.
`showAuthForm` state를 추가해 인증된 상태에서는 폼을 숨기고 "인증 변경" 버튼만 표시하도록 변경했다.
미인증 상태에서는 기존 동작과 동일하게 폼이 바로 표시된다.
footer 저장 버튼도 `showAuthForm`일 때만 표시된다.

#### 5. 플러그인 이모지 시스템 (`usePluginStore.ts` + 각 페이지)

`usePluginStore`에 이모지 상태를 추가했다.

- `emojiMap: Record<string, string>` — localStorage에서 초기화
- `getEmoji(pluginId)` — 커스텀 이모지 → 기본 이모지 → '🔌' 순 폴백
- `setEmoji(pluginId, emoji)` — localStorage 저장 + 스토어 업데이트

기본 이모지 맵:
- `com.doxus.obsidian`: `🪨`
- `com.doxus.confluence`: `🌊`
- `com.doxus.github`: `🐙`

`PluginSettingsModal` 헤더에 인플레이스 이모지 편집 UI를 추가했다.
이모지 버튼 클릭 시 같은 자리가 input으로 교체되고, 이모지 입력 즉시 저장 후 input이 닫힌다.

`SearchPage`의 `pluginIcon()` 함수와 `ProjectsPage`의 `pluginMeta()` 함수의 아이콘 필드가
`usePluginStore.getState().getEmoji()`를 사용하도록 변경했다.
두 컴포넌트에 `usePluginStore((s) => s.emojiMap)` 구독을 추가해 리렌더를 보장한다.

#### 6. SearchPage 프로젝트 그룹 기본 접힘 (`SearchPage.tsx`)

`ProjectGroup` 컴포넌트의 `useState(true)` → `useState(false)`로 변경해 기본 접힘 상태로 변경했다.

### 영향 범위

- `apps/desktop/src-tauri/src/main.rs`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/pages/SearchPage.tsx`
- `apps/desktop/src/pages/MarketPage.tsx`
- `apps/desktop/src/pages/ProjectsPage.tsx`
- `apps/desktop/src/stores/usePluginStore.ts`

## 결과

- Rust 런타임 패닉 해소 — 앱 시작 시 `tokio` 관련 패닉 없음
- 캐시 cleanup 이벤트를 사용자가 토스트로 인지할 수 있음
- 인증 완료 플러그인의 TTL 설정 접근이 한 단계 줄어 UX 개선
- 플러그인별 이모지 커스터마이징으로 시각적 식별성 향상
- 검색 결과 그룹이 기본 접힘 상태여서 초기 화면이 간결해짐

## 교훈

| 문제 | 원인 | 해결 |
|------|------|------|
| `no method named 'emit'` 컴파일 에러 | `emit`은 `tauri::Emitter` trait에 정의됨 | `use tauri::Emitter;` import 추가 |
| `Manager` unused import 경고 | `Emitter` trait으로 충분 | `Manager` import 제거 |
| 이모지 투명 input 오버레이 방식 불일치 | 데스크톱 이모지 입력 UX 특성 | 클릭 시 인플레이스 input 교체 방식으로 변경 |
| 포트 1420 충돌 | 이전 dev server 프로세스 잔존 | `lsof -ti:1420 \| xargs kill -9` 후 재시작 |

**이모지 저장소 선택 근거**: 순수 UI 설정이라 백엔드 DB 불필요. localStorage로 충분하고 즉각적이다.

**standalone 함수에서 스토어 접근 패턴**: 컴포넌트 외부 함수(`pluginIcon`, `pluginMeta`)에서는 `usePluginStore.getState()`를 사용하고, 컴포넌트에 `usePluginStore((s) => s.emojiMap)` 구독을 추가해 이모지 변경 시 리렌더를 보장한다.

## 관련 문서

- [[doxus-frontend-rules]]
- [[docs/devlog/2026-04-10-doxus-sqlite-vec-wasm-bridge-mcp-extraction]]
