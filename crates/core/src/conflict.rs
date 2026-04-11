use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::DbError;

/// Resolution decision for a document conflict.
#[derive(Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Remote content differs — overwrite local with remote.
    UseRemote,
    /// Content is identical — skip the update.
    Skip,
}

/// last-indexed-wins conflict resolution.
///
/// Computes SHA-256 of `remote_content` and compares with `local_hash`.
/// Returns `Skip` when hashes match (no change), `UseRemote` otherwise.
///
/// NOTE: "last-indexed-wins" means the decision is purely content-hash based —
/// we do not compare timestamps. The caller is responsible for only invoking
/// this after the remote document has been fetched (i.e., remote is the
/// most-recently-seen version).
pub fn resolve_conflict(local_hash: &str, remote_content: &str) -> ConflictResolution {
    let remote_hash = format!("{:x}", Sha256::digest(remote_content.as_bytes()));
    if local_hash == remote_hash {
        ConflictResolution::Skip
    } else {
        ConflictResolution::UseRemote
    }
}

/// Record a `sync_conflict` event in `audit_log` for the given project and document.
pub fn record_conflict(
    conn: &Connection,
    project_id: i64,
    source_doc_id: &str,
) -> Result<(), DbError> {
    let payload = serde_json::json!({"source_doc_id": source_doc_id}).to_string();
    conn.execute(
        "INSERT INTO audit_log(project_id, event_type, payload, occurred_at)
         VALUES (?1, 'sync_conflict', ?2, unixepoch())",
        rusqlite::params![project_id, payload],
    )
    .map_err(DbError::Sqlite)?;
    Ok(())
}
