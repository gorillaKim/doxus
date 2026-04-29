---
title: "Auto Updater & Post-Update Migration — TDD 전체 구현 (Phase 1–5)"
aliases:
  - auto-updater-migration-tdd
  - 자동업데이터-마이그레이션-tdd
  - 2026-04-29 doxus 데브로그
tags:
  - devlog
  - tdd
  - tauri
  - rust
  - ci-cd
agent_model: claude-sonnet-4-6
created: "2026-04-29"
updated: "2026-04-29"
---

<!-- docsmith: auto-generated 2026-04-29 -->

## 개요

`docs/plan/auto-updater-and-migration.md` 계획서를 기반으로 Auto Updater와 Post-Update Migration 기능을 Phase 1~5 전체 구현했다. Tauri plugin-updater 설정부터 GitHub Actions CI/CD 파이프라인, TypeScript updateManager 서비스, Rust 마이그레이션 훅, 프론트엔드 알림 UI까지 TDD 방식으로 순차 완성했다. Opus 코드 리뷰에서 CRITICAL 1개·HIGH 5개·MEDIUM 3개 이슈가 발견되어 모두 수정 반영됐다.

## 주요작업

### Phase 1: Tauri plugin-updater 설정 및 ED25519 pubkey guard `[easy]`

- **변경 파일**: `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/capabilities/default.json`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/package.json`
- **결과**: `tauri-plugin-updater` 의존성 등록 완료. `build.rs`에서 `TAURI_SIGNING_PUBLIC_KEY`가 placeholder 값이면 `compile_error!`로 빌드 타임에 즉시 실패하는 guard 포함. capabilities에 `updater:allow-*` 권한 추가.

### Phase 2: GitHub Actions release.yml + ci.yml 작성 및 Opus 리뷰 후 CRITICAL·HIGH 수정 `[hard]`

- **변경 파일**: `.github/workflows/release.yml`, `.github/workflows/ci.yml`
- **결과**: macOS universal binary 빌드 파이프라인 완성. Opus 리뷰에서 sidecar x86_64 바이너리 누락 CRITICAL과 placeholder pubkey 미검증 HIGH를 포함한 총 7건 수정 후 워크플로우 확정.

### Phase 3: updateManager.ts 구현 및 Vitest 18개 테스트 `[hard]`

- **변경 파일**: `apps/desktop/src/services/updateManager.ts`, `apps/desktop/src/services/__tests__/updateManager.test.ts`, `apps/desktop/vite.config.ts`
- **결과**: `checkForUpdates`, `downloadAndInstall`, `relaunchApp`을 `update:*` 이벤트 네임스페이스로 래핑한 서비스 구현 완료. `vi.hoisted()` 패턴으로 Tauri 플러그인 mock 초기화 순서 문제 해결. fake timer 대신 real timer + `timeoutMs: 50`으로 timeout 검증. 18개 테스트 통과.

### Phase 4a: V40__add_system_config.sql 마이그레이션 및 DB 함수 `[easy]`

- **변경 파일**: `crates/core/src/db/migrations/V40__add_system_config.sql`, `crates/core/src/db/mod.rs`
- **결과**: `system_config` 테이블 생성 및 `INSERT OR IGNORE` 부트스트랩(앱 버전 `0.0.0` 초기화) 포함. `get_system_config`, `set_system_config` DB 함수 추가. Rust 단위 테스트 통과.

### Phase 4b: detect_and_migrate, PostUpdateHook trait, TauriReindexHook 구현 및 Rust 테스트 11개 `[very_hard]`

- **변경 파일**: `apps/desktop/src-tauri/src/update_manager.rs`, `crates/core/src/sync_manager.rs`, `apps/desktop/src-tauri/src/lib.rs`
- **결과**: semver 버전 비교 기반 `detect_and_migrate` 엔진 구현. `PostUpdateHook` trait으로 마이그레이션 로직과 버전 감지 분리. `TauriReindexHook`은 생성자 패턴(캡슐화)으로 완성. E0597 lifetime error 해결 포함. Rust 테스트 11개 통과.

### Phase 5: migrationListener.ts, App.tsx migration toast, main.rs 시작 시 자동 실행 `[medium]`

- **변경 파일**: `apps/desktop/src/services/migrationListener.ts`, `apps/desktop/src/services/__tests__/migrationListener.test.ts`, `apps/desktop/src/App.tsx`, `apps/desktop/src-tauri/src/main.rs`
- **결과**: Tauri `migration:*` 이벤트를 구독하는 `migrationListener` 서비스 구현. StrictMode cleanup race 수정(useEffect에서 unlisten cleanup 반환). migration toast UI 완성. `main.rs` 시작 시 `detect_and_migrate` 자동 실행. Vitest 7개 테스트 통과.

### SettingsPage.tsx 업데이트 UI 섹션 추가 `[easy]`

- **변경 파일**: `apps/desktop/src/pages/SettingsPage.tsx`
- **결과**: 업데이트 확인 → 다운로드 → 설치 → 재시작 전체 플로우를 프로그레스 바 포함 UI로 완성.

## 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| release.yml sidecar x86_64 바이너리 누락 — macOS universal binary 빌드 시 x86_64 sidecar가 없어 런타임 크래시 가능 | critical | true | CI에서 x86_64-apple-darwin 타겟 별도 빌드 후 sidecar 디렉토리에 복사하는 스텝 추가 |
| release.yml 플레이스홀더 pubkey 미검증 — TAURI_SIGNING_PUBLIC_KEY가 placeholder 그대로면 서명 검증 실패 | high | true | 워크플로우 시작 시 pubkey가 PLACEHOLDER 값이면 즉시 실패하는 pre-flight 검증 스텝 추가 |
| E0597 lifetime error in force_reindex_all_projects — 클로저 내 borrow lifetime이 충분히 길지 않아 컴파일 실패 | high | true | 클로저 외부에서 named variable로 바인딩하여 lifetime 명시적으로 연장 |
| Vitest mock 초기화 순서 문제 — @tauri-apps/plugin-updater mock이 hoisting 전에 평가되어 테스트 실패 | high | true | vi.hoisted() 패턴으로 mock factory를 hoisting 단계에서 먼저 실행 |
| Fake timer 사용 시 timeout 테스트 실패 — vi.useFakeTimers() 환경에서 Promise.race timeout이 정상 동작하지 않음 | medium | true | fake timer 제거 후 real timer + timeoutMs: 50으로 실제 타임아웃 동작 검증 |
| TauriReindexHook 필드 pub 노출 — 캡슐화 위반 (Opus 코드 리뷰 지적) | medium | true | pub 필드 제거 후 생성자 패턴(new 함수) 도입으로 캡슐화 복원 |
| App.tsx React StrictMode cleanup race — useEffect cleanup 없이 이벤트 리스너 중복 등록 가능 (Opus 코드 리뷰 지적) | medium | true | useEffect에서 unlisten 함수를 cleanup으로 반환하도록 수정 |

## 배운점

- `vi.hoisted()`는 Vitest에서 mock factory를 ES module hoisting과 동일한 타이밍에 실행시켜 import 순서 문제를 해결한다. `@tauri-apps/plugin-updater` 같은 Tauri 플러그인 mocking에 필수 패턴이다.
- Rust lifetime E0597은 클로저 내부에서 외부 참조를 캡처할 때 발생한다. 클로저 진입 전 named variable로 분리하면 borrow checker가 lifetime을 정확히 추적할 수 있다.
- `semver` crate의 `Version::parse`는 prerelease 식별자를 포함한 문자열도 파싱하지만, MAJOR.MINOR.PATCH 코어만 비교할 때는 `Version::new`로 재구성해야 prerelease 간 비교 오차를 방지한다.
- Tauri plugin-updater의 Started/Progress/Finished 콜백을 UpdateManager 래퍼로 `update:*` 이벤트 네임스페이스로 단일화하면 테스트 가능성과 유지보수성이 크게 향상된다.
- GitHub Actions에서 macOS universal binary를 만들 때 arm64 네이티브 빌드와 x86_64 크로스 컴파일을 별도 수행한 뒤 sidecar 바이너리를 수동 복사해야 번들이 정상 작동한다.
- `build.rs`에서 `compile_error!`로 빌드 타임에 pubkey placeholder를 감지하면 잘못된 설정으로 배포되는 사고를 예방할 수 있다.
- `PostUpdateHook` trait 패턴은 마이그레이션 로직을 버전 감지 엔진과 분리하여 테스트 가능성을 높인다. 테스트에서 `MockHook`을 주입해 실제 재인덱싱 없이 마이그레이션 분기를 검증할 수 있다.

## 개선할점

- Phase 3 테스트에서 fake timer 문제로 실제 50ms를 기다리는 우회책을 사용했다. 향후 AbortController 기반 취소 패턴으로 리팩토링하면 fake timer와 완전히 호환되는 timeout 구현이 가능하다.
- `release.yml`에서 sidecar 바이너리 경로를 하드코딩했다. 바이너리 이름이 바뀌면 CI가 조용히 실패할 수 있으므로 `Cargo.toml`에서 동적으로 읽어오는 스크립트로 개선 필요.
- `detect_and_migrate`의 버전 분기 조건들이 `update_manager.rs`에 inline으로 작성되어 있다. 버전이 늘어나면 조건문이 길어지므로 `MigrationRegistry` 구조체로 분리하여 등록 방식으로 관리하면 가독성이 향상된다.
- `SettingsPage.tsx` 업데이트 섹션이 로컬 `useState`로 상태를 관리한다. 업데이트 상태를 Zustand `useSettingsStore`에 통합하면 다른 컴포넌트에서도 접근 가능해진다.
- V40 마이그레이션에서 기존 사용자를 `0.0.0`으로 초기화하면 불필요한 `force_reindex`가 트리거될 수 있다. `documents` 테이블 row 수로 신규 설치와 기존 사용자를 구분하는 로직 추가를 고려해야 한다.

## 하네스 개선 제안

<!-- skill_candidate: 세션에서 Opus 코드 리뷰를 2회 별도로 실행했다. 매 구현 Phase 완료 후 반복적으로 같은 패턴으로 리뷰를 요청했다. -->
**제안**: TDD 구현 세션용 'phase-review' 스킬 후보 — Phase 완료 시 자동으로 Opus code-reviewer를 트리거하고 CRITICAL/HIGH 이슈만 필터링하여 즉시 fix 루프를 시작하는 훅 추가
**근거**: 2회 Opus 리뷰 실행, 각 리뷰에서 CRITICAL·HIGH 이슈 즉시 수정 패턴 반복 감지

<!-- optimization: Phase 4a(DB 마이그레이션)와 Phase 3(updateManager.ts) 구현이 순차적으로 실행되었으나 두 작업 간 직접적인 코드 의존성이 없다. -->
**제안**: Phase 3 프론트엔드 모듈과 Phase 4a DB 마이그레이션은 병렬 실행 가능 — team executor 2개를 동시에 실행하여 전체 구현 시간 단축
**근거**: updateManager.ts는 DB system_config를 직접 참조하지 않으며 Tauri IPC 레이어만 사용함

<!-- rule_candidate: Tauri 플러그인 mock 시 vi.hoisted() 패턴을 이번 세션에서 처음 발견하고 디버깅에 시간을 소요했다. -->
**제안**: CLAUDE.md 또는 doxus 프로젝트 rules에 'Vitest + Tauri 플러그인 mock 패턴' 섹션 추가 — `vi.hoisted()` 필수 사용 및 `@tauri-apps/plugin-updater` mock 예시 코드 등록
**근거**: vi.hoisted() 미사용으로 테스트 실패 후 패턴 발견, 이후 migrationListener 테스트에서도 동일 패턴 재사용

## 관련 문서

- [[Auto Updater & Post-Update Migration Plan]]
- [[Frontend 규칙 (Tauri v2 + React 19)]]
- [[데이터베이스 규칙]]
- [[대용량 Confluence 인덱싱 버그 2건 수정 (깜빡임 + 문서 누락)]]
