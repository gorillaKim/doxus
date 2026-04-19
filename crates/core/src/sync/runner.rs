use rusqlite::{Connection, OptionalExtension};

use doxus_plugin_sdk::{DocSource, FetchChangesOpts};

use crate::conflict::{record_conflict, resolve_conflict, ConflictResolution};
use crate::observability::{persist_audit, AuditEvent};
use super::db::SyncDb;
use super::scheduler::{SyncError, SyncScheduler};

#[derive(Debug)]
pub struct SyncResult {
    pub instance_id: i64,
    pub documents_updated: usize,
    pub new_cursor: Option<String>,
}

/// Runs one sync cycle: fetches due instances via `SyncScheduler`, calls
/// `fetch_changes` on the `DocSource`, then marks each instance synced.
pub struct SyncRunner<S: DocSource> {
    scheduler: SyncScheduler,
    source: S,
}

impl<S: DocSource + Send + Sync> SyncRunner<S> {
    pub fn new(scheduler: SyncScheduler, source: S) -> Self {
        Self { scheduler, source }
    }

    /// Run one sync cycle.
    /// For each due instance: fetch changes, mark synced, collect results.
    pub async fn run_once(&self, conn: &Connection) -> Result<Vec<SyncResult>, SyncError> {
        let sync_db = SyncDb::new(conn);
        let due = self.scheduler.due_instances(&sync_db)?;

        let mut results = Vec::new();

        for instance in due {
            persist_audit(conn, &AuditEvent::SyncStart { source_instance_id: instance.id });

            let opts = FetchChangesOpts {
                since: 0,
                cursor: instance.sync_cursor.clone(),
                page_size: 100,
                known_ids: vec![],
            };

            match self.source.fetch_changes(opts).await {
                Ok(changeset) => {
                    let new_cursor = changeset.next_cursor.clone();
                    let mut applied = 0usize;

                    for doc in &changeset.updated {
                        // Look up existing content_hash for this document.
                        let existing_hash: Option<String> = conn
                            .query_row(
                                "SELECT content_hash FROM documents \
                                 WHERE project_id = ?1 AND source_doc_id = ?2",
                                rusqlite::params![instance.project_id, doc.id.0],
                                |r| r.get(0),
                            )
                            .optional()
                            .map_err(|e| SyncError::Db(e))?;

                        match existing_hash {
                            Some(ref local_hash) => {
                                match resolve_conflict(local_hash, &doc.content) {
                                    ConflictResolution::Skip => {
                                        // Content unchanged — skip and continue
                                    }
                                    ConflictResolution::UseRemote => {
                                        // Record conflict before overwriting
                                        record_conflict(conn, instance.project_id, &doc.id.0)
                                            .map_err(|e| match e {
                                                crate::db::DbError::Sqlite(inner) => {
                                                    SyncError::Db(inner)
                                                }
                                                crate::db::DbError::Migration { reason, .. } => {
                                                    SyncError::Plugin(reason)
                                                }
                                            })?;
                                        applied += 1;
                                    }
                                }
                            }
                            None => {
                                // New document — no conflict
                                applied += 1;
                            }
                        }
                    }

                    sync_db
                        .mark_synced(instance.id, new_cursor.as_deref())
                        .map_err(SyncError::Db)?;
                    persist_audit(conn, &AuditEvent::SyncComplete {
                        source_instance_id: instance.id,
                        docs_synced: applied,
                    });
                    results.push(SyncResult {
                        instance_id: instance.id,
                        documents_updated: applied,
                        new_cursor,
                    });
                }
                Err(e) => {
                    persist_audit(conn, &AuditEvent::PluginError {
                        plugin_id: self.source.metadata().id.clone(),
                        message: e.to_string(),
                    });
                    return Err(SyncError::Plugin(e.to_string()));
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use async_trait::async_trait;
    use doxus_plugin_sdk::{
        Capabilities, ChangeSet, DocumentStream, FetchAllOpts, HealthStatus, PluginConfig,
        PluginError, PluginMetadata, PluginSecrets, RawDocument, SourceDocId,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ── Mock DocSource ─────────────────────────────────────────────────────────

    struct MockSource {
        call_count: Arc<AtomicUsize>,
        /// documents returned per fetch_changes call
        docs: Vec<RawDocument>,
        next_cursor: Option<String>,
        should_fail: bool,
    }

    impl MockSource {
        fn new(docs: Vec<RawDocument>, next_cursor: Option<String>) -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                docs,
                next_cursor,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                docs: vec![],
                next_cursor: None,
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl DocSource for MockSource {
        fn metadata(&self) -> &PluginMetadata {
            Box::leak(Box::new(PluginMetadata {
                id: "com.mock".into(),
                name: "Mock".into(),
                version: "0.0.1".into(),
                kind: doxus_plugin_sdk::PluginKind::Builtin,
            }))
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                incremental_sync: true,
                oauth: false,
                native_search: false,
            }
        }

        async fn validate_config(&self, _: &PluginConfig) -> Result<(), PluginError> {
            Ok(())
        }

        async fn initialize(&mut self, _: PluginConfig, _: PluginSecrets) -> Result<(), PluginError> {
            Ok(())
        }

        async fn fetch_all(&self, _: FetchAllOpts) -> Result<DocumentStream, PluginError> {
            Ok(DocumentStream {
                documents: vec![],
                next_cursor: None,
                estimated_total: None,
            })
        }

        async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(PluginError::NetworkError("mock failure".into()));
            }
            Ok(ChangeSet {
                updated: self.docs.clone(),
                deleted_ids: vec![],
                next_cursor: self.next_cursor.clone(),
            })
        }

        async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
            Err(PluginError::NotFound(id.0.clone()))
        }

        async fn health_check(&self) -> HealthStatus {
            HealthStatus { healthy: true, message: None }
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    fn insert_instance(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, installed_at)
             VALUES ('com.mock', 'Mock', '0.0.1', unixepoch())",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('runner-proj', 'Runner', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let pid: i64 = conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES ('com.mock', ?1, 'runner-src', '{}', unixepoch())",
            rusqlite::params![pid],
        )
        .unwrap();
        conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap()
    }

    fn make_doc(id: &str) -> RawDocument {
        use std::collections::HashMap;
        RawDocument {
            id: SourceDocId(id.into()),
            title: Some(id.into()),
            content: "body".into(),
            content_type: doxus_plugin_sdk::ContentType::Markdown,
            url: None,
            metadata: HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: None,
        }
    }

    // ── Tests ──────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_once_no_due_instances_returns_empty() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);
        // Mark as just synced
        SyncDb::new(&db.conn).mark_synced(id, None).unwrap();

        let runner = SyncRunner::new(SyncScheduler::new(3600), MockSource::new(vec![], None));
        let results = runner.run_once(&db.conn).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn run_once_syncs_due_instance_and_marks_synced() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);

        let source = MockSource::new(vec![make_doc("doc-1"), make_doc("doc-2")], None);
        let call_count = Arc::clone(&source.call_count);

        let runner = SyncRunner::new(SyncScheduler::new(3600), source);
        let results = runner.run_once(&db.conn).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].instance_id, id);
        assert_eq!(results[0].documents_updated, 2);
        assert_eq!(results[0].new_cursor, None);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Instance should now be marked synced (no longer due)
        let sync_db = SyncDb::new(&db.conn);
        let due = sync_db.due_instances(3600).unwrap();
        assert!(due.is_empty());
    }

    #[tokio::test]
    async fn run_once_persists_cursor_from_changeset() {
        let db = TestDb::new();
        let id = insert_instance(&db.conn);

        let source = MockSource::new(vec![make_doc("doc-x")], Some("cursor-abc".into()));
        let runner = SyncRunner::new(SyncScheduler::new(0), source);
        let results = runner.run_once(&db.conn).await.unwrap();

        assert_eq!(results[0].new_cursor, Some("cursor-abc".into()));

        let cursor = SyncDb::new(&db.conn).get_cursor(id).unwrap();
        assert_eq!(cursor, Some("cursor-abc".into()));
    }

    #[tokio::test]
    async fn run_once_propagates_plugin_error() {
        let db = TestDb::new();
        insert_instance(&db.conn);

        let runner = SyncRunner::new(SyncScheduler::new(3600), MockSource::failing());
        let err = runner.run_once(&db.conn).await.unwrap_err();
        assert!(matches!(err, SyncError::Plugin(_)));
    }

    #[tokio::test]
    async fn run_once_calls_fetch_changes_for_each_due_instance() {
        let db = TestDb::new();
        // Insert two separate instances (different projects to satisfy UNIQUE constraint)
        insert_instance(&db.conn);
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES ('runner-proj-2', 'Runner2', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid2: i64 = db
            .conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
                 VALUES ('com.mock', ?1, 'runner-src-2', '{}', unixepoch())",
                rusqlite::params![pid2],
            )
            .unwrap();

        let source = MockSource::new(vec![], None);
        let call_count = Arc::clone(&source.call_count);

        let runner = SyncRunner::new(SyncScheduler::new(3600), source);
        let results = runner.run_once(&db.conn).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_once_writes_sync_start_audit_log() {
        let db = TestDb::new();
        insert_instance(&db.conn);
        let source = MockSource::new(vec![], None);
        let runner = SyncRunner::new(SyncScheduler::new(3600), source);
        runner.run_once(&db.conn).await.unwrap();

        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM audit_log WHERE event_type = 'sync_start'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "sync_start should be recorded in audit_log");
    }

    #[tokio::test]
    async fn run_once_writes_sync_complete_audit_log() {
        let db = TestDb::new();
        insert_instance(&db.conn);
        let source = MockSource::new(vec![], None);
        let runner = SyncRunner::new(SyncScheduler::new(3600), source);
        runner.run_once(&db.conn).await.unwrap();

        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM audit_log WHERE event_type = 'sync_complete'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "sync_complete should be recorded in audit_log");
    }

    #[tokio::test]
    async fn run_once_writes_plugin_error_audit_log_on_failure() {
        let db = TestDb::new();
        insert_instance(&db.conn);
        let source = MockSource::failing();
        let runner = SyncRunner::new(SyncScheduler::new(3600), source);
        let _ = runner.run_once(&db.conn).await; // expected to fail

        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM audit_log WHERE event_type = 'plugin_error'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "plugin_error should be recorded in audit_log on sync failure");
    }
}
