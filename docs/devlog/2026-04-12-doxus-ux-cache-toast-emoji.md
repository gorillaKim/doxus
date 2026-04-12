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

---

## 2026-04-12 (세션 2) — 테스트 회귀 수정 + 검색 프리뷰 메타데이터 표시

### 작업 내용

#### 1. 테스트 회귀 수정 (518 tests → all green)

이전 세션에서 추가된 V13 마이그레이션(`created_at`, `updated_at`, `metadata_json` 컬럼)과 Confluence fetch_all URL 변경이 테스트를 깨뜨렸다.

**수정 1 — `search.rs` `make_async_engine`**
- V10~V13 마이그레이션이 누락되어 in-memory DB에 `created_at` 컬럼이 없어 panic
- `make_async_engine` 헬퍼에 V10~V13 include_str! 추가

**수정 2 — Confluence 테스트 mock 경로**
- `fetch_all` 구현체가 `/rest/api/content/search` (CQL)로 전환됐는데 unit/통합/token_refresh 테스트는 여전히 `/rest/api/content` mock 사용
- `lib.rs`, `integration_test.rs`, `token_refresh_test.rs` 전체 교체

#### 2. 검색 프리뷰 메타데이터 표시 수정

**문제 1 — Frontmatter가 본문에 그대로 노출**
- ReactMarkdown이 `---...---` YAML 블록을 목록으로 렌더링
- `stripFrontmatter()` 함수 추가, `fetchPreview`에서 content 설정 전 적용

**문제 2 — DocMetaPanel이 빈 상태에서 렌더링 안 됨**
- `hasAny`가 false면 `null` 반환 → 패널 자체가 미표시
- 빈 상태에서도 패널 렌더링, "메타데이터 없음 — 재인덱싱 시 채워집니다" 힌트 표시

**문제 3 — get_document_content가 tags/aliases/metadata를 응답에 포함하지 않음**
- Obsidian/GitHub 브랜치: `{ title, content, file_path }` 만 반환
- DB에서 메타데이터를 별도 조회(`get_document_content_impl`)해 응답에 병합

### 핵심 인사이트

`get_document_content`의 plugin 브랜치(obsidian, github, confluence)가 각각 live fetch → raw content만 반환하고 있었다. tags/aliases는 DB에 따로 저장되므로 live fetch 경로에서도 DB 메타를 병합해야 한다.

---

## 2026-04-12 (세션 3) — Phase 6 관측성 & 디버깅 구현 (TDD)

### 배경

Phase 6 관측성 개발 계획을 수립하고 critic 리뷰를 거쳐 TDD 방식으로 구현했다.
리뷰에서 발견된 주요 수정사항:
- Task 4 (MCP 진단 도구) 는 이미 mcp-server에 구현되어 있어 삭제
- `index_engine.rs` 파일이 없고 실제 인덱싱 로직은 `search.rs`에 있음
- `log_audit` 시그니처 변경 불필요 → `persist_audit(conn, event)` 별도 함수로 설계

### 작업 내용

#### 1. PluginLogModal UI 추가 (SettingsPage.tsx)

플러그인 로그 버튼 클릭 시 모달이 열리도록 구현.
- `PluginLogEntry` 인터페이스 (id, project_id, event_type, payload, occurred_at)
- 백엔드 실제 반환 타입과 일치하도록 수정 (기존 코드는 `level, message` 기대했으나 실제는 다름)
- 에러 이벤트는 빨간색, 정상은 초록색으로 구분
- 배경 클릭 또는 X로 닫기

#### 2. persist_audit 함수 구현 (crates/core/src/observability.rs)

기존 `log_audit(event)` (tracing only) 유지, 새로운 `persist_audit(conn, event)` 추가.

```rust
pub fn persist_audit(conn: &rusqlite::Connection, event: &AuditEvent) {
    log_audit(event);
    let payload = serde_json::to_string(event).unwrap_or_default();
    let _ = conn.execute(
        "INSERT INTO audit_log (project_id, event_type, payload, occurred_at) \
         VALUES (?1, ?2, ?3, unixepoch())",
        rusqlite::params![event.project_id(), event.event_type_str(), payload],
    );
}
```

