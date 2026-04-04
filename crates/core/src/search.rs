use crate::db::schema::SearchHit;
use rusqlite::Connection;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("embedding failed: {0}")]
    Embedding(String),
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

pub struct SearchEngine<'a> {
    conn: &'a Connection,
}

impl<'a> SearchEngine<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Index a document: insert/replace into documents and chunks tables.
    /// The FTS5 triggers will keep chunks_fts in sync automatically.
    pub fn index_document(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), SearchError> {
        // Upsert document
        self.conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed)
             VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())
             ON CONFLICT(project_id, source_doc_id) DO UPDATE SET
                title = excluded.title,
                content = excluded.content,
                content_hash = excluded.content_hash,
                last_indexed = excluded.last_indexed",
            rusqlite::params![project_id, source_doc_id, title, content, content],
        )?;

        let doc_id: i64 = self.conn.query_row(
            "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
            rusqlite::params![project_id, source_doc_id],
            |row| row.get(0),
        )?;

        // Delete old chunks (triggers handle FTS cleanup)
        self.conn.execute(
            "DELETE FROM chunks WHERE document_id = ?1",
            [doc_id],
        )?;

        // Insert single chunk (whole content)
        self.conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index)
             VALUES (?1, ?2, 0)",
            rusqlite::params![doc_id, content],
        )?;

        Ok(())
    }

    /// Convenience search: query string + options, returns `Hit` with RRF scoring.
    pub fn search_simple(
        &self,
        query: &str,
        opts: &SearchOpts,
    ) -> Result<Vec<Hit>, SearchError> {
        let limit = opts.limit.unwrap_or(20) as i64;

        let project_filter = match &opts.project_ids {
            Some(ids) if !ids.is_empty() => {
                let id_list: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
                format!("AND d.project_id IN ({})", id_list.join(","))
            }
            _ => "AND p.status = 'active'".to_string(),
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
        let rows: Vec<(i64, i64, String, Option<String>, String, f64)> = stmt
            .query_map(rusqlite::params![query, limit], |row| {
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

        // Apply RRF scoring (currently FTS-only; vector rank will be merged here later)
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

    /// Hybrid search: FTS5 + (optionally) vector similarity, merged via RRF.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        match query.mode {
            SearchMode::Fts | SearchMode::Hybrid => self.fts_search(query),
            SearchMode::Vector => {
                // Vector-only search requires sqlite-vec extension
                // Fall back to FTS for now
                self.fts_search(query)
            }
        }
    }

    fn fts_search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let limit = query.limit as i64;

        // Build project filter
        let project_filter = if query.project_ids.is_empty() {
            // Only search active projects
            "AND p.status = 'active'".to_string()
        } else {
            let ids: Vec<String> = query.project_ids.iter().map(|id| id.to_string()).collect();
            format!("AND d.project_id IN ({})", ids.join(","))
        };

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

        let mut stmt = self.conn.prepare(&sql)?;
        let hits = stmt
            .query_map([query.text.as_str(), &limit.to_string()], |row| {
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
            .filter_map(|r| r.ok())
            .collect();

        Ok(hits)
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
}
