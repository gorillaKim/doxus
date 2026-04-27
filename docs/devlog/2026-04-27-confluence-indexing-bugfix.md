---
title: "대용량 Confluence 인덱싱 버그 2건 수정 (깜빡임 + 문서 누락)"
aliases:
  - confluence-indexing-bugfix
  - 컨플루언스-인덱싱-버그수정
  - 2026-04-27 doxus 데브로그
tags:
  - devlog
  - troubleshooting
  - confluence
  - indexing
  - frontend
agent_model: claude-sonnet-4-6
created: "2026-04-27"
updated: "2026-04-27"
---

<!-- docsmith: auto-generated 2026-04-27 -->

## 개요

Confluence 대용량 인덱싱(500+ 문서) 환경에서 발생하는 버그 2건을 수정했다.
설정 페이지에서 진행 중인 프로젝트 숫자가 순간적으로 튀는 race condition과,
인덱싱 완료 후 검색 페이지에 23개 문서만 표시되는 캐시 갱신 누락 문제가 주된 원인이었다.

## 주요작업

### 설정 페이지 '처리 중인 프로젝트' 숫자 깜빡임/튐 버그 수정 (93→600 순간 표시) `[hard]`

- **변경 파일**: `crates/core/src/sync_manager.rs`, `apps/desktop/src-tauri/src/commands/search.rs`
- **결과**: `force_mark_task_started` + `mark_task_started/done` 추가로 인덱싱 시작 시점부터 정확한 카운트 유지. 폴링이 task 등록보다 먼저 실행되는 race condition 해소.

### 검색 페이지 인덱싱 완료 후 23개 문서만 표시 버그 수정 (실제 500+개) `[medium]`

- **변경 파일**: `apps/desktop/src/pages/SearchPage.tsx`
- **결과**: `project-indexed` 이벤트 수신 시 `listAllDocuments` 갱신 로직 추가. 인덱싱 완료 신호를 받는 즉시 문서 목록을 재조회하도록 처리.

### Confluence V2 API limit 파라미터 버그 및 updated_at fallback 수정 `[medium]`

- **변경 파일**: `crates/plugins/confluence/src/lib.rs`
- **결과**: V2 API에 limit 파라미터를 올바르게 전달하도록 수정. `updated_at` 필드가 없는 문서에 대한 fallback 처리 추가.

### indexing.rs 빈 페이지 처리 로직 및 무한루프 방어 코드 추가 `[medium]`

- **변경 파일**: `crates/core/src/indexing.rs`
- **결과**: 빈 페이지 반환 시 루프 종료 조건 명시, 무한루프 방어. Confluence API가 빈 결과를 반환할 때 명시적 break로 안전하게 탈출.

## 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| 인덱싱 시작 전 task가 등록되지 않은 상태에서 폴링이 먼저 실행되어 숫자가 튀는 race condition | high | true | `force_mark_task_started`로 인덱싱 시작 즉시 task를 sync_manager에 등록 |
| Confluence V2 API에서 limit 파라미터가 잘못 전달되어 페이지당 최소 문서만 반환 | high | true | V2 API limit 파라미터 수정 및 `updated_at` 필드 fallback 처리 추가 |
| 인덱싱 완료 후 SearchPage가 문서 목록을 갱신하지 않아 기존 캐시된 23개만 표시 | high | true | `project-indexed` 이벤트 핸들러에서 `listAllDocuments` 재호출 |
| indexing.rs에서 빈 페이지 반환 시 루프 탈출 조건 미비로 무한루프 가능성 | medium | true | 빈 페이지 감지 시 명시적 break 로직 추가 |

## 배운점

- Tauri 이벤트 기반 UI 갱신에서 race condition은 task 등록 시점과 폴링 시점의 선후 관계에서 발생한다. 폴링 주기가 짧을수록 미등록 상태에서 조회될 가능성이 높아짐.
- Confluence V2 API는 V1과 limit 파라미터 형식이 다를 수 있어 버전별 별도 검증이 필요하다.
- 대용량 인덱싱(500+ 문서)에서 발생하는 버그는 소규모 테스트에서 재현이 안 된다. 통합 테스트에 대용량 시나리오를 포함해야 실질적으로 검증 가능하다.
- Gemini 초기 수정 후 Opus 리뷰에서 추가 문제가 발견됨 — 멀티 에이전트 리뷰 단계가 실질적 가치를 가진다.

## 개선할점

