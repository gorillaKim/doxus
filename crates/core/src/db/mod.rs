use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;
use std::sync::Once;
use thiserror::Error;

pub mod schema;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed (V{version}): {reason}")]
    Migration { version: u32, reason: String },
    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),
}

/// Apply SQLite PRAGMAs recommended for doxus.
pub fn apply_pragmas(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -16000;",
    )
}

pub type R2d2Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Clone, Debug)]
pub struct DbPool {
    read: R2d2Pool,
    write: R2d2Pool,
}

impl DbPool {
    pub fn read_conn(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, r2d2::Error> {
        self.read.get()
    }

    pub fn write_conn(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, r2d2::Error> {
        self.write.get()
    }

    pub fn get(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, r2d2::Error> {
        self.write.get()
    }
}

#[derive(Debug)]
pub struct DbWriteConnectionCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for DbWriteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        apply_pragmas(conn)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct DbReadConnectionCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for DbReadConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        // Read-only connections do not need journal_mode or synchronous settings,
        // and attempting to write them can fail or hang.
        // We only apply basic read pragmas.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -16000;",
        )?;
        Ok(())
    }
}

/// Create a connection pool for doxus database.
pub fn create_pool(path: &std::path::Path) -> Result<DbPool, DbError> {
    ensure_vec_extension();

    // 1. Initialize schema and run migrations on a single connection first.
    {
        let _conn = open(path)?;
    }

    // 2. Build the write pool.
    let write_manager = r2d2_sqlite::SqliteConnectionManager::file(path);
    let write_pool = r2d2::Pool::builder()
        .max_size(1)
        .connection_customizer(Box::new(DbWriteConnectionCustomizer))
        .build(write_manager)?;

    // 3. Build the read pool.
    let read_flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        | rusqlite::OpenFlags::SQLITE_OPEN_URI
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let read_manager = r2d2_sqlite::SqliteConnectionManager::file(path).with_flags(read_flags);
    let read_pool = r2d2::Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(DbReadConnectionCustomizer))
        .build(read_manager)?;

    Ok(DbPool {
        read: read_pool,
        write: write_pool,
    })
}

/// Run all migrations in order. Idempotent.
pub fn migrate(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  INTEGER NOT NULL
        );",
    )
    .map_err(DbError::Sqlite)?;

    let migrations: &[(&str, &str)] = MIGRATIONS;
    for (i, (_name, sql)) in migrations.iter().enumerate() {
        let version = (i + 1) as u32;
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM _migrations WHERE version = ?1",
                [version],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if already_applied {
            continue;
        }

        conn.execute_batch(sql).map_err(|e| DbError::Migration {
            version,
            reason: e.to_string(),
        })?;

        conn.execute(
            "INSERT INTO _migrations(version, applied_at) VALUES (?1, unixepoch())",
            [version],
        )
        .map_err(DbError::Sqlite)?;
    }

    Ok(())
}

/// Get a value from the system_config table. Returns None if key doesn't exist.
pub fn get_system_config(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    match conn.query_row(
        "SELECT value FROM system_config WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    ) {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(DbError::Sqlite(e)),
    }
}

/// Set a value in the system_config table (upsert).
pub fn set_system_config(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .map_err(DbError::Sqlite)?;
    Ok(())
}

static VEC_INIT: Once = Once::new();

/// Register the sqlite-vec extension process-wide (idempotent).
pub fn ensure_vec_extension() {
    VEC_INIT.call_once(|| unsafe {
        // SAFETY: sqlite3_vec_init is a valid C function pointer with the exact
        // signature expected by sqlite3_auto_extension. The transmute converts
        // from a typed fn pointer to the opaque fn() that sqlite3_auto_extension
        // requires.
        let init_fn = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init_fn));
    });
}

/// Create the vec0 virtual table for chunk embeddings (idempotent).
pub fn create_vec0_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings \
         USING vec0(chunk_id INTEGER PRIMARY KEY, vector int8[384]);",
    )
    .map_err(DbError::Sqlite)?;
    Ok(())
}

