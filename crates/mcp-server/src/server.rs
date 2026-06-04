use doxus_core::embedding::EmbeddingProvider;
use doxus_core::db::DbPool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct McpServer {
    pub(crate) conn: DbPool,
    pub(crate) db_path: PathBuf,
    pub(crate) embedder: Arc<Mutex<Option<Arc<dyn EmbeddingProvider + Send + Sync>>>>,
    pub(crate) plugin_manager: Arc<doxus_core::plugin::PluginManager>,
    pub(crate) plugins_dir: PathBuf,
    pub(crate) allow_file_scheme: bool,
}

impl McpServer {
    pub fn new(
        conn: DbPool,
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
        conn: DbPool,
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

    /// Provides access to the underlying SQLite connection pool.
    pub fn conn(&self) -> DbPool {
        self.conn.clone()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.db_path.clone()
    }

    /// Provides access to the embedding provider, if currently loaded.
    pub fn embedder(&self) -> Option<Arc<dyn EmbeddingProvider + Send + Sync>> {
        self.embedder.lock().ok().and_then(|g| g.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TestContext {
        _temp_dir: TempDir,
        server: McpServer,
    }

    fn make_test_server() -> TestContext {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = doxus_core::db::create_pool(&db_path).unwrap();
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(PathBuf::from("/tmp")));
        let server = McpServer::new(pool, db_path, None, pm, PathBuf::from("/tmp"));
        TestContext { _temp_dir: temp_dir, server }
    }

    fn poison_mutex<T: Send + 'static>(mutex: &Arc<Mutex<T>>) {
        let m = Arc::clone(mutex);
        let _ = std::thread::spawn(move || {
            let _g = m.lock().unwrap();
            panic!("intentional poison for test");
        })
        .join();
        assert!(mutex.lock().is_err(), "mutex must be poisoned after setup");
    }

    // TDD: embedder() must return None — not panic — when Mutex is poisoned.
    #[test]
    fn embedder_returns_none_when_mutex_poisoned() {
        let ctx = make_test_server();
        poison_mutex(&ctx.server.embedder);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.server.embedder()));
        assert!(result.is_ok(), "embedder() must not panic on poisoned mutex");
        assert!(result.unwrap().is_none(), "embedder() must return None when poisoned");
    }

    // TDD: engine() must not panic when embedder Mutex is poisoned.
    #[test]
    fn engine_does_not_panic_when_embedder_poisoned() {
        let ctx = make_test_server();
        poison_mutex(&ctx.server.embedder);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.server.engine()));
        assert!(result.is_ok(), "engine() must not panic on poisoned embedder mutex");
    }
}
