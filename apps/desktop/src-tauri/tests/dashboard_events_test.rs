/// Dashboard sync event tests (TDD - RED phase)
///
/// Verifies that spawn_sync_loop emits `sync:progress` and `sync:complete`
/// Tauri events when an AppHandle is provided, and operates silently when
/// app_handle is None (CLI mode).
///
/// These tests exercise the new `app_handle: Option<AppHandle>` parameter on
/// `spawn_sync_loop` from `doxus-mcp`.  Because AppHandle cannot be
/// constructed in unit tests without a running Tauri application, the event
/// emission is verified indirectly via an `EventSink` abstraction that the
/// production code accepts as a generic parameter.
///
/// The `EventSink` trait is defined in `doxus-mcp::sync_loop` and has two
/// implementations:
///   * `TauriEventSink`   — wraps `tauri::AppHandle` (production)
///   * `NoopEventSink`    — no-op (CLI / None case)
///
/// Tests here use a third implementation, `RecordingEventSink`, defined
/// locally so that assertions can be made without a real AppHandle.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use doxus_core::db;
use doxus_mcp::sync_loop::{EventSink, SyncEvent, spawn_sync_loop_with_sink};
use rusqlite::Connection;
use tempfile::TempDir;

// ── helpers ──────────────────────────────────────────────────────────────────

fn open_test_db() -> (Arc<Mutex<Connection>>, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open(&path).unwrap();
    (Arc::new(Mutex::new(conn)), dir)
}

fn make_plugin_manager() -> Arc<doxus_core::plugin::PluginManager> {
    let dir = TempDir::new().unwrap();
    let path = dir.into_path();
    Arc::new(doxus_core::plugin::PluginManager::new(path))
}

/// Test-only EventSink that records all emitted events.
#[derive(Clone, Default)]
struct RecordingEventSink {
    events: Arc<Mutex<Vec<SyncEvent>>>,
}

impl RecordingEventSink {
    fn emitted(&self) -> Vec<SyncEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: SyncEvent) {
        self.events.lock().unwrap().push(event);
    }
}

// ── RED: sync_loop_emits_progress_event ──────────────────────────────────────

/// `spawn_sync_loop_with_sink` must emit at least one `SyncEvent::Progress`
/// while processing a due instance.
#[tokio::test]
async fn sync_loop_emits_progress_event() {
    use doxus_core::plugin::PluginManager;

    let vault_dir = TempDir::new().unwrap();
    std::fs::write(vault_dir.path().join("note.md"), "# Test\ncontent").unwrap();

    let (conn, _db_dir) = open_test_db();
    {
        let guard = conn.lock().unwrap();
        insert_obsidian_instance(&guard, vault_dir.path().to_str().unwrap());
    }

    let plugins_dir = TempDir::new().unwrap();
    let mut pm = PluginManager::new(plugins_dir.path().to_path_buf());
    pm.register_factory("com.doxus.obsidian", || {
        Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
    });
    let plugin_manager = Arc::new(pm);

    let sink = RecordingEventSink::default();

    // interval_secs = 0 → always due
    let handle = spawn_sync_loop_with_sink(
        Arc::clone(&conn),
        None,
        Arc::clone(&plugin_manager),
        0,
        sink.clone(),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    handle.shutdown().await;

    let events = sink.emitted();
    assert!(
        events.iter().any(|e| matches!(e, SyncEvent::Progress { .. })),
        "expected at least one SyncEvent::Progress, got: {events:?}"
    );
}

// ── RED: sync_loop_emits_complete_event ──────────────────────────────────────

/// `spawn_sync_loop_with_sink` must emit a `SyncEvent::Complete` after a
/// successful sync.
#[tokio::test]
async fn sync_loop_emits_complete_event() {
    use doxus_core::plugin::PluginManager;

    let vault_dir = TempDir::new().unwrap();
    std::fs::write(vault_dir.path().join("doc.md"), "# Doc\nbody").unwrap();

    let (conn, _db_dir) = open_test_db();
    {
        let guard = conn.lock().unwrap();
        insert_obsidian_instance(&guard, vault_dir.path().to_str().unwrap());
    }

    let plugins_dir = TempDir::new().unwrap();
    let mut pm = PluginManager::new(plugins_dir.path().to_path_buf());
    pm.register_factory("com.doxus.obsidian", || {
        Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
    });
    let plugin_manager = Arc::new(pm);

    let sink = RecordingEventSink::default();

    let handle = spawn_sync_loop_with_sink(
        Arc::clone(&conn),
        None,
        Arc::clone(&plugin_manager),
        0,
        sink.clone(),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    handle.shutdown().await;

    let events = sink.emitted();
    assert!(
        events.iter().any(|e| matches!(e, SyncEvent::Complete { .. })),
        "expected at least one SyncEvent::Complete, got: {events:?}"
    );
}

// ── RED: noop sink does not panic ─────────────────────────────────────────────

/// When no sink is provided (NoopEventSink), the loop operates normally
/// without panicking — this is the CLI-mode compatibility requirement.
#[tokio::test]
async fn sync_loop_noop_sink_does_not_panic() {
    use doxus_mcp::sync_loop::{NoopEventSink, spawn_sync_loop_with_sink};

    let (conn, _dir) = open_test_db();
    let handle = spawn_sync_loop_with_sink(
        Arc::clone(&conn),
        None,
        make_plugin_manager(),
        1,
        NoopEventSink,
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown().await;
    // reaching here = no panic
}

// ── fixture helpers ───────────────────────────────────────────────────────────

fn insert_obsidian_instance(conn: &Connection, vault_path: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
         VALUES ('com.doxus.obsidian', 'Obsidian', '0.1.0', unixepoch())",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO projects(name, display_name, path, created_at, updated_at)
         VALUES ('obsidian-proj', 'Obsidian', ?1, unixepoch(), unixepoch())",
        rusqlite::params![vault_path],
    )
    .unwrap();
    let pid: i64 = conn
        .query_row("SELECT last_insert_rowid()", [], |r: &rusqlite::Row| r.get(0))
        .unwrap();
    let config = format!(r#"{{"path":"{}"}}"#, vault_path);
    conn.execute(
        "INSERT OR IGNORE INTO source_instances(plugin_id, project_id, name, config_json, created_at)
         VALUES ('com.doxus.obsidian', ?1, 'obsidian-src', ?2, unixepoch())",
        rusqlite::params![pid, config],
    )
    .unwrap();
}
