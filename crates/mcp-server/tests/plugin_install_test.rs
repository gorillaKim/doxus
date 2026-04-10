use doxus_mcp::McpServer;
use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use tempfile::TempDir;

fn setup_db() -> (Connection, TempDir) {
    let tmp = TempDir::new().unwrap();
    let conn = Connection::open_in_memory().unwrap();
    // Apply minimal schema needed for plugin_install
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS plugins (
             id            TEXT PRIMARY KEY,
             name          TEXT NOT NULL,
             version       TEXT NOT NULL,
             kind          TEXT NOT NULL DEFAULT 'external',
             trust_level   TEXT NOT NULL DEFAULT 'unverified',
             manifest_json TEXT NOT NULL DEFAULT '{}',
             wasm_sha256   TEXT,
             auto_update   INTEGER NOT NULL DEFAULT 0,
             enabled       INTEGER NOT NULL DEFAULT 1,
             installed_at  INTEGER NOT NULL
         );",
    )
    .unwrap();
    (conn, tmp)
}

fn make_server(conn: Connection, plugins_dir: PathBuf) -> McpServer {
    McpServer::new(conn, None, plugins_dir)
}

// --- test 1: url 파라미터 없으면 DB-only 등록 성공 ---

#[test]
fn test_plugin_install_without_url_registers_in_db() {
    let (conn, tmp) = setup_db();
    let server = make_server(conn, tmp.path().to_path_buf());

    let resp = server.dispatch_tool(
        "doxus_plugin_install",
        json!(1),
        &json!({ "id": "com.test.plugin", "version": "1.0.0" }),
    );

    assert!(resp.error.is_none(), "url is optional; DB-only install should succeed, got: {:?}", resp.error);
}

// --- test 2: url 있으면 파일이 plugins_dir에 저장됨 ---

#[test]
fn test_plugin_install_with_local_file_url_saves_wasm() {
    std::env::set_var("DOXUS_ALLOW_FILE_INSTALL", "1");
    let (conn, tmp) = setup_db();
    let plugins_dir = tmp.path().join("plugins");

    // Create a fake .wasm file to serve as the download source
    let wasm_src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&wasm_src_dir).unwrap();
    let wasm_src = wasm_src_dir.join("test.wasm");
    std::fs::write(&wasm_src, b"\x00asm\x01\x00\x00\x00").unwrap(); // minimal WASM magic

    // Use file:// URL so test doesn't need network
    let url = format!("file://{}", wasm_src.display());

    let server = make_server(conn, plugins_dir.clone());
    let resp = server.dispatch_tool(
        "doxus_plugin_install",
        json!(1),
        &json!({ "id": "com.test.plugin", "version": "1.0.0", "url": url }),
    );

    assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
    assert!(
        plugins_dir.join("com.test.plugin.wasm").exists(),
        "wasm file should be saved in plugins_dir"
    );
}

// --- test 3: 설치 후 DB에 레코드 생성 ---

#[test]
fn test_plugin_install_db_record_created() {
    std::env::set_var("DOXUS_ALLOW_FILE_INSTALL", "1");
    let (_conn, tmp) = setup_db();
    let plugins_dir = tmp.path().join("plugins");

    let wasm_src = tmp.path().join("plugin.wasm");
    std::fs::write(&wasm_src, b"\x00asm\x01\x00\x00\x00").unwrap();
    let url = format!("file://{}", wasm_src.display());

    // We need to query DB after dispatch, so use a file-based DB in tmp dir.
    let db_path = tmp.path().join("test.db");
    let conn2 = Connection::open(&db_path).unwrap();
    conn2.execute_batch(
        "CREATE TABLE IF NOT EXISTS plugins (
             id            TEXT PRIMARY KEY,
             name          TEXT NOT NULL,
             version       TEXT NOT NULL,
             kind          TEXT NOT NULL DEFAULT 'external',
             trust_level   TEXT NOT NULL DEFAULT 'unverified',
             manifest_json TEXT NOT NULL DEFAULT '{}',
             wasm_sha256   TEXT,
             auto_update   INTEGER NOT NULL DEFAULT 0,
             enabled       INTEGER NOT NULL DEFAULT 1,
             installed_at  INTEGER NOT NULL
         );",
    )
    .unwrap();

    let server = make_server(conn2, plugins_dir);
    let resp = server.dispatch_tool(
        "doxus_plugin_install",
        json!(1),
        &json!({ "id": "com.test.plugin", "version": "2.0.0", "url": url }),
    );
    assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);

    // Re-open and check
    let verify = Connection::open(&db_path).unwrap();
    let count: i64 = verify
        .query_row(
            "SELECT COUNT(*) FROM plugins WHERE id = 'com.test.plugin'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "plugin record should be in DB");

    let version: String = verify
        .query_row(
            "SELECT version FROM plugins WHERE id = 'com.test.plugin'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, "2.0.0");
}

// --- test 4: http/https 외 scheme은 거부 ---

#[test]
fn test_plugin_install_rejects_non_http_scheme() {
    let (conn, tmp) = setup_db();
    let server = make_server(conn, tmp.path().join("plugins"));

    // ftp:// should be rejected (SSRF hardening: only http/https allowed in production)
    // file:// is allowed only in tests — we test invalid scheme here
    let resp = server.dispatch_tool(
        "doxus_plugin_install",
        json!(1),
        &json!({ "id": "com.test.plugin", "version": "1.0.0", "url": "ftp://evil.com/x.wasm" }),
    );

    assert!(resp.error.is_some(), "ftp:// should be rejected");
    let err = resp.error.unwrap();
    assert!(
        err.message.contains("scheme") || err.message.contains("url") || err.message.contains("invalid"),
        "error should indicate invalid url/scheme, got: {}",
        err.message
    );
}
