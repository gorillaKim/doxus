---
title: "워크스페이스 버그픽스 3건"
aliases:
  - workspace-bugfixes-2026-04-13
  - 워크스페이스 버그픽스
tags:
  - devlog
  - troubleshooting
  - bugfix
  - rust
  - typescript
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

# 워크스페이스 버그픽스 3건

## 배경

데스크톱 빌드 및 코어 테스트 실행 중 발생한 버그 3건을 수정했다.
Rust 테스트 단언 오류 1건, TypeScript 컴파일 에러 2건으로 구성된다.

## 변경 내용

### 주요 변경사항

#### 1. `replace_section` 테스트 — substring 매칭 문제 수정

**파일:** `crates/core/src/document/section.rs`

테스트에서 `new_content`를 `"새로운 배경 내용입니다."`로 설정했는데, 이 문자열이 단언 대상인 `"배경 내용입니다."`를 부분 문자열로 포함하여 `!result.contains(...)` 단언이 항상 실패했다.

```rust
// before
let new_content = "## 배경\n\n새로운 배경 내용입니다.\n";
assert!(!result.contains("배경 내용입니다."));

// after
let new_content = "## 배경\n\n완전히 교체된 내용입니다.\n";
assert!(!result.contains("배경 내용입니다."));
```

---

#### 2. `MarketPage.tsx` — `Array.at()` lib 타겟 불일치 수정

**파일:** `apps/desktop/src/pages/MarketPage.tsx`

`Array.prototype.at()`은 ES2022 이후 추가된 메서드로, tsconfig의 lib 타겟이 낮아 타입 정의가 없어 컴파일 에러가 발생했다. 인덱스 접근 방식으로 대체했다.

```typescript
// before
const emoji = [...e.target.value].at(-1) ?? '';

// after
const arr = [...e.target.value];
const emoji = arr[arr.length - 1] ?? '';
```

---

#### 3. `useWorkspaceStore.ts` — Zustand 미사용 변수 `get` 제거

**파일:** `apps/desktop/src/stores/useWorkspaceStore.ts`

`create<WorkspaceState>((set, get) => ...)` 에서 `get`을 선언만 하고 사용하지 않아 TS6133 에러가 발생했다. 선언에서 제거했다.

```typescript
// before
export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({

// after
export const useWorkspaceStore = create<WorkspaceState>((set) => ({
```

### 영향 범위

- `crates/core/src/document/section.rs` — 테스트 코드만 수정, 로직 변경 없음
- `apps/desktop/src/pages/MarketPage.tsx` — 동작 동일, 타입 호환성 확보
- `apps/desktop/src/stores/useWorkspaceStore.ts` — 동작 동일, 불필요한 인자 제거

## 결과

- Rust `replace_section` 테스트 통과
- TypeScript 컴파일 에러 0건
- 런타임 동작 변경 없음

## 교훈

- 테스트 단언 작성 시 새 내용이 기존 내용의 부분 문자열이 되지 않도록 주의한다.
- `Array.prototype.at()`처럼 신규 JS 메서드 사용 시 tsconfig `lib` 타겟을 먼저 확인하거나, 범용적인 인덱스 접근으로 대체한다.
- Zustand `create` 콜백에서 실제로 필요한 인자만 선언한다.

## 관련 문서

- [[module-map]]
- [[tech-stack]]
