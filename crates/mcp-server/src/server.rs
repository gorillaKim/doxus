use doxus_core::embedding::EmbeddingProvider;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct McpServer {
    pub(crate) conn: Arc<Mutex<Connection>>,
    pub(crate) db_path: PathBuf,
    pub(crate) embedder: Arc<Mutex<Option<Arc<dyn EmbeddingProvider + Send + Sync>>>>,
    pub(crate) plugin_manager: Arc<doxus_core::plugin::PluginManager>,
    pub(crate) plugins_dir: PathBuf,
    pub(crate) allow_file_scheme: bool,
}

impl McpServer {
    pub fn new(
        conn: Arc<Mutex<Connection>>,
        db_path: PathBuf,
        embedder: Option<Arc<dyn EmbeddingProvider + Send + Sync>>,
        plugin_manager: Arc<doxus_core::plugin::PluginManager>,
        plugins_dir: PathBuf,
    ) -> Self {
        Self {
            conn,
            db_path,
            embedder: Arc::new(Mutex::new(embedder)),
            plugin_manager,
            plugins_dir,
            allow_file_scheme: false,
        }
    }

    pub fn new_with_file_scheme(
        conn: Arc<Mutex<Connection>>,
        db_path: PathBuf,
        embedder: Option<Arc<dyn EmbeddingProvider + Send + Sync>>,
        plugin_manager: Arc<doxus_core::plugin::PluginManager>,
        plugins_dir: PathBuf,
    ) -> Self {
        Self {
            conn,
            db_path,
            embedder: Arc::new(Mutex::new(embedder)),
            plugin_manager,
            plugins_dir,
            allow_file_scheme: true,
        }
    }

    /// Provides access to the underlying SQLite connection.
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.db_path.clone()
    }

    /// Provides access to the embedding provider, if currently loaded.
    pub fn embedder(&self) -> Option<Arc<dyn EmbeddingProvider + Send + Sync>> {
        self.embedder.lock().unwrap().clone()
    }

    /// Provides access to the shared, mutable embedding provider holder.
    pub fn embedder_arc(&self) -> Arc<Mutex<Option<Arc<dyn EmbeddingProvider + Send + Sync>>>> {
        Arc::clone(&self.embedder)
    }

    /// Provides access to the plugin directory path.
    pub fn plugins_dir(&self) -> &std::path::Path {
        &self.plugins_dir
    }

    /// Provides access to the plugin manager.
    pub fn plugin_manager(&self) -> &Arc<doxus_core::plugin::PluginManager> {
        &self.plugin_manager
    }

    /// Creates a SearchEngine instance for this server.
    pub fn engine(&self) -> Arc<doxus_core::search::SearchEngine> {
        use doxus_core::search::SearchEngine;
        let embedder = self.embedder().unwrap_or_else(|| {
            // FTS-only fallback
            Arc::new(doxus_core::embedding::NoOpEmbedder)
        });
        Arc::new(SearchEngine::with_embedder(self.conn(), embedder))
    }

    /// Creates an IndexingService instance for this server.
    pub fn indexer(&self) -> doxus_core::indexing::IndexingService {
        use doxus_core::indexing::IndexingService;
        IndexingService::new(self.conn(), Arc::clone(&self.plugin_manager), self.engine())
    }
}
