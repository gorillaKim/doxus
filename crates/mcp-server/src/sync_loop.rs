//! Background sync loop for doxus-mcp.
//!
//! Spawns a tokio task that periodically checks for source instances that are
//! due for synchronization (via `SyncScheduler::due_instances`) and logs them.
//! The loop terminates when the shutdown sender is dropped or a `true` value is
//! sent through the watch channel.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use doxus_core::sync::{SyncDb, SyncScheduler};
use rusqlite::Connection;
use tokio::sync::watch;

/// Handle returned by [`spawn_sync_loop`].  Drop or send `true` to stop the loop.
pub struct SyncLoopHandle {
    pub shutdown_tx: watch::Sender<bool>,
    pub join_handle: tokio::task::JoinHandle<()>,
}

impl SyncLoopHandle {
    /// Signal the sync loop to stop and wait for it to finish.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.join_handle.await;
    }
}

/// Spawn the background sync loop.
///
/// * `conn` — shared SQLite connection wrapped in `Arc<Mutex<Connection>>`.
/// * `interval_secs` — how often to poll for due instances (also used as the
///   staleness threshold passed to `SyncScheduler`).
///
/// Returns a [`SyncLoopHandle`] that can be used to trigger a graceful shutdown.
pub fn spawn_sync_loop(
    conn: Arc<Mutex<Connection>>,
    interval_secs: u64,
) -> SyncLoopHandle {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let sleep_duration = Duration::from_secs(interval_secs.max(1));

    let join_handle = tokio::spawn(async move {
        let scheduler = SyncScheduler::new(interval_secs);

        loop {
            // Poll for due instances.
            {
                let guard = match conn.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        tracing::error!("sync_loop: failed to lock connection: {e}");
                        break;
                    }
                };
                let sync_db = SyncDb::new(&*guard);
                match scheduler.due_instances(&sync_db) {
                    Ok(due) if !due.is_empty() => {
                        tracing::info!(
                            count = due.len(),
                            "sync_loop: {} instance(s) due for sync",
                            due.len()
                        );
                        for inst in &due {
                            tracing::debug!(
                                instance_id = inst.id,
                                plugin_id = %inst.plugin_id,
                                "sync_loop: instance due"
                            );
                        }
                    }
                    Ok(_) => {
                        tracing::debug!("sync_loop: no instances due");
                    }
                    Err(e) => {
                        tracing::warn!("sync_loop: scheduler error: {e}");
                    }
                }
            }

            // Wait for next tick or shutdown signal.
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {}
                result = shutdown_rx.changed() => {
                    if result.is_err() || *shutdown_rx.borrow() {
                        tracing::info!("sync_loop: shutdown signal received");
                        break;
                    }
                }
            }
        }

        tracing::info!("sync_loop: exited");
    });

    SyncLoopHandle { shutdown_tx, join_handle }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_core::db;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn open_test_db() -> (Arc<Mutex<Connection>>, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = db::open(&path).unwrap();
        (Arc::new(Mutex::new(conn)), dir)
    }

    #[tokio::test]
    async fn loop_starts_and_shuts_down_gracefully() {
        let (conn, _dir) = open_test_db();
        let handle = spawn_sync_loop(conn, 1);
        // Give the loop one tick to start.
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.shutdown().await;
        // If we reach here, shutdown completed without hanging.
    }

    #[tokio::test]
    async fn loop_runs_multiple_iterations() {
        use doxus_core::sync::SyncDb;

        use std::sync::atomic::{AtomicUsize, Ordering};

        let (conn, _dir) = open_test_db();

        // Insert a source instance so due_instances returns something.
        {
            let guard = conn.lock().unwrap();
            insert_test_instance(&guard);
        }

        // Use a very short interval (100 ms) so we get multiple iterations quickly.
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let conn_clone = Arc::clone(&conn);
        let poll_count = Arc::new(AtomicUsize::new(0));
        let poll_count_clone = Arc::clone(&poll_count);

        let join = tokio::spawn(async move {
            let scheduler = SyncScheduler::new(0); // 0 = always due
            let sleep_dur = Duration::from_millis(100);
            loop {
                {
                    let guard = conn_clone.lock().unwrap();
                    let sync_db = SyncDb::new(&*guard);
                    if let Ok(due) = scheduler.due_instances(&sync_db) {
                        poll_count_clone.fetch_add(due.len(), Ordering::SeqCst);
                    }
                }
                tokio::select! {
                    _ = tokio::time::sleep(sleep_dur) => {}
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() { break; }
                    }
                }
            }
        });

        // Let it run for ~350 ms → at least 3 iterations.
        tokio::time::sleep(Duration::from_millis(350)).await;
        let _ = shutdown_tx.send(true);
        let _ = join.await;

        assert!(
            poll_count.load(Ordering::SeqCst) >= 3,
            "expected at least 3 poll iterations, got {}",
            poll_count.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn shutdown_tx_drop_stops_loop() {
        let (conn, _dir) = open_test_db();
        let handle = spawn_sync_loop(conn, 1);
        // Drop the sender — loop should notice channel closed and exit.
        drop(handle.shutdown_tx);
        // join_handle should complete within reasonable time.
        let _ = tokio::time::timeout(Duration::from_secs(3), handle.join_handle)
            .await
            .expect("loop did not stop after sender was dropped");
    }

    fn insert_test_instance(conn: &Connection) {
        conn.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
             VALUES ('com.test.loop', 'Loop Plugin', '0.0.1', unixepoch())",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('loop-proj', 'Loop', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let pid: i64 = conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES ('com.test.loop', ?1, 'loop-src', '{}', unixepoch())",
            rusqlite::params![pid],
        )
        .unwrap();
    }
}