/// Open a DB connection, apply PRAGMAs, and run migrations.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    ensure_vec_extension();
    let conn = Connection::open(path).map_err(DbError::Sqlite)?;
    apply_pragmas(&conn).map_err(DbError::Sqlite)?;
    create_vec0_table(&conn)?;
    migrate(&conn)?;
    let _ = conn.execute_batch("PRAGMA shrink_memory;");
    Ok(conn)
}

/// Perform a manual WAL checkpoint (TRUNCATE) and shrink memory.
pub fn checkpoint_db(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA shrink_memory;")
        .map_err(DbError::Sqlite)
}

/// Open a DB connection in Read-Only mode without running migrations.
pub fn open_readonly(path: &Path) -> Result<Connection, DbError> {
    ensure_vec_extension();
    // SQLITE_OPEN_READONLY | SQLITE_OPEN_URI
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        | rusqlite::OpenFlags::SQLITE_OPEN_URI
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(DbError::Sqlite)?;
    apply_pragmas(&conn).map_err(DbError::Sqlite)?;
    Ok(conn)
}

/// Migration SQL tuples: (name, sql).
/// V4 (sqlite-vec) is skipped here — vec0 requires the extension loaded first.
static MIGRATIONS: &[(&str, &str)] = &[
    (
        "V1__initial_projects",
        include_str!("migrations/V1__initial_projects.sql"),
    ),
    (
        "V2__documents",
        include_str!("migrations/V2__documents.sql"),
    ),
    (
        "V3__chunks_fts",
        include_str!("migrations/V3__chunks_fts.sql"),
    ),
    (
        "V4__vec0_placeholder",
        "-- vec0 DDL applied separately via create_vec0_table()",
    ),
    ("V5__graph", include_str!("migrations/V5__graph.sql")),
    (
        "V6__view_counts",
        include_str!("migrations/V6__view_counts.sql"),
    ),
    ("V7__plugins", include_str!("migrations/V7__plugins.sql")),
    (
        "V8__workspace",
        include_str!("migrations/V8__workspace.sql"),
    ),
    (
        "V9__workspace_content",
        include_str!("migrations/V9__workspace_content.sql"),
    ),
    (
        "V10__plugin_kv",
        include_str!("migrations/V10__plugin_kv.sql"),
    ),
    (
        "V11__project_source",
        include_str!("migrations/V11__project_source.sql"),
    ),
    (
        "V12__content_cache",
        include_str!("migrations/V12__content_cache.sql"),
    ),
    (
        "V13__document_meta",
        include_str!("migrations/V13__document_meta.sql"),
    ),
    (
        "V14__workspace_unification",
        include_str!("migrations/V14__workspace_unification.sql"),
    ),
    (
        "V15__drop_legacy_workspace",
        include_str!("migrations/V15__drop_legacy_workspace_tables.sql"),
    ),
    (
        "V16__expand_doc_type",
        include_str!("migrations/V16__expand_doc_type.sql"),
    ),
    (
        "V17__content_cache_data",
        include_str!("migrations/V17__content_cache_data.sql"),
    ),
    (
        "V18__remove_default_workspace",
        include_str!("migrations/V18__remove_default_workspace.sql"),
    ),
    (
        "V19__add_url_to_documents",
        include_str!("migrations/V19__add_url_to_documents.sql"),
    ),
    (
        "V20__clear_document_content",
        include_str!("migrations/V20__clear_document_content.sql"),
    ),
    (
        "V21__add_cascade_triggers",
        include_str!("migrations/V21__add_cascade_triggers.sql"),
    ),
    (
        "V22__placeholder",
        include_str!("migrations/V22__placeholder.sql"),
    ),
    (
        "V23__remove_document_content_column",
        include_str!("migrations/V23__remove_document_content_column.sql"),
    ),
    (
        "V24__vector_int8_schema",
        include_str!("migrations/V24__vector_int8_schema.sql"),
    ),
    (
        "V25__force_reindex_after_quantization",
        include_str!("migrations/V25__force_reindex_after_quantization.sql"),
    ),
    (
        "V26__force_resync",
        include_str!("migrations/V26__force_resync.sql"),
    ),
    (
        "V27__hybrid_storage_schema",
        include_str!("migrations/V27__hybrid_storage_schema.sql"),
    ),
    (
        "V28__hybrid_storage_repair",
        include_str!("migrations/V28__hybrid_storage_repair.sql"),
    ),
    (
        "V29__add_source_project_id",
        include_str!("migrations/V29__add_source_project_id.sql"),
    ),
    (
        "V30__add_sync_config",
        include_str!("migrations/V30__add_sync_config.sql"),
    ),
    (
        "V31__add_title_to_fts",
        include_str!("migrations/V31__add_title_to_fts.sql"),
    ),
    (
        "V32__date_indexes",
        include_str!("migrations/V32__date_indexes.sql"),
    ),
    (
        "V33__reindex_history",
        include_str!("migrations/V33__reindex_history.sql"),
    ),
    (
        "V34__scheduler",
        include_str!("migrations/V34__scheduler.sql"),
    ),
    (
        "V35__document_freshness",
        include_str!("migrations/V35__document_freshness.sql"),
    ),
    (
        "V36__freshness_config",
        include_str!("migrations/V36__freshness_config.sql"),
    ),
    (
        "V37__link_optimization",
        include_str!("migrations/V37__link_optimization.sql"),
    ),
    (
        "V38__scheduler_details",
        include_str!("migrations/V38__scheduler_details.sql"),
    ),
    (
        "V39__add_last_fetched_at",
        include_str!("migrations/V39__add_last_fetched_at.sql"),
    ),
    (
        "V40__add_system_config",
        include_str!("migrations/V40__add_system_config.sql"),
    ),
    (
        "V41__add_summary_column",
        include_str!("migrations/V41__add_summary_column.sql"),
    ),
    (
        "V42__feedback_schema",
        include_str!("migrations/V42__feedback_schema.sql"),
    ),
    (
        "V43__co_refs_schema",
        include_str!("migrations/V43__co_refs_schema.sql"),
    ),
];

