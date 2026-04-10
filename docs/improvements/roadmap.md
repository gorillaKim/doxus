---
title: doxus UI/UX & 기능 개선 로드맵
category: improvements
created: 2026-04-05
updated: 2026-04-05
---

# doxus 개선 로드맵

요청된 개선 항목을 복잡도·의존성 기준으로 그룹화한 계획서.

---

## 그룹 A — 빠른 수정 (1~2일, 의존성 없음)

### A-1. 설정 > 앱 정보 > Phase 정보 제거
- **파일**: `apps/desktop/src/pages/SettingsPage.tsx`
- **작업**: `InfoRow label="Phase"` 라인 삭제

### A-2. MCP 연결 테스트 메시지 개선
- **현황**: `"✗ MCP 서버가 실행되지 않음 (Phase 1에서 구현 예정)"` — 하드코딩된 stub
- **파일**: `apps/desktop/src/pages/SettingsPage.tsx` `handleMcpTest()`
- **작업**: `get_system_status`가 이미 `mcp.status`를 `TcpStream::connect("127.0.0.1:7700")`로 실제 확인 중 → 테스트 버튼도 동일 커맨드 재호출로 대체
  ```typescript
  // 기존 setTimeout stub 제거, 실제 상태 반영
  const res = await invoke<SystemStatus>('get_system_status');
  setMcpTestResult(res.mcp.status === 'running' ? '✓ 연결됨' : '✗ 실행되지 않음');
  ```

### A-3. 대시보드 통계 실제 데이터 연결
- **현황**: "인덱싱된 문서 —", "마지막 동기화 —" 하드코딩
- **파일**: `apps/desktop/src/pages/DashboardPage.tsx`
- **작업**: `useEffect`에서 `invoke('search_engine_status')` 호출 → `total_documents` 표시
  - 마지막 동기화: `documents` 테이블 `MAX(last_indexed)` → `get_system_status`에 필드 추가

---

## 그룹 B — 레이아웃 UX (2~3일, 프론트엔드만)

### B-1. LNB(사이드바) 접기/펼치기
- **파일**: `apps/desktop/src/components/layout/AppShell.tsx`, `Sidebar.tsx`
- **작업**:
  - `sidebarOpen: boolean` 상태 추가 (또는 `useSettingsStore`에 persist)
  - 접힌 상태: 아이콘만 표시 (`w-14`), 펼친 상태: 텍스트+아이콘 (`w-56`)
  - 토글 버튼: 사이드바 하단 또는 TopBar에 배치
  - CSS transition: `transition-all duration-200`

### B-2. ChatDrawer 폭 마우스 드래그 조절
- **파일**: `apps/desktop/src/components/layout/ChatDrawer.tsx`
- **작업**:
  - `drawerWidth: number` 상태 (기본 384, min 280, max 700)
  - 좌측 경계에 drag handle div 추가
  - `onMouseDown` → `document.addEventListener('mousemove', ...)` 패턴
  - localStorage에 폭 저장 (새로고침 유지)

### B-3. 검색 좌측 디렉토리 트리 UI
- **파일**: `apps/desktop/src/pages/SearchPage.tsx`
- **작업**:
  - `file_path`를 파싱해 디렉토리 구조 트리 생성
  - 좌측 컬럼에 VSCode 스타일 트리 렌더링 (폴더 접기/펼치기)
  - 파일 클릭 → `invoke('get_document_content', { filePath })` → 우측 preview
  - 백엔드: `get_document_content` 커맨드 추가 (`documents` 테이블에서 content 조회)
  - 레이아웃: `트리(w-56) | 검색결과(flex-1) | preview(flex-1)` 3단 구조

---

## 그룹 C — 대시보드 랭킹 UI (1~2일)

### C-1. 문서 랭킹 표시
- **현황**: `view_counts` 테이블이 DB 스키마에 존재하나 UI 미노출
- **백엔드**: `get_top_documents` 커맨드 추가
  ```sql
  SELECT d.title, d.file_path, v.view_count
  FROM view_counts v JOIN documents d ON v.document_id = d.id
  ORDER BY v.view_count DESC LIMIT 5
  ```
- **UI**: `DashboardPage`에 "자주 찾는 문서" 섹션 추가 (랭킹 카드)

---

## 그룹 D — 워크스페이스 기능 확장 (3~4일)

### D-1. 문서 수정 기능
- **현황**: 워크스페이스 문서 생성은 있으나 수정 불가
- **백엔드**: `update_workspace_document(id, title, content)` 커맨드
- **UI**: 문서 클릭 → 인라인 편집 모드 or 모달 에디터
  - 마크다운 에디터: `@uiw/react-md-editor` 또는 textarea + preview 토글

### D-2. TODO 템플릿 추가
- **파일**: `apps/desktop/src/pages/WorkspacePage.tsx` `BUILTIN_TEMPLATES`
- **작업**: 템플릿 항목 추가
  ```typescript
  { id: 'todo', name: 'TODO 목록', description: '할 일 체크리스트' }
  ```
