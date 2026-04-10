use crate::db::schema::SearchHit;
use crate::embedding::{EmbeddingError, EmbeddingProvider};
use rusqlite::Connection;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("embedding failed: {0}")]
    Embedding(String),
    #[error("connection lock poisoned")]
    LockPoisoned,
    #[error("task join error: {0}")]
    Join(String),
}

impl From<EmbeddingError> for SearchError {
    fn from(e: EmbeddingError) -> Self {
        SearchError::Embedding(e.to_string())
    }
}

impl From<tokio::task::JoinError> for SearchError {
    fn from(e: tokio::task::JoinError) -> Self {
        SearchError::Join(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub project_ids: Vec<i64>,
    pub limit: usize,
    pub mode: SearchMode,
}

#[derive(Debug, Clone, Default)]
pub enum SearchMode {
    #[default]
    Hybrid,
    Fts,
    Vector,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            project_ids: vec![],
            limit: 20,
            mode: SearchMode::Hybrid,
        }
    }

    pub fn with_projects(mut self, ids: Vec<i64>) -> Self {
        self.project_ids = ids;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Simplified search options for convenience API.
#[derive(Debug, Default)]
pub struct SearchOpts {
    pub project_ids: Option<Vec<i64>>,
    pub limit: Option<usize>,
}

/// A ranked search result.
#[derive(Debug, Clone)]
pub struct Hit {
    pub document_id: i64,
    pub project_id: i64,
    pub source_doc_id: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub score: f64,
}

/// RRF constant (k=60 is standard).
const RRF_K: usize = 60;

fn rrf_score(rank: usize) -> f64 {
    1.0 / (RRF_K + rank) as f64
}

/// Merge two ranked result lists via Reciprocal Rank Fusion.
fn rrf_merge(fts_hits: Vec<SearchHit>, vec_hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut scores: HashMap<i64, (f64, SearchHit)> = HashMap::new();

    for (rank, hit) in fts_hits.into_iter().enumerate() {
        use std::collections::hash_map::Entry;
        match scores.entry(hit.chunk_id) {
            Entry::Vacant(v) => {
                v.insert((rrf_score(rank + 1), hit));
            }
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                e.0 += rrf_score(rank + 1);
                if e.1.snippet.is_empty() && !hit.snippet.is_empty() {
                    e.1.snippet = hit.snippet;
                }
            }
        }
    }
    for (rank, hit) in vec_hits.into_iter().enumerate() {
        use std::collections::hash_map::Entry;
        match scores.entry(hit.chunk_id) {
            Entry::Vacant(v) => {
                v.insert((rrf_score(rank + 1), hit));
            }
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                e.0 += rrf_score(rank + 1);
                if e.1.snippet.is_empty() && !hit.snippet.is_empty() {
                    e.1.snippet = hit.snippet;
                }
            }
        }
    }

    let mut merged: Vec<SearchHit> = scores
        .into_values()
        .map(|(score, mut hit)| {
            hit.score = score;
            hit
        })
        .collect();
    merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

/// No-op embedder for FTS-only usage (avoids requiring a real model).
struct NoOpEmbedder;

#[async_trait::async_trait]
impl EmbeddingProvider for NoOpEmbedder {
    async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Err(EmbeddingError::Inference("no embedder configured".into()))
    }
    fn dimension(&self) -> usize {
        384
    }
    fn model_info(&self) -> &crate::embedding::ModelInfo {
        // This is only called in error paths; use a static leak for simplicity
        static INFO: std::sync::OnceLock<crate::embedding::ModelInfo> = std::sync::OnceLock::new();
        INFO.get_or_init(|| crate::embedding::ModelInfo {
            name: "noop".to_string(),
            dimension: 384,
            max_tokens: 0,
        })
    }
}

pub struct SearchEngine {
    conn: Arc<Mutex<Connection>>,
    embedder: Arc<dyn EmbeddingProvider>,
}

