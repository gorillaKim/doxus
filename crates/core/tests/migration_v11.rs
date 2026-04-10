/// Migration V11 — document_aliases 테이블 검증
///
/// TestDb (in-memory SQLite + 전체 마이그레이션)를 사용하므로
/// 실제 ~/.doxus/db/nexus.db 에는 아무런 영향을 주지 않습니다.
use doxus_core::db::TestDb;
use rusqlite::params;

// ── 헬퍼 ─────────────────────────────────────────────────────────────────────

fn insert_project(db: &TestDb, name: &str) -> i64 {
    db.conn
        .execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at)
             VALUES (?1, ?1, '/tmp', 'active', unixepoch(), unixepoch())",
            [name],
        )
        .unwrap();
    db.conn
        .query_row("SELECT id FROM projects WHERE name = ?1", [name], |r| r.get::<_, i64>(0))
        .unwrap()
}

fn insert_document(db: &TestDb, project_id: i64, source_doc_id: &str) -> i64 {
    db.conn
        .execute(
            "INSERT INTO documents (project_id, source_doc_id, content, content_hash, last_indexed)
             VALUES (?1, ?2, 'hello', 'abc', unixepoch())",
            params![project_id, source_doc_id],
        )
        .unwrap();
    db.conn
        .query_row(
            "SELECT id FROM documents WHERE source_doc_id = ?1",
            [source_doc_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
}

// ── 테스트 1: document_aliases 테이블 존재 확인 ───────────────────────────────

#[test]
fn test_document_aliases_table_exists() {
    let db = TestDb::new();
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_aliases'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "document_aliases 테이블이 존재해야 합니다");
}

// ── 테스트 2: chunks 테이블 존재 확인 (V3에서 생성됨) ────────────────────────

#[test]
fn test_chunks_table_exists() {
    let db = TestDb::new();
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "chunks 테이블이 존재해야 합니다");
}

// ── 테스트 3: alias INSERT 후 SELECT ─────────────────────────────────────────

#[test]
fn test_insert_and_query_alias() {
    let db = TestDb::new();
    let pid = insert_project(&db, "test-vault");
    let doc_id = insert_document(&db, pid, "doc-001");

    db.conn
        .execute(
            "INSERT INTO document_aliases (document_id, alias) VALUES (?1, ?2)",
            params![doc_id, "my-alias"],
        )
        .expect("alias INSERT should succeed");

    // tool_resolve_alias 와 동일한 쿼리 패턴으로 검증
    let (source_doc_id, project_name): (String, String) = db
        .conn
        .query_row(
            "SELECT d.source_doc_id, p.name
             FROM document_aliases da
             JOIN documents d ON da.document_id = d.id
             JOIN projects p ON d.project_id = p.id
             WHERE da.alias = ?1
             LIMIT 1",
            ["my-alias"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("alias SELECT should return a row");

    assert_eq!(source_doc_id, "doc-001");
    assert_eq!(project_name, "test-vault");
}

// ── 테스트 4: chunk INSERT 후 COUNT ──────────────────────────────────────────

#[test]
fn test_insert_and_query_chunk() {
    let db = TestDb::new();
    let pid = insert_project(&db, "notes");
    let doc_id = insert_document(&db, pid, "doc-abc");

    db.conn
        .execute(
            "INSERT INTO chunks (document_id, content, chunk_index) VALUES (?1, 'chunk body', 0)",
            params![doc_id],
        )
        .expect("chunk INSERT should succeed");

    // tool_inspect_document 와 동일한 서브쿼리 패턴으로 검증
    let chunk_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM chunks c WHERE c.document_id = ?1",
            params![doc_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();

    assert_eq!(chunk_count, 1, "chunk가 1개 있어야 합니다");
}
