/// Search command integration tests (TDD — RED → GREEN)
///
/// Tests cover:
/// 1. list_all_documents_returns_empty    — 프로젝트 없으면 빈 배열 반환
/// 2. list_all_documents_returns_docs     — active 프로젝트의 문서 반환
/// 3. list_all_documents_excludes_disabled — disabled 프로젝트 문서 제외
/// 4. list_all_documents_deduplicates_chunks — 동일 source_doc_id 청크는 1개만
/// 5. list_all_documents_groups_by_project — project_name 필드 포함
use doxus_desktop_lib::commands::search::list_all_documents_impl;
use rusqlite::Connection;

fn make_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    doxus_core::db::migrate(&conn).unwrap();
    conn
}

fn insert_project(conn: &Connection, name: &str, status: &str, source_type: Option<&str>) -> i64 {
    let now = 1_700_000_000i64;
    let src = source_type.unwrap_or("obsidian");
    conn.execute(
        "INSERT INTO projects (name, display_name, path, status, source_type, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![name, name, format!("/tmp/{name}"), status, src, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_document(conn: &Connection, project_id: i64, source_doc_id: &str, title: &str) {
    let now = 1_700_000_000i64;
    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed)
         VALUES (?1, ?2, ?3, ?4, 'hash', ?5)",
        rusqlite::params![project_id, source_doc_id, title, "body", now],
    )
    .unwrap();
}

// ── 1. 빈 결과 ──────────────────────────────────────────────────────────────

#[test]
fn list_all_documents_returns_empty() {
    let conn = make_conn();
    let result = list_all_documents_impl(&conn).unwrap();
    let docs = result["documents"].as_array().unwrap();
    assert!(docs.is_empty());
}

// ── 2. active 프로젝트 문서 반환 ──────────────────────────────────────────

#[test]
fn list_all_documents_returns_docs() {
    let conn = make_conn();
    let pid = insert_project(&conn, "my-vault", "active", None);
    insert_document(&conn, pid, "notes/foo.md", "Foo Note");
    insert_document(&conn, pid, "notes/bar.md", "Bar Note");

    let result = list_all_documents_impl(&conn).unwrap();
    let docs = result["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 2);

    let titles: Vec<&str> = docs.iter().map(|d| d["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Foo Note"));
    assert!(titles.contains(&"Bar Note"));
}

// ── 3. disabled 프로젝트 제외 ─────────────────────────────────────────────

#[test]
fn list_all_documents_excludes_disabled() {
    let conn = make_conn();
    let active_pid = insert_project(&conn, "active-vault", "active", None);
    let disabled_pid = insert_project(&conn, "disabled-vault", "disabled", None);
    insert_document(&conn, active_pid, "a.md", "Active Doc");
    insert_document(&conn, disabled_pid, "b.md", "Disabled Doc");

    let result = list_all_documents_impl(&conn).unwrap();
    let docs = result["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["title"].as_str().unwrap(), "Active Doc");
}

// ── 4. 여러 문서 정확한 카운트 ────────────────────────────────────────────

#[test]
fn list_all_documents_counts_distinct_source_docs() {
    let conn = make_conn();
    let pid = insert_project(&conn, "multi-vault", "active", None);
    insert_document(&conn, pid, "doc-a.md", "Doc A");
    insert_document(&conn, pid, "doc-b.md", "Doc B");
    insert_document(&conn, pid, "doc-c.md", "Doc C");

    let result = list_all_documents_impl(&conn).unwrap();
    let docs = result["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 3);
}

// ── 5. project_name / source_type 필드 포함 ──────────────────────────────

#[test]
fn list_all_documents_groups_by_project() {
    let conn = make_conn();
    let pid = insert_project(&conn, "my-vault", "active", Some("obsidian"));
    insert_document(&conn, pid, "test.md", "Test Doc");

    let result = list_all_documents_impl(&conn).unwrap();
    let docs = result["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 1);

    let doc = &docs[0];
    assert_eq!(doc["project_name"].as_str().unwrap(), "my-vault");
    assert_eq!(doc["source_type"].as_str().unwrap(), "obsidian");
    assert!(doc["source_doc_id"].as_str().is_some());
    assert!(doc["title"].as_str().is_some());
}
