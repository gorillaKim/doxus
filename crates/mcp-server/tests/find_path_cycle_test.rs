use doxus_mcp::McpServer;
use rusqlite::Connection;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn setup_path_db() -> (doxus_core::db::DbPool, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db");
    let pool = doxus_core::db::create_pool(&db_path).unwrap();
    {
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS projects (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL UNIQUE,
                 display_name TEXT NOT NULL,
                 path TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'active',
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS documents (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 project_id INTEGER NOT NULL,
                 source_doc_id TEXT NOT NULL,
                 title TEXT,
                 content TEXT NOT NULL DEFAULT '',
                 content_hash TEXT NOT NULL DEFAULT '',
                 last_indexed INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(project_id, source_doc_id)
             );
             CREATE TABLE IF NOT EXISTS document_links (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 source_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 target_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE
             );",
        )
        .unwrap();
    }
    (pool, tmp)
}

fn insert_project(conn: &Connection) -> i64 {
    conn.execute(
        "INSERT INTO projects (name, display_name, path, created_at, updated_at) VALUES ('test', 'Test', '/tmp', 0, 0)",
        [],
    )
    .unwrap();
    conn.query_row("SELECT id FROM projects WHERE name='test'", [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn insert_doc(conn: &Connection, proj_id: i64, source_doc_id: &str) -> i64 {
    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, content_hash) VALUES (?1, ?2, '')",
        rusqlite::params![proj_id, source_doc_id],
    )
    .unwrap();
    conn.query_row(
        "SELECT id FROM documents WHERE source_doc_id=?1",
        [source_doc_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_link(conn: &Connection, source_id: i64, target_id: i64) {
    conn.execute(
        "INSERT INTO document_links (source_id, target_id, target_raw) VALUES (?1, ?2, '')",
        rusqlite::params![source_id, target_id],
    )
    .unwrap();
}

/// Reproduces H-1: doc-1 in trail falsely excludes doc-10 via LIKE '%doc-1%'
#[tokio::test]
async fn find_path_no_false_positive_on_prefix_ids() {
    let (pool, tmp) = setup_path_db();
    let _proj_id = {
        let conn = pool.get().unwrap();
        let proj_id = insert_project(&conn);

        let id1 = insert_doc(&conn, proj_id, "doc-1");
        let id10 = insert_doc(&conn, proj_id, "doc-10");
        let id100 = insert_doc(&conn, proj_id, "doc-100");

        // doc-1 -> doc-10 -> doc-100
        insert_link(&conn, id1, id10);
        insert_link(&conn, id10, id100);
        proj_id
    };

    let pm = Arc::new(doxus_core::plugin::PluginManager::new(
        tmp.path().to_path_buf(),
    ));
    let server = McpServer::new(
        pool,
        tmp.path().join("test.db"),
        None,
        pm,
        tmp.path().to_path_buf(),
    );
    let resp = server
        .dispatch_tool(
            "doxus_find_path",
            json!(1),
            &json!({"project": "test", "from": "doc-1", "to": "doc-100"}),
        )
        .await;

    assert!(
        resp.error.is_none(),
        "find_path should succeed: {:?}",
        resp.error
    );
    let text = resp
        .result
        .as_ref()
        .and_then(|r| r["content"][0]["text"].as_str())
        .unwrap_or("");
    assert!(
        text.contains("doc-10"),
        "path should go through doc-10, got: {}",
        text
    );
}

/// Real cycles should not cause infinite loops — BFS terminates
#[tokio::test]
async fn find_path_detects_real_cycle_and_avoids_infinite_loop() {
    let (pool, tmp) = setup_path_db();
    let _proj_id = {
        let conn = pool.get().unwrap();
        let proj_id = insert_project(&conn);

        let ida = insert_doc(&conn, proj_id, "doc-a");
        let idb = insert_doc(&conn, proj_id, "doc-b");
        let _idc = insert_doc(&conn, proj_id, "doc-c");

        // doc-a -> doc-b -> doc-a (cycle), doc-c is unreachable
        insert_link(&conn, ida, idb);
        insert_link(&conn, idb, ida);
        proj_id
    };

    let pm = Arc::new(doxus_core::plugin::PluginManager::new(
        tmp.path().to_path_buf(),
    ));
    let server = McpServer::new(
        pool,
        tmp.path().join("test.db"),
        None,
        pm,
        tmp.path().to_path_buf(),
    );
    let resp = server
        .dispatch_tool(
            "doxus_find_path",
            json!(1),
            &json!({"project": "test", "from": "doc-a", "to": "doc-c"}),
        )
        .await;

    // Should return a response (not hang), with no path found
    let text = resp
        .result
        .as_ref()
        .and_then(|r| r["content"][0]["text"].as_str())
        .unwrap_or("");
    assert!(
        text.contains("no path found") || text.contains("null"),
        "expected no path found message, got: {}",
        text
    );
}
