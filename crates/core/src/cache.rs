use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("ttl_minutes must be >= 10, got {0}")]
    TtlTooShort(u32),
    #[error("system clock error: {0}")]
    Clock(String),
}

pub struct ContentCache<'a> {
    conn: &'a Connection,
}

impl<'a> ContentCache<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Returns cached content for (plugin_id, doc_id) if present and not expired.
    /// Does NOT update expires_at on hit — use `touch` for TTL reset.
    pub fn get(&self, plugin_id: &str, doc_id: &str) -> Result<Option<String>, CacheError> {
        let now = now_secs()?;
        let result = self.conn.query_row(
            "SELECT content FROM content_cache
             WHERE plugin_id = ?1 AND doc_id = ?2 AND expires_at > ?3",
            params![plugin_id, doc_id, now],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(content) => Ok(Some(content)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CacheError::Sqlite(e)),
        }
    }

    /// Returns the full cached document object if present and not expired.
    pub fn get_full(&self, plugin_id: &str, doc_id: &str) -> Result<Option<String>, CacheError> {
        let now = now_secs()?;
        let result = self.conn.query_row(
            "SELECT data_json FROM content_cache
             WHERE plugin_id = ?1 AND doc_id = ?2 AND expires_at > ?3",
            params![plugin_id, doc_id, now],
            |row| row.get::<_, Option<String>>(0),
        );
        match result {
            Ok(Some(data)) => Ok(Some(data)),
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CacheError::Sqlite(e)),
        }
    }

    /// Insert or replace cache entry. Returns `CacheError::TtlTooShort` if `ttl_minutes < 10`.
    pub fn set(
        &self,
        plugin_id: &str,
        doc_id: &str,
        content: &str,
        ttl_minutes: u32,
    ) -> Result<(), CacheError> {
        if ttl_minutes < 10 {
            return Err(CacheError::TtlTooShort(ttl_minutes));
        }
        let now = now_secs()?;
        let expires_at = now + (ttl_minutes as i64) * 60;
        self.conn.execute(
            "INSERT INTO content_cache(plugin_id, doc_id, content, cached_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(plugin_id, doc_id) DO UPDATE SET
               content    = excluded.content,
               cached_at  = excluded.cached_at,
               expires_at = excluded.expires_at",
            params![plugin_id, doc_id, content, now, expires_at],
        )?;
        Ok(())
    }

    /// Insert or replace full document cache entry.
    pub fn set_full(
        &self,
        plugin_id: &str,
        doc_id: &str,
        content: &str,
        data_json: &str,
        ttl_minutes: u32,
    ) -> Result<(), CacheError> {
        if ttl_minutes < 10 {
            return Err(CacheError::TtlTooShort(ttl_minutes));
        }
        let now = now_secs()?;
        let expires_at = now + (ttl_minutes as i64) * 60;
        self.conn.execute(
            "INSERT INTO content_cache(plugin_id, doc_id, content, data_json, cached_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(plugin_id, doc_id) DO UPDATE SET
               content    = excluded.content,
               data_json  = excluded.data_json,
               cached_at  = excluded.cached_at,
               expires_at = excluded.expires_at",
            params![plugin_id, doc_id, content, data_json, now, expires_at],
        )?;
        Ok(())
    }

    /// Reset TTL for an existing cache entry (re-access within TTL window).
    pub fn touch(
        &self,
        plugin_id: &str,
        doc_id: &str,
        ttl_minutes: u32,
    ) -> Result<bool, CacheError> {
        let now = now_secs()?;
        let new_expires_at = now + (ttl_minutes as i64) * 60;
        let affected = self.conn.execute(
            "UPDATE content_cache SET expires_at = ?1
             WHERE plugin_id = ?2 AND doc_id = ?3 AND expires_at > ?4",
            params![new_expires_at, plugin_id, doc_id, now],
        )?;
        Ok(affected > 0)
    }

    /// Remove a specific cache entry (used by force_refresh).
    pub fn invalidate(&self, plugin_id: &str, doc_id: &str) -> Result<(), CacheError> {
        self.conn.execute(
            "DELETE FROM content_cache WHERE plugin_id = ?1 AND doc_id = ?2",
            params![plugin_id, doc_id],
        )?;
        Ok(())
    }

    /// Delete all expired entries. Returns the number of rows removed.
    pub fn cleanup_expired(&self) -> Result<usize, CacheError> {
        let now = now_secs()?;
        let deleted = self.conn.execute(
            "DELETE FROM content_cache WHERE expires_at <= ?1",
            params![now],
        )?;
        Ok(deleted)
    }
}