impl SearchEngine {
    /// Create a new SearchEngine with an embedding provider.
    pub fn with_embedder(conn: Arc<Mutex<Connection>>, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self { conn, embedder }
    }

    /// Create a SearchEngine from a borrowed connection (FTS-only, sync-compatible).
    /// This wraps the connection in Arc<Mutex<>> with a NoOpEmbedder.
    /// Primarily for backward compatibility with callers that pass &Connection.
    pub fn new(conn: &Connection) -> SyncSearchEngine<'_> {
        SyncSearchEngine::from_conn(conn)
    }

    /// Create a SearchEngine owning the connection (FTS-only).
    pub fn new_fts_only(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            embedder: Arc::new(NoOpEmbedder),
        }
    }

    /// Index a document: insert/replace into documents, chunks, and chunk_embeddings.
    pub async fn index_document_async(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), SearchError> {
        // 1. Generate embedding
        let embedding_result = self.embedder.embed(&[content]).await;
        let emb_bytes: Option<Vec<u8>> = match embedding_result {
            Ok(vecs) => {
                let emb = vecs.into_iter().next().ok_or_else(|| {
                    SearchError::Embedding("empty embedding result".into())
                })?;
                Some(emb.iter().flat_map(|f| f.to_le_bytes()).collect())
            }
            Err(EmbeddingError::Inference(_)) => None, // NoOp embedder - skip vector storage
            Err(e) => return Err(SearchError::Embedding(e.to_string())),
        };

        // 2. DB writes via spawn_blocking
        let conn = Arc::clone(&self.conn);
        let source_doc_id = source_doc_id.to_string();
        let title = title.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<(), SearchError> {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            index_document_sync(&conn, project_id, &source_doc_id, &title, &content, emb_bytes.as_deref())
        })
        .await??;

        Ok(())
    }

    /// Hybrid search: FTS5 + vector similarity, merged via RRF.
    pub async fn search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        match query.mode {
            SearchMode::Fts => self.fts_search_async(query).await,
            SearchMode::Vector => self.vector_search_async(query).await,
            SearchMode::Hybrid => {
                let fts_hits = self.fts_search_async(query).await?;
                let vec_hits = self.vector_search_async(query).await.unwrap_or_default();
                Ok(rrf_merge(fts_hits, vec_hits))
            }
        }
    }

    async fn fts_search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let conn = Arc::clone(&self.conn);
        let query = query.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            fts_search_sync(&conn, &query)
        })
        .await?
    }

    async fn vector_search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let embedding = self.embedder.embed(&[query.text.as_str()]).await
            .map_err(|e| SearchError::Embedding(e.to_string()))?;
        let emb = embedding.into_iter().next()
            .ok_or_else(|| SearchError::Embedding("empty".into()))?;
        let emb_bytes: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();

        let conn = Arc::clone(&self.conn);
        let limit = query.limit as i64;
        let project_ids = query.project_ids.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            vector_search_sync(&conn, &emb_bytes, limit, &project_ids)
        })
        .await?
    }
}

// ── Sync free functions (used inside spawn_blocking) ─────────────────────────

fn index_document_sync(
    conn: &Connection,
    project_id: i64,
    source_doc_id: &str,
    title: &str,
    content: &str,
    emb_bytes: Option<&[u8]>,
) -> Result<(), SearchError> {
    // Upsert document
    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed)
         VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())
         ON CONFLICT(project_id, source_doc_id) DO UPDATE SET
            title = excluded.title,
            content = excluded.content,
            content_hash = excluded.content_hash,
            last_indexed = excluded.last_indexed",
        rusqlite::params![project_id, source_doc_id, title, content, content_hash],
    )?;

    let doc_id: i64 = conn.query_row(
        "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
        rusqlite::params![project_id, source_doc_id],
        |row| row.get(0),
    )?;

    // Delete old chunks (triggers handle FTS cleanup)
    conn.execute("DELETE FROM chunks WHERE document_id = ?1", [doc_id])?;

    // Insert single chunk (whole content)
    conn.execute(
        "INSERT INTO chunks (document_id, content, chunk_index) VALUES (?1, ?2, 0)",
        rusqlite::params![doc_id, content],
    )?;

    // Store embedding in chunk_embeddings if provided
    if let Some(bytes) = emb_bytes {
        let chunk_id: i64 = conn.query_row(
            "SELECT id FROM chunks WHERE document_id = ?1 AND chunk_index = 0",
            [doc_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO chunk_embeddings(chunk_id, embedding) VALUES (?1, ?2)",
            rusqlite::params![chunk_id, bytes],
        )?;
    }

    Ok(())
}

