use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use doxus_core::embedding::EmbeddingProvider;
use doxus_core::plugin::PluginManager;
use doxus_core::secrets::UnifiedKeychainStore;
use doxus_agent::sync_sidecar::SyncSidecarManager;
use doxus_agent::prompt::PromptLoader;
use doxus_core::sync_manager::{SyncManager, SyncTrigger};
use doxus_core::scheduler::SchedulerManager;
use tokio::sync::{mpsc, RwLock};

/// Shared, swappable embedder. The inner `Arc<dyn ...>` can be replaced at runtime
/// (e.g. after the user downloads the ONNX model on first launch) while existing
/// Arc clones returned by `current()` remain valid for any in-flight work.
pub type SharedEmbedder = Arc<RwLock<Arc<dyn EmbeddingProvider + Send + Sync>>>;

pub fn builtin_plugin_ids() -> &'static [&'static str] {
    &["com.doxus.obsidian", "com.doxus.confluence", "com.doxus.github"]
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_plugin_ids_contains_all_registered_plugins() {
        let ids = super::builtin_plugin_ids();
        assert!(ids.contains(&"com.doxus.obsidian"));
        assert!(ids.contains(&"com.doxus.confluence"));
        assert!(ids.contains(&"com.doxus.github"));
    }
}

pub struct OAuthPending {
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub type PendingMessages = Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>;

pub struct AppState {
    pub conn: doxus_core::db::DbPool,
    pub plugin_manager: Arc<PluginManager>,
    pub plugins_dir: PathBuf,
    pub oauth_pending: Mutex<HashMap<String, OAuthPending>>,
    pub embedder: SharedEmbedder,
    // Agent sidecar
    pub sidecar: Arc<SyncSidecarManager>,
    pub sidecar_script: PathBuf,
    pub prompt_loader: PromptLoader,
    pub pending_messages: PendingMessages,
    pub reader_started: Arc<AtomicBool>,
    pub secret_store: Arc<UnifiedKeychainStore>,
    pub sync_manager: Arc<SyncManager>,
    pub scheduler_manager: Arc<SchedulerManager>,
    pub collected_messages: Arc<Mutex<HashMap<String, String>>>,
    // MCP HTTP server (앱 생애주기에 묶임)
    pub mcp_process: Arc<Mutex<Option<std::process::Child>>>,
    pub mcp_endpoint: Arc<Mutex<Option<String>>>,
    pub mcp_token: Arc<Mutex<String>>,
}

impl AppState {
    pub fn new(
        conn: doxus_core::db::DbPool,
        plugins_dir: PathBuf,
        sidecar_script: PathBuf,
        embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
        keychain_migrated: bool,
    ) -> (Self, mpsc::Receiver<SyncTrigger>) {
        let prompt_loader = PromptLoader::new().expect("PromptLoader init failed");
        prompt_loader.ensure_defaults().ok();

        // App-start: clean up any expired cache entries from previous sessions
        {
            if let Ok(conn_guard) = conn.get() {
                let cache = doxus_core::cache::ContentCache::new(&conn_guard);
                if let Ok(n) = cache.cleanup_expired() {
                    if n > 0 {
                        eprintln!("[cache] cleaned {n} expired entries on startup");
                    }
                }
            }
        }

        let shared_embedder: SharedEmbedder = Arc::new(RwLock::new(embedder.clone()));
        let search_engine = Arc::new(doxus_core::search::SearchEngine::with_embedder(conn.clone(), embedder));

        // 1. PluginManager를 먼저 생성하고 내장 플러그인 등록
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

        // 2. 초기화된 plugin_manager를 IndexingService에 전달
        let indexing_service = Arc::new(doxus_core::indexing::IndexingService::new(
            conn.clone(),
            plugin_manager.clone(),
            search_engine.clone()
        ));

        let (sync_manager, rx) = SyncManager::new(indexing_service.clone());
        let sync_manager = Arc::new(sync_manager);
        
        let scheduler_manager = Arc::new(SchedulerManager::new(conn.clone(), indexing_service));

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
                conn,
                plugin_manager,
                plugins_dir,
                oauth_pending: Mutex::new(HashMap::new()),
                embedder: shared_embedder,
                sidecar: Arc::new(SyncSidecarManager::new()),
                sidecar_script,
                prompt_loader,
                pending_messages: Arc::new(Mutex::new(HashMap::new())),
                reader_started: Arc::new(AtomicBool::new(false)),
                secret_store,
                sync_manager,
                scheduler_manager,
                collected_messages: Arc::new(Mutex::new(HashMap::new())),
                mcp_process: Arc::new(Mutex::new(None)),
                mcp_endpoint: Arc::new(Mutex::new(None)),
                mcp_token: Arc::new(Mutex::new(String::new())),
            },
            rx
        )
    }
}
