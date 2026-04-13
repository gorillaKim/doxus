---
title: "템플릿 변수 분리 설계 구현"
aliases:
  - template-variable-separation
  - 템플릿 변수 분리
tags:
  - devlog
  - feature
  - mcp
  - template
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

# 템플릿 변수 분리 설계 구현

## 배경

에이전트가 `doxus_apply_template`을 사용할 때 frontmatter 메타데이터와 본문 변수를 구분하지 못하는 문제가 있었다. Obsidian, Confluence 등의 플러그인은 frontmatter를 플랫폼별 메타데이터로 매핑해야 하므로(Confluence → Page Properties, Notion → Database Properties), API 수준에서 frontmatter와 본문 변수를 명확히 분리할 필요가 있었다.

또한 프론트엔드에 `BUILTIN_TEMPLATES` 상수가 하드코딩되어 있어 Rust `with_builtins()` 목록과 불일치가 발생했고, `todo`·`techspec` 템플릿 선택 시 "추가 변수 없음"이 표시되는 버그가 있었다.

## 변경 내용

### 주요 변경사항

**`crates/core/src/workspace/template.rs`**
- `AUTO_INJECT: &[&str] = &["created", "updated"]` 상수 추가 — 자동 주입 변수는 에이전트에게 노출하지 않음
- `TemplateInfo`에 `frontmatter_fields: Vec<String>`, `body_variables: Vec<String>` 필드 추가
- `extract_frontmatter_variables(src)` — `---...---` 구간의 변수 추출, auto-inject 제외
- `extract_body_variables(src)` — body 구간 변수에서 frontmatter 변수 및 auto-inject 제거
- `split_template_sections(src)` — frontmatter/body 구간 분리 헬퍼
- `todo`, `techspec` 2종 내장 템플릿 추가 → 총 12종
- frontmatter 값에 포함된 따옴표 처리: `.trim_matches('"')` 추가

**`crates/mcp-server/src/lib.rs`**
- `doxus_list_templates` — `{"templates": [...]}` 형태로 래핑, content 미포함
- `doxus_get_template` — `frontmatter_fields`/`body_variables` 분리 반환
- `doxus_apply_template` — `frontmatter`/`variables` 분리 수신, 병합 후 렌더링
- `tool_status` — 워크스페이스 프로젝트를 카운트에서 제외 (`WHERE source_type != 'workspace'`)

**`apps/desktop/src-tauri/src/commands/workspace.rs`**
- `get_template` 커맨드 추가 — `frontmatter_fields`, `body_variables` 분리 반환
- `apply_template` 커맨드 추가 — frontmatter/variables 분리 수신
- `list_templates` 커맨드 개선 — 내장 12종 + DB 커스텀 반환

**`apps/desktop/src/` (프론트엔드)**
- `BUILTIN_TEMPLATES`, `BUILTIN_DOC_TYPES` 상수 완전 제거
- 마운트 시 `invoke('list_templates')` 동적 로딩으로 전환 (단일 진실 공급원 = Rust `with_builtins()`)
- `NewDocModal` — `allTemplates: TemplateSummary[]` 기반 통합 렌더링
- `TemplateModal` — `[변수]` 탭 신규 추가
  - 본문의 `{{var}}` 실시간 감지 및 목록 표시
  - 각 변수 옆 [삽입] → textarea 커서 위치에 `{{name}}` 삽입
  - 새 변수명 입력 → [추가] → 본문 끝에 `{{name}}` 삽입

### 영향 범위

- core 템플릿 API (`TemplateInfo` 구조 변경)
- MCP `doxus_apply_template` 인터페이스 변경 (하위 호환 불가 — `variables` → `frontmatter` + `variables` 분리)
- Tauri IPC `apply_template`, `get_template`, `list_template` 커맨드
- 프론트엔드 `NewDocModal`, `TemplateModal` 컴포넌트

## 결과

- core 30개 테스트 전부 통과
- MCP 70개 테스트 전부 통과
- 에이전트가 `doxus_apply_template` 호출 시 frontmatter 필드와 본문 변수를 구분하여 전달 가능
- 프론트엔드 템플릿 목록이 Rust 소스와 항상 일치 (동적 로딩)
- 플러그인이 frontmatter를 플랫폼 메타데이터로 매핑할 수 있는 구조 확보

## 교훈

**단일 진실 공급원 원칙:** 프론트엔드와 백엔드에 같은 목록을 중복 정의하면 반드시 불일치가 생긴다. 내장 템플릿 목록은 Rust 한 곳에서만 관리하고 프론트엔드는 API로 받아오는 것이 맞다.

**API 경계에서의 타입 설계:** `variables` 하나로 모든 것을 받던 인터페이스를 `frontmatter` + `variables`로 분리하는 것은 작은 변경이지만, MCP 도구 시그니처가 바뀌므로 에이전트 프롬프트/예시도 함께 업데이트해야 한다.