fn fts_search_sync(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
    let limit = query.limit as i64;

    let (project_filter, base_param_count) = if query.project_ids.is_empty() {
        ("AND p.status = 'active'".to_string(), 2usize)
    } else {
        let placeholders: Vec<String> = (0..query.project_ids.len())
            .map(|i| format!("?{}", i + 3))
            .collect();
        (format!("AND d.project_id IN ({})", placeholders.join(", ")), 2 + query.project_ids.len())
    };
    let _ = base_param_count;

    let sql = format!(
        "SELECT d.id, c.id, d.title, d.file_path, c.heading_path,
                snippet(chunks_fts, 0, '<b>', '</b>', '…', 20) AS snippet,
                bm25(chunks_fts) AS score
         FROM chunks_fts
         JOIN chunks c ON c.id = chunks_fts.rowid
         JOIN documents d ON d.id = c.document_id
         JOIN projects p ON p.id = d.project_id
         WHERE chunks_fts MATCH ?1
         {project_filter}
         ORDER BY score
         LIMIT ?2"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(query.text.clone()),
        Box::new(limit),
    ];
    for id in &query.project_ids {
        params.push(Box::new(*id));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let hits = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(SearchHit {
                document_id: row.get(0)?,
                chunk_id: row.get(1)?,
                title: row.get(2)?,
                file_path: row.get(3)?,
                heading_path: row.get(4)?,
                snippet: row.get(5)?,
                score: row.get::<_, f64>(6).unwrap_or(0.0).abs(),
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(hits)
}

fn vector_search_sync(
    conn: &Connection,
    emb_bytes: &[u8],
    limit: i64,
    project_ids: &[i64],
) -> Result<Vec<SearchHit>, SearchError> {
    let project_filter = if project_ids.is_empty() {
        "AND p.status = 'active'".to_string()
    } else {
        let placeholders: Vec<String> = (0..project_ids.len())
            .map(|i| format!("?{}", i + 3))
            .collect();
        format!("AND d.project_id IN ({})", placeholders.join(", "))
    };

    // vec0 KNN requires LIMIT on the virtual table query directly,
    // so we use a subquery to get candidate chunk_ids first, then join.
    let sql = format!(
        "SELECT c.id, c.document_id, d.title, d.file_path, c.heading_path, c.content, knn.distance
         FROM (
             SELECT chunk_id, distance FROM chunk_embeddings
             WHERE embedding MATCH ?1 AND k = ?2
         ) knn
         JOIN chunks c ON knn.chunk_id = c.id
         JOIN documents d ON d.id = c.document_id
         JOIN projects p ON p.id = d.project_id
         WHERE 1=1 {project_filter}
         ORDER BY knn.distance"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(emb_bytes.to_vec()),
        Box::new(limit),
    ];
    for id in project_ids {
        params.push(Box::new(*id));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let hits = stmt
        .query_map(param_refs.as_slice(), |row| {
            let distance: f64 = row.get(6)?;
            Ok(SearchHit {
                chunk_id: row.get(0)?,
                document_id: row.get(1)?,
                title: row.get(2)?,
                file_path: row.get(3)?,
                heading_path: row.get(4)?,
                snippet: row.get(5)?,
                score: 1.0 / (RRF_K as f64 + distance),
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(hits)
}

// ── SyncSearchEngine (backward-compatible wrapper) ───────────────────────────

/// Synchronous search engine that borrows a connection directly.
/// Used by callers that already hold a lock on the connection.
pub struct SyncSearchEngine<'a> {
    conn: &'a Connection,
}

impl<'a> SyncSearchEngine<'a> {
    /// Create a SyncSearchEngine from a borrowed connection.
    /// Preferred constructor; `SearchEngine::new` delegates here for backward compat.
    pub fn from_conn(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Index a document (sync, FTS only — no embedding).
    pub fn index_document(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), SearchError> {
        index_document_sync(self.conn, project_id, source_doc_id, title, content, None)
    }

    /// Convenience search: query string + options, returns `Hit` with RRF scoring.
    pub fn search_simple(
        &self,
        query: &str,
        opts: &SearchOpts,
    ) -> Result<Vec<Hit>, SearchError> {
        let limit = opts.limit.unwrap_or(20) as i64;

        let (project_filter, project_id_params): (String, Vec<i64>) = match &opts.project_ids {
            Some(ids) if !ids.is_empty() => {
                let placeholders: Vec<String> = (0..ids.len())
                    .map(|i| format!("?{}", i + 3))
                    .collect();
                (format!("AND d.project_id IN ({})", placeholders.join(", ")), ids.clone())
            }
            _ => ("AND p.status = 'active'".to_string(), vec![]),
        };

        let sql = format!(
            "SELECT d.id, d.project_id, d.source_doc_id, d.title,
                    snippet(chunks_fts, 0, '<b>', '</b>', '...', 20) AS snippet,
                    bm25(chunks_fts) AS fts_score
             FROM chunks_fts
             JOIN chunks c ON c.id = chunks_fts.rowid
             JOIN documents d ON d.id = c.document_id
             JOIN projects p ON p.id = d.project_id
             WHERE chunks_fts MATCH ?1
             {project_filter}
             ORDER BY fts_score
             LIMIT ?2"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(query.to_string()),
            Box::new(limit),
        ];
        for id in &project_id_params {
            params.push(Box::new(*id));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows: Vec<(i64, i64, String, Option<String>, String, f64)> = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get::<_, f64>(5).unwrap_or(0.0),
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut rrf_map: HashMap<i64, Hit> = HashMap::new();
        for (rank, (doc_id, project_id, source_doc_id, title, snippet, _fts_score)) in
            rows.into_iter().enumerate()
        {
            let entry = rrf_map.entry(doc_id).or_insert_with(|| Hit {
                document_id: doc_id,
                project_id,
                source_doc_id,
                title,
                snippet: Some(snippet),
                score: 0.0,
            });
            entry.score += rrf_score(rank + 1);
        }

        let mut hits: Vec<Hit> = rrf_map.into_values().collect();
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(hits)
    }

    /// FTS search using SearchQuery.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        match query.mode {
            SearchMode::Fts | SearchMode::Hybrid => fts_search_sync(self.conn, query),
            SearchMode::Vector => {
                // Vector-only search requires embedder — fall back to FTS
                fts_search_sync(self.conn, query)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    fn insert_project(db: &TestDb, name: &str, path: &str) -> i64 {
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES (?1, ?1, ?2, unixepoch(), unixepoch())",
                rusqlite::params![name, path],
            )
            .unwrap();
        db.conn
            .query_row(
                "SELECT id FROM projects WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap()
    }

    // ── Existing SearchQuery-based tests ────────────────────────────────

    fn insert_test_data(db: &TestDb) {
        let pid = insert_project(db, "vault", "/vault");
        db.conn
            .execute(
                "INSERT INTO documents(project_id, source_doc_id, title, content, content_hash)
                 VALUES (?1, 'doc1', 'Rust Programming', 'Rust is a systems language', 'h1')",
                [pid],
            )
            .unwrap();
        let did: i64 = db
            .conn
            .query_row("SELECT id FROM documents WHERE source_doc_id='doc1'", [], |r| r.get(0))
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO chunks(document_id, content, chunk_index)
                 VALUES (?1, 'Rust is a systems programming language focused on safety', 0)",
                [did],
            )
            .unwrap();
    }

    #[test]
    fn fts_search_returns_results() {
        let db = TestDb::new();
        insert_test_data(&db);

        let engine = SearchEngine::new(&db.conn);
        let query = SearchQuery::new("Rust programming");
        let hits = engine.search(&query).unwrap();
        assert!(!hits.is_empty(), "should find Rust document");
        assert_eq!(hits[0].title.as_deref(), Some("Rust Programming"));
    }

    #[test]
    fn fts_search_empty_returns_empty() {
        let db = TestDb::new();
        let engine = SearchEngine::new(&db.conn);
        let query = SearchQuery::new("xyzzy nonexistent");
        let hits = engine.search(&query).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_query_builder() {
        let q = SearchQuery::new("hello")
            .with_projects(vec![1, 2])
            .with_limit(5);
        assert_eq!(q.text, "hello");
        assert_eq!(q.project_ids, vec![1, 2]);
        assert_eq!(q.limit, 5);
    }

    // ── TDD: index_document + search_simple + RRF ───────────────────────

    #[test]
    fn search_returns_empty_for_no_documents() {
        let db = TestDb::new();
        let engine = SearchEngine::new(&db.conn);
        let hits = engine.search_simple("hello", &SearchOpts::default()).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_finds_indexed_document() {
        let db = TestDb::new();
        let pid = insert_project(&db, "test", "/tmp");
        db.conn
            .execute(
                "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed)
                 VALUES (?1, 'doc1', 'Hello World', 'hello world content', 'abc', 0)",
                [pid],
            )
            .unwrap();

        let engine = SearchEngine::new(&db.conn);
        engine.index_document(pid, "doc1", "Hello World", "hello world content").unwrap();
        let hits = engine.search_simple("hello", &SearchOpts::default()).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].title.as_deref(), Some("Hello World"));
    }

    #[test]
    fn search_ranks_by_rrf_score() {
        let db = TestDb::new();
        let pid = insert_project(&db, "p", "/tmp");
        let engine = SearchEngine::new(&db.conn);
        engine.index_document(pid, "d1", "Rust Programming", "rust language programming systems").unwrap();
        engine.index_document(pid, "d2", "Python Tutorial", "python tutorial beginner scripting").unwrap();
        let hits = engine.search_simple("rust programming", &SearchOpts::default()).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].source_doc_id, "d1");
    }

    #[test]
    fn search_respects_project_filter() {
        let db = TestDb::new();
        let p1 = insert_project(&db, "p1", "/a");
        let p2 = insert_project(&db, "p2", "/b");
        let engine = SearchEngine::new(&db.conn);
        engine.index_document(p1, "x", "X Doc", "unique xyzzy content").unwrap();
        engine.index_document(p2, "y", "Y Doc", "unique xyzzy content").unwrap();
        let opts = SearchOpts { project_ids: Some(vec![p1]), ..Default::default() };
        let hits = engine.search_simple("xyzzy", &opts).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_doc_id, "x");
    }

    // ── Async test helper ───────────────────────────────────────────────

    /// Build a fresh in-memory SearchEngine with embedder plus a project row.
    /// Returns `(engine, conn, project_id)` — conn is needed for direct DB assertions.
    fn make_async_engine(project_name: &str) -> (SearchEngine, Arc<Mutex<Connection>>, i64) {
        crate::db::ensure_vec_extension();
        let c = Connection::open_in_memory().unwrap();
        crate::db::apply_pragmas(&c).unwrap();
        crate::db::create_vec0_table(&c).unwrap();
        let migrations = &[
            include_str!("db/migrations/V1__initial_projects.sql"),
            include_str!("db/migrations/V2__documents.sql"),
            include_str!("db/migrations/V3__chunks_fts.sql"),
            include_str!("db/migrations/V5__graph.sql"),
            include_str!("db/migrations/V6__view_counts.sql"),
            include_str!("db/migrations/V7__plugins.sql"),
            include_str!("db/migrations/V8__workspace.sql"),
            include_str!("db/migrations/V9__workspace_content.sql"),
        ];
        for sql in migrations {
            c.execute_batch(sql).unwrap();
        }
        c.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at) \
             VALUES (?1, ?1, '/tmp', unixepoch(), unixepoch())",
            [project_name],
        ).unwrap();
        let project_id: i64 = c
            .query_row("SELECT id FROM projects WHERE name = ?1", [project_name], |r| r.get(0))
            .unwrap();

        let conn = Arc::new(Mutex::new(c));
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(crate::embedding::MockEmbedder::new(384));
        let engine = SearchEngine::with_embedder(Arc::clone(&conn), embedder);
        (engine, conn, project_id)
    }

    // ── TDD: async SearchEngine with embedder ───────────────────────────

    #[test]
    fn search_with_project_filter_uses_parameterized_query() {
        // Verify that project_id filtering works correctly with parameterized binding
        let db = TestDb::new();
        let p1 = insert_project(&db, "alpha", "/a");
        let p2 = insert_project(&db, "beta", "/b");
        let p3 = insert_project(&db, "gamma", "/c");
        let engine = SearchEngine::new(&db.conn);
        engine.index_document(p1, "a1", "Alpha Doc", "unique foobar content").unwrap();
        engine.index_document(p2, "b1", "Beta Doc", "unique foobar content").unwrap();
        engine.index_document(p3, "c1", "Gamma Doc", "unique foobar content").unwrap();

        // Filter to p1 and p3 only
        let opts = SearchOpts { project_ids: Some(vec![p1, p3]), ..Default::default() };
        let hits = engine.search_simple("foobar", &opts).unwrap();
        assert_eq!(hits.len(), 2);
        let ids: Vec<&str> = hits.iter().map(|h| h.source_doc_id.as_str()).collect();
        assert!(ids.contains(&"a1"));
        assert!(ids.contains(&"c1"));
        assert!(!ids.contains(&"b1"));
    }

    #[tokio::test]
    async fn index_document_stores_embedding() {
        let (engine, conn, pid) = make_async_engine("emb-test");

        engine
            .index_document_async(pid, "doc1", "Test", "hello world")
            .await
            .unwrap();

        // Verify embedding was stored
        let c = conn.lock().unwrap();
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM chunk_embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "embedding should be stored in chunk_embeddings");
    }

    #[tokio::test]
    async fn vector_search_returns_results() {
        let (engine, _conn, pid) = make_async_engine("vtest");

        // Index two documents
        engine.index_document_async(pid, "d1", "Doc One", "content one").await.unwrap();
        engine.index_document_async(pid, "d2", "Doc Two", "content two").await.unwrap();

        // Vector search
        let query = SearchQuery {
            text: "content".to_string(),
            project_ids: vec![],
            limit: 10,
            mode: SearchMode::Vector,
        };
        let hits = engine.search_async(&query).await.unwrap();
        assert_eq!(hits.len(), 2, "should find both documents via vector search");
    }

    #[tokio::test]
    async fn hybrid_search_merges_fts_and_vector() {
        let (engine, _conn, pid) = make_async_engine("htest");

        engine.index_document_async(pid, "h1", "Rust Guide", "rust programming language").await.unwrap();

        let query = SearchQuery::new("rust programming");
        let hits = engine.search_async(&query).await.unwrap();
        assert!(!hits.is_empty(), "hybrid search should return results");
    }
}
