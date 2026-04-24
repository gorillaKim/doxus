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

/// Unified store that packs everything into a single JSON blob in the OS keychain.
/// Reduces the number of permission prompts in macOS.
pub struct UnifiedKeychainStore {
    service: String,
    account: String,
    cache: RwLock<HashMap<String, String>>,
    load_once: once_cell::sync::OnceCell<Result<(), String>>,
}

impl UnifiedKeychainStore {
    pub fn new(service: &str, account: &str) -> Self {
        Self {
            service: service.to_string(),
            account: account.to_string(),
            cache: RwLock::new(HashMap::new()),
            load_once: once_cell::sync::OnceCell::new(),
        }
    }

    /// Load the entire JSON blob from keychain and populate the cache.
    /// 테스트 환경에서는 `DOXUS_SKIP_KEYCHAIN=1`를 설정하여 keychain 접근을 건너뛸 수 있습니다.
    pub fn load_from_keychain(&self) -> Result<(), SecretsError> {
        if std::env::var("DOXUS_SKIP_KEYCHAIN").unwrap_or_default() == "1" {
            return Ok(());
        }

        let entry = keyring::Entry::new(&self.service, &self.account)
            .map_err(|e| SecretsError::Keychain(e.to_string()))?;
        
        match entry.get_password() {
            Ok(json) => {
                let data: HashMap<String, String> = serde_json::from_str(&json)
                    .map_err(|e| SecretsError::Keychain(format!("json parse error: {}", e)))?;
                *self.cache.write().unwrap() = data;
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                tracing::info!("[Secrets] Unified store entry not found (expected on first run)");
                *self.cache.write().unwrap() = HashMap::new();
                Ok(())
            }
            Err(e) => Err(SecretsError::Keychain(e.to_string())),
        }
    }

    pub fn ensure_loaded(&self) -> Result<(), SecretsError> {
        self.load_once.get_or_init(|| {
            self.load_from_keychain().map_err(|e| e.to_string())
        }).as_ref().map_err(|e| SecretsError::Keychain(e.clone()))?;
        Ok(())
    }

    /// Persist the current cache to the keychain as a JSON blob.
    fn save_to_keychain(&self) -> Result<(), SecretsError> {
        if std::env::var("DOXUS_SKIP_KEYCHAIN").unwrap_or_default() == "1" {
            return Ok(());
        }
        self.ensure_loaded()?;
        let entry = keyring::Entry::new(&self.service, &self.account)
            .map_err(|e| SecretsError::Keychain(e.to_string()))?;
        
        let json = {
            let cache = self.cache.read().unwrap();
            serde_json::to_string(&*cache)
                .map_err(|e| SecretsError::Keychain(format!("json serialize error: {}", e)))?
        };
        
        entry.set_password(&json).map_err(|e| SecretsError::Keychain(e.to_string()))
    }

    fn make_key(service: &str, key: &str) -> String {
        format!("{}:{}", service, key)
    }

    /// Update multiple keys at once and save to the keychain in a single operation.
    pub fn set_bulk(&self, service: &str, updates: &std::collections::HashMap<String, String>) -> Result<(), SecretsError> {
        self.ensure_loaded()?;
        {
            let mut cache = self.cache.write().unwrap();
            for (key, value) in updates {
                let store_key = Self::make_key(service, key);
                cache.insert(store_key, value.to_string());
            }
        }
        self.save_to_keychain()
    }
}

/// Migrate secrets from old naming patterns to the UnifiedKeychainStore.
pub fn migrate_legacy_secrets(
    unified_store: &UnifiedKeychainStore,
    plugin_ids: &[&str],
) -> Result<(), SecretsError> {
    unified_store.ensure_loaded()?;

    for &plugin_id in plugin_ids {
        let legacy_patterns = vec![
            // Pattern 1: (doxus.{id}, key)
            (format!("doxus.{}", plugin_id), vec!["api_token".to_string(), "token".to_string(), "email".to_string()]),
            // Pattern 2: (doxus, doxus:{id}:{key})
            ("doxus".to_string(), vec![
                format!("doxus:{}:api_token", plugin_id),
                format!("doxus:{}:token", plugin_id),
                format!("doxus:{}:email", plugin_id),
            ]),
            // Pattern 3: (doxus-{id}, key)
            (format!("doxus-{}", plugin_id), vec!["api_token".to_string(), "token".to_string(), "email".to_string()]),
        ];

        for (service, keys) in legacy_patterns {
            for key in &keys {
                let entry = keyring::Entry::new(&service, key)
                    .map_err(|e| SecretsError::Keychain(e.to_string()))?;
                
                if let Ok(password) = entry.get_password() {
                    tracing::info!("[Secrets] Migrating legacy secret '{}' to unified store", key);
                    let target_service = plugin_id.to_string();
                    let target_key = if key.starts_with("doxus:") {
                        key.split(':').last().unwrap_or(key)
                    } else {
                        key
                    };
                    
                    unified_store.set(&target_service, target_key, &password)?;
                    // Delete legacy entry after successful migration
                    let _ = entry.delete_credential();
                }
            }
        }
    }
    Ok(())
}

impl SecretStore for UnifiedKeychainStore {
    fn get(&self, service: &str, key: &str) -> Result<String, SecretsError> {
        self.ensure_loaded()?;
        let store_key = Self::make_key(service, key);
        let cache = self.cache.read().unwrap();
        cache.get(&store_key).cloned().ok_or(SecretsError::NotFound(key.to_string()))
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), SecretsError> {
        let store_key = Self::make_key(service, key);
        let mut cache = self.cache.write().unwrap();
        cache.insert(store_key, value.to_string());
        self.save_to_keychain()
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), SecretsError> {
        let store_key = Self::make_key(service, key);
        let mut cache = self.cache.write().unwrap();
        if cache.remove(&store_key).is_none() {
            return Err(SecretsError::NotFound(key.to_string()));
        }
        self.save_to_keychain()
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
    store.set(plugin_id, key, token)
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

    // ── UnifiedKeychainStore tests ───────────────────────────────────────────

    #[test]
    fn unified_store_namespace_isolation() {
        let store = UnifiedKeychainStore::new("test", "test_account");
        // Mock save/load not implemented yet, using in-memory part for now.
        store.set("svc1", "k1", "v1").unwrap();
        store.set("svc2", "k1", "v2").unwrap();

        assert_eq!(store.get("svc1", "k1").unwrap(), "v1");
        assert_eq!(store.get("svc2", "k1").unwrap(), "v2");
    }

    #[test]
    fn unified_store_get_not_found() {
        let store = UnifiedKeychainStore::new("test", "test_account");
        let err = store.get("svc", "missing").unwrap_err();
        assert!(matches!(err, SecretsError::NotFound(_)));
    }

    #[test]
    #[ignore = "touches real keychain"]
    fn unified_store_persistence() {
        let service = "doxus_test_service";
        let account = "doxus_test_account";
        let store1 = UnifiedKeychainStore::new(service, account);
        
        store1.set("svc", "key", "secret_value").unwrap();
        
        let store2 = UnifiedKeychainStore::new(service, account);
        store2.load_from_keychain().unwrap();
        
        assert_eq!(store2.get("svc", "key").unwrap(), "secret_value");
        
        // Cleanup
        store2.delete("svc", "key").unwrap();
    }
}
