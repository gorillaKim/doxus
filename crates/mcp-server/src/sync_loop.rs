//! Background sync loop for doxus-mcp.
//!
//! Spawns a tokio task that periodically checks for source instances that are
//! due for synchronization (via `SyncScheduler::due_instances`) and logs them.
//! The loop terminates when the shutdown sender is dropped or a `true` value is
//! sent through the watch channel.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use doxus_core::plugin::PluginManager;
use doxus_core::sync::{SyncDb, SyncScheduler};
use doxus_plugin_sdk::{FetchChangesOpts, PluginConfig, PluginError, PluginSecrets, SecretValue};
use rand::Rng;

use tokio::sync::watch;

// ── Retry policy ─────────────────────────────────────────────────────────────

/// Configuration for exponential-backoff retry behaviour.
pub struct RetryPolicy {
    /// Maximum number of retries (attempts = max_retries + 1).
    pub max_retries: u32,
    /// Base delay for the first retry interval.
    pub base_delay: Duration,
    /// Hard upper bound on any single sleep duration (before jitter).
    pub max_delay: Duration,
}

/// Retry `f` up to `policy.max_retries` additional times on error, sleeping
/// with exponential backoff + jitter between attempts.
///
/// Delay formula: `min(base * 2^attempt, max_delay) + rand(0 .. base * 0.1)`
pub async fn retry_with_backoff<F, Fut, T, E>(policy: &RetryPolicy, f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut last_err;
    match f().await {
        Ok(v) => return Ok(v),
        Err(e) => last_err = e,
    }

    for attempt in 0..policy.max_retries {
        // base * 2^attempt, capped at max_delay
        let exp = policy.base_delay.saturating_mul(2u32.pow(attempt));
        let capped = exp.min(policy.max_delay);
        // jitter: uniform [0, base * 0.1)
        let jitter_nanos = {
            let max_jitter = (policy.base_delay.as_nanos() / 10).max(1) as u64;
            rand::thread_rng().gen_range(0..max_jitter)
        };
        let sleep = capped + Duration::from_nanos(jitter_nanos);
        tokio::time::sleep(sleep).await;

        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

// ── Rate limit handling ───────────────────────────────────────────────────────

/// Action returned by [`handle_rate_limited`].
#[derive(Debug, PartialEq, Eq)]
pub enum RateLimitAction {
    /// The wait completed normally — caller should retry.
    Retry,
    /// A shutdown signal arrived during the wait — caller should stop.
    Shutdown,
}

/// Maximum number of seconds to wait when rate-limited (prevents unbounded sleeps).
const MAX_RATE_LIMIT_WAIT_SECS: u64 = 300;

/// Sleep for `retry_after_secs` (capped at [`MAX_RATE_LIMIT_WAIT_SECS`]), but
/// cancel immediately if a shutdown signal arrives.
/// Returns [`RateLimitAction`] so the caller can decide what to do.
pub async fn handle_rate_limited(
    retry_after_secs: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> RateLimitAction {
    let retry_after_secs = retry_after_secs.min(MAX_RATE_LIMIT_WAIT_SECS);
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(retry_after_secs)) => {
            RateLimitAction::Retry
        }
        result = shutdown_rx.changed() => {
            if result.is_err() || *shutdown_rx.borrow() {
                RateLimitAction::Shutdown
            } else {
                RateLimitAction::Retry
            }
        }
    }
}

/// Like [`retry_with_backoff`] but understands [`PluginError::RateLimited`].
///
/// * `RateLimited` — waits the requested duration (or cancels on shutdown),
///   then retries **without** consuming a retry counter slot.
/// * Any other error — uses the normal exponential-backoff retry logic.
/// * Returns `Err(PluginError)` once retries are exhausted or the loop is shut
///   down.
pub async fn retry_with_backoff_rate_aware<F, Fut, T>(
    policy: &RetryPolicy,
    shutdown_rx: &mut watch::Receiver<bool>,
    f: F,
) -> Result<T, PluginError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, PluginError>>,
{
    let mut failure_count: u32 = 0;

    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(PluginError::RateLimited { retry_after_secs }) => {
                tracing::warn!(retry_after_secs, "rate limited — waiting before retry");
                match handle_rate_limited(retry_after_secs, shutdown_rx).await {
                    RateLimitAction::Retry => continue,
                    RateLimitAction::Shutdown => {
                        return Err(PluginError::Internal("shutdown during rate limit wait".into()));
                    }
                }
            }
            Err(e) => {
                if failure_count >= policy.max_retries {
                    return Err(e);
                }
                let exp = policy.base_delay.saturating_mul(2u32.pow(failure_count));
                let capped = exp.min(policy.max_delay);
                let jitter_nanos = {
                    let max_jitter = (policy.base_delay.as_nanos() / 10).max(1) as u64;
                    rand::thread_rng().gen_range(0..max_jitter)
                };
                tokio::time::sleep(capped + Duration::from_nanos(jitter_nanos)).await;
                failure_count += 1;
            }
        }
    }
}

