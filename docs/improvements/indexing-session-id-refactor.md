---
title: 인덱싱 삭제 로직 — 타임스탬프 기반 → 세션 ID 기반 리팩토링
updated: 2026-04-30
tags:
  - improvement
  - indexing
  - refactor
  - clock-drift
---

# 인덱싱 삭제 로직 세션 ID 기반 리팩토링

## 배경

현재 증분 인덱싱에서 "이번 동기화에서 사라진 문서"를 삭제하는 로직은
타임스탬프 비교(`last_indexed < sync_start_time`)에 의존한다.

```rust
// crates/core/src/indexing.rs (현재)
conn.execute(
    "DELETE FROM documents WHERE project_id = ? AND last_indexed < ?",
    params![project_id, sync_start_time],
)?;
```

`sync_start_time`을 Rust에서 단일 생성하여 전달하는 방식으로 Clock Drift 버그는 해결했으나
**타임스탬프 기반 삭제 자체가 내재적으로 불안정**하다. 시스템 시계 조정, NTP 동기화, VM 일시정지 등
엣지 케이스에서 여전히 오동작할 수 있다.

## 목표 설계

세션 ID(UUID)를 인덱싱 시작 시 발급하여, 이번 세션에서 처리된 문서를 마킹한 뒤
세션 ID가 다른 문서를 삭제하는 방식으로 변경한다.

```sql
-- 스키마 변경
ALTER TABLE documents ADD COLUMN index_session_id TEXT;
```

```rust
// 인덱싱 시작 시
let session_id = Uuid::new_v4().to_string();

// 각 문서 저장 시
conn.execute(
    "INSERT OR REPLACE INTO documents (..., index_session_id) VALUES (..., ?)",
    params![..., session_id],
)?;

// 동기화 완료 후 삭제
conn.execute(
    "DELETE FROM documents WHERE project_id = ? AND index_session_id != ?",
    params![project_id, session_id],
)?;
```

## 장점

| 항목 | 타임스탬프 방식 | 세션 ID 방식 |
|------|--------------|------------|
| 시계 조정 영향 | 있음 | **없음** |
| NTP 동기화 영향 | 있음 | **없음** |
| VM 일시정지 영향 | 있음 | **없음** |
| 논리적 정확성 | 근사값 | **완전** |
| 구현 복잡도 | 낮음 | 중간 |

## 마이그레이션 영향

- `V41__add_index_session_id.sql` 마이그레이션 추가 필요
- `index_session_id` 컬럼 추가 (`ALTER TABLE documents ADD COLUMN index_session_id TEXT`)
- 기존 레코드는 `NULL`로 초기화 — 다음 인덱싱 시 자동 갱신됨

## 관련 파일

- [crates/core/src/indexing.rs](../../crates/core/src/indexing.rs) — 삭제 로직 위치
- [crates/core/src/db/migrations/](../../crates/core/src/db/migrations/) — 마이그레이션 추가 위치

> **근거**: 2026-04-27 devlog `confluence-indexing-bugfix` 세션 3 — Clock Drift 해결 후 개선할점으로 명시.