fn now_secs() -> Result<i64, CacheError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| CacheError::Clock(e.to_string()))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    #[test]
    fn get_returns_none_when_empty() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        let result = cache.get("com.doxus.confluence", "page/123").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn set_then_get_returns_content() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        cache
            .set("com.doxus.confluence", "page/123", "hello world", 30)
            .unwrap();
        let result = cache.get("com.doxus.confluence", "page/123").unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn expired_entry_returns_none() {
        let db = TestDb::new();
        // Insert an already-expired entry directly (expires_at in the past)
        let now = now_secs().unwrap();
        db.conn
            .execute(
                "INSERT INTO content_cache(plugin_id, doc_id, content, cached_at, expires_at)
                 VALUES ('com.doxus.confluence', 'page/expired', 'stale', ?1, ?2)",
                params![now - 7200, now - 1],
            )
            .unwrap();
        let cache = ContentCache::new(&db.conn);
        let result = cache.get("com.doxus.confluence", "page/expired").unwrap();
        assert!(result.is_none(), "expired entry should not be returned");
    }

    #[test]
    fn set_overwrites_existing_entry() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        cache
            .set("com.doxus.confluence", "page/123", "v1", 30)
            .unwrap();
        cache
            .set("com.doxus.confluence", "page/123", "v2", 30)
            .unwrap();
        let result = cache.get("com.doxus.confluence", "page/123").unwrap();
        assert_eq!(result, Some("v2".to_string()));
    }

    #[test]
    fn touch_extends_ttl_of_live_entry() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        cache
            .set("com.doxus.confluence", "page/123", "content", 30)
            .unwrap();
        let touched = cache.touch("com.doxus.confluence", "page/123", 30).unwrap();
        assert!(touched, "touch should return true for a live entry");
    }

    #[test]
    fn touch_returns_false_for_expired_entry() {
        let db = TestDb::new();
        let now = now_secs().unwrap();
        db.conn
            .execute(
                "INSERT INTO content_cache(plugin_id, doc_id, content, cached_at, expires_at)
                 VALUES ('com.doxus.confluence', 'page/exp', 'old', ?1, ?2)",
                params![now - 7200, now - 1],
            )
            .unwrap();
        let cache = ContentCache::new(&db.conn);
        let touched = cache.touch("com.doxus.confluence", "page/exp", 30).unwrap();
        assert!(
            !touched,
            "touch should return false for expired/missing entry"
        );
    }

    #[test]
    fn invalidate_removes_entry() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        cache
            .set("com.doxus.confluence", "page/123", "data", 30)
            .unwrap();
        cache
            .invalidate("com.doxus.confluence", "page/123")
            .unwrap();
        let result = cache.get("com.doxus.confluence", "page/123").unwrap();
        assert!(result.is_none(), "invalidated entry should not be returned");
    }

    #[test]
    fn cleanup_expired_removes_only_expired_rows() {
        let db = TestDb::new();
        let now = now_secs().unwrap();
        // One live entry
        db.conn
            .execute(
                "INSERT INTO content_cache(plugin_id, doc_id, content, cached_at, expires_at)
                 VALUES ('com.doxus.confluence', 'page/live', 'live', ?1, ?2)",
                params![now, now + 1800],
            )
            .unwrap();
        // Two expired entries
        for i in 0..2 {
            db.conn
                .execute(
                    "INSERT INTO content_cache(plugin_id, doc_id, content, cached_at, expires_at)
                     VALUES ('com.doxus.confluence', ?1, 'old', ?2, ?3)",
                    params![format!("page/expired{i}"), now - 7200, now - 1],
                )
                .unwrap();
        }
        let cache = ContentCache::new(&db.conn);
        let deleted = cache.cleanup_expired().unwrap();
        assert_eq!(deleted, 2, "should delete exactly 2 expired rows");
        // Live entry still accessible
        let live = cache.get("com.doxus.confluence", "page/live").unwrap();
        assert!(live.is_some(), "live entry should survive cleanup");
    }

    #[test]
    fn different_plugins_do_not_interfere() {
        let db = TestDb::new();
        let cache = ContentCache::new(&db.conn);
        cache
            .set("com.doxus.confluence", "doc/1", "confluence-data", 30)
            .unwrap();
        cache
            .set("com.doxus.github", "doc/1", "github-data", 30)
            .unwrap();
        let conf = cache.get("com.doxus.confluence", "doc/1").unwrap();
        let gh = cache.get("com.doxus.github", "doc/1").unwrap();
        assert_eq!(conf, Some("confluence-data".to_string()));
        assert_eq!(gh, Some("github-data".to_string()));
    }
}
