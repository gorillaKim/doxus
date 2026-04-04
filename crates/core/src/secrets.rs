use std::collections::HashMap;
use std::sync::Mutex;
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
