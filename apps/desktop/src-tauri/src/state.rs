use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use rusqlite::Connection;
use doxus_core::embedding::EmbeddingProvider;
use doxus_core::plugin::PluginManager;
use doxus_agent::sync_sidecar::SyncSidecarManager;
use doxus_agent::prompt::PromptLoader;

pub struct OAuthPending {
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub type PendingMessages = Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>;

pub struct AppState {
    pub conn: Arc<Mutex<Connection>>,
    pub plugin_manager: PluginManager,
    pub plugins_dir: PathBuf,
    pub oauth_pending: Mutex<HashMap<String, OAuthPending>>,
    pub embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
    // Agent sidecar
    pub sidecar: Arc<SyncSidecarManager>,
    pub sidecar_script: PathBuf,
    pub prompt_loader: PromptLoader,
    pub pending_messages: PendingMessages,
    pub reader_started: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(conn: Connection, plugins_dir: PathBuf, sidecar_script: PathBuf, embedder: Arc<dyn EmbeddingProvider + Send + Sync>) -> Self {
        let prompt_loader = PromptLoader::new().expect("PromptLoader init failed");
        prompt_loader.ensure_defaults().ok();
        // App-start: clean up any expired cache entries from previous sessions
        {
            let cache = doxus_core::cache::ContentCache::new(&conn);
            if let Ok(n) = cache.cleanup_expired() {
                if n > 0 {
                    eprintln!("[cache] cleaned {n} expired entries on startup");
                }
            }
        }
        Self {
            conn: Arc::new(Mutex::new(conn)),
            plugin_manager: PluginManager::new(plugins_dir.clone()),
            plugins_dir,
            oauth_pending: Mutex::new(HashMap::new()),
            embedder,
            sidecar: Arc::new(SyncSidecarManager::new()),
            sidecar_script,
            prompt_loader,
            pending_messages: Arc::new(Mutex::new(HashMap::new())),
            reader_started: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_core::embedding::MockEmbedder;

    fn make_embedder() -> Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync> {
        Arc::new(MockEmbedder::new(384))
    }

    #[test]
    fn app_state_creates_with_in_memory_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        let state = AppState::new(conn, PathBuf::from("/tmp"), PathBuf::from("/tmp/fake.mjs"), make_embedder());
        let _guard = state.conn.lock().unwrap();
        drop(_guard);
        let pending = state.oauth_pending.lock().unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn app_state_embedder_dimension_is_set() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        let state = AppState::new(conn, PathBuf::from("/tmp"), PathBuf::from("/tmp/fake.mjs"), make_embedder());
        assert_eq!(state.embedder.dimension(), 384);
    }
}
