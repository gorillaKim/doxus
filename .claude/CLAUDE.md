# doxus — Claude Code 컨텍스트

doxus는 **obsidian-nexus의 차세대 진화판**으로, WASM 플러그인 기반 다중 소스 통합 문서 검색 허브다.
로컬 퍼스트 + 에이전트 친화적 설계를 핵심으로 하며, 강력한 그래프 분석 기능을 갖추고 있다.

## 🛠 주요 도구 및 인터페이스

### 1. MCP Tools (30+ 도구 제공)
에이전트가 `doxus-mcp`를 통해 사용할 수 있는 대표적인 도구들:

- **Search**: `doxus_search`, `doxus_get_document`, `doxus_get_section`, `doxus_get_metadata`
- **Graph**: `doxus_get_links`, `doxus_get_backlinks`, `doxus_find_path`, `doxus_get_cluster`
- **Project**: `doxus_list_projects`, `doxus_add_project`, `doxus_index_project`, `doxus_sync_project`
- **Management**: `doxus_list_documents`, `doxus_get_toc`, `doxus_resolve_alias`, `doxus_get_ranking`

### 2. CLI Interface
터미널에서 직접 실행 가능한 파워 유저용 도구 (`./target/debug/doxus`):

- **Search**: `doxus search <query>`
- **Graph Analysis**:
  - `doxus graph links <project> <id>`: 정방향 링크 조회
  - `doxus graph cluster <project> <id>`: 다중 홉 지식 클러스터(관련 문서) 탐색
  - `doxus graph path <project> <from> <to>`: 두 문서 사이의 최단 경로 검색
- **Indexing**: `doxus index`: 모든 프로잭트 인덱싱 갱신
- **Status**: `doxus status`: 시스템 및 인덱스 상태 보고

## 📐 Rules & Guidelines

상세 규칙은 `.claude/rules/` 에 정의되어 있다:

| 파일 | 내용 |
|------|------|
| [architecture.md](rules/architecture.md) | 전체 아키텍처 원칙, 모노레포 레이아웃 |
| [plugin-system.md](rules/plugin-system.md) | WASM 플러그인 SDK, Host Function 규격, wasm32-wasip1 타겟 |
| [database.md](rules/database.md) | SQLite 저장소, FTS5/Vector 스키마, sqlite-vec 제약, FTS5 쿼리 안전 가드 |
| [indexing.md](rules/indexing.md) | 인덱싱 파이프라인 — Clock Drift 방지, 배치 처리 안전, 빈 페이지 방어 |
| [agent-mcp.md](rules/agent-mcp.md) | MCP 도구 명명 규칙 및 에이전트 연동 가이드 |
| [rust-conventions.md](rules/rust-conventions.md) | 에러 처리, 의존성 관리 규칙 |
| [frontend.md](rules/frontend.md) | Tauri v2 + React 19, Zustand, Tauri 이벤트 race guard |
| [testing.md](rules/testing.md) | 테스트 전략, Vitest + Tauri mock 패턴, StrictMode cleanup |
| [git-workflow.md](rules/git-workflow.md) | 커밋 컨벤션, 브랜치 전략, Phase 태그 |

## 🚀 현재 상태

- **Core Engine (v0.1.0)**: ONNX 임베딩(all-MiniLM-L6-v2), FTS5 + sqlite-vec 하이브리드 검색, RRF 랭킹 안정화 완료.
- **MCP Server**: 38개 `doxus_*` 도구 구현 (검색·문서·그래프·프로젝트·플러그인·진단).
- **Graph Tools**: 지식 그래프 탐색 및 최단 경로 검색 지원 (CLI/MCP 공통).
- **Desktop (Beta)**: Tauri v2 기반 검색, 프로젝트 관리 UI, Auto Updater 구현 완료.
- **Auto Updater**: ED25519 서명 검증, GitHub Actions CI/CD, post-update 마이그레이션 훅 완료.

## ⚠️ 핵심 제약사항

- **ID 처리**: 숫자형 `db_id`와 문자열 `source_doc_id`를 모두 지원해야 함.
- **저장소 전략**: `DocumentService`를 사용하여 본문 내용을 가져와야 함 (DB 직접 조회 금지).
- **데이터 보존**: `remove_project`는 인덱스만 삭제하며, 원본 파일은 건드리지 않음.
- **플러그인**: 모든 외부 연동은 WASM 샌드박스를 통해서만 수행.
