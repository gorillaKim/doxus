---
title: 프로젝트 전체 진단 — 개선 필요 항목 (2026-06-10)
category: improvements
priority: high
created: 2026-06-10
tags:
  - improvement
  - diagnosis
  - ci
  - concurrency
  - mcp
---

# 프로젝트 전체 진단 — 개선 필요 항목 (2026-06-10)

> 코드 품질 · 프로젝트 건강 · 아키텍처 3개 축으로 수행한 전체 진단 결과.
> 기준 버전: v0.1.14 (마지막 커밋 2026-06-05).
> 항목 해결 시 완료 표시로 업데이트할 것.

## Executive Summary

기반 인프라(마이그레이션 V1–V43 멱등 시스템, r2d2+WAL 풀, 로컬 ONNX 임베딩 파이프라인, file watcher 디바운싱, 테스트 626개, CI clippy `-D warnings`)는 탄탄하다. 반면 **CI 테스트 커버리지 공백**, **MCP stub 도구**, **증분 동기화 미구현**, **동시성 락 구조**가 주요 개선 대상이다.

---

## 🔴 High — 신뢰성/품질에 직접 영향

### 1. CI가 전체 테스트의 극히 일부만 실행
- `.github/workflows/ci.yml:42-46` — 전체 626개 테스트 중 `db::tests`, `reindex::tests`, `update_manager::tests`, `state::tests`만 실행.
- core 크레이트 380개 테스트 대부분이 CI에서 검증되지 않음.
- `cargo fmt --check` 단계 없음 (clippy는 있음).

### 2. `unwrap()/expect()/panic!` 827건
- crate별: core 589 / mcp-server 138 / agent 49 / cli 37 / plugin-sdk 13 (비테스트 포함 카운트).
- deadlock 수정 이력(e52d622, 55eddca, edb3f1f)이 반복된 것에 비해 `lock().unwrap()` poison-unsafe 패턴 다수 잔존.

### 3. MCP 도구 39개 중 ~10개가 stub/미구현
- `doxus_index_project`, `doxus_sync_project`: "CLI 쓰세요" 메시지만 반환.
- `doxus_plugin_install/remove/update`: DB만 갱신, 실제 WASM 다운로드/파일 정리 없음.
- `doxus_plugin_info`: 함수 선언만 있고 본문 없음.
- `doxus_resolve_alias`: 알리아스 해석 로직 미구현.
- 에이전트가 호출하면 조용히 실패하는 UX → 최소한 정직한 not-implemented 에러 필요.

### 4. 증분 동기화(incremental sync) 미구현
- Obsidian/Confluence/GitHub 플러그인 모두 `fetch_changes()`가 항상 빈 ChangeSet 반환.
- 동기화 프레임워크는 존재하나 핵심 로직 부재.

---

## 🟡 Medium — 유지보수성/아키텍처

### 5. SyncManager 락 구조
- `crates/core/src/sync_manager.rs:48-58` — Arc\<Mutex\> 8개, 락 획득 순서 미문서화.
- deadlock 수정이 반복된 이력상 구조적 위험. mutex 통합(8개 → 상태 구조체 1-2개) 검토.

### 6. 데스크톱 `run_reindex()` 가 core 우회
- `apps/desktop/src-tauri/src/commands/search.rs:6-86` — IndexingService 대신 독자 SQL 실행.
- MCP 서버와 로직 중복, 스키마 변경 시 드리프트 위험.

### 7. 거대 파일 12개 (>800줄)
- plugins/github/src/lib.rs 1797줄, confluence 1655줄, obsidian 1379줄 — 구조 유사, plugin-sdk로 공통 추출 여지.
- desktop commands/market.rs 1214줄, commands/search.rs 1167줄, mcp-server/tools/search.rs 1080줄 등.

### 8. 마이그레이션 네이밍 혼란
- `V18__remove_default_workspace.sql` / `V18__remove_workspace_projects.sql` 중복, V22 결번.
- 적용 순서는 `db/mod.rs:231~` 정적 배열이라 결정적(실버그 아님)이나 파일명만 보면 오해 소지.
- `db/mod.rs:110` doc 주석 "V1–V42"도 stale (실제 V43).

### 9. 라이브러리 코드에 `println!/eprintln!` 20건
- core + mcp-server. tracing이 이미 의존성에 있으므로 교체 대상.

---

## 🟢 Low — 위생/문서

### 10. 리포 루트 잡동사니
- `test_search.js` git 추적 중 (삭제 대상), `doxus.db`(0B)·`request.json`은 ignore 됨.
- `docs/final-test.md`, `docs/title-test.md` 테스트 잔재.

### 11. 문서 이중화/노후
- 루트 `UNIMPLEMENTED_ITEMS.md`(2026-04-10 기준)와 `docs/unimplemented.md`, `docs/implementation-status.md`, `docs/todo.md` 병존.
- 4월 이후 구현된 항목(feedback ranking, LTM summary, co-occurrence refs 등) 미반영.

### 12. 미커밋 변경 2파일 (확인 필요)
- `apps/desktop/src-tauri/src/main.rs`, `crates/mcp-server/src/http_server.rs` (+6/-72).
- HTTP Bearer auth 제거 — deadlock 수정의 완성본으로 보이나 2026-06-05 이후 미커밋.

---

## 권장 실행 순서

| Phase | 내용 | 항목 |
|-------|------|------|
| 1. 안전망 | 미커밋 정리, CI 전체 테스트 + fmt check, 루트 정리 | #12, #1, #10 |
| 2. MCP stub 해소 | 정직한 에러 → index/sync 실구현 → plugin_info | #3 |
| 3. 동시성/중복 | 락 순서 문서화·통합, poison-safe, run_reindex 통합, println→tracing | #2, #5, #6, #9 |
| 4. 구조 개선 | 플러그인 공통 추출, 마이그레이션 정리, 문서 통합 | #7, #8, #11 |
| (별도) | 증분 동기화 구현 — 플러그인별 대형 피처 | #4 |
