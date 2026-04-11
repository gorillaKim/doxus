/// Workspace command integration tests (TDD — RED phase)
///
/// Tests cover:
/// 1. list_workspaces_returns_all     — DB에 삽입한 문서가 전부 반환됨
/// 2. create_workspace_persists       — create 후 DB에 실제로 저장됨
/// 3. switch_workspace_updates_state  — delete 후 목록에서 제거됨
use doxus_desktop_lib::commands::workspace::{
    create_workspace_document_in_conn, delete_workspace_document_in_conn,
    list_workspace_documents_in_conn,
};
use rusqlite::Connection;

fn make_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    doxus_core::db::migrate(&conn).unwrap();
    conn
}

// ── 1. list_workspaces_returns_all ────────────────────────────────────────────

#[test]
fn list_workspaces_returns_all() {
    let conn = make_conn();

    create_workspace_document_in_conn(&conn, "Doc Alpha", None).unwrap();
    create_workspace_document_in_conn(&conn, "Doc Beta", None).unwrap();
    create_workspace_document_in_conn(&conn, "Doc Gamma", Some("todo".to_string())).unwrap();

    let docs = list_workspace_documents_in_conn(&conn).unwrap();

    assert_eq!(docs.len(), 3);
    let titles: Vec<&str> = docs.iter().map(|d| d["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Doc Alpha"));
    assert!(titles.contains(&"Doc Beta"));
    assert!(titles.contains(&"Doc Gamma"));
}

// ── 2. create_workspace_persists ──────────────────────────────────────────────

#[test]
fn create_workspace_persists() {
    let conn = make_conn();

    let doc = create_workspace_document_in_conn(&conn, "My Spec", Some("techspec".to_string()))
        .unwrap();

    // 반환값에 id, title, created_at 포함
    assert!(doc["id"].as_i64().unwrap() > 0);
    assert_eq!(doc["title"].as_str().unwrap(), "My Spec");
    assert!(doc["created_at"].as_i64().unwrap() > 0);

    // DB에 실제로 저장됐는지 확인
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM workspace_documents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    // 템플릿 초기 콘텐츠가 들어갔는지 확인
    let content: Option<String> = conn
        .query_row(
            "SELECT content FROM workspace_documents WHERE id = ?1",
            rusqlite::params![doc["id"].as_i64().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        content.as_deref().unwrap_or("").contains("기술 명세서"),
        "techspec template content should be present"
    );
}

// ── 3. switch_workspace_updates_state ─────────────────────────────────────────
//
// "워크스페이스 전환"의 의미를 doxus-desktop 맥락에서 해석:
// delete_workspace_document 를 호출하면 해당 문서가 목록에서 제거된다.
// (Tauri AppState 전환이 아닌, DB 상태 전환을 검증)

#[test]
fn switch_workspace_updates_state() {
    let conn = make_conn();

    let a = create_workspace_document_in_conn(&conn, "Keep Me", None).unwrap();
    let b = create_workspace_document_in_conn(&conn, "Delete Me", None).unwrap();

    let id_b = b["id"].as_i64().unwrap();
    delete_workspace_document_in_conn(&conn, id_b).unwrap();

    let docs = list_workspace_documents_in_conn(&conn).unwrap();

    assert_eq!(docs.len(), 1);
    assert_eq!(
        docs[0]["id"].as_i64().unwrap(),
        a["id"].as_i64().unwrap()
    );
}
