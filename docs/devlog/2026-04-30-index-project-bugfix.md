---
title: "doxus_index_project 버그 4건 수정 (TDD)"
aliases:
  - index-project-bugfix
  - 인덱스-프로젝트-버그수정
  - 2026-04-30 doxus 데브로그
tags:
  - devlog
  - bugfix
  - indexing
  - tdd
  - mcp
agent_model: claude-sonnet-4-6
reviewed_by: claude-opus-4-7
created: "2026-04-30"
updated: "2026-04-30"
---

<!-- docsmith: auto-generated 2026-04-30 -->

## 개요

`doxus_index_project` MCP 도구가 정상 동작하지 않는다는 보고로 진단을 시작했다.
Sonnet 진단 + Opus 리뷰를 통해 HIGH severity 5개, MEDIUM 1개를 확인하고
TDD 방식으로 4개를 수정했다.

## 주요작업

### Bug 1: `inject_keychain_auth` 이중 호출 dead code 제거 `[easy]`

- **변경 파일**: `crates/core/src/indexing.rs`
- **문제**: `index_project_with_progress` 내 66번 줄에서 임시 `PluginConfig`를 생성해
  `inject_keychain_auth`를 호출한 뒤 즉시 드롭하는 dead code가 존재했다.
  두 번째 호출(69번 줄)만 실제로 유효하여 config 변경이 유실되고 keychain을 2회 조회했다.
  `index_project_changes`(242번 줄)는 단일 호출로 올바르게 구현되어 있어 copy-paste 실수임을 확인했다.
- **수정**: 66번 줄(throwaway config 대상 첫 번째 호출) 제거
- **테스트**: `test_inject_keychain_auth_called_once_on_real_config` — DOXUS_SKIP_KEYCHAIN 환경으로
  멱등성 및 중복 주입 없음 확인

### Issue D: partial failure 시 `remove_deleted_documents` 무조건 실행 → 대규모 데이터 유실 `[hard]`

- **변경 파일**: `crates/core/src/indexing.rs`
- **문제**: 인덱싱 루프(`async { loop { ... } }.await`)가 중간에 에러(네트워크 끊김 등)를 반환해도
  200번 줄의 `remove_deleted_documents`가 항상 실행됐다. 이 함수는
  `last_indexed < sync_start_time`인 문서를 전부 삭제하므로, 아직 fetch하지 못한 문서들이
  한꺼번에 삭제되는 대규모 데이터 유실이 발생할 수 있었다.
  `.claude/rules/indexing.md`에 명시된 Clock Drift 버그 클래스와 동일 패턴이다.
- **수정**:
  ```rust
  // 기존: 항상 실행
  let _ = self.remove_deleted_documents(project_id, sync_start_time).await;
  
  // 수정: result.is_ok()일 때만 실행
  if result.is_ok() {
      let _ = self.remove_deleted_documents(project_id, sync_start_time).await;
      if let Ok(conn) = self.conn.lock() {
          let _ = LinkResolver::resolve_project_links(&conn, project_id);
      }
  }
  // IndexComplete audit log는 성공/실패 무관하게 항상 기록
  ```
- **테스트**: `test_partial_failure_does_not_delete_existing_documents`
  — 두 번째 페이지에서 NetworkError를 반환하는 MockPlugin 사용,
  기존 10개 문서가 보존되는지 assert.
  수정 전: `assert_eq!(doc_count, 10)` → `left: 10, right: 0` 실패로 버그 재현 확인.

### Bug 3: `server.indexer()` 헬퍼 무시하고 수동 `IndexingService` 생성 `[easy]`

- **변경 파일**: `crates/mcp-server/src/tools/project.rs`
- **문제**: `McpServer::indexer()` 헬퍼(server.rs:90-93)가 존재하는데도 `index_project` MCP 툴에서
  동일한 생성 로직을 6줄로 수동 구현했다. 이후 `engine()` 헬퍼에 변경이 생겨도 이 코드에 반영되지 않는
  shotgun surgery 위험이 있었다.
