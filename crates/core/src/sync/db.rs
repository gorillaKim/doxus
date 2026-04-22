use rusqlite::{Connection, OptionalExtension};

/// DB helper for sync-related operations on `source_instances`.
pub struct SyncDb<'a> {
    conn: &'a Connection,
}

/// A source instance that is due for synchronization.
#[derive(Debug, Clone)]
pub struct DueInstance {
    pub id: i64,
    pub plugin_id: String,
    pub project_id: i64,
    pub sync_cursor: Option<String>,
    pub last_synced: Option<i64>,
    pub project_name: String,
    pub config_json: String,
}

impl<'a> SyncDb<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Mark a source_instance sync complete — updates `last_synced` and `sync_cursor`.
    pub fn mark_synced(
        &self,
        instance_id: i64,
        cursor: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE source_instances SET last_synced = unixepoch(), sync_cursor = ?1 WHERE id = ?2",
            rusqlite::params![cursor, instance_id],
        )?;
        Ok(())
    }

    /// Fetch instances due for sync: `last_synced IS NULL` or older than `interval_secs`.
    /// Only returns instances belonging to `active` projects.
    pub fn due_instances(
        &self,
        interval_secs: i64,
    ) -> Result<Vec<DueInstance>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT si.id, si.plugin_id, si.project_id, si.sync_cursor, si.last_synced, p.name, si.config_json
             FROM source_instances si
             JOIN projects p ON si.project_id = p.id
             WHERE p.status = 'active'
               AND (si.last_synced IS NULL OR unixepoch() - si.last_synced >= ?1)
             ORDER BY COALESCE(si.last_synced, 0) ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![interval_secs], |r| {
            Ok(DueInstance {
                id: r.get(0)?,
                plugin_id: r.get(1)?,
                project_id: r.get(2)?,
                sync_cursor: r.get(3)?,
                last_synced: r.get(4)?,
                project_name: r.get(5)?,
                config_json: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        })?;
        rows.collect()
    }

    /// Get the current `sync_cursor` for a specific instance.
    pub fn get_cursor(&self, instance_id: i64) -> Result<Option<String>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT sync_cursor FROM source_instances WHERE id = ?1",
                rusqlite::params![instance_id],
                |r| r.get(0),
            )
            .optional()
            .map(|o| o.flatten())
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::db::TestDb;

    /// Insert a minimal plugin + project + source_instance, returning the instance id.
    fn insert_test_instance(conn: &Connection) -> i64 {
        // plugins.id is the FK target
        conn.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
             VALUES ('com.test', 'Test Plugin', '0.0.1', unixepoch())",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('test-proj', 'Test', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let proj_id: i64 = conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES ('com.test', ?1, 'test-src', '{}', unixepoch())",
            rusqlite::params![proj_id],
        )
        .unwrap();
        conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn mark_synced_updates_last_synced() {
        let db = TestDb::new();
        let id = insert_test_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        sync_db.mark_synced(id, Some("cursor-123")).unwrap();
        let cursor = sync_db.get_cursor(id).unwrap();
        assert_eq!(cursor, Some("cursor-123".to_string()));
    }

    #[test]
    fn mark_synced_clears_cursor_when_none() {
        let db = TestDb::new();
        let id = insert_test_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        // Set a cursor first
        sync_db.mark_synced(id, Some("old-cursor")).unwrap();
        // Then clear it
        sync_db.mark_synced(id, None).unwrap();
        let cursor = sync_db.get_cursor(id).unwrap();
        assert_eq!(cursor, None);
    }

    #[test]
    fn due_instances_returns_unsynced() {
        let db = TestDb::new();
        insert_test_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        let due = sync_db.due_instances(3600).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].plugin_id, "com.test");
        assert_eq!(due[0].project_name, "test-proj");
    }

    #[test]
    fn due_instances_excludes_recently_synced() {
        let db = TestDb::new();
        let id = insert_test_instance(&db.conn);
        let sync_db = SyncDb::new(&db.conn);
        sync_db.mark_synced(id, None).unwrap();
        // 1-hour interval — just synced, should not be due
        let due = sync_db.due_instances(3600).unwrap();
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn due_instances_returns_overdue() {
        let db = TestDb::new();
        let id = insert_test_instance(&db.conn);
        // Manually set last_synced to 2 hours ago
        db.conn
            .execute(
                "UPDATE source_instances SET last_synced = unixepoch() - 7200 WHERE id = ?1",
                rusqlite::params![id],
            )
            .unwrap();
        let sync_db = SyncDb::new(&db.conn);
        // 1-hour interval — 2 hours old, should be due
        let due = sync_db.due_instances(3600).unwrap();
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn due_instances_excludes_disabled_projects() {
        let db = TestDb::new();
        let id = insert_test_instance(&db.conn);
        db.conn
            .execute(
                "UPDATE projects SET status = 'disabled'
                 WHERE id = (SELECT project_id FROM source_instances WHERE id = ?1)",
                rusqlite::params![id],
            )
            .unwrap();
        let sync_db = SyncDb::new(&db.conn);
        let due = sync_db.due_instances(0).unwrap();
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn get_cursor_returns_none_for_unknown_instance() {
        let db = TestDb::new();
        let sync_db = SyncDb::new(&db.conn);
        let cursor = sync_db.get_cursor(999).unwrap();
        assert_eq!(cursor, None);
    }

    #[test]
    fn due_instances_ordered_oldest_first() {
        let db = TestDb::new();
        // Insert plugin once
        db.conn
            .execute(
                "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
                 VALUES ('com.test', 'Test Plugin', '0.0.1', unixepoch())",
                [],
            )
            .unwrap();
        // Insert two projects + instances with different last_synced
        for (name, offset) in [("proj-a", 7200i64), ("proj-b", 3600i64)] {
            db.conn
                .execute(
                    &format!(
                        "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                         VALUES ('{name}', '{name}', '/tmp', unixepoch(), unixepoch())"
                    ),
                    [],
                )
                .unwrap();
            let pid: i64 = db
                .conn
                .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
                .unwrap();
            db.conn
                .execute(
                    "INSERT INTO source_instances(plugin_id, project_id, name, config_json, last_synced, created_at)
                     VALUES ('com.test', ?1, 'src', '{}', unixepoch() - ?2, unixepoch())",
                    rusqlite::params![pid, offset],
                )
                .unwrap();
        }
        let sync_db = SyncDb::new(&db.conn);
        let due = sync_db.due_instances(1800).unwrap(); // 30-min interval
        assert_eq!(due.len(), 2);
        // proj-a was synced 2h ago, proj-b 1h ago — proj-a should come first
        assert_eq!(due[0].project_name, "proj-a");
        assert_eq!(due[1].project_name, "proj-b");
    }
}
