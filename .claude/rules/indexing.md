# 인덱싱 파이프라인 규칙

## 시간 소스 단일화 (Clock Drift 방지)

DB에 저장할 타임스탬프는 반드시 **Rust에서 단 한 번 생성하여 파라미터로 전달**해야 한다.
SQLite의 `datetime('now')`와 Rust `SystemTime::now()` 사이의 미세한 차이(Clock Drift)가
"방금 인덱싱한 문서가 `last_indexed < sync_start_time` 조건에 걸려 삭제됨" 유형의 HIGH 버그를 유발한다.

```rust
// 올바른 예 — 단 한 번 생성, 파라미터로 전달
let sync_start_time: i64 = SystemTime::now()
    .duration_since(UNIX_EPOCH)?.as_secs() as i64;
index_documents(conn, docs, sync_start_time)?;
delete_stale(conn, sync_start_time)?;

// 잘못된 예 — 함수 내부에서 각자 time 생성 → 시점 불일치
fn delete_stale(conn: &Connection) {
    let now = SystemTime::now(); // ← sync_start_time과 다른 값
    conn.execute("DELETE ... WHERE last_indexed < ?", [now])?;
}
```

> **근거**: 2026-04-27 devlog — Confluence 500+ 문서 인덱싱 시 방금 저장한 문서가 삭제되는 HIGH 버그.
> 향후 개선 방향: 타임스탬프 대신 세션 ID(UUID)를 부여해 `session_id != current_session` 조건으로 삭제하면 Clock Drift를 구조적으로 제거 가능.

---

## 배치 처리 안전 원칙

배치 루프에서 최적화를 위한 `continue` / `early return`을 작성할 때,
스킵 로직이 **필수 사이드 이펙트(메타데이터 DB 저장, audit_log 기록 등)까지 건너뛰지 않는지** 반드시 확인해야 한다.

```rust
// 잘못된 예 — 임베딩이 없으면 documents 레코드 저장도 통째로 스킵됨
// → 다음 인덱싱 때 "기록 없음"으로 판단해 재인덱싱 반복
for doc in docs {
    let embeddings = generate_embeddings(&doc)?;
    if embeddings.is_empty() { continue; }  // ← documents INSERT도 건너뜀
    save_to_db(&doc, &embeddings)?;
}

// 올바른 예 — 메타데이터는 무조건 저장, 임베딩만 조건부
for doc in docs {
    save_document_record(&doc)?;           // 항상 실행
    let embeddings = generate_embeddings(&doc)?;
    if !embeddings.is_empty() {
        save_embeddings(&doc.id, &embeddings)?;
    }
}
```

> **근거**: 2026-04-27 devlog — 본문 없는 문서가 포함된 배치 전체가 임베딩 스킵 로직에 걸려
> DB 저장이 누락되고, 매 인덱싱마다 반복 재처리되는 HIGH 버그.

---

## 빈 페이지 / 무한 루프 방어

페이지네이션 루프에서 외부 API가 빈 결과를 반환할 때 **명시적 `break`** 로 탈출해야 한다.
`next_cursor`에만 의존하면 API가 빈 페이지와 함께 cursor를 계속 반환하는 경우 무한 루프가 발생한다.

```rust
loop {
    let page = plugin.fetch_all(FetchAllOpts { cursor, page_size: 50 }).await?;

    if page.documents.is_empty() {
        break;  // ← cursor 여부와 관계없이 빈 페이지면 종료
    }

    index_batch(page.documents).await?;

    match page.next_cursor {
        Some(c) => cursor = Some(c),
        None => break,
    }
}
```

> **근거**: 2026-04-27 devlog — Confluence 플러그인에서 빈 페이지 반환 시 루프 탈출 조건 미비.

---

## 증분 인덱싱 재처리 판단 기준

`needs_reindexing` 판단은 다음 순서로 확인한다. **청크 수가 0이라는 이유만으로 재인덱싱을 트리거하지 말 것** — 본문이 없는 문서(빈 파일)는 정상적으로 청크가 0개일 수 있다.

| 조건 | 재인덱싱 여부 |
|------|-------------|
| `documents` 레코드 없음 | 항상 재인덱싱 |
| `content_hash` 변경됨 | 재인덱싱 |
| `last_indexed` < `sync_start_time` — 단, 위 조건 해당 시만 | 재인덱싱 |
| 청크 수 = 0 (본문 없는 문서) | **재인덱싱 안 함** — documents 레코드만 갱신 |

> **근거**: 2026-04-27 devlog — "청크 0개" 조건 과잉 트리거로 약 50개 문서가 매 동기화마다 재인덱싱.
