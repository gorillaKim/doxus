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
         PRAGMA cache_size = -32000;",
    )
}

/// Run all migrations V1–V8 in order. Idempotent.
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
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

/// Create the vec0 virtual table for chunk embeddings (idempotent).
pub fn create_vec0_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings \
         USING vec0(chunk_id INTEGER PRIMARY KEY, embedding FLOAT[384]);",
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
        // Insert a fake embedding (384-dim, all 0.1)
        let embedding: Vec<f32> = vec![0.1f32; 384];
        let emb_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        db.conn
            .execute(
                "INSERT INTO chunk_embeddings(chunk_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![1i64, emb_bytes],
            )
            .expect("insert embedding");

        // KNN query
        let query_emb: Vec<f32> = vec![0.1f32; 384];
        let query_bytes: Vec<u8> = query_emb.iter().flat_map(|f| f.to_le_bytes()).collect();

        let (chunk_id, distance): (i64, f64) = db
            .conn
            .query_row(
                "SELECT chunk_id, distance FROM chunk_embeddings WHERE embedding MATCH ?1 ORDER BY distance LIMIT 1",
                rusqlite::params![query_bytes],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("KNN query should return result");

        assert_eq!(chunk_id, 1);
        assert!(distance < 0.001, "identical vectors should have near-zero distance");
    }
}
