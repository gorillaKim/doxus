use doxus_core::embedding::EmbeddingProvider;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct McpServer {
    pub(crate) conn: Arc<Mutex<Connection>>,
    pub(crate) embedder: Option<Arc<dyn EmbeddingProvider + Send + Sync>>,
    pub(crate) plugin_manager: Arc<doxus_core::plugin::PluginManager>,
    pub(crate) plugins_dir: PathBuf,
    pub(crate) allow_file_scheme: bool,
}

impl McpServer {
    pub fn new(
        conn: Arc<Mutex<Connection>>,
        embedder: Option<Arc<dyn EmbeddingProvider + Send + Sync>>,
        plugin_manager: Arc<doxus_core::plugin::PluginManager>,
        plugins_dir: PathBuf,
    ) -> Self {
        Self {
            conn,
            embedder,
            plugin_manager,
            plugins_dir,
            allow_file_scheme: false,
        }
    }

    pub fn new_with_file_scheme(
        conn: Arc<Mutex<Connection>>,
        embedder: Option<Arc<dyn EmbeddingProvider + Send + Sync>>,
        plugin_manager: Arc<doxus_core::plugin::PluginManager>,
        plugins_dir: PathBuf,
    ) -> Self {
        Self {
            conn,
            embedder,
            plugin_manager,
            plugins_dir,
            allow_file_scheme: true,
        }
    }

    /// Provides access to the underlying SQLite connection.
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }

    /// Provides access to the embedding provider, if available.
    pub fn embedder(&self) -> Option<&Arc<dyn EmbeddingProvider + Send + Sync>> {
        self.embedder.as_ref()
    }

    /// Provides access to the plugin directory path.
    pub fn plugins_dir(&self) -> &std::path::Path {
        &self.plugins_dir
    }

    /// Provides access to the plugin manager.
    pub fn plugin_manager(&self) -> &Arc<doxus_core::plugin::PluginManager> {
        &self.plugin_manager
    }
}
