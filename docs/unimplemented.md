---
title: doxus 통합 미구현 및 TODO 관리대장 (SSOT)
updated: 2026-06-11
---

# doxus 통합 미구현 및 TODO 관리대장 (SSOT)

> **최종 업데이트:** 2026-06-11  
> **기준 상태:** v0.1.3 (Core Engine 및 Desktop Beta)  
> **목적:** doxus 프로젝트의 실제 미구현 항목과 할 일(TODO) 목록을 일원화하여 관리하는 Single Source of Truth(SSOT) 문서입니다.

---

## 1. 최근 구현 완료 항목 (이전 대비 완료)

이전 분석서 및 TODO 문서에서 미구현으로 식별되었으나, 실제 구현 완료 및 검증된 핵심 기능들입니다.

| 기능 영역 | 구체적 구현 사항 | 관련 파일/모듈 |
|:---|:---|:---|
| **LTM 요약** | 문서 인덱싱 단계에서 `lead3_extract()`를 통해 마크다운 파싱 및 500자 요약을 추출하여 `documents.summary` 컬럼에 자동 영속화 | `crates/core/src/summarizer.rs`, `V41__add_summary_column.sql` |
| **피드백 랭킹** | 에이전트로부터 `doxus_record_feedback` MCP 도구 요청을 받아 SQLite에 기록하고, 하이브리드 검색(FTS5 + Vec) 시 피드백 스코어 가중치를 랭킹 보정에 자동 반영 | `V42__feedback_schema.sql`, `crates/core/src/search/mod.rs` |
| **공동 참조 (Co-Refs)** | 에이전트가 동일 세션에서 공동 참조한 문서 쌍의 관계(`document_co_refs`)를 트래킹하고, 스케줄러를 통해 90일 이상 미사용된 관계를 정리하는 가지치기(`co_refs_prune`) 태스크 가동 | `V43__co_refs_schema.sql`, `crates/core/src/scheduler/executor.rs` |
| **자동 업데이트** | `tauri-plugin-updater` 연동, ED25519 서명 검증을 거친 업데이트 다운로드 및 재시작 시 자동 실행. `detect_and_migrate`를 이용한 post-update 마이그레이션 훅 완료 | `apps/desktop/src-tauri/src/main.rs`, `doxus_desktop_lib::update_manager` |
| **설정 영속화** | 테마(light / dark / system) 선택, 로컬 디버그 태그 설정 및 API 저장 상태를 `config.toml`과 동기화하는 Tauri 커맨드 및 UI 연동 완료 | `doxus_desktop_lib::commands::settings`, `SettingsPage.tsx` |
| **인증 상태 IPC** | SecretStore 기반으로 Confluence 및 GitHub의 인증 필수 필드가 구성되어 있는지 확인하여 UI에 bool 값을 넘겨주는 `plugin_get_auth_status` IPC 구현 | `apps/desktop/src-tauri/src/commands/market.rs` |

---

## 2. 폐지 및 스펙 아웃(Spec-Out) 항목

* **Phase 7 워크스페이스(Workspace) 기능 전면 폐지**:  
  아키텍처 단순화와 Obsidian 등 마크다운 기반 기본 저장소 플러그인 강화에 따라, 독자적인 워크스페이스 DB 테이블 및 관련 템플릿 로직(`workspace_apply_template` 등)은 스펙에서 완전히 제외되었습니다. 이에 따라 관련된 모든 UI 및 IPC API TODO 항목이 정리되었습니다.

---

## 3. 잔존 미구현 및 TODO 목록

### 3.1 Phase 3: Confluence 플러그인 안정화
* **Confluence OAuth 토큰 자동 갱신 흐름 완비**
  * **현재 상태:** `is_expired()` 판정 및 `refresh_token()` API는 구현되어 작동하나, 실제 Confluence 플러그인이 API HTTP 요청을 보낼 때 만료 여부를 매번 사전에 검증하고 갱신을 실행하는 연동 흐름이 일부 누락되어 보완이 필요함.

### 3.2 Phase 4-5: 에코시스템 및 마켓플레이스
* **플러그인 마켓플레이스 UI와 RegistryClient 연동**
  * **현재 상태:** `RegistryClient`는 정적 주소의 레지스트리를 읽을 수 있게 완성되었으나, `MarketPage.tsx`가 여전히 `MOCK_PLUGINS` 하드코딩 배열을 렌더링하고 있음. 실제 API 호출 결과와 연동 필요.