- 인덱싱 시작 시 UI 측 낙관적 업데이트(optimistic update) 적용을 고려해 task 등록 지연 문제를 근본적으로 해소.
- Confluence 플러그인 통합 테스트에 대용량(100+ 문서) 시나리오 추가 필요.
- `sync_manager`의 task 생명주기를 명시적 상태 머신으로 관리하면 race condition 유형 버그를 구조적으로 예방할 수 있음.
- `project-indexed` 이벤트 외 `index_progress` 이벤트에서도 점진적 문서 카운트 갱신을 고려해 UX 개선.

## 하네스 개선 제안

<!-- optimization: Gemini 초기 수정 후 Opus 리뷰를 별도 턴에서 순차 실행함 -->
**제안**: exec→review 단계를 자동으로 연결하는 파이프라인 구성
**근거**: 두 에이전트 작업 간 의존성은 수정 완료 여부뿐이므로, 수정 완료 시그널을 트리거로 리뷰 에이전트를 자동 기동할 수 있음

## 관련 문서

- [[Confluence 검색 점수 0.00 버그 수정 — 표시 버그 + 문서 청킹 구현]]
- [[doxus UX 개선 — 캐시 토스트, 플러그인 이모지 시스템, MarketPage 인증 폼 접힘]]

---

## 2026-04-27 세션 2 — 설정 페이지 인덱싱 진행률 표시 및 재인덱싱 버그 수정

<!-- docsmith: auto-generated 2026-04-27 -->

### 주요작업

#### 설정 페이지 인덱싱 진행률 X/Y개 표시 추가 `[medium]`

- **변경 파일**: `crates/plugins/confluence/src/lib.rs`, `crates/core/src/indexing.rs`, `crates/core/src/sync_manager.rs`, `apps/desktop/src-tauri/src/commands/search.rs`, `apps/desktop/src/pages/SettingsPage.tsx`
- **결과**: ConfluenceCqlResult.totalSize → DocumentStream.estimated_total → on_progress(total_docs) → index_progress 이벤트 → UI X/Y개 형식 표시까지 전체 파이프라인 연결 완료

#### 강제 재인덱싱 시 active task 미표시 버그 수정 `[medium]`

- **변경 파일**: `apps/desktop/src-tauri/src/commands/search.rs`, `crates/core/src/sync_manager.rs`
- **결과**: trigger_reindex에서 force_mark_task_started를 즉시 호출하고 run_task에서 중복 체크 제거

#### init_watchers/start_loop 동시 실행 버그 도입 및 수정 `[hard]`

- **변경 파일**: `apps/desktop/src-tauri/src/main.rs`, `crates/core/src/sync_manager.rs`
- **결과**: 독립 task 분리 시도 → 동시 인덱싱 2x 카운팅 발생 → 순차 실행 + 선등록 방식으로 올바르게 수정

#### sqlite-vec chunk_embeddings COUNT(*) 항상 0 반환 버그 수정 `[easy]`

- **변경 파일**: `apps/desktop/src-tauri/src/commands/market.rs`
- **결과**: chunks 테이블 카운트로 교체하여 정확한 청크 수 반환

#### build-dmg.sh에 Confluence WASM 자동 빌드 추가 `[easy]`

- **변경 파일**: `scripts/build-dmg.sh`
- **결과**: DMG 빌드 시 WASM 플러그인 자동 재빌드 포함

### 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| init_watchers와 start_loop를 독립 task로 분리 시 동시 인덱싱으로 문서 수 2배 카운팅 | high | true | 순차 실행으로 되돌리고 trigger_reindex에서 force_mark_task_started로 선등록 |
| sqlite-vec 가상 테이블 COUNT(*)가 항상 0 반환 | medium | true | chunks 테이블 카운트로 교체 |
| 강제 재인덱싱 후 UI active task 즉시 미표시 | medium | true | trigger_reindex 호출 시점에 force_mark_task_started 선등록 |

### 배운점

- sqlite-vec 가상 테이블은 COUNT(*)가 정상 동작하지 않으므로 실제 데이터 테이블 카운트 사용 필요
- 비동기 task를 독립 실행으로 분리할 때 동일 리소스 경쟁 조건 반드시 검토 필요
- Confluence CQL API totalSize 필드로 전체 문서 수 파악 가능, estimated_total로 진행률 UI 활용

### 개선할점

- SyncManager task 상태 전이 로직이 분산되어 있어 상태 머신 패턴 리팩토링 고려
- sync_manager 병렬 실행 시나리오에 대한 통합 테스트 추가 필요
- progress_callback 타입을 구조체로 래핑하면 향후 필드 추가 시 변경 범위 감소

### 하네스 개선 제안

<!-- rule_candidate: sqlite-vec 가상 테이블 COUNT(*) 문제가 database.md 규칙에 미문서화 -->
**제안**: database.md에 'sqlite-vec 가상 테이블 COUNT(*) 사용 금지' 규칙 추가
**근거**: chunk_embeddings COUNT(*) → chunks COUNT 교체 패턴으로 실제 문제 확인됨
---

