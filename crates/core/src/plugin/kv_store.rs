use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KvError {
    #[error("namespace '{0}' not allowed by plugin manifest")]
    NamespaceNotAllowed(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("mutex lock poisoned")]
    LockPoisoned,
}

/// SQLite-backed KV store for WASM plugins.
/// Each entry is scoped by (plugin_id, namespace, key).
/// Only namespaces listed in the plugin manifest are accessible.
#[derive(Clone)]
pub struct KvStore {
    conn: Arc<Mutex<Connection>>,
    plugin_id: String,
    allowed_namespaces: Vec<String>,
}

impl KvStore {
    /// Create a new KvStore backed by an in-memory SQLite connection (for tests / legacy use).
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory db for kv");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_kv (
                plugin_id   TEXT    NOT NULL,
                namespace   TEXT    NOT NULL,
                key         TEXT    NOT NULL,
                value       BLOB    NOT NULL,
                updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (plugin_id, namespace, key)
            );",
        )
        .expect("kv table creation");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            plugin_id: "unknown".into(),
            allowed_namespaces: vec![],
        }
    }

    /// Create a KvStore backed by a shared DB connection (production path).
    /// `allowed_namespaces` should come from the plugin manifest.
    pub fn with_connection(
        conn: Arc<Mutex<Connection>>,
        plugin_id: impl Into<String>,
        allowed_namespaces: Vec<String>,
    ) -> Self {
        Self {
            conn,
            plugin_id: plugin_id.into(),
            allowed_namespaces,
        }
    }

    /// Ensure the plugin_kv table exists in the backing connection.
    pub fn init_table(&self) -> Result<(), KvError> {
        let conn = self.conn.lock().map_err(|_| KvError::LockPoisoned)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_kv (
                plugin_id   TEXT    NOT NULL,
                namespace   TEXT    NOT NULL,
                key         TEXT    NOT NULL,
                value       BLOB    NOT NULL,
                updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (plugin_id, namespace, key)
            );",
        )?;
        Ok(())
    }

    fn check_namespace(&self, namespace: &str) -> Result<(), KvError> {
        if self.allowed_namespaces.contains(&namespace.to_string()) {
            Ok(())
        } else {
            Err(KvError::NamespaceNotAllowed(namespace.to_string()))
        }
    }

    /// Get a value from (namespace, key). Returns None if not found.
    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError> {
        self.check_namespace(namespace)?;
        let conn = self.conn.lock().map_err(|_| KvError::LockPoisoned)?;
        let result: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM plugin_kv WHERE plugin_id = ?1 AND namespace = ?2 AND key = ?3",
                params![self.plugin_id, namespace, key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Set a value at (namespace, key). Upserts.
    pub fn set(&self, namespace: &str, key: &str, value: Vec<u8>) -> Result<(), KvError> {
        self.check_namespace(namespace)?;
        let conn = self.conn.lock().map_err(|_| KvError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO plugin_kv(plugin_id, namespace, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())
             ON CONFLICT(plugin_id, namespace, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![self.plugin_id, namespace, key, value],
        )?;
        Ok(())
    }

    // ── Legacy flat-key API (no namespace) — used by WasmDocSourceAdapter tests ──

    /// Legacy: get without namespace validation (uses empty namespace "").
    /// Only works when "" is in allowed_namespaces or store was created with `new()`.
    pub fn get_raw(&self, key: &str) -> Option<Vec<u8>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM plugin_kv WHERE plugin_id = ?1 AND namespace = '' AND key = ?2",
            params![self.plugin_id, key],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Legacy: set without namespace validation.
    pub fn set_raw(&self, key: String, value: Vec<u8>) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO plugin_kv(plugin_id, namespace, key, value, updated_at)
             VALUES (?1, '', ?2, ?3, unixepoch())
             ON CONFLICT(plugin_id, namespace, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![self.plugin_id, key, value],
        );
    }
}

impl Default for KvStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_ns(namespaces: &[&str]) -> KvStore {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_kv (
                plugin_id   TEXT    NOT NULL,
                namespace   TEXT    NOT NULL,
                key         TEXT    NOT NULL,
                value       BLOB    NOT NULL,
                updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (plugin_id, namespace, key)
            );",
        )
        .unwrap();
        KvStore::with_connection(
            Arc::new(Mutex::new(conn)),
            "com.test.plugin",
            namespaces.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn get_missing_returns_none() {
        let store = store_with_ns(&["default"]);
        assert!(store.get("default", "missing").unwrap().is_none());
    }

    #[test]
    fn set_then_get_returns_value() {
        let store = store_with_ns(&["default"]);
        store.set("default", "key", b"value".to_vec()).unwrap();
        assert_eq!(store.get("default", "key").unwrap(), Some(b"value".to_vec()));
    }

    #[test]
    fn overwrite_replaces_value() {
        let store = store_with_ns(&["ns"]);
        store.set("ns", "k", b"first".to_vec()).unwrap();
        store.set("ns", "k", b"second".to_vec()).unwrap();
        assert_eq!(store.get("ns", "k").unwrap(), Some(b"second".to_vec()));
    }

    #[test]
    fn namespace_not_in_manifest_returns_error() {
        let store = store_with_ns(&["allowed"]);
        let err = store.get("forbidden", "key").unwrap_err();
        assert!(matches!(err, KvError::NamespaceNotAllowed(_)));
    }

    #[test]
    fn namespace_not_in_manifest_set_returns_error() {
        let store = store_with_ns(&["allowed"]);
        let err = store.set("forbidden", "key", b"v".to_vec()).unwrap_err();
        assert!(matches!(err, KvError::NamespaceNotAllowed(_)));
    }

    #[test]
    fn plugin_id_isolation() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_kv (
                plugin_id   TEXT    NOT NULL,
                namespace   TEXT    NOT NULL,
                key         TEXT    NOT NULL,
                value       BLOB    NOT NULL,
                updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (plugin_id, namespace, key)
            );",
        )
        .unwrap();
        let shared = Arc::new(Mutex::new(conn));

        let store_a = KvStore::with_connection(shared.clone(), "plugin.a", vec!["ns".into()]);
        let store_b = KvStore::with_connection(shared.clone(), "plugin.b", vec!["ns".into()]);

        store_a.set("ns", "key", b"from_a".to_vec()).unwrap();
        // store_b should not see store_a's value
        assert!(store_b.get("ns", "key").unwrap().is_none());
    }

    #[test]
    fn lock_poisoned_returns_error() {
        use std::sync::{Arc, Mutex};
        // Poison the mutex by panicking inside a lock guard
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_kv (
                plugin_id   TEXT    NOT NULL,
                namespace   TEXT    NOT NULL,
                key         TEXT    NOT NULL,
                value       BLOB    NOT NULL,
                updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (plugin_id, namespace, key)
            );",
        )
        .unwrap();
        let shared = Arc::new(Mutex::new(conn));
        let shared_clone = shared.clone();
        // Poison the mutex
        let _ = std::panic::catch_unwind(|| {
            let _guard = shared_clone.lock().unwrap();
            panic!("poison");
        });
        assert!(shared.is_poisoned());

        let store = KvStore::with_connection(shared, "com.test.plugin", vec!["ns".into()]);
        let err = store.get("ns", "key").unwrap_err();
        assert!(matches!(err, KvError::LockPoisoned));
        let err2 = store.set("ns", "key", b"v".to_vec()).unwrap_err();
        assert!(matches!(err2, KvError::LockPoisoned));
    }

    #[test]
    fn legacy_raw_api_works() {
        let store = KvStore::new();
        assert!(store.get_raw("x").is_none());
        store.set_raw("x".into(), b"hello".to_vec());
        assert_eq!(store.get_raw("x"), Some(b"hello".to_vec()));
    }
}
