use doxus_core::embedding::EmbeddingProvider;
use doxus_core::db::DbPool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};

pub struct McpServer {
    pub(crate) conn: DbPool,
    pub(crate) db_path: PathBuf,
    pub(crate) embedder: Arc<Mutex<Option<Arc<dyn EmbeddingProvider + Send + Sync>>>>,
    pub(crate) plugin_manager: Arc<doxus_core::plugin::PluginManager>,
    pub(crate) plugins_dir: PathBuf,
    pub(crate) allow_file_scheme: bool,
    pub(crate) session_docs: Arc<Mutex<HashMap<String, (HashSet<i64>, std::time::Instant)>>>,
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
            session_docs: Arc::new(Mutex::new(HashMap::new())),
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
            session_docs: Arc::new(Mutex::new(HashMap::new())),
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

    /// Records a document access event in a given session, and flushes any sessions
    /// that have been idle for 5 minutes or more.
    pub fn record_session_access(&self, session_id: &str, doc_id: i64) {
        let now = std::time::Instant::now();
        let mut session_map = match self.session_docs.lock() {
            Ok(m) => m,
            Err(_) => return,
        };

        // 1. Record the document ID and update last activity timestamp for the current session
        let entry = session_map
            .entry(session_id.to_string())
            .or_insert_with(|| (HashSet::new(), now));
        entry.0.insert(doc_id);
        entry.1 = now;
        println!("[DEBUG] Recorded session: {}, docs: {:?}", session_id, entry.0);

        // 2. Identify expired sessions (idle for 5 minutes/300 seconds)
        let mut expired = Vec::new();
        for (sid, (_docs, last_time)) in session_map.iter() {
            let duration = now.checked_duration_since(*last_time).map(|d| d.as_secs()).unwrap_or(0);
            if duration >= 300 && sid != session_id {
                expired.push(sid.clone());
            }
        }

        // 3. Flush expired sessions to the database
        for sid in expired {
            if let Some((docs, _)) = session_map.remove(&sid) {
                self.flush_session_data(docs);
            }
        }
    }

    /// Flushes accumulated document IDs in a session to the co-occurrence reference table.
    fn flush_session_data(&self, docs: HashSet<i64>) {
        println!("[DEBUG] flush_session_data called with docs: {:?}", docs);
        if docs.len() < 2 {
            println!("[DEBUG] docs len < 2, early returning");
            return;
        }

        let mut doc_list: Vec<i64> = docs.into_iter().collect();
        doc_list.sort_unstable();

        let conn_pool = self.conn();
        let conn = conn_pool.get().expect("Failed to acquire write connection from pool");

        let last_accessed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Write co-occurrence links for all A < B pairs
        for i in 0..doc_list.len() {
            for j in (i + 1)..doc_list.len() {
                let doc_a = doc_list[i];
                let doc_b = doc_list[j];

                let sql = "INSERT INTO document_co_refs (doc_a_id, doc_b_id, co_occurrence_count, last_accessed)
                           VALUES (?1, ?2, 1, ?3)
                           ON CONFLICT(doc_a_id, doc_b_id) DO UPDATE SET
                               co_occurrence_count = co_occurrence_count + 1,
                               last_accessed = ?3";
                let rows = conn.execute(sql, rusqlite::params![doc_a, doc_b, last_accessed]).unwrap();
                println!("[DEBUG] execute sql: (A={}, B={}) rows affected: {}", doc_a, doc_b, rows);
            }
        }
    }

    /// Force flushes all active sessions to the database.
    pub fn flush_all_sessions(&self) {
        let mut session_map = match self.session_docs.lock() {
            Ok(m) => m,
            Err(_) => return,
        };
        let keys: Vec<String> = session_map.keys().cloned().collect();
        for key in keys {
            if let Some((docs, _)) = session_map.remove(&key) {
                self.flush_session_data(docs);
            }
        }
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.flush_all_sessions();
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

    #[test]
    fn test_co_refs_session_tracking() {
        let ctx = make_test_server();

        // 1. Insert dummy project and documents to satisfy foreign key constraints
        {
            let conn = ctx.server.conn().get().unwrap();
            conn.execute("INSERT INTO projects (id, name, storage_strategy, display_name, path, created_at, updated_at) VALUES (1, 'test-project', 'local', 'Test Project', '/tmp/test', 1234567, 1234567)", rusqlite::params![]).unwrap();
            conn.execute("INSERT INTO documents (id, project_id, source_doc_id, title, content_hash) VALUES (10, 1, 'doc_a', 'Doc A', 'hash1')", rusqlite::params![]).unwrap();
            conn.execute("INSERT INTO documents (id, project_id, source_doc_id, title, content_hash) VALUES (20, 1, 'doc_b', 'Doc B', 'hash2')", rusqlite::params![]).unwrap();
            conn.execute("INSERT INTO documents (id, project_id, source_doc_id, title, content_hash) VALUES (30, 1, 'doc_c', 'Doc C', 'hash3')", rusqlite::params![]).unwrap();
        }

        // 2. Record accesses (doc_a_id: 10, 20)
        ctx.server.record_session_access("session-1", 10);
        ctx.server.record_session_access("session-1", 20);

        // 3. Before flush, nothing is in the database
        {
            let conn = ctx.server.conn().get().unwrap();
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM document_co_refs", rusqlite::params![], |r| r.get(0)).unwrap();
            assert_eq!(count, 0);
        }

        // 4. Force flush
        ctx.server.flush_all_sessions();

        // 5. Verify the (10, 20) pair exists and has correct values
        {
            let conn = ctx.server.conn().get().unwrap();
            let (doc_a, doc_b, co_count): (i64, i64, i64) = conn.query_row(
                "SELECT doc_a_id, doc_b_id, co_occurrence_count FROM document_co_refs WHERE doc_a_id = 10 AND doc_b_id = 20",
                rusqlite::params![],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            ).unwrap();
            assert_eq!(doc_a, 10);
            assert_eq!(doc_b, 20);
            assert_eq!(co_count, 1);
        }

        // 6. Record another session and verify accumulation
        ctx.server.record_session_access("session-2", 20);
        ctx.server.record_session_access("session-2", 10); // reverse order
        ctx.server.flush_all_sessions();

        {
            let conn = ctx.server.conn().get().unwrap();
            let updated_co_count: i64 = conn.query_row(
                "SELECT co_occurrence_count FROM document_co_refs WHERE doc_a_id = 10 AND doc_b_id = 20",
                rusqlite::params![],
                |r| r.get(0)
            ).unwrap();
            assert_eq!(updated_co_count, 2);
        }
    }
}
