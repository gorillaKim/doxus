---
title: "워크스페이스 기능 TDD 전면 재구현"
aliases:
  - workspace-reimplementation-tdd
  - 워크스페이스 재구현 TDD
tags:
  - devlog
  - feature
  - workspace
  - tdd
  - rust
  - react
  - tauri
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

# 워크스페이스 기능 TDD 전면 재구현

## 배경

기존 doxus 워크스페이스는 별도 `workspaces` 테이블을 두는 구조였다. 이 구조는 `projects` 테이블과 역할이 중복되고, 다중 워크스페이스 관리 복잡도를 불필요하게 높였다. 또한 템플릿 시스템이 frontmatter 구조화 UI 없이 단순 텍스트 편집 수준에 머물렀다.

이를 단일 디폴트 워크스페이스 + `projects` 테이블 통합 구조로 전환하고, frontmatter 구조화 편집 UI를 갖춘 템플릿 시스템을 TDD 방식으로 전면 재구현했다.

## 변경 내용

### 주요 변경사항

#### DB 마이그레이션 (V14 / V15)

- `V14__workspace_unification.sql`
  - `projects` 테이블에 `is_default` 컬럼 추가
  - `UNIQUE INDEX` (partial, `WHERE is_default=1`) — 디폴트 워크스페이스 단 하나 보장
  - `templates` 테이블 신규 생성
  - 기존 `workspaces` / `workspace_documents` 데이터를 `projects` / `documents`로 마이그레이션
- `V15__drop_legacy_workspace_tables.sql`
  - `workspaces`, `workspace_documents`, `workspace_templates` 테이블 DROP
- 마이그레이션 테스트 6개 통과:
  - `v14_templates_table_exists`
  - `v14_projects_has_is_default_column`
  - `v14_only_one_default_workspace_allowed`
  - 외 3개

#### Core 구현 (Rust)

**`crates/core/src/workspace/mod.rs`** — 재작성

- `ensure_default_workspace()`: 앱 시작 시 단일 디폴트 워크스페이스 보장, `~/.doxus/workspaces/default/` 자동 생성

**`crates/core/src/document/section.rs`** — 신규

- ATX 헤딩(`#`, `##`, ...) 기반 섹션 파서
- frontmatter 블록 및 코드펜스 구간 인식하여 헤딩 오탐 방지
- `replace_section`, `insert_section_after`, `delete_section` 구현
- 테스트 10개

**`crates/core/src/document/frontmatter.rs`** — 신규

- YAML frontmatter 파서: `parse_frontmatter`, `build_document`, `fill_placeholders`
- 테스트 9개

#### Tauri 커맨드 재작성 (`apps/desktop/src-tauri/src/commands/workspace.rs`)

| 커맨드 | 역할 |
|--------|------|
| `ensure_default_workspace_cmd` | 앱 시작 시 디폴트 워크스페이스 초기화 |
| `list_workspace_documents` / `create_workspace_document` / `update_workspace_document` / `delete_workspace_document` | 문서 CRUD |
| `get_document_sections` / `update_document_section` / `insert_document_section` / `delete_document_section` | 섹션 단위 편집 |
| `list_templates` / `create_template` / `update_template` / `delete_template` / `create_document_from_template` | 템플릿 CRUD |

추가 동작:
- `enqueue_reindex()`: 문서 저장 시 `tokio::spawn`으로 FTS5 + 벡터 즉시 재인덱싱
- `sync_document_to_file()`: DB 변경 내용을 파일시스템에 동기화

#### 프론트엔드 재구현

**`useWorkspaceStore`** — 전면 재작성

- 단일 workspace 모델, 문서 / 섹션 / 템플릿 CRUD 액션 통합

**`WorkspacePage`** — 전면 재작성

- 워크스페이스 탭 제거(단일 공간), 문서 / 템플릿 2탭 구조

**템플릿 시스템**

- 기본 제공 템플릿 10개: 메모, 회의록, 의사결정, 일지, 회고, TODO, 기술 명세서, 라이브러리, 학습 노트, 아티클
- 모든 템플릿 공통 필수 필드: `title`, `aliases`, `created`, `updated`, `tags`
- `TemplateModal` 3탭 구조:
  - **Frontmatter**: 구조화 UI, 필수 필드 잠금
  - **본문**: 마크다운 편집
  - **전체 미리보기**: ReactMarkdown 렌더링
- 새 템플릿 생성 시 필수 5개 필드 자동 추가

### 영향 범위

- `crates/core/src/db/migrations/V14__workspace_unification.sql` (신규)
- `crates/core/src/db/migrations/V15__drop_legacy_workspace_tables.sql` (신규)
- `crates/core/src/workspace/mod.rs` (재작성)
- `crates/core/src/document/section.rs` (신규)
- `crates/core/src/document/frontmatter.rs` (신규)
- `crates/core/src/document/mod.rs` (수정)
- `apps/desktop/src-tauri/src/commands/workspace.rs` (전면 재작성)
- `apps/desktop/src/stores/useWorkspaceStore.ts` (전면 재작성)
- `apps/desktop/src/pages/WorkspacePage.tsx` (전면 재작성)

## 결과

- Rust 전체 테스트 245개 통과, 실패 0
- TypeScript `tsc --noEmit` 통과
- 워크스페이스 구조가 `projects` 테이블로 단일화되어 쿼리 복잡도 감소
- 섹션 단위 편집 API가 core에 위치하여 CLI / MCP에서도 재사용 가능한 구조 확보
- 템플릿 frontmatter 필수 필드 잠금으로 문서 메타데이터 일관성 보장

## 교훈

- Partial UNIQUE INDEX (`WHERE is_default=1`)는 "단 하나의 디폴트" 제약을 DB 레벨에서 강제하는 가장 단순한 방법이다. 애플리케이션 레이어 유효성 검사에 의존하지 않아 안전하다.
- ATX 헤딩 파서는 frontmatter 블록과 코드펜스를 구간으로 추적하지 않으면 코드 예시 내 `##` 등을 섹션 구분으로 오탐한다. 상태 기반 파싱이 필수다.
- `tokio::spawn`으로 재인덱싱을 분리하면 문서 저장 응답 속도를 유지하면서 검색 인덱스 신선도를 보장할 수 있다. 단, 스폰된 태스크의 에러는 별도 로깅 채널이 없으면 소실되므로 `audit_log` 기록을 연결해 두는 것이 좋다.

## 관련 문서

- [[doxus 아키텍처 원칙]]
- [[데이터베이스 규칙]]
- [[Frontend 규칙 (Tauri v2 + React 19)]]
