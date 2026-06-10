/// Confluence auto-reindex integration tests (TDD — RED → GREEN)
///
/// Tests cover:
/// 1. reindex_if_stale_skips_when_hash_same   — 해시 동일하면 reindex 안 함
/// 2. reindex_if_stale_updates_when_hash_differs — 해시 다르면 reindex 실행
/// 3. reindex_if_stale_returns_false_when_doc_not_in_db — DB에 없으면 false (신규)
use doxus_desktop_lib::commands::search::reindex_if_stale;
use rusqlite::Connection;

fn make_conn() -> Connection {
    doxus_core::db::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    doxus_core::db::create_vec0_table(&conn).unwrap();
    doxus_core::db::migrate(&conn).unwrap();
    conn
}

fn insert_project(conn: &Connection, name: &str) -> i64 {
    let now = 1_700_000_000i64;
    conn.execute(
        "INSERT INTO projects (name, display_name, path, status, created_at, updated_at)
         VALUES (?1, ?1, '/tmp', 'active', ?2, ?2)",
        rusqlite::params![name, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_document(
    conn: &Connection,
    project_id: i64,
    source_doc_id: &str,
    title: &str,
    content: &str,
) {
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let now = 1_700_000_000i64;
    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, title, content_hash, last_indexed)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![project_id, source_doc_id, title, hash, now],
    )
    .unwrap();
}

// ── 1. 해시 동일 → reindex 스킵 ──────────────────────────────────────────────

#[test]
fn reindex_if_stale_skips_when_hash_same() {
    let conn = make_conn();
    let pid = insert_project(&conn, "confluence-space");
    insert_document(&conn, pid, "page/123", "My Page", "Hello World");

    // 동일 content → reindex 불필요
    let reindexed = reindex_if_stale(
        &conn,
        "confluence-space",
        "page/123",
        "My Page",
        "Hello World",
    )
    .unwrap();
    assert!(!reindexed, "hash identical — should skip reindex");
}

// ── 2. 해시 다름 → reindex 실행 ──────────────────────────────────────────────

#[test]
fn reindex_if_stale_updates_when_hash_differs() {
    let conn = make_conn();
    let pid = insert_project(&conn, "confluence-space");
    insert_document(&conn, pid, "page/123", "My Page", "old content");

    // 새 content → hash 달라야 함
    let reindexed = reindex_if_stale(
        &conn,
        "confluence-space",
        "page/123",
        "My Page Updated",
        "new content",
    )
    .unwrap();
    assert!(reindexed, "hash differs — should trigger reindex");

    // DB content_hash가 갱신됐는지 확인
    use sha2::{Digest, Sha256};
    let new_hash = format!("{:x}", Sha256::digest("new content".as_bytes()));
    let stored_hash: String = conn.query_row(
        "SELECT content_hash FROM documents WHERE project_id = ?1 AND source_doc_id = 'page/123'",
        rusqlite::params![pid],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(
        stored_hash, new_hash,
        "content_hash should be updated after reindex"
    );
}

// ── 3. DB에 없는 신규 문서 → false 반환 (reindex 불필요, 별도 인덱싱 경로) ───

#[test]
fn reindex_if_stale_returns_false_when_doc_not_in_db() {
    let conn = make_conn();
    insert_project(&conn, "confluence-space");

    let reindexed = reindex_if_stale(
        &conn,
        "confluence-space",
        "page/999",
        "New Page",
        "brand new",
    )
    .unwrap();
    assert!(
        !reindexed,
        "doc not in DB — nothing to compare, skip reindex"
    );
}
