use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct KvStore {
    inner: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl KvStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.read().ok()?.get(key).cloned()
    }

    pub fn set(&self, key: String, value: Vec<u8>) {
        if let Ok(mut m) = self.inner.write() {
            m.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_missing_returns_none() {
        let store = KvStore::new();
        assert!(store.get("missing").is_none());
    }

    #[test]
    fn set_then_get_returns_value() {
        let store = KvStore::new();
        store.set("key".into(), b"value".to_vec());
        assert_eq!(store.get("key"), Some(b"value".to_vec()));
    }

    #[test]
    fn overwrite_replaces_value() {
        let store = KvStore::new();
        store.set("key".into(), b"first".to_vec());
        store.set("key".into(), b"second".to_vec());
        assert_eq!(store.get("key"), Some(b"second".to_vec()));
    }
}