// ── Event sink abstraction ────────────────────────────────────────────────────

/// Events emitted by the sync loop to interested observers (e.g. Tauri UI).
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Sync started for a source instance.
    Progress {
        instance_id: i64,
        plugin_id: String,
    },
    /// Sync completed successfully.
    Complete {
        instance_id: i64,
        updated: usize,
    },
    /// Sync failed after all retries.
    Error {
        instance_id: i64,
        message: String,
    },
}

/// Abstraction over "somewhere to send sync events".
///
/// The production implementation (`TauriEventSink`) wraps `tauri::AppHandle`.
/// Tests use a `RecordingEventSink`.  CLI mode uses [`NoopEventSink`].
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: SyncEvent);
}

/// No-op sink for CLI mode (no Tauri runtime available).
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: SyncEvent) {}
}

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

pub fn spawn_sync_loop(
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    embedder: Option<Arc<dyn doxus_core::embedding::EmbeddingProvider>>,
    plugin_manager: Arc<PluginManager>,
    interval_secs: u64,
) -> SyncLoopHandle {
    spawn_sync_loop_with_sink(conn, embedder, plugin_manager, interval_secs, NoopEventSink)
}

/// Spawn the background sync loop with an [`EventSink`] for UI notifications.
pub fn spawn_sync_loop_with_sink<S: EventSink>(
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    embedder: Option<Arc<dyn doxus_core::embedding::EmbeddingProvider>>,
    plugin_manager: Arc<PluginManager>,
    interval_secs: u64,
    sink: S,
) -> SyncLoopHandle {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let sleep_duration = Duration::from_secs(interval_secs.max(1));

    let join_handle = tokio::spawn(async move {
        let scheduler = SyncScheduler::new(interval_secs);

        loop {
            let due = {
                let guard = match conn.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        tracing::error!("sync_loop: failed to lock connection: {e}");
                        break;
                    }
                };
                let sync_db = doxus_core::sync::SyncDb::new(&*guard);
                match scheduler.due_instances(&sync_db) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("sync_loop: scheduler error: {e}");
                        vec![]
                    }
                }
            };

            if due.is_empty() {
                tracing::debug!("sync_loop: no instances due");
            } else {
                tracing::info!(count = due.len(), "sync_loop: {} instance(s) due for sync", due.len());
                for inst in due {
                    sink.emit(SyncEvent::Progress {
                        instance_id: inst.id,
                        plugin_id: inst.plugin_id.clone(),
                    });

                    match plugin_manager.get_source(&inst.plugin_id) {
                        Some(mut source) => {
                            let mut config_fields: std::collections::HashMap<String, serde_json::Value> =
                                match serde_json::from_str(&inst.config_json) {
                                    Ok(f) => f,
                                    Err(e) => {
                                        tracing::warn!(
                                            instance_id = inst.id,
                                            plugin_id = %inst.plugin_id,
                                            error = %e,
                                            "sync_loop: malformed config_json, skipping instance"
                                        );
                                        sink.emit(SyncEvent::Error {
                                            instance_id: inst.id,
                                            message: format!("malformed config_json: {e}"),
                                        });
                                        continue;
                                    }
                                };

                            // Tauri 저장 형식 대응: "fields" 키가 있으면 내부 객체 사용
                            if let Some(inner) = config_fields.get("fields").and_then(|v| v.as_object()) {
                                config_fields = inner.clone().into_iter().collect();
                            }
                            let mut plugin_config = PluginConfig { fields: config_fields };
                            let mut plugin_secrets = PluginSecrets::default();
                            
                            // 키체인에서 인증 정보 로드하여 설정 및 시크릿에 주입
                            crate::auth::inject_keychain_auth(&inst.plugin_id, &mut plugin_config, &mut plugin_secrets);

                            if let Err(e) = source.initialize(plugin_config, plugin_secrets).await {
                                tracing::warn!(
                                    instance_id = inst.id,
                                    error = %e,
                                    "sync_loop: failed to initialize plugin, skipping"
                                );
                                sink.emit(SyncEvent::Error {
                                    instance_id: inst.id,
                                    message: format!("initialize failed: {e}"),
                                });
                                continue;
                            }
                            let retry_policy = RetryPolicy {
                                max_retries: 3,
                                base_delay: Duration::from_secs(1),
                                max_delay: Duration::from_secs(30),
                            };
                            let cursor = inst.sync_cursor.clone();
                            let fetch_result = retry_with_backoff(&retry_policy, || {
                                let cursor = cursor.clone();
                                let opts = FetchChangesOpts {
                                    since: 0,
                                    cursor,
                                    page_size: 100,
                                    known_ids: vec![],
                                };
                                source.fetch_changes(opts)
                            })
                            .await;
                            match fetch_result {
                                Ok(changeset) => {
                                    let updated_count = changeset.updated.len();
                                    let new_cursor = changeset.next_cursor.clone();
                                    
                                    // 실제 검색 엔진에 인덱싱 수행
                                    if let Some(ref provider) = embedder {
                                        let engine = doxus_core::search::SearchEngine::with_embedder(
                                            Arc::clone(&conn),
                                            Arc::clone(provider),
                                        );
                                        
                                        for doc in &changeset.updated {
                                            let meta = doxus_core::search::DocMeta {
                                                tags: doc.tags.clone(),
                                                aliases: doc.aliases.clone(),
                                                created_at: doc.created_at,
                                                updated_at: doc.updated_at,
                                                relative_path: doc.relative_path.clone(),
                                                metadata: doc.metadata.clone(),
                                            };
                                            
                                            // 비동기로 인덱싱 수행 (실패 시 로그 기록)
                                            if let Err(e) = engine.index_document_async_with_meta(
                                                inst.project_id, 
                                                &doc.id.0, 
                                                doc.title.as_deref().unwrap_or("Untitled"), 
                                                &doc.content, 
                                                meta
                                            ).await {
                                                tracing::error!(instance_id = inst.id, doc_id = %doc.id.0, error = %e, "sync_loop: document indexing failed");
                                            }
                                        }
                                        tracing::info!(instance_id = inst.id, count = updated_count, "sync_loop: batch indexing completed");
                                    }

                                    match conn.lock() {
                                        Ok(guard) => {
                                            let sync_db = SyncDb::new(&*guard);
                                            if let Err(e) = sync_db.mark_synced(inst.id, new_cursor.as_deref()) {
                                                tracing::warn!(
                                                    instance_id = inst.id,
                                                    error = %e,
                                                    "sync_loop: failed to mark synced"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                instance_id = inst.id,
                                                error = %e,
                                                "sync_loop: connection lock poisoned, aborting loop"
                                            );
                                            return;
                                        }
                                    }
                                    tracing::info!(
                                        instance_id = inst.id,
                                        updated = updated_count,
                                        "sync_loop: sync completed"
                                    );
                                    sink.emit(SyncEvent::Complete {
                                        instance_id: inst.id,
                                        updated: updated_count,
                                    });
                                }
                                Err(e) => {
                                    tracing::error!(
                                        instance_id = inst.id,
                                        error = %e,
                                        "sync_loop: sync failed after retries"
                                    );
                                    sink.emit(SyncEvent::Error {
                                        instance_id: inst.id,
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                        None => {
                            tracing::debug!(
                                plugin_id = %inst.plugin_id,
                                "sync_loop: no source for plugin, skipping"
                            );
                        }
                    }
                }
            }

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
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn open_test_db() -> (Arc<Mutex<Connection>>, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = db::open(&path).unwrap();
        (Arc::new(Mutex::new(conn)), dir)
    }

    fn make_plugin_manager() -> Arc<doxus_core::plugin::PluginManager> {
        let dir = TempDir::new().unwrap();
        // We intentionally leak the TempDir here because we only need the PluginManager
        // to exist for the test duration and tests are short-lived.
        let path = dir.into_path();
        Arc::new(doxus_core::plugin::PluginManager::new(path))
    }

    #[tokio::test]
    async fn loop_starts_and_shuts_down_gracefully() {
        let (conn, _dir) = open_test_db();
        let handle = spawn_sync_loop(conn, None, make_plugin_manager(), 1);
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
        let conn_clone: Arc<Mutex<Connection>> = Arc::clone(&conn);
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
        let handle = spawn_sync_loop(conn, None, make_plugin_manager(), 1);
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
            .query_row("SELECT last_insert_rowid()", [], |r: &rusqlite::Row| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES ('com.test.loop', ?1, 'loop-src', '{}', unixepoch())",
            rusqlite::params![pid],
        )
        .unwrap();
    }

    // ── A-1: sync_loop calls run_once for obsidian instance ──────────────────

    fn insert_obsidian_instance(conn: &Connection, vault_path: &str) -> i64 {
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
        conn.query_row("SELECT last_insert_rowid()", [], |r: &rusqlite::Row| r.get(0))
            .unwrap()
    }

    #[tokio::test]
    async fn sync_loop_calls_run_once_for_obsidian_instance() {
        use doxus_core::plugin::PluginManager;
        use tempfile::TempDir;

        let vault_dir = TempDir::new().unwrap();
        // Write a markdown file so fetch_changes has something to return
        std::fs::write(vault_dir.path().join("note.md"), "# Test\ncontent").unwrap();

        let (conn, _db_dir) = open_test_db();
        let instance_id = {
            let guard = conn.lock().unwrap();
            insert_obsidian_instance(&guard, vault_dir.path().to_str().unwrap())
        };

        let plugins_dir = TempDir::new().unwrap();
        let mut pm = PluginManager::new(plugins_dir.path().to_path_buf());
        pm.register_factory("com.doxus.obsidian", || {
            Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
        });
        let plugin_manager = Arc::new(pm);

        // interval_secs = 0 → always due
        let handle = spawn_sync_loop(Arc::clone(&conn), None, Arc::clone(&plugin_manager), 0);
        // Give the loop time to run at least one iteration
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.shutdown().await;

        // Verify: last_synced should now be set for the instance
        let guard = conn.lock().unwrap();
        let last_synced: Option<i64> = guard
            .query_row(
                "SELECT last_synced FROM source_instances WHERE id = ?1",
                rusqlite::params![instance_id],
                |r: &rusqlite::Row| r.get::<_, Option<i64>>(0),
            )
            .unwrap();
        assert!(
            last_synced.is_some(),
            "expected last_synced to be set after run_once, but it was NULL"
        );
    }
}