**자동 주입 변수 처리:** `created`, `updated`처럼 시스템이 자동으로 채우는 필드는 에이전트 노출 변수 목록에서 제외해야 불필요한 혼란을 줄일 수 있다.

## 관련 문서

- [[doxus 아키텍처]]
- [[워크스페이스 템플릿 관리]]

---

## 2026-04-13 (후속 버그픽스)

### 1. 템플릿 페이지 에러 수정

`useWorkspaceStore.fetchTemplates`가 `invoke<Template[]>('list_templates')` 호출 → 실제 Tauri는 `{ templates: [...] }` 반환 → `templates` 상태에 객체가 들어가 `.map()` 에러 발생.

수정: `invoke<{ templates: ... }>` + `res.templates.filter(t => t.source === 'custom')`.

### 2. 기본 제공/저장된 템플릿 중복 해결

`list_templates` 응답이 builtin + custom 둘 다 포함하여 "저장된 템플릿" 섹션에 중복 표시되는 문제.

수정:
- `workspace.rs` `list_templates`: DB custom 템플릿에 전체 필드(`id, doc_type, content, created_at`) 포함
- store `fetchTemplates`: `source === 'custom'` 필터로 builtin 제외
- WorkspacePage: "기본 제공" → `allTemplates.filter(source==='builtin')`, "저장된 템플릿" → store `templates`

### 3. 빌트인 카드 클릭 시 변수 비어있는 문제

빌트인 카드 클릭 시 `content: ''` 전달 → TemplateModal 변수 탭 비어있음.

수정: 카드 `onClick`을 `async`로 변경, `invoke('get_template', { name })` 먼저 호출해 실제 content 로드 후 모달 오픈.

### 4. TemplateModal UX 개선

- 탭 순서 변경: `Frontmatter → 본문 → 변수` → `Frontmatter → 변수 → 본문 → 전체 미리보기`
- "본문에 삽입" 버튼 제거 (탭 전환 시 포커스 이탈로 실효성 없음)
- 변수 탭에 수정/삭제 기능 추가:
  - 수정: 변수명 클릭 또는 [수정] 버튼 → 인라인 input → Enter/blur 저장, Escape 취소
  - `renameVar(original, next)`: body 내 `{{oldName}}` → `{{newName}}` 전체 교체 (regex)
  - `deleteVar(varName)`: body 내 `{{varName}}` 전체 제거 (regex)

### 5. 템플릿 저장 에러 표시

`catch (e) { console.error(e); }` → 에러를 모달 푸터에 빨간 텍스트로 표시. `saveError` state 추가.

### 6. doc_type CHECK constraint 확장 (V16 마이그레이션)

**문제:** `CHECK(doc_type IN ('note','meeting','decision','journal','retrospective','todo','techspec','other'))` — devlog, weekly, study, library, article, history 저장 불가.

**원인:** V14에서 templates 테이블 생성 시 구형 CHECK 제약 사용.

**해결:** V16 마이그레이션 추가.
- SQLite는 CHECK 제약 직접 수정 불가 → 테이블 재생성 (CREATE + INSERT + DROP + RENAME)
- 새 허용 목록: note, meeting, decision, journal, retrospective, todo, techspec, devlog, weekly, study, library, article, history, other
- `crates/core/src/db/mod.rs` MIGRATIONS 배열에 V16 등록

`DOC_TYPE_ICONS`에 devlog(💻), weekly(📅), history(🏛️) 아이콘 추가.

### 발생한 버그 요약

| 버그 | 원인 | 해결 |
|------|------|------|
| 템플릿 페이지 에러 | store가 `{ templates: [...] }` 객체를 배열로 오해 | invoke 타입 수정 + 필터 |
| 중복 템플릿 표시 | list_templates가 builtin+custom 모두 반환, 필터 없음 | source 필터 적용 |
| 빌트인 변수 비어있음 | 카드 클릭 시 `content: ''` 하드코딩 | get_template 비동기 호출 |
| 저장 안됨 (CHECK 에러) | doc_type 허용 목록 미갱신 | V16 마이그레이션 |
| include_str! 임베딩 | SQL 파일 추가해도 MIGRATIONS 배열 미등록 | mod.rs에 수동 등록 |

### 학습

- SQLite CHECK 제약은 ALTER TABLE로 수정 불가 → 테이블 재생성 패턴 필요
- Tauri의 `include_str!` 기반 마이그레이션은 파일 생성 후 반드시 MIGRATIONS 배열에도 등록해야 함
- 에러를 `console.error`로만 처리하면 디버깅이 매우 어려움 → UI에 에러 표시 필수