* **WASM 플러그인 릴리즈 서명 자동화**
  * **현재 상태:** 데스크톱 앱 내에서 `verify_plugin()`을 통한 ED25519 서명 검증은 완비되었으나, 플러그인 개발자가 손쉽게 서명을 생성하고 배포할 수 있는 CI 파이프라인 및 서명 생성 CLI 도구가 부족함.
* **플러그인 SemVer 버전 해석기 개선**
  * **현재 상태:** 버전 단순 문자열 매칭만 지원하여, 유연한 semver range(`^`, `~`) 파싱 및 최적 버전 확인 매커니즘 고도화 필요.
* **공식 레지스트리 서버(Registry Server) 구축**
  * **현재 상태:** `docs/specs/registry-server-spec.md` 명세서 기반으로 GitHub Pages 또는 S3 상에 정적 JSON 레지스트리 파일 업로드 및 WASM 바이너리 호스팅 필요.

### 3.3 Phase 8: Desktop UI 고도화 (잔여 과제)
* **DashboardPage 실시간 동기화 업데이트**
  * **현재 상태:** 리소스 사용량에 대해서는 5초 주기 폴링이 돌고 있으나, 전체 문서 수량 및 동기화 상태 등은 Tauri 이벤트 리스너(`sync:progress/complete`)와 온전히 동기화되지 않고 있음. 최초 로드 이후 실시간 이벤트 수신 렌더링 처리 필요.
* **키보드 단축키 (Shortcut)**
  * `Cmd+K` (검색창 포커스), `Cmd+J` (에이전트 ChatDrawer 토글), `Cmd+N` (새 파일 작성) 등의 글로벌 단축키 작동을 위한 전역 keydown 이벤트 핸들러(`AppShell.tsx`) 연동 필요.
* **시스템 트레이 (System Tray)**
  * `tauri-plugin-tray` 의존성을 추가하여 최소화 시 트레이로 가기, 트레이 빠른 동기화 메뉴, 완전 종료 등의 윈도우 라이프사이클 래핑 필요.
* **오프라인 감지 및 처리**
  * `navigator.onLine` 상태를 감지하여 네트워크가 차단되었을 때 오프라인 경고 배너를 노출하고, 외부 플러그인 동기화 기능을 자동으로 비활성화하는 가드 구현.
* **성능 최적화 (가상 스크롤 및 디바운스)**
  * 검색 결과가 매우 많을 경우의 렌더링 부하를 줄이기 위한 가상 스크롤(Virtual Scroll) 적용 및 검색 입력창의 디바운스(Debounce) 튜닝.
* **웹 접근성 (Accessibility)**
  * 스크린 리더 지원을 위한 주요 버튼/인풋에 `aria-label` 부여 및 Tab 키 네비게이션 포커스 링 개선.

### 3.4 기타 및 배포 고도화
* **CLI commands 검증 및 누락 CLI 보완**
  * CLI 메인 모듈 내 `doxus sync` 및 `doxus agent start` 등 정의되지 않거나 partial stub인 도구 점검 및 완성.
* **릴리즈 빌드 최적화**
  * production 빌드 시 Vite의 코드 분할(chunking, lazy import) 설정을 개선하여 첫 로드 속도 최적화.

---

## 4. 작업 규모 요약

| 카테고리 | 잔여 항목 수 | 예상 총 LOC | 작업 우선순위 |
|:---|:---:|:---:|:---:|
| **Confluence 토큰 자동 갱신** | 1 | ~80 LOC | 높음 |
| **마켓플레이스 UI 실데이터 연동** | 1 | ~200 LOC | 보통 |
| **SemVer 버전 해석 & 서명 도구** | 2 | ~250 LOC | 보통 |
| **Desktop UI 고도화 (트레이/단축키/실시간 등)** | 6 | ~400 LOC | 보통~낮음 |
| **레지스트리 및 배포 최적화** | 2 | ~200 LOC | 낮음 |
| **합계** | **12** | **~1,130 LOC** | |

**총 작업 예상 규모:** 집중 개발 시 1인 기준 약 1~2주 소요.
