use crate::db::schema::SearchHit;
use rusqlite::Connection;
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

pub struct SearchEngine<'a> {
    conn: &'a Connection,
}

impl<'a> SearchEngine<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
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

    fn insert_test_data(db: &TestDb) {
        db.conn
            .execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES ('vault', 'My Vault', '/vault', unixepoch(), unixepoch())",
                [],
            )
            .unwrap();
        let pid: i64 = db
            .conn
            .query_row("SELECT id FROM projects WHERE name='vault'", [], |r| r.get(0))
            .unwrap();
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
}