// ── Test helper ──────────────────────────────────────────────────────────────

/// In-memory SQLite DB with all migrations applied. Used in tests.
#[cfg(any(test, feature = "test-helpers"))]
pub struct TestDb {
    pub conn: Connection,
}

#[cfg(any(test, feature = "test-helpers"))]
impl Default for TestDb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-helpers"))]
impl TestDb {
    pub fn new() -> Self {
        ensure_vec_extension();
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_pragmas(&conn).expect("pragmas");
        create_vec0_table(&conn).expect("vec0 table");
        // Apply all migrations
        let migrations: &[(&str, &str)] = MIGRATIONS;
        for (i, (_name, sql)) in migrations.iter().enumerate() {
            let version = (i + 1) as u32;
            conn.execute_batch(sql).unwrap_or_else(|e| {
                panic!("migration V{version} failed: {e}");
            });
        }
        Self { conn }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_creates_tables() {
        let db = TestDb::new();
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // projects, documents, chunks, chunks_fts (virtual), document_aliases,
        // document_links, document_tags, document_metadata, view_counts, audit_log,
        // plugins, source_instances, registry_cache, plugin_audit_log, plugin_logs,
        // workspace_templates, workspace_documents, _migrations
        assert!(count >= 10, "expected at least 10 tables, got {count}");
    }

    #[test]
    fn test_project_insert_and_query() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES ('test-vault', 'Test Vault', '/tmp/vault', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();

        let name: String = db
            .conn
            .query_row(
                "SELECT name FROM projects WHERE display_name = 'Test Vault'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "test-vault");
    }

    #[test]
    fn test_project_status_constraint() {
        let db = TestDb::new();
        let result = db.conn.execute(
            "INSERT INTO projects(name, display_name, path, status, created_at, updated_at)
             VALUES ('p', 'P', '/p', 'invalid', unixepoch(), unixepoch())",
            [],
        );
        assert!(result.is_err(), "invalid status should be rejected");
    }

    #[test]
    fn test_foreign_key_cascade() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES ('proj', 'Proj', '/p', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='proj'", [], |r| {
                r.get(0)
            })
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO documents(project_id, source_doc_id, content_hash)
                 VALUES (?1, 'doc1', 'abc')",
                [pid],
            )
            .unwrap();
        db.conn
            .execute("DELETE FROM projects WHERE id=?1", [pid])
            .unwrap();
        let doc_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(doc_count, 0, "documents should cascade delete");
    }

    #[test]
    fn test_idempotent_migration() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        create_vec0_table(&conn).unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }

    #[test]
    fn vec0_table_exists_after_open() {
        let db = TestDb::new();
        let name: String = db
            .conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='chunk_embeddings'",
                [],
                |r| r.get(0),
            )
            .expect("chunk_embeddings table should exist");
        assert_eq!(name, "chunk_embeddings");
    }

    #[test]
    fn can_insert_and_query_embedding() {
        let db = TestDb::new();
        // Insert a fake int8 embedding (384-dim, all 64)
        let embedding: Vec<i8> = vec![64i8; 384];
        let emb_bytes: Vec<u8> = embedding.iter().map(|&i| i as u8).collect();

        db.conn
            .execute(
                "INSERT INTO chunk_embeddings(chunk_id, vector) VALUES (?1, vec_int8(?2))",
                rusqlite::params![1i64, emb_bytes],
            )
            .expect("insert embedding");

        // KNN query
        let query_bytes = emb_bytes.clone();

        let (chunk_id, distance): (i64, f64) = db
            .conn
            .query_row(
                "SELECT chunk_id, distance FROM chunk_embeddings WHERE vector MATCH vec_int8(?1) ORDER BY distance LIMIT 1",
                rusqlite::params![query_bytes],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("KNN query should return result");

        assert_eq!(chunk_id, 1);
        assert!(
            distance < 0.001,
            "identical vectors should have near-zero distance"
        );
    }

    // ── V14 워크스페이스 통합 마이그레이션 테스트 ────────────────────────────

    #[test]
    fn v14_templates_table_exists() {
        let db = TestDb::new();
        let exists: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='templates'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "templates 테이블이 존재해야 함");
    }

    #[test]
    fn v14_projects_has_is_default_column() {
        let db = TestDb::new();
        // is_default 컬럼이 있으면 INSERT가 성공해야 함
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, is_default, created_at, updated_at)
             VALUES ('ws-test', '테스트 워크스페이스', '/tmp/ws', 'workspace', 0, unixepoch(), unixepoch())",
            [],
        ).expect("is_default 컬럼이 projects에 존재해야 함");
    }

    // (V18에서 유니크 인덱스가 제거되었으므로 더 이상 중복 체크하지 않음)

    #[test]
    fn v14_templates_supports_global_and_project_scoped() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, created_at, updated_at)
             VALUES ('proj-a', 'Proj A', '/tmp/a', 'obsidian', unixepoch(), unixepoch())",
            [],
        ).unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='proj-a'", [], |r| {
                r.get(0)
            })
            .unwrap();

        // 전역 템플릿 (project_id NULL)
        db.conn.execute(
            "INSERT INTO templates(name, doc_type, content, created_at) VALUES ('전역 메모', 'note', '# 메모', unixepoch())",
            [],
        ).expect("전역 템플릿 허용");

        // 프로젝트 전용 템플릿
        db.conn
            .execute(
                "INSERT INTO templates(project_id, name, doc_type, content, created_at)
             VALUES (?1, '프로젝트 템플릿', 'meeting', '# 회의록', unixepoch())",
                [pid],
            )
            .expect("프로젝트 전용 템플릿 허용");

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn v14_templates_cascade_delete_with_project() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, created_at, updated_at)
             VALUES ('proj-del', 'Del Proj', '/tmp/del', 'workspace', unixepoch(), unixepoch())",
            [],
        ).unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='proj-del'", [], |r| {
                r.get(0)
            })
            .unwrap();

        db.conn
            .execute(
                "INSERT INTO templates(project_id, name, doc_type, content, created_at)
             VALUES (?1, '삭제될 템플릿', 'note', '', unixepoch())",
                [pid],
            )
            .unwrap();

        db.conn
            .execute("DELETE FROM projects WHERE id=?1", [pid])
            .unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM templates WHERE project_id=?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "프로젝트 삭제 시 템플릿도 CASCADE 삭제되어야 함");
    }

    // ── V40: system_config 테이블 ────────────────────────────────────────────

    #[test]
    fn v40_system_config_table_exists() {
        let db = TestDb::new();
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='system_config'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "system_config 테이블이 존재해야 함");
    }

    #[test]
    fn v42_document_feedbacks_table_exists() {
        let db = TestDb::new();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_feedbacks'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1, "document_feedbacks 테이블이 존재해야 함");
    }

    #[test]
    fn v40_system_config_bootstrap_default() {
        let db = TestDb::new();
        let val: String = db
            .conn
            .query_row(
                "SELECT value FROM system_config WHERE key='last_run_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(val, "0.0.0", "부트스트랩: last_run_version 초기값은 0.0.0");
    }

    #[test]
    fn test_get_system_config_returns_bootstrap_value() {
        let db = TestDb::new();
        let val = get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(val, Some("0.0.0".to_string()));
    }

    #[test]
    fn test_set_and_get_system_config() {
        let db = TestDb::new();
        set_system_config(&db.conn, "last_run_version", "0.2.0").unwrap();
        let val = get_system_config(&db.conn, "last_run_version").unwrap();
        assert_eq!(val, Some("0.2.0".to_string()));
    }

    #[test]
    fn test_get_system_config_missing_key() {
        let db = TestDb::new();
        let val = get_system_config(&db.conn, "nonexistent_key").unwrap();
        assert_eq!(val, None, "존재하지 않는 키는 None 반환");
    }

    #[test]
    fn v14_workspace_documents_no_longer_exist_after_v15() {
        let db = TestDb::new();
        // V15에서 workspace_documents, workspaces, workspace_templates DROP
        let old_tables: Vec<String> = {
            let mut stmt = db.conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('workspaces','workspace_documents','workspace_templates')"
            ).unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert!(
            old_tables.is_empty(),
            "V15 이후 구 워크스페이스 테이블이 없어야 함: {:?}",
            old_tables
        );
    }

    #[test]
    fn test_document_url_persistence() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('test-proj', 'Test', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let project_id: i64 = db.conn.last_insert_rowid();

        let test_url = "obsidian://open?path=/tmp/test.md";

        // This should succeed
        let res = db.conn.execute(
            "INSERT INTO documents(project_id, source_doc_id, title, content_hash, url)
             VALUES (?1, 'doc1', 'Doc 1', 'abc', ?2)",
            rusqlite::params![project_id, test_url],
        );

        assert!(
            res.is_ok(),
            "Failed to insert document with url: {:?}",
            res.err()
        );
    }

    #[test]
    fn v30_projects_has_sync_policy_json() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('policy-test', 'Policy Test', '/tmp', '{\"type\":\"manual\"}', unixepoch(), unixepoch())",
            [],
        ).expect("sync_policy_json column should exist in projects table");

        let policy: String = db
            .conn
            .query_row(
                "SELECT sync_policy_json FROM projects WHERE name='policy-test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(policy, "{\"type\":\"manual\"}");
    }

    // ── V32: 날짜 인덱스 ──────────────────────────────────────────────────

    #[test]
    fn v32_created_at_index_exists() {
        let db = TestDb::new();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_documents_created_at'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1, "idx_documents_created_at 인덱스가 존재해야 함");
    }

    #[test]
    fn v32_updated_at_index_exists() {
        let db = TestDb::new();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_documents_updated_at'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1, "idx_documents_updated_at 인덱스가 존재해야 함");
    }

    // ── V33: reindex_history 테이블 ────────────────────────────────────────

    #[test]
    fn v33_reindex_history_table_exists() {
        let db = TestDb::new();
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='reindex_history'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "reindex_history 테이블이 존재해야 함");
    }

    #[test]
    fn v33_reindex_history_accepts_basic_insert() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('rh-test', 'RH Test', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='rh-test'", [], |r| {
                r.get(0)
            })
            .unwrap();

        db.conn.execute(
            "INSERT INTO reindex_history(project_id, scope, started_at) VALUES (?1, 'all', unixepoch())",
            [pid],
        ).expect("reindex_history 기본 INSERT 성공해야 함");

        let status: String = db
            .conn
            .query_row(
                "SELECT status FROM reindex_history WHERE project_id=?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending", "status 기본값은 'pending'");
    }

    #[test]
    fn v33_reindex_history_cascade_delete() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('rh-cascade', 'RH Cascade', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='rh-cascade'", [], |r| {
                r.get(0)
            })
            .unwrap();

        db.conn.execute(
            "INSERT INTO reindex_history(project_id, scope, started_at) VALUES (?1, 'all', unixepoch())",
            [pid],
        ).unwrap();

        db.conn
            .execute("DELETE FROM projects WHERE id=?1", [pid])
            .unwrap();
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM reindex_history WHERE project_id=?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "프로젝트 삭제 시 reindex_history도 CASCADE 삭제");
    }

    // ── V34: 스케줄러 매니저 ───────────────────────────────────────────────

    #[test]
    fn v34_scheduler_tables_exist() {
        let db = TestDb::new();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('scheduled_jobs', 'job_runs')",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 2, "scheduled_jobs, job_runs 테이블이 존재해야 함");
    }

    #[test]
    fn v34_scheduled_jobs_cascade_delete() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('sch-cascade', 'Sch Cascade', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db
            .conn
            .query_row(
                "SELECT id FROM projects WHERE name='sch-cascade'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        db.conn.execute(
            "INSERT INTO scheduled_jobs(project_id, job_name, executor, action, schedule_json, next_run_at, created_at)
             VALUES (?1, 'test job', 'system', 'test_action', '{}', unixepoch(), unixepoch())",
            [pid],
        ).unwrap();
        let job_id: i64 = db.conn.last_insert_rowid();

        db.conn
            .execute(
                "INSERT INTO job_runs(job_id, started_at) VALUES (?1, unixepoch())",
                [job_id],
            )
            .unwrap();

        // CASCADE DELETE TEST
        db.conn
            .execute("DELETE FROM projects WHERE id=?1", [pid])
            .unwrap();

        let job_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM scheduled_jobs WHERE project_id=?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        let run_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM job_runs WHERE job_id=?1",
                [job_id],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(job_count, 0, "프로젝트 삭제 시 scheduled_jobs CASCADE 삭제");
        assert_eq!(run_count, 0, "scheduled_jobs 삭제 시 job_runs CASCADE 삭제");
    }

    // ── V35, V36: 문서 신선도 관리 (Document Freshness) ────────────────

    #[test]
    fn v35_v36_freshness_tables_exist() {
        let db = TestDb::new();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('document_freshness', 'document_change_log')",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(
            count, 2,
            "document_freshness, document_change_log 테이블이 존재해야 함"
        );
    }

    #[test]
    fn v36_projects_has_freshness_policy_json() {
        let db = TestDb::new();
        // Insert dummy project to verify column existence
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, freshness_policy_json, created_at, updated_at)
             VALUES ('fresh-test', 'Fresh Test', '/tmp', '{}', unixepoch(), unixepoch())",
            [],
        ).expect("freshness_policy_json column should exist in projects table");
    }

    #[test]
    fn v35_freshness_trigger_works() {
        let db = TestDb::new();
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('trigger-test', 'Trigger Test', '/tmp', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db.conn.last_insert_rowid();

        db.conn.execute(
            "INSERT INTO documents(project_id, source_doc_id, content_hash) VALUES (?1, 'doc1', 'hash_v1')",
            [pid],
        ).unwrap();
        let doc_id: i64 = db.conn.last_insert_rowid();

        // 1. Initial freshness record manually
        db.conn.execute(
            "INSERT INTO document_freshness(document_id, freshness_score, status, retention_tier, first_seen_at, score_updated_at)
             VALUES (?1, 50.0, 'aging', 'short', unixepoch(), unixepoch())",
            [doc_id],
        ).unwrap();

        // 2. Trigger content_hash update
        db.conn
            .execute(
                "UPDATE documents SET content_hash = 'hash_v2' WHERE id = ?1",
                [doc_id],
            )
            .unwrap();

        // 3. Verify trigger resets freshness and updates log
        let score: f64 = db
            .conn
            .query_row(
                "SELECT freshness_score FROM document_freshness WHERE document_id = ?1",
                [doc_id],
                |r| r.get(0),
            )
            .unwrap();
        let status: String = db
            .conn
            .query_row(
                "SELECT status FROM document_freshness WHERE document_id = ?1",
                [doc_id],
                |r| r.get(0),
            )
            .unwrap();
        let changes: i64 = db
            .conn
            .query_row(
                "SELECT change_count FROM document_freshness WHERE document_id = ?1",
                [doc_id],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(score, 100.0, "Score should reset to 100.0");
        assert_eq!(status, "fresh", "Status should reset to 'fresh'");
        assert_eq!(changes, 1, "Change count should increment");

        let log_count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM document_change_log WHERE document_id = ?1 AND old_hash = 'hash_v1' AND new_hash = 'hash_v2'",
            [doc_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(log_count, 1, "Log table should record the hash change");
    }

    #[test]
    fn test_checkpoint_db_truncates_wal() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_checkpoint.db");
        let wal_path = dir.path().join("test_checkpoint.db-wal");

        // 1. Create conn1, insert data and keep it open to ensure WAL file is kept
        let conn1 = open(&db_path).unwrap();
        conn1.execute("CREATE TABLE foo (bar TEXT);", []).unwrap();
        conn1.execute("BEGIN TRANSACTION;", []).unwrap();
        for i in 0..100 {
            conn1
                .execute("INSERT INTO foo VALUES (?1);", [format!("data-{}", i)])
                .unwrap();
        }
        conn1.execute("COMMIT;", []).unwrap();

        assert!(
            wal_path.exists(),
            "WAL file should exist while connection is alive"
        );
        let initial_size = std::fs::metadata(&wal_path).unwrap().len();
        assert!(initial_size > 0, "WAL file should have some content");

        // 2. Open DB again (conn2). Under current code this truncates the WAL.
        // We assert that it does NOT truncate on open (this will fail under current implementation).
        let conn2 = open(&db_path).unwrap();
        let opened_size = std::fs::metadata(&wal_path).unwrap().len();
        assert_eq!(
            opened_size, initial_size,
            "WAL size should not be truncated on db::open"
        );

        // 3. Close conn1 and perform manual checkpoint. This should truncate the WAL to 0.
        drop(conn1);
        checkpoint_db(&conn2).unwrap();
        let checkpointed_size = std::fs::metadata(&wal_path).unwrap().len();
        assert_eq!(
            checkpointed_size, 0,
            "WAL size should be 0 after explicit checkpoint_db(TRUNCATE)"
        );
    }
}
