use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use rusqlite::Connection;
use doxus_core::embedding::EmbeddingProvider;
use doxus_core::plugin::PluginManager;
use doxus_core::secrets::UnifiedKeychainStore;
use doxus_agent::sync_sidecar::SyncSidecarManager;
use doxus_agent::prompt::PromptLoader;
use doxus_core::sync_manager::{SyncManager, SyncTrigger};
use tokio::sync::mpsc;

pub struct OAuthPending {
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub type PendingMessages = Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>;

pub struct AppState {
    pub conn: Arc<Mutex<Connection>>,
    pub plugin_manager: Arc<PluginManager>,
    pub plugins_dir: PathBuf,
    pub oauth_pending: Mutex<HashMap<String, OAuthPending>>,
    pub embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
    // Agent sidecar
    pub sidecar: Arc<SyncSidecarManager>,
    pub sidecar_script: PathBuf,
    pub prompt_loader: PromptLoader,
    pub pending_messages: PendingMessages,
    pub reader_started: Arc<AtomicBool>,
    pub secret_store: Arc<UnifiedKeychainStore>,
    pub sync_manager: Arc<SyncManager>,
}

impl AppState {
    pub fn new(
        conn: Connection,
        plugins_dir: PathBuf,
        sidecar_script: PathBuf,
        embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
        keychain_migrated: bool,
    ) -> (Self, mpsc::Receiver<SyncTrigger>) {
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

        let conn_arc = Arc::new(Mutex::new(conn));
        let search_engine = Arc::new(doxus_core::search::SearchEngine::with_embedder(conn_arc.clone(), embedder.clone()));
        let indexing_service = Arc::new(doxus_core::indexing::IndexingService::new(conn_arc.clone(), Arc::new(PluginManager::new(plugins_dir.clone())), search_engine.clone()));
        let (sync_manager, rx) = SyncManager::new(indexing_service);
        let sync_manager = Arc::new(sync_manager);

        let mut plugin_manager = PluginManager::new(plugins_dir.clone());
        plugin_manager.register_factory(&PluginManager::normalize_id("obsidian"), || {
            Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
        });
        plugin_manager.register_factory(&PluginManager::normalize_id("confluence"), || {
            Box::new(doxus_plugin_confluence::ConfluencePlugin::new())
        });
        plugin_manager.register_factory(&PluginManager::normalize_id("github"), || {
            Box::new(doxus_plugin_github::GitHubPlugin::new())
        });

        let plugin_manager = Arc::new(plugin_manager);
        let secret_store = Arc::new(UnifiedKeychainStore::new("doxus", "com.doxus.secrets.v1"));

        // Background keychain migration
        if !keychain_migrated {
            let store_clone = secret_store.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _ = doxus_core::secrets::migrate_legacy_secrets(
                    &store_clone,
                    &["com.doxus.confluence", "com.doxus.github"],
                );
            });
        }

        (
            Self {
                conn: conn_arc,
                plugin_manager,
                plugins_dir,
                oauth_pending: Mutex::new(HashMap::new()),
                embedder,
                sidecar: Arc::new(SyncSidecarManager::new()),
                sidecar_script,
                prompt_loader,
                pending_messages: Arc::new(Mutex::new(HashMap::new())),
                reader_started: Arc::new(AtomicBool::new(false)),
                secret_store,
                sync_manager,
            },
            rx
        )
    }
}
