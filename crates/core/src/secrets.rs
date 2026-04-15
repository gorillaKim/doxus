use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("keychain error: {0}")]
    Keychain(String),
}

/// Abstract secret store interface
pub trait SecretStore: Send + Sync {
    fn get(&self, service: &str, key: &str) -> Result<String, SecretsError>;
    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError>;
    fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError>;
}

/// macOS Keychain / Linux Secret Service backed store
pub struct SystemKeychain;

impl SecretStore for SystemKeychain {
    fn get(&self, service: &str, key: &str) -> Result<String, SecretsError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| SecretsError::Keychain(e.to_string()))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecretsError::NotFound(key.to_string()),
            other => SecretsError::Keychain(other.to_string()),
        })
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| SecretsError::Keychain(e.to_string()))?;
        entry.set_password(value).map_err(|e| SecretsError::Keychain(e.to_string()))
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| SecretsError::Keychain(e.to_string()))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecretsError::NotFound(key.to_string()),
            other => SecretsError::Keychain(other.to_string()),
        })
    }
}

/// Session-scoped cache wrapper — fetches from inner store once per (service, key) per process lifetime.
/// `set` updates the cache; `delete` removes the entry. Errors from inner are never cached.
pub struct CachedSecretStore<S> {
    inner: S,
    cache: RwLock<HashMap<(String, String), String>>,
}

