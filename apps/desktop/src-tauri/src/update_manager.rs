use doxus_core::db;
use semver::Version;

#[derive(Debug, PartialEq)]
pub enum MigrationOutcome {
    /// First run — no previous version stored.
    FirstRun,
    /// Same version as last run, nothing to do.
    UpToDate,
    /// Upgraded from `from` to `to`, migrations ran.
    Upgraded { from: String, to: String },
    /// Downgraded from `from` to `to`. No data-destructive action taken.
    Downgraded { from: String, to: String },
}

#[derive(Debug)]
pub struct MigrationResult {
    pub outcome: MigrationOutcome,
    pub reindex_triggered: bool,
}

/// Core logic: detect version change and run post-update migrations.
///
/// Pure function — receives `current_version` as a string so Tauri is not required.
/// Should be called **after** DB migrations (V1–V40) have already run.
///
/// On `Upgraded`: writes `audit_log` entry, runs any matching migration hooks,
/// then updates `last_run_version` to `current_version`.
/// On `Downgraded`: writes a warning `audit_log` entry, updates `last_run_version`.
/// On `FirstRun`/`UpToDate`: just updates (or keeps) `last_run_version`.
pub fn detect_and_migrate(
    conn: &rusqlite::Connection,
    indexing: Option<&dyn PostUpdateHook>,
    current_version: &str,
) -> Result<MigrationResult, String> {
    let current = Version::parse(current_version)
        .map_err(|e| format!("invalid current_version '{}': {}", current_version, e))?;

    let last_raw = db::get_system_config(conn, "last_run_version")
        .map_err(|e| e.to_string())?;

    // Determine outcome
    let outcome = match &last_raw {
        None => MigrationOutcome::FirstRun,
        Some(v) if v == "0.0.0" => MigrationOutcome::FirstRun,
        Some(raw) => {
            let last = Version::parse(raw)
                .map_err(|e| format!("invalid stored last_run_version '{}': {}", raw, e))?;
            if current > last {
                MigrationOutcome::Upgraded {
                    from: raw.clone(),
                    to: current_version.to_string(),
                }
            } else if current < last {
                MigrationOutcome::Downgraded {
                    from: raw.clone(),
                    to: current_version.to_string(),
                }
            } else {
                MigrationOutcome::UpToDate
            }
        }
    };

    let mut reindex_triggered = false;

    match &outcome {
        MigrationOutcome::UpToDate => {
            // Nothing to do
        }
        MigrationOutcome::FirstRun => {
            record_audit(conn, "migration_completed", None, &serde_json::json!({
                "from_version": null,
                "to_version": current_version,
            }));
            db::set_system_config(conn, "last_run_version", current_version)
                .map_err(|e| e.to_string())?;
        }
        MigrationOutcome::Upgraded { from, to } => {
            record_audit(conn, "migration_started", None, &serde_json::json!({
                "from_version": from,
                "to_version": to,
            }));

            // Version-specific migration hooks
            let from_ver = Version::parse(from).unwrap();

            // ≥ 0.2.0: force full reindex (embedding format changed)
            if current >= Version::new(0, 2, 0) && from_ver < Version::new(0, 2, 0) {
                if let Some(hook) = indexing {
                    if let Err(e) = hook.force_reindex_all() {
                        record_audit(conn, "migration_failed", None, &serde_json::json!({
                            "from_version": from,
                            "to_version": to,
                            "error": e,
                        }));
                        // Non-fatal: log and continue — user can reindex manually
                        tracing::error!(error = %e, "force_reindex_all failed during post-update migration");
                    } else {
                        reindex_triggered = true;
                    }
                }
            }

            db::set_system_config(conn, "last_run_version", to)
                .map_err(|e| e.to_string())?;
            record_audit(conn, "migration_completed", None, &serde_json::json!({
                "from_version": from,
                "to_version": to,
            }));
        }
        MigrationOutcome::Downgraded { from, to } => {
            tracing::warn!(from = %from, to = %to, "downgrade detected — skipping post-update migrations");
            record_audit(conn, "downgrade_detected", None, &serde_json::json!({
                "from_version": from,
                "to_version": to,
            }));
            db::set_system_config(conn, "last_run_version", to)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(MigrationResult { outcome, reindex_triggered })
}

fn record_audit(conn: &rusqlite::Connection, event_type: &str, project_id: Option<i64>, payload: &serde_json::Value) {
    let _ = conn.execute(
        "INSERT INTO audit_log(project_id, event_type, payload, occurred_at) VALUES (?1, ?2, ?3, unixepoch())",
        rusqlite::params![project_id, event_type, payload.to_string()],
    );
}

/// Abstraction over force-reindex so tests can mock it.
pub trait PostUpdateHook: Send + Sync {
    fn force_reindex_all(&self) -> Result<(), String>;
}

/// Production PostUpdateHook — emits Tauri events and spawns async reindex.
pub struct TauriReindexHook {
    handle: tauri::AppHandle,
    conn: doxus_core::db::DbPool,
    indexing: std::sync::Arc<doxus_core::indexing::IndexingService>,
}

impl TauriReindexHook {
    pub fn new(
        handle: tauri::AppHandle,
        conn: doxus_core::db::DbPool,
        indexing: std::sync::Arc<doxus_core::indexing::IndexingService>,
    ) -> Self {
        Self { handle, conn, indexing }
    }
}

impl PostUpdateHook for TauriReindexHook {
    fn force_reindex_all(&self) -> Result<(), String> {
        use tauri::Emitter;
        // Emit is best-effort — frontend may not be mounted yet at startup.
        let _ = self.handle.emit("migration:reindex_started", serde_json::json!({}));

        let handle = self.handle.clone();
        let conn = self.conn.clone();
        let indexing = self.indexing.clone();

        tauri::async_runtime::spawn(async move {
            match doxus_core::reindex::force_reindex_all_projects(conn, indexing).await {
                Ok(count) => {
                    handle
                        .emit("migration:reindex_completed", serde_json::json!({ "count": count }))
                        .ok();
                }
                Err(e) => {
                    tracing::error!(error = %e, "force_reindex_all_projects failed during post-update migration");
                }
            }
        });

        Ok(())
    }
}

/// Tauri IPC command — delegates to `tauri_plugin_process::restart`.
#[tauri::command]
#[allow(unreachable_code)]
pub async fn relaunch_app(handle: tauri::AppHandle) -> Result<(), String> {
    handle.restart();
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_core::db::TestDb;

    struct AlwaysOkReindex;
    impl PostUpdateHook for AlwaysOkReindex {
        fn force_reindex_all(&self) -> Result<(), String> { Ok(()) }
    }

    struct AlwaysFailReindex;
    impl PostUpdateHook for AlwaysFailReindex {
        fn force_reindex_all(&self) -> Result<(), String> {
            Err("reindex failed".to_string())
        }
    }

    fn ok_hook() -> &'static dyn PostUpdateHook { &AlwaysOkReindex }
    fn fail_hook() -> &'static dyn PostUpdateHook { &AlwaysFailReindex }

    // ── Happy path ────────────────────────────────────────────────────────────

    #[test]
    fn first_run_no_last_version() {
        let db = TestDb::new();
        let result = detect_and_migrate(&db.conn, None, "0.1.0").unwrap();
        assert_eq!(result.outcome, MigrationOutcome::FirstRun);
        let stored = doxus_core::db::get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(stored.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn first_run_default_sentinel() {
        // V40 inserts "0.0.0" as a sentinel — should still be treated as FirstRun
        let db = TestDb::new();
        let result = detect_and_migrate(&db.conn, None, "0.1.0").unwrap();
        assert_eq!(result.outcome, MigrationOutcome::FirstRun);
    }

    #[test]
    fn up_to_date_same_version() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.1.0").unwrap();
        let result = detect_and_migrate(&db.conn, None, "0.1.0").unwrap();
        assert_eq!(result.outcome, MigrationOutcome::UpToDate);
        assert!(!result.reindex_triggered);
    }

    #[test]
    fn upgrade_below_0_2_no_reindex() {
        // 0.1.0 → 0.1.5: same MINOR series, no reindex hook
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.1.0").unwrap();
        let result = detect_and_migrate(&db.conn, Some(ok_hook()), "0.1.5").unwrap();
        assert!(matches!(result.outcome, MigrationOutcome::Upgraded { .. }));
        assert!(!result.reindex_triggered, "no reindex for 0.1.x→0.1.y");
    }

    #[test]
    fn upgrade_to_0_2_triggers_reindex() {
        // 0.1.0 → 0.2.0: embedding format changed, reindex required
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.1.0").unwrap();
        let result = detect_and_migrate(&db.conn, Some(ok_hook()), "0.2.0").unwrap();
        assert!(matches!(result.outcome, MigrationOutcome::Upgraded { .. }));
        assert!(result.reindex_triggered);
        let stored = doxus_core::db::get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(stored.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn semver_ordering_correctness() {
        // Regression: string comparison "0.10.0" < "0.9.0" would be wrong
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.9.0").unwrap();
        let result = detect_and_migrate(&db.conn, None, "0.10.0").unwrap();
        assert!(
            matches!(result.outcome, MigrationOutcome::Upgraded { .. }),
            "0.10.0 must be detected as NEWER than 0.9.0"
        );
    }

    #[test]
    fn downgrade_detected_updates_version_no_migration() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.5.0").unwrap();
        let result = detect_and_migrate(&db.conn, Some(ok_hook()), "0.3.0").unwrap();
        assert!(matches!(result.outcome, MigrationOutcome::Downgraded { .. }));
        assert!(!result.reindex_triggered);
        let stored = doxus_core::db::get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(stored.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn reindex_failure_is_non_fatal() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.1.0").unwrap();
        let result = detect_and_migrate(&db.conn, Some(fail_hook()), "0.2.0").unwrap();
        assert!(matches!(result.outcome, MigrationOutcome::Upgraded { .. }));
        assert!(!result.reindex_triggered, "failed reindex is not counted as triggered");
        let stored = doxus_core::db::get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(stored.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn audit_log_entry_written_on_upgrade() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.1.0").unwrap();
        detect_and_migrate(&db.conn, None, "0.1.5").unwrap();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'migration_completed'",
            [], |r| r.get::<_, i64>(0),
        ).unwrap();
        assert!(count >= 1, "audit_log should record migration_completed");
    }

    #[test]
    fn audit_log_entry_written_on_downgrade() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "0.5.0").unwrap();
        detect_and_migrate(&db.conn, None, "0.3.0").unwrap();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'downgrade_detected'",
            [], |r| r.get::<_, i64>(0),
        ).unwrap();
        assert!(count >= 1, "audit_log should record downgrade_detected");
    }

    #[test]
    fn invalid_stored_version_returns_err() {
        let db = TestDb::new();
        doxus_core::db::set_system_config(&db.conn, "last_run_version", "not-semver").unwrap();
        let result = detect_and_migrate(&db.conn, None, "0.1.0");
        assert!(result.is_err(), "invalid stored version should return Err");
    }
}