`AuditEvent`에 `event_type_str()`, `project_id()` 메서드 추가.

TDD: 5개 테스트 (persist_audit_inserts_into_db, persist_audit_stores_project_id 등) 작성 → Red → Green.

**이슈**: SQLite FK 제약이 기본 ON 상태라 project_id=42 INSERT 실패. make_conn()에 테스트 프로젝트 행 삽입으로 해결.

#### 3. search.rs 인덱싱 audit 연결

`index_document_async_with_meta`의 `spawn_blocking` 내부에 audit 호출 추가:

```rust
persist_audit(&conn, &AuditEvent::IndexStart { project_id });
index_document_sync(...)?;
persist_audit(&conn, &AuditEvent::IndexComplete { project_id, docs_indexed: 1 });
```

TDD: 3개 테스트 (index_start/index_complete 기록, project_id 확인).

#### 4. sync/runner.rs 동기화 audit 연결

`run_once`의 instance 처리 루프에 audit 호출 추가:

```rust
persist_audit(conn, &AuditEvent::SyncStart { source_instance_id: instance.id });
// ... fetch_changes ...
// 성공 시:
persist_audit(conn, &AuditEvent::SyncComplete { source_instance_id: instance.id, docs_synced: applied });
// 실패 시:
persist_audit(conn, &AuditEvent::PluginError { plugin_id: ..., message: e.to_string() });
```

TDD: 3개 테스트 (sync_start, sync_complete, plugin_error 기록).

#### 5. PluginLogModal 개선 + 신규 Tauri 커맨드

**모달 개선:**
- event_type 필터 드롭다운 (전체/index_start/index_complete/sync_start/sync_complete/plugin_error)
- 로그 초기화 버튼 (clear_audit_log 호출)
- `audit:new` Tauri 이벤트 리스너 (실시간 push 대기)
- `useRef` + `listen` import 추가

**신규 Tauri 커맨드 3개 (market.rs):**
- `clear_audit_log` — audit_log 전체 삭제
- `get_embedding_status` — DB의 임베딩 커버리지 조회 (embedded_chunks 수)
- `trigger_sync` — source_instances의 last_sync를 0으로 리셋해 강제 동기화 예약

**개발도구 버튼 2개 추가 (SettingsPage.tsx):**
- 임베딩 상태: 모델명 + 임베딩된 청크 수 표시
- 동기화 강제 실행: trigger_sync 호출

### 테스트 결과

`cargo test --workspace` — 전체 통과 (0 failed)

신규 테스트: 11개 추가
- observability: 5개 (persist_audit_*)
- search: 3개 (index_document_writes_index_start/complete, audit_log_has_correct_project_id)
- sync/runner: 3개 (run_once_writes_sync_start/complete/plugin_error)

### 핵심 인사이트

**`let _`은 숨겨진 버그**: `persist_audit`에서 `let _ = conn.execute(...)` 패턴은 INSERT 실패를 완전히 무시한다. 테스트에서 count=0으로 발견했고, 원인은 SQLite FK 제약 (project_id 없는 행 INSERT 불가). 테스트 헬퍼에 프로젝트 픽스처를 추가해 해결.

**audit:new Tauri emit 보류**: `persist_audit`는 `crates/core`에 있어 Tauri `AppHandle` 의존성을 가질 수 없다. 프론트엔드에 이벤트 리스너만 준비해두고, 실제 emit은 Phase 6 후속 작업에서 Tauri 커맨드 레이어에서 처리 예정.

### 영향 범위

- `crates/core/src/observability.rs`
- `crates/core/src/search.rs`
- `crates/core/src/sync/runner.rs`
- `apps/desktop/src-tauri/src/commands/market.rs`
- `apps/desktop/src-tauri/src/main.rs`
- `apps/desktop/src/pages/SettingsPage.tsx`
