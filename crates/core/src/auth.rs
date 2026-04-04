use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("secret not found: {0}")]
    NotFound(String),
}

/// Abstraction over OS keychain (macOS Security.framework in prod, in-memory in tests)
pub trait SecretStore: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<String, AuthError>;
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), AuthError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), AuthError>;
}

/// In-memory implementation for tests and CI
#[derive(Default)]
pub struct MemorySecretStore {
    inner: std::sync::RwLock<std::collections::HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, service: &str, account: &str) -> Result<String, AuthError> {
        let key = format!("{service}:{account}");
        self.inner
            .read()
            .map_err(|_| AuthError::Keychain("lock poisoned".into()))?
            .get(&key)
            .cloned()
            .ok_or(AuthError::NotFound(key))
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), AuthError> {
        let key = format!("{service}:{account}");
        self.inner
            .write()
            .map_err(|_| AuthError::Keychain("lock poisoned".into()))?
            .insert(key, secret.to_string());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), AuthError> {
        let key = format!("{service}:{account}");
        self.inner
            .write()
            .map_err(|_| AuthError::Keychain("lock poisoned".into()))?
            .remove(&key);
        Ok(())
    }
}

/// OAuth flow description returned by oauth_start
#[derive(Debug, Clone)]
pub struct OAuthFlow {
    pub auth_url: String,
    pub state: String,
    pub redirect_uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_set_get() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "s3cr3t").unwrap();
        let val = store.get("svc", "acct").unwrap();
        assert_eq!(val, "s3cr3t");
    }

    #[test]
    fn memory_store_not_found() {
        let store = MemorySecretStore::new();
        let result = store.get("svc", "missing");
        assert!(matches!(result, Err(AuthError::NotFound(_))));
    }

    #[test]
    fn memory_store_delete() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "val").unwrap();
        store.delete("svc", "acct").unwrap();
        let result = store.get("svc", "acct");
        assert!(matches!(result, Err(AuthError::NotFound(_))));
    }

    #[test]
    fn memory_store_overwrite() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "first").unwrap();
        store.set("svc", "acct", "second").unwrap();
        let val = store.get("svc", "acct").unwrap();
        assert_eq!(val, "second");
    }
}
