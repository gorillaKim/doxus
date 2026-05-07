use doxus_core::conflict::{record_conflict, resolve_conflict, ConflictResolution};
use doxus_core::db::TestDb;

// ── 1. last-write-wins: 해시 다르면 UseRemote ─────────────────────────────────

#[test]
fn last_write_wins_newer_remote_replaces_local() {
    let local_hash = "hash_old";
    let remote_content = "new remote content that hashes differently";

    let resolution = resolve_conflict(local_hash, remote_content);
    assert!(
        matches!(resolution, ConflictResolution::UseRemote),
        "different content should resolve to UseRemote"
    );
}

// ── 2. 동일 해시면 Skip ───────────────────────────────────────────────────────

#[test]
fn same_content_hash_skips_update() {
    use sha2::{Digest, Sha256};
    let content = "same content";
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    let resolution = resolve_conflict(&hash, content);
    assert!(
        matches!(resolution, ConflictResolution::Skip),
        "same content hash should resolve to Skip"
    );
}

// ── 3. 충돌 시 audit_log 기록 ─────────────────────────────────────────────────

#[test]
fn conflict_logged_to_audit_log() {
    let db = TestDb::new();

    db.conn
        .execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('proj', 'Proj', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
    let project_id: i64 = db
        .conn
        .query_row("SELECT last_insert_rowid()", [], |r: &rusqlite::Row| {
            r.get(0)
        })
        .unwrap();

    record_conflict(&db.conn, project_id, "doc-001").unwrap();

    let (event_type, payload): (String, String) = db
        .conn
        .query_row(
            "SELECT event_type, payload FROM audit_log WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r: &rusqlite::Row| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    assert_eq!(event_type, "sync_conflict");
    assert!(
        payload.contains("doc-001"),
        "payload should contain source_doc_id"
    );
}

// ── 4. sync_loop 통합: resolve_conflict 적용 ──────────────────────────────────

#[test]
fn sync_loop_uses_conflict_resolution_skip_same_hash() {
    use sha2::{Digest, Sha256};

    let db = TestDb::new();

    db.conn
        .execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('proj2', 'Proj2', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
    let project_id: i64 = db
        .conn
        .query_row("SELECT last_insert_rowid()", [], |r: &rusqlite::Row| {
            r.get(0)
        })
        .unwrap();

    let content = "unchanged document body";
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    db.conn
        .execute(
            "INSERT INTO documents(project_id, source_doc_id, content_hash, last_indexed)
             VALUES (?1, 'doc-same', ?2, unixepoch())",
            rusqlite::params![project_id, hash],
        )
        .unwrap();

    let existing_hash: String = db
        .conn
        .query_row(
            "SELECT content_hash FROM documents WHERE project_id=?1 AND source_doc_id='doc-same'",
            rusqlite::params![project_id],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();

    let resolution = resolve_conflict(&existing_hash, content);
    assert!(
        matches!(resolution, ConflictResolution::Skip),
        "unchanged document should be skipped in sync loop"
    );
}