impl<S: SecretStore> CachedSecretStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl<S: SecretStore> SecretStore for CachedSecretStore<S> {
    fn get(&self, service: &str, key: &str) -> Result<String, SecretsError> {
        let cache_key = (service.to_string(), key.to_string());
        {
            let r = self.cache.read().unwrap();
            if let Some(v) = r.get(&cache_key) {
                return Ok(v.clone());
            }
        }
        let value = self.inner.get(service, key)?;
        self.cache.write().unwrap().insert(cache_key, value.clone());
        Ok(value)
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError> {
        self.inner.set(service, key, value)?;
        self.cache
            .write()
            .unwrap()
            .insert((service.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError> {
        self.inner.delete(service, key)?;
        self.cache
            .write()
            .unwrap()
            .remove(&(service.to_string(), key.to_string()));
        Ok(())
    }
}

/// In-memory store for tests
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn map_key(service: &str, key: &str) -> String {
        format!("{service}/{key}")
    }
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, service: &str, key: &str) -> Result<String, SecretsError> {
        let map = self.inner.lock().unwrap();
        map.get(&Self::map_key(service, key))
            .cloned()
            .ok_or_else(|| SecretsError::NotFound(key.to_string()))
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError> {
        let mut map = self.inner.lock().unwrap();
        map.insert(Self::map_key(service, key), value.to_string());
        Ok(())
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError> {
        let mut map = self.inner.lock().unwrap();
        map.remove(&Self::map_key(service, key))
            .map(|_| ())
            .ok_or_else(|| SecretsError::NotFound(key.to_string()))
    }
}

/// Validate and store a plugin API token.
/// Returns Ok(()) if token was stored successfully.
pub fn store_plugin_token(
    store: &dyn SecretStore,
    plugin_id: &str,
    key: &str,
    token: &str,
) -> Result<(), SecretsError> {
    if token.trim().is_empty() {
        return Err(SecretsError::NotFound("empty token".into()));
    }
    store.set(&format!("doxus.{plugin_id}"), key, token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ── Test double: counts get() calls to verify cache hits ─────────────────

    struct CountingStore {
        inner: MemorySecretStore,
        get_count: Arc<AtomicUsize>,
    }

    impl CountingStore {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let counter = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    inner: MemorySecretStore::new(),
                    get_count: Arc::clone(&counter),
                },
                counter,
            )
        }
    }

    impl SecretStore for CountingStore {
        fn get(&self, service: &str, key: &str) -> Result<String, SecretsError> {
            self.get_count.fetch_add(1, Ordering::SeqCst);
            self.inner.get(service, key)
        }
        fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError> {
            self.inner.set(service, key, value)
        }
        fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError> {
            self.inner.delete(service, key)
        }
    }

    // ── CachedSecretStore tests ───────────────────────────────────────────────

    #[test]
    fn cached_store_second_get_hits_cache_not_inner() {
        let (counting, counter) = CountingStore::new();
        counting.inner.set("svc", "key", "val").unwrap();
        let cached = CachedSecretStore::new(counting);

        assert_eq!(cached.get("svc", "key").unwrap(), "val");
        assert_eq!(cached.get("svc", "key").unwrap(), "val");

        // inner.get called exactly once — second hit served from cache
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cached_store_set_updates_cache_no_inner_get_needed() {
        let (counting, counter) = CountingStore::new();
        let cached = CachedSecretStore::new(counting);

        cached.set("svc", "key", "new_val").unwrap();
        let val = cached.get("svc", "key").unwrap();

        assert_eq!(val, "new_val");
        // get() should not call inner at all — value was cached by set()
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn cached_store_delete_removes_from_cache() {
        let (counting, _) = CountingStore::new();
        counting.inner.set("svc", "key", "val").unwrap();
        let cached = CachedSecretStore::new(counting);

        cached.get("svc", "key").unwrap(); // populate cache
        cached.delete("svc", "key").unwrap();

        let err = cached.get("svc", "key").unwrap_err();
        assert!(matches!(err, SecretsError::NotFound(_)));
    }

    #[test]
    fn cached_store_not_found_error_not_cached() {
        let (counting, counter) = CountingStore::new();
        let cached = CachedSecretStore::new(counting);

        // First get — miss → inner called
        assert!(cached.get("svc", "missing").is_err());
        // Second get — should try inner again (errors are not cached)
        assert!(cached.get("svc", "missing").is_err());

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_store_different_service_key_pairs_isolated() {
        let (counting, counter) = CountingStore::new();
        counting.inner.set("svc1", "key", "a").unwrap();
        counting.inner.set("svc2", "key", "b").unwrap();
        let cached = CachedSecretStore::new(counting);

        assert_eq!(cached.get("svc1", "key").unwrap(), "a");
        assert_eq!(cached.get("svc2", "key").unwrap(), "b");
        // Repeat — both should be cached
        assert_eq!(cached.get("svc1", "key").unwrap(), "a");
        assert_eq!(cached.get("svc2", "key").unwrap(), "b");

        assert_eq!(counter.load(Ordering::SeqCst), 2); // 2 misses, 2 hits
    }

    #[test]
    fn cached_store_set_overwrites_existing_cache_entry() {
        let (counting, counter) = CountingStore::new();
        counting.inner.set("svc", "key", "old").unwrap();
        let cached = CachedSecretStore::new(counting);

        cached.get("svc", "key").unwrap(); // cache "old"
        cached.set("svc", "key", "new").unwrap(); // should update cache
        let val = cached.get("svc", "key").unwrap(); // should return "new" from cache

        assert_eq!(val, "new");
        assert_eq!(counter.load(Ordering::SeqCst), 1); // only initial miss
    }

    #[test]
    fn memory_store_get_missing_returns_not_found() {
        let store = MemorySecretStore::new();
        let err = store.get("svc", "missing").unwrap_err();
        assert!(matches!(err, SecretsError::NotFound(_)));
    }

    #[test]
    fn memory_store_set_then_get() {
        let store = MemorySecretStore::new();
        store.set("svc", "key", "value").unwrap();
        assert_eq!(store.get("svc", "key").unwrap(), "value");
    }

    #[test]
    fn memory_store_delete_then_get_returns_not_found() {
        let store = MemorySecretStore::new();
        store.set("svc", "key", "value").unwrap();
        store.delete("svc", "key").unwrap();
        let err = store.get("svc", "key").unwrap_err();
        assert!(matches!(err, SecretsError::NotFound(_)));
    }

    #[test]
    fn memory_store_different_services_isolated() {
        let store = MemorySecretStore::new();
        store.set("svc1", "key", "val1").unwrap();
        store.set("svc2", "key", "val2").unwrap();
        assert_eq!(store.get("svc1", "key").unwrap(), "val1");
        assert_eq!(store.get("svc2", "key").unwrap(), "val2");
    }

    #[test]
    fn secrets_error_display() {
        let not_found = SecretsError::NotFound("mykey".into());
        assert_eq!(not_found.to_string(), "secret not found: mykey");

        let keychain_err = SecretsError::Keychain("OS error".into());
        assert_eq!(keychain_err.to_string(), "keychain error: OS error");
    }

    #[test]
    fn store_plugin_token_rejects_empty_token() {
        let store = MemorySecretStore::new();
        let err = store_plugin_token(&store, "com.doxus.test", "api_token", "").unwrap_err();
        assert!(matches!(err, SecretsError::NotFound(_)));

        let err2 = store_plugin_token(&store, "com.doxus.test", "api_token", "   ").unwrap_err();
        assert!(matches!(err2, SecretsError::NotFound(_)));
    }
}