- **수정**: 119-125번 줄을 `server.indexer()` 단일 호출로 교체

### Bug 2: `add_project`가 `source_instances` 행을 생성하지 않음 `[moderate]`

- **변경 파일**: `crates/mcp-server/src/tools/project.rs`
- **문제**: `add_project`가 `projects` 테이블에만 INSERT하고 `source_instances`를 생성하지 않아:
  1. `get_project_config`의 fallback 쿼리가 항상 사용됨
  2. `source_type` 파라미터가 없어 non-obsidian 소스 지정 불가
  3. `sync_project`는 source_instances 없으면 "no source instance" 반환 → `sync_project`와 동작 불일치
- **수정**: `source_type`·`config` 파라미터 추가, projects + source_instances atomic INSERT:
  ```rust
  // projects에 source_type 포함 INSERT
  conn_lock.execute(
      "INSERT INTO projects(name, display_name, path, source_type, ...) VALUES ...",
      params![name, display_name, path, source_type, ...],
  )?;
  let project_id = conn_lock.last_insert_rowid();
  // source_instances 동시 생성
  conn_lock.execute(
      "INSERT INTO source_instances(plugin_id, project_id, config_json, ...) VALUES ...",
      params![plugin_id, project_id, config_json, ...],
  )?;
  ```
- **테스트**:
  - `test_add_project_creates_source_instances_row`: Confluence 타입으로 추가 후 source_instances 행 존재 확인
  - `test_add_project_obsidian_default_source_type`: source_type 없으면 `com.doxus.obsidian` 기본값 확인

## 이슈 및 배운 점

### Opus 리뷰에서 추가 발견된 Issue F (미수정, 별도 트래킹)

`remove_deleted_documents`의 DELETE 쿼리가 `last_indexed IS NULL` 행도 삭제한다.
`documents.last_indexed`가 nullable인 경우 `add_project` 직후의 사전 삽입 행이 삭제될 수 있다.
이번 Issue D 수정으로 result.is_ok() 가드가 추가되어 부분적으로 완화됐지만,
완전한 수정은 세션 ID 기반 삭제 방식(`.claude/rules/architecture.md` 참조)으로 리팩토링 필요.

### TDD 가치 확인

Issue D 테스트(`test_partial_failure_does_not_delete_existing_documents`)는
수정 전에 `assert_eq!(10, 0)` 실패로 버그를 정확히 재현했다.
Mock plugin을 구현해서 두 번째 페이지에서 NetworkError를 주입하는 방식이
기존 `test_index_project_audits_on_plugin_not_found` 패턴을 확장한 것으로 효과적이었다.

### pre-existing 실패

`test_indexing_skip_unchanged_documents`는 이번 변경 이전부터 실패하는 기존 버그.
git stash로 확인 완료. 별도 이슈로 트래킹 필요.

## 개선할 점

- Issue F: `delete WHERE last_indexed IS NULL` 위험 → 세션 ID 기반 삭제로 전환
- `block_in_place` + `handle.block_on()` 패턴 → MCP dispatch 전체 async 전환으로 근본 해결
- `add_project` fallback 경로(`get_project_config` 2차 쿼리)를 deprecated로 문서화

## 하네스 개선 제안

- **Mock DocSource 헬퍼**: 페이지별 응답/에러를 설정 가능한 `MockPlugin::builder()` 패턴을
  `crates/core/src/test_helpers.rs`에 추가하면 인덱싱 관련 테스트 작성이 훨씬 쉬워질 것.
- **inject_keychain_auth 테스트 격리**: 현재 `DOXUS_SKIP_KEYCHAIN=1` env var로 격리하는데,
  테스트 전용 `inject_auth_with_store(store: &dyn SecretStore)` 패턴이 더 안전함.