- **백엔드**: `create_workspace_document`에서 template_id='todo'일 때 초기 컨텐츠 반환
  ```markdown
  # TODO

  ## 오늘
  - [ ]

  ## 이번 주
  - [ ]

  ## 백로그
  - [ ]
  ```

### D-3. 테크스펙 템플릿 추가
- **작업**: 템플릿 항목 추가 + 초기 컨텐츠
  ```markdown
  # [기능명] 기술 명세서

  ## 개요
  > 한 줄 요약

  ## 요구사항
  ### 기능 요구사항
  - [ ] FR-01:

  ### 비기능 요구사항
  - [ ] NFR-01:

  ## 상세 구현 계획
  ### 아키텍처
  ### API 설계
  ### DB 스키마 변경
  ### 테스트 계획

  ## 리스크 및 미결 사항
  ```

---

## 그룹 E — Agent MCP 연동 (3~5일, 복잡도 높음)

### E-1. Agent 채팅에 doxus-mcp/CLI 툴 연동
- **참고**: obsidian-nexus의 `crates/agent/src/` 패턴
- **현황**: `agent_send_message`는 Claude CLI를 단순 `-p message`로 호출 → MCP 도구 없음
- **목표**: Claude가 채팅 중 `docnx_search`, `docnx_get_document` 등 MCP 도구 사용 가능

**구현 방향 (2가지 옵션):**

**옵션 A — MCP 서버 연결 (권장)**:
```rust
// Claude CLI에 MCP 서버 주소 전달
.args([
    "-p", message,
    "--output-format", "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--mcp-server", "doxus=http://127.0.0.1:7700",  // doxus-mcp 서버
])
```
- 전제: doxus-mcp 서버가 실행 중이어야 함
- `--mcp-server` 플래그는 Claude Code CLI가 지원하는 경우

**옵션 B — 시스템 프롬프트 + 도구 정의 직접 주입**:
```rust
// ~/.doxus/agents/librarian/system.md 로드
// DocSource 검색 결과를 컨텍스트로 삽입
let context = build_context_from_search(&conn, message)?;
let augmented_message = format!("{}\n\nContext:\n{}", message, context);
```

**작업 순서**:
1. doxus-mcp 서버 실제 기동 여부 확인 (`127.0.0.1:7700`)
2. Claude CLI `--mcp-server` 플래그 지원 여부 테스트
3. 옵션 선택 후 `stream_claude()` 수정
4. 시스템 프롬프트 하네스 (`~/.doxus/agents/librarian/system.md`) 구현

---

## 그룹 F — 설정 개발 도구 기능 연동 (2일)

### F-1. Phase 6 관측성 구현 현황 확인 및 연동
- **현황**: 설정 하단 "DB 재인덱싱", "검색 엔진 상태", "플러그인 로그" 버튼이 stub
- **확인 필요**:
  - `trigger_reindex` → 이미 구현됨 ✅
  - `search_engine_status` → 이미 구현됨 ✅
  - `get_plugin_logs` → `market.rs`에 구현됨 ✅
- **작업**: `SettingsPage.tsx` DevButton 핸들러를 실제 결과 표시로 교체
  ```typescript
  // DB 재인덱싱
  const res = await invoke<{ indexed: number; message: string }>('trigger_reindex');
  setResult(res.message);

  // 검색 엔진 상태
  const res = await invoke<{ total_documents: number; total_projects: number }>('search_engine_status');
  setResult(`문서 ${res.total_documents}개, 프로젝트 ${res.total_projects}개`);

  // 플러그인 로그
  const res = await invoke<{ logs: { level: string; message: string }[] }>('get_plugin_logs');
  setResult(`최근 로그 ${res.logs.length}건`);
  ```

---

## 그룹 G — Confluence 연동 테스트 (별도 추적)

### G-1. Confluence 플러그인 연동 검증
- **현황**: Phase 3에서 Confluence WASM 플러그인 구현됨
- **테스트 필요 항목**:
  - `market_install_plugin('com.doxus.confluence')` 성공 여부
  - OAuth 플로우 (`plugin_start_oauth` → 브라우저 → callback) 동작
  - `add_project` 후 인덱싱 실행 → 실제 페이지 수집 여부
  - 검색 결과에 Confluence 문서 포함 여부
- **환경**: Confluence Cloud 테스트 스페이스 필요

---

## 우선순위 실행 순서

| 순서 | 그룹 | 예상 소요 | 비고 |
|------|------|-----------|------|
| 1 | A (빠른 수정 3종) | 반나절 | 즉시 효과 |
| 2 | F (개발도구 연동) | 반나절 | 이미 백엔드 있음 |
| 3 | C (대시보드 랭킹) | 1일 | 백엔드 1개 추가 |
| 4 | B-1 (LNB 접기) | 1일 | UX 개선 |
| 5 | D (워크스페이스) | 2~3일 | 템플릿 2종 + 수정 |
| 6 | B-2, B-3 (드래그/트리) | 2일 | 복잡한 프론트엔드 |
| 7 | E (Agent MCP) | 3~5일 | 가장 복잡, MCP 서버 전제 |
| 8 | G (Confluence 테스트) | 환경 준비 후 | 외부 의존성 |