## 2026-04-27 세션 3 — 반복 인덱싱 버그(50개 누락) 근본 해결 및 DMG 빌드 안정화

<!-- docsmith: auto-generated 2026-04-27 -->

### 주요작업

#### 무한 반복 인덱싱 버그(기록 없음/청크 0개) 해결 `[hard]`

- **변경 파일**: `crates/core/src/indexing.rs`, `crates/core/src/search/mod.rs`
- **결과**: 증분 인덱싱 시마다 약 50개의 문서가 매번 다시 인덱싱되는 현상 해결.
    - **Clock Drift 방지**: Rust의 `sync_start_time`과 SQLite의 저장 시점 차이로 인해 인덱싱 직후 문서가 삭제되던 로직을, Rust에서 생성한 고정 타임스탬프를 DB 파라미터로 전달하도록 통일하여 해결.
    - **빈 본문 배치 저장 보장**: 본문이 없는 문서(청크 0개)가 포함된 배치가 임베딩 과정에서 통째로 스킵되어 DB에 기록되지 않던 버그 수정. 본문이 없어도 `documents` 레코드는 저장하여 다음 인덱싱 시 `[skip]`되도록 함.
    - **재인덱싱 판단 기준 교정**: 청크가 0개라는 이유만으로 `needs_reindexing`을 트리거하던 과잉 로직 제거.

#### DMG 패키지 빌드 타겟 및 플러그인 로딩 최적화 `[medium]`

- **변경 파일**: `scripts/build-dmg.sh`
- **결과**: `wasm32-wasip1` 타겟 적용 및 네이티브 전용 플러그인 제외 설정을 통해 전체 빌드 및 패키징 정상화. (74MB `doxus_0.1.0_aarch64.dmg` 생성 확인)

#### 디버깅 로깅 시스템 강화 `[easy]`

- **변경 파일**: `crates/core/src/indexing.rs`, `crates/core/src/search/mod.rs`
- **결과**: 인덱싱 결정 이유(기록 없음, 시간 다름, 해시 다름 등)를 상세히 출력하도록 `log_d!` 매크로 및 `tracing` 로그 보강.

### 이슈

| 이슈 | severity | 해결 | 해결방법 |
|------|----------|------|---------|
| 인덱싱 시작 시간과 DB 저장 시간의 미세한 차이(Clock Drift)로 인해 방금 인덱싱한 문서가 삭제됨 | high | true | 모든 인덱싱 함수에 Rust에서 생성한 `sync_start_time`을 인자로 전달하여 시간 소스 통일 |
| 본문 없는 문서들만 모인 배치가 임베딩 스킵 로직에 걸려 DB 저장이 아예 안 됨 | high | true | `index_documents_batch_async`에서 임베딩은 스킵하되 DB 메타데이터 저장은 수행하도록 구조 변경 |
| `wasm32-wasi` 타겟 지원 중단으로 인한 플러그인 빌드 실패 | medium | true | 최신 표준인 `wasm32-wasip1` 타겟으로 교체 |

### 배운점

- **시간 소스의 단일화 (Single Source of Truth)**: 분산 시스템이나 DB 연동 시, '현재 시간'은 반드시 한 곳에서 생성하여 파라미터로 전달해야 동시성/일관성 버그를 피할 수 있다.
- **배치 처리의 함정**: 대량 처리를 위한 스킵 로직(`continue`, `early return`)을 짤 때는, 필수적인 사이드 이펙트(메타데이터 갱신 등)까지 같이 스킵되지 않는지 반드시 검토해야 한다.
- **WASM 에코시스템 변화**: `wasm32-wasi`에서 `wasm32-wasip1`으로의 타겟 전환과 같이 툴체인 업데이트에 따른 빌드 스크립트 유지보수가 중요하다.

### 개선할점

- `last_indexed < sync_start_time` 기반의 삭제 로직은 여전히 위험 요소가 있으므로, 향후 '현재 세션 ID'를 부여하여 세션 ID가 다른 문서를 삭제하는 방식으로 더 안전하게 리팩토링 고려.
- 테스트 스위트에 '본문이 없는 문서' 시나리오를 추가하여 인덱싱 고착 현상 재발 방지.

### 하네스 개선 제안

**제안**: `indexing.rs`의 삭제 로직을 타임스탬프 기반에서 세션 ID 기반으로 변경
**근거**: 시간차(Clock Drift) 문제는 아무리 정교하게 소스를 통일해도 엣지 케이스가 발생할 수 있으나, 유니크한 세션 ID는 논리적으로 완벽하게 현재 세션의 결과물만 보존 가능함
