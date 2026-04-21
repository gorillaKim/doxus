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
}

/// Apply SQLite PRAGMAs recommended for doxus.
pub fn apply_pragmas(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -32000;",
    )
}

/// Run all migrations V1–V12 in order. Idempotent.
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

static VEC_INIT: Once = Once::new();

/// Register the sqlite-vec extension process-wide (idempotent).
pub fn ensure_vec_extension() {
    VEC_INIT.call_once(|| unsafe {
        // SAFETY: sqlite3_vec_init is a valid C function pointer with the exact
        // signature expected by sqlite3_auto_extension. The transmute converts
        // from a typed fn pointer to the opaque fn() that sqlite3_auto_extension
        // requires.
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
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
    Ok(conn)
}

/// Migration SQL tuples: (name, sql).
/// V4 (sqlite-vec) is skipped here — vec0 requires the extension loaded first.
static MIGRATIONS: &[(&str, &str)] = &[
    ("V1__initial_projects",   include_str!("migrations/V1__initial_projects.sql")),
    ("V2__documents",          include_str!("migrations/V2__documents.sql")),
    ("V3__chunks_fts",         include_str!("migrations/V3__chunks_fts.sql")),
    ("V4__vec0_placeholder",   "-- vec0 DDL applied separately via create_vec0_table()"),
    ("V5__graph",              include_str!("migrations/V5__graph.sql")),
    ("V6__view_counts",        include_str!("migrations/V6__view_counts.sql")),
    ("V7__plugins",            include_str!("migrations/V7__plugins.sql")),
    ("V8__workspace",          include_str!("migrations/V8__workspace.sql")),
    ("V9__workspace_content",  include_str!("migrations/V9__workspace_content.sql")),
    ("V10__plugin_kv",         include_str!("migrations/V10__plugin_kv.sql")),
    ("V11__project_source",    include_str!("migrations/V11__project_source.sql")),
    ("V12__content_cache",     include_str!("migrations/V12__content_cache.sql")),
    ("V13__document_meta",          include_str!("migrations/V13__document_meta.sql")),
    ("V14__workspace_unification",  include_str!("migrations/V14__workspace_unification.sql")),
    ("V15__drop_legacy_workspace",  include_str!("migrations/V15__drop_legacy_workspace_tables.sql")),
    ("V16__expand_doc_type",        include_str!("migrations/V16__expand_doc_type.sql")),
    ("V17__content_cache_data",     include_str!("migrations/V17__content_cache_data.sql")),
    ("V18__remove_default_workspace", include_str!("migrations/V18__remove_default_workspace.sql")),
    ("V19__add_url_to_documents",      include_str!("migrations/V19__add_url_to_documents.sql")),
    ("V20__clear_document_content",    include_str!("migrations/V20__clear_document_content.sql")),
    ("V21__add_cascade_triggers",      include_str!("migrations/V21__add_cascade_triggers.sql")),
    ("V23__remove_document_content_column", include_str!("migrations/V23__remove_document_content_column.sql")),
    ("V24__vector_int8_schema", include_str!("migrations/V24__vector_int8_schema.sql")),
    ("V25__force_reindex_after_quantization", include_str!("migrations/V25__force_reindex_after_quantization.sql")),
    ("V26__force_resync", include_str!("migrations/V26__force_resync.sql")),
    ("V27__hybrid_storage_schema", include_str!("migrations/V27__hybrid_storage_schema.sql")),
    ("V28__hybrid_storage_repair", include_str!("migrations/V28__hybrid_storage_repair.sql")),
    ("V29__add_source_project_id", include_str!("migrations/V29__add_source_project_id.sql")),
    ("V30__add_sync_config", include_str!("migrations/V30__add_sync_config.sql")),
];

// ── Test helper ──────────────────────────────────────────────────────────────

/// In-memory SQLite DB with all migrations applied. Used in tests.
#[cfg(any(test, feature = "test-helpers"))]
pub struct TestDb {
    pub conn: Connection,
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
            .query_row("SELECT id FROM projects WHERE name='proj'", [], |r| r.get(0))
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO documents(project_id, source_doc_id, content, content_hash)
                 VALUES (?1, 'doc1', 'hello', 'abc')",
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
        // Run migrate twice — should not fail
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
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
        assert!(distance < 0.001, "identical vectors should have near-zero distance");
    }

    // ── V14 워크스페이스 통합 마이그레이션 테스트 ────────────────────────────

    #[test]
    fn v14_templates_table_exists() {
        let db = TestDb::new();
        let exists: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='templates'",
            [], |r| r.get(0),
        ).unwrap();
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

    #[test]
    fn v14_only_one_default_workspace_allowed() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, is_default, created_at, updated_at)
             VALUES ('ws-a', 'WS A', '/tmp/a', 'workspace', 1, unixepoch(), unixepoch())",
            [],
        ).expect("첫 번째 is_default=1 허용");

        let result = db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, is_default, created_at, updated_at)
             VALUES ('ws-b', 'WS B', '/tmp/b', 'workspace', 1, unixepoch(), unixepoch())",
            [],
        );
        assert!(result.is_err(), "두 번째 is_default=1은 UNIQUE INDEX 위반이어야 함");
    }

    #[test]
    fn v14_templates_supports_global_and_project_scoped() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, source_type, created_at, updated_at)
             VALUES ('proj-a', 'Proj A', '/tmp/a', 'obsidian', unixepoch(), unixepoch())",
            [],
        ).unwrap();
        let pid: i64 = db.conn.query_row(
            "SELECT id FROM projects WHERE name='proj-a'", [], |r| r.get(0),
        ).unwrap();

        // 전역 템플릿 (project_id NULL)
        db.conn.execute(
            "INSERT INTO templates(name, doc_type, content, created_at) VALUES ('전역 메모', 'note', '# 메모', unixepoch())",
            [],
        ).expect("전역 템플릿 허용");

        // 프로젝트 전용 템플릿
        db.conn.execute(
            "INSERT INTO templates(project_id, name, doc_type, content, created_at)
             VALUES (?1, '프로젝트 템플릿', 'meeting', '# 회의록', unixepoch())",
            [pid],
        ).expect("프로젝트 전용 템플릿 허용");

        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM templates", [], |r| r.get(0),
        ).unwrap();
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
        let pid: i64 = db.conn.query_row(
            "SELECT id FROM projects WHERE name='proj-del'", [], |r| r.get(0),
        ).unwrap();

        db.conn.execute(
            "INSERT INTO templates(project_id, name, doc_type, content, created_at)
             VALUES (?1, '삭제될 템플릿', 'note', '', unixepoch())",
            [pid],
        ).unwrap();

        db.conn.execute("DELETE FROM projects WHERE id=?1", [pid]).unwrap();

        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM templates WHERE project_id=?1", [pid], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0, "프로젝트 삭제 시 템플릿도 CASCADE 삭제되어야 함");
    }

    #[test]
    fn v14_workspace_documents_no_longer_exist_after_v15() {
        let db = TestDb::new();
        // V15에서 workspace_documents, workspaces, workspace_templates DROP
        let old_tables: Vec<String> = {
            let mut stmt = db.conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('workspaces','workspace_documents','workspace_templates')"
            ).unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert!(old_tables.is_empty(), "V15 이후 구 워크스페이스 테이블이 없어야 함: {:?}", old_tables);
    }

    #[test]
    fn test_document_url_persistence() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('test-proj', 'Test', '/tmp', unixepoch(), unixepoch())",
            []
        ).unwrap();
        let project_id: i64 = db.conn.last_insert_rowid();

        let test_url = "obsidian://open?path=/tmp/test.md";
        
        // This is expected to fail initially (TDD)
        let res = db.conn.execute(
            "INSERT INTO documents(project_id, source_doc_id, title, content, content_hash, url)
             VALUES (?1, 'doc1', 'Doc 1', 'hello', 'abc', ?2)",
            rusqlite::params![project_id, test_url]
        );

        assert!(res.is_ok(), "Failed to insert document with url: {:?}", res.err());
    }

    #[test]
    fn v30_projects_has_sync_policy_json() {
        let db = TestDb::new();
        db.conn.execute(
            "INSERT INTO projects(name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('policy-test', 'Policy Test', '/tmp', '{\"type\":\"manual\"}', unixepoch(), unixepoch())",
            [],
        ).expect("sync_policy_json column should exist in projects table");
        
        let policy: String = db.conn.query_row(
            "SELECT sync_policy_json FROM projects WHERE name='policy-test'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(policy, "{\"type\":\"manual\"}");
    }
}
