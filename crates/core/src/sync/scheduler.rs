use super::db::{DueInstance, SyncDb};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("plugin error: {0}")]
    Plugin(String),
}

/// Determines which source instances are due for synchronization based on a
/// fixed interval, delegating the DB query to `SyncDb`.
pub struct SyncScheduler {
    interval_secs: u64,
}

impl SyncScheduler {
    pub fn new(interval_secs: u64) -> Self {
        Self { interval_secs }
    }

    pub fn interval_secs(&self) -> u64 {
        self.interval_secs
    }

    /// Returns source instances that are due for synchronization.
    /// Delegates to `SyncDb::due_instances` with the configured interval.
    pub fn due_instances<'a>(&self, db: &'a SyncDb<'a>) -> Result<Vec<DueInstance>, SyncError> {
        db.due_instances(self.interval_secs as i64)
            .map_err(SyncError::Db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use rusqlite::Connection;

    fn insert_instance(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
             VALUES ('com.test.sched', 'Sched Plugin', '0.0.1', unixepoch())",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('sched-proj', 'Sched', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let pid: i64 = conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get::<_, i64>(0))
            .unwrap();
        conn.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES ('com.test.sched', ?1, 'sched-src', '{}', unixepoch())",
            rusqlite::params![pid],
        )
        .unwrap();
        conn.query_row("SELECT last_insert_rowid()", [], |r| r.get::<_, i64>(0))
            .unwrap()
    }

    #[test]
    fn scheduler_delegates_to_sync_db_unsynced() {
        let db = TestDb::new();
        insert_instance(&db.conn);
        let scheduler = SyncScheduler::new(3600);
        let sync_db = SyncDb::new(&db.conn);
        let due = scheduler.due_instances(&sync_db).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].plugin_id, "com.test.sched");
    }

    #[test]
    fn scheduler_excludes_recently_synced() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        sync_db.mark_synced(id, None).unwrap();

        let scheduler = SyncScheduler::new(3600);
        let due = scheduler.due_instances(&sync_db).unwrap();
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn scheduler_returns_overdue_based_on_interval() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);
        // Simulate synced 2 hours ago
        db.conn
            .execute(
                "UPDATE source_instances SET last_synced = unixepoch() - 7200 WHERE id = ?1",
                rusqlite::params![id],
            )
            .unwrap();

        // 1-hour interval → 2h old is due
        let scheduler = SyncScheduler::new(3600);
        let sync_db = SyncDb::new(&db.conn);
        let due = scheduler.due_instances(&sync_db).unwrap();
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn scheduler_zero_interval_always_returns_due() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        // Mark as synced just now
        sync_db.mark_synced(id, None).unwrap();

        // 0-second interval — everything is always overdue
        let scheduler = SyncScheduler::new(0);
        let due = scheduler.due_instances(&sync_db).unwrap();
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn scheduler_stores_interval() {
        let s = SyncScheduler::new(1800);
        assert_eq!(s.interval_secs(), 1800);
    }
}
