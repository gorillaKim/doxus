pub use crate::db::schema::{BatchIndexingRequest, DocMeta, Hit, SearchHit};
use crate::embedding::{EmbeddingError, EmbeddingProvider};

use crate::search::highlighter::Highlighter;
use rusqlite::{params, Connection};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::Arc;
use std::collections::hash_map::Entry;
use tracing;
use thiserror::Error;

pub mod highlighter;

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
    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),
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
    pub offset: usize,
    pub mode: SearchMode,
    pub created_after: Option<i64>,
    pub created_before: Option<i64>,
    pub updated_after: Option<i64>,
    pub updated_before: Option<i64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Hybrid,
    Fts,
    Vector,
}

/// Parameters for indexing a document.
pub struct IndexParams<'a> {
    pub project_id: i64,
    pub source_doc_id: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub meta: DocMeta,
    pub strategy: &'a str,
    pub last_indexed: i64,
}

/// Parameters for synchronous indexing.
pub(crate) struct SyncIndexParams<'a> {
    pub project_id: i64,
    pub source_doc_id: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub chunks: &'a [crate::chunker::Chunk],
    pub chunk_embeddings: &'a [Vec<u8>],
    pub meta: &'a DocMeta,
    pub strategy: &'a str,
    pub last_indexed: i64,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            project_ids: vec![],
            limit: 20,
            offset: 0,
            mode: SearchMode::Hybrid,
            created_after: None,
            created_before: None,
            updated_after: None,
            updated_before: None,
            tags: vec![],
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_projects(mut self, ids: Vec<i64>) -> Self {
        self.project_ids = ids;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

#[derive(Debug, Default)]
pub struct SearchOpts {
    pub project_ids: Option<Vec<i64>>,
    pub limit: Option<usize>,
}

const RRF_K: usize = 60;
const VECTOR_MAX_L2_DISTANCE: f64 = 1.0;

fn sanitize_fts_token(token: &str) -> String {
    token
        .replace('"', "")
        .replace(['(', ')', '^', '~'], "")
        .replace('-', " ")
}

fn build_fts_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let escaped = sanitize_fts_token(trimmed);
    let mut parts: Vec<String> = vec![format!("\"{}\"", escaped)];

    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() > 1 {
        for word in &words {
            let w = word.trim();
            if !w.is_empty() {
                parts.push(format!("\"{}\"", sanitize_fts_token(w)));
            }
        }
    }

    if trimmed.contains('_') {
        for part in trimmed.split('_') {
            let p = part.trim();
            if !p.is_empty() {
                parts.push(format!("\"{}\"", sanitize_fts_token(p)));
            }
        }
    }

    if trimmed.chars().count() <= 3 {
        parts.push(format!("\"{}\"*", escaped));
    }

    parts.join(" OR ")
}

fn rrf_score(rank: usize) -> f64 {
    1.0 / (RRF_K + rank) as f64
}

fn rrf_merge(fts_hits: Vec<SearchHit>, vec_hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut scores: HashMap<i64, (f64, SearchHit)> = HashMap::new();

    for (rank, hit) in fts_hits.into_iter().enumerate() {
        match scores.entry(hit.chunk_id) {
            Entry::Vacant(v) => { v.insert((rrf_score(rank + 1), hit)); }
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                e.0 += rrf_score(rank + 1);
                if e.1.snippet.is_empty() && !hit.snippet.is_empty() { e.1.snippet = hit.snippet; }
            }
        }
    }
    for (rank, hit) in vec_hits.into_iter().enumerate() {
        match scores.entry(hit.chunk_id) {
            Entry::Vacant(v) => { v.insert((rrf_score(rank + 1), hit)); }
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                e.0 += rrf_score(rank + 1);
                if e.1.snippet.is_empty() && !hit.snippet.is_empty() { e.1.snippet = hit.snippet; }
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

impl From<SearchHit> for Hit {
    fn from(sh: SearchHit) -> Self {
        Hit {
            document_id: sh.document_id,
            chunk_id: sh.chunk_id,
            project_id: sh.project_id,
            source_doc_id: sh.source_doc_id,
            title: sh.title,
            file_path: sh.file_path,
            url: sh.url,
            heading_path: sh.heading_path,
            snippet: Some(sh.snippet),
            context_content: sh.context_content,
            metadata_json: sh.metadata_json,
            last_indexed: sh.last_indexed,
            project_name: sh.project_name,
            score: sh.score,
            created_at: sh.created_at,
            updated_at: sh.updated_at,
            tags: sh.tags,
            summary: sh.summary,
        }
    }
}

pub struct SearchEngine {
    conn: crate::db::DbPool,
    embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
}

impl SearchEngine {
    pub fn with_embedder(conn: crate::db::DbPool, embedder: Arc<dyn EmbeddingProvider + Send + Sync>) -> Self {
        Self { conn, embedder }
    }

    pub async fn rebuild_vector_table(&self) -> Result<(), SearchError> {
        let dim = self.embedder.dimension();
        let conn = self.conn.write_conn()?;
        conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS chunk_embeddings;
             CREATE VIRTUAL TABLE chunk_embeddings USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                vector int8[{}]
             );", dim
        ))?;
        Ok(())
    }

    pub fn sync(conn: &Connection) -> SyncSearchEngine<'_> {
        SyncSearchEngine::from_conn(conn)
    }

    pub fn new_fts_only(pool: crate::db::DbPool) -> Self {
        Self {
            conn: pool,
            embedder: Arc::new(crate::embedding::NoOpEmbedder) as Arc<dyn EmbeddingProvider + Send + Sync>,
        }
    }

    pub async fn index_document_async(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
        strategy: &str,
    ) -> Result<(), SearchError> {
        let now = chrono::Utc::now().timestamp();
        self.index_document_async_with_meta(IndexParams {
            project_id,
            source_doc_id,
            title,
            content,
            meta: DocMeta::default(),
            strategy,
            last_indexed: now,
        }).await
    }

    pub async fn index_documents_batch_async(
        &self,
        mut requests: Vec<BatchIndexingRequest>,
        last_indexed: i64,
    ) -> Result<(), SearchError> {
        if requests.is_empty() { return Ok(()); }

        // Process in smaller sub-batches (5 documents) to keep memory footprint extremely low.
        // This prevents massive memory spikes (10GB+) during ONNX inference for large files.
        //
        // Memory notes:
        //  - We drain() sub-batches out of the input Vec so we never hold the full input twice.
        //  - After embeddings are produced, the text-only buffer (`sub_flat_texts`) is dropped
        //    before spawn_blocking so the peak is not doubled across the await boundary.
        //  - spawn_blocking only receives owned chunks, embeddings, and a lean document vec
        //    (no redundant content copy beyond what the DB write itself needs).
        const SUB_BATCH: usize = 5;
        while !requests.is_empty() {
            let take = SUB_BATCH.min(requests.len());
            let mut sub_batch: Vec<BatchIndexingRequest> = requests.drain(..take).collect();
            for req in &mut sub_batch {
                req.content = clean_markdown(&req.content);
            }

            // Chunk each document and build the flat embedding-text list used only for embed().
            let mut sub_chunks: Vec<crate::chunker::Chunk> = Vec::new();
            let mut sub_chunk_counts: Vec<usize> = Vec::with_capacity(sub_batch.len());
            {
                let mut sub_flat_texts: Vec<String> = Vec::new();
                for req in &sub_batch {
                    let chunks = crate::chunker::split_chunks(
                        &req.content,
                        crate::chunker::ChunkConfig {
                            title: Some(req.title.clone()),
                            ..Default::default()
                        },
                    );
                    sub_chunk_counts.push(chunks.len());
                    for chunk in chunks {
                        sub_flat_texts.push(chunk.embedding_text.clone());
                        sub_chunks.push(chunk);
                    }
                }

                let mut sub_flat_embeddings: Vec<Vec<u8>> = Vec::new();
                if !sub_flat_texts.is_empty() {
                let text_refs: Vec<&str> = sub_flat_texts.iter().map(|s| s.as_str()).collect();
                let embedding_vecs = self.embedder.embed(&text_refs).await?;
                // Explicitly drop the text buffers before spawn_blocking to avoid 2x peak.
                drop(text_refs);
                drop(sub_flat_texts);

                sub_flat_embeddings = Vec::with_capacity(embedding_vecs.len());
                for emb in embedding_vecs {
                    let quantized = crate::embedding::quantize_to_i8(&emb);
                    let bytes: Vec<u8> = quantized.into_iter().map(|i| i as u8).collect();
                    sub_flat_embeddings.push(bytes);
                }
                }

                let pool = self.conn.clone();
                let sub_chunk_counts_moved = std::mem::take(&mut sub_chunk_counts);
                let sub_chunks_moved = std::mem::take(&mut sub_chunks);

                tokio::task::spawn_blocking(move || -> Result<(), SearchError> {
                    let mut conn = pool.write_conn()?;
                    let tx = conn.transaction()?;

                    let mut current_chunk_offset = 0;
                    for (doc_idx, req) in sub_batch.into_iter().enumerate() {
                        let num_chunks = sub_chunk_counts_moved[doc_idx];
                        let doc_chunks = &sub_chunks_moved[current_chunk_offset..current_chunk_offset + num_chunks];
                        let doc_embeddings = &sub_flat_embeddings[current_chunk_offset..current_chunk_offset + num_chunks];

                        let strategy: String = tx
                            .query_row(
                                "SELECT storage_strategy FROM projects WHERE id = ?1",
                                params![req.project_id],
                                |r| r.get(0),
                            )
                            .unwrap_or_else(|_| "full".to_string());

                        index_document_sync(
                            &tx,
                            SyncIndexParams {
                                project_id: req.project_id,
                                source_doc_id: &req.source_doc_id,
                                title: &req.title,
                                content: &req.content,
                                chunks: doc_chunks,
                                chunk_embeddings: doc_embeddings,
                                meta: &req.meta,
                                strategy: &strategy,
                                last_indexed,
                            },
                        )?;
                        current_chunk_offset += num_chunks;
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await??;
            }
        }

        // Single WAL checkpoint after all sub-batches complete — avoids per-batch
        // lock contention and blocking the conn for multiple checkpoint cycles.
        let pool = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            if let Ok(c) = pool.get() {
                let _ = c.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
                let _ = c.execute_batch("PRAGMA shrink_memory;");
            }
        })
        .await
        .ok();

        Ok(())
    }

    pub async fn index_document_async_with_meta(
        &self,
        params: IndexParams<'_>,
    ) -> Result<(), SearchError> {
        let strategy = params.strategy.to_string();
        let sanitized = clean_markdown(params.content);
        let chunks = crate::chunker::split_chunks(&sanitized, crate::chunker::ChunkConfig { title: Some(params.title.to_string()), ..Default::default() });
        if chunks.is_empty() { return Ok(()); }

        let texts: Vec<&str> = chunks.iter().map(|c| c.embedding_text.as_str()).collect();
        let embedding_vecs = self.embedder.embed(&texts).await.unwrap_or_default();
        let mut chunk_embeddings = Vec::new();
        for emb in embedding_vecs {
            let quantized = crate::embedding::quantize_to_i8(&emb);
            let bytes: Vec<u8> = quantized.into_iter().map(|i| i as u8).collect();
            chunk_embeddings.push(bytes);
        }

        let pool = self.conn.clone();
        let source_doc_id = params.source_doc_id.to_string();
        let title = params.title.to_string();
        let content = sanitized;
        let project_id = params.project_id;
        let last_indexed = params.last_indexed;
        let meta = params.meta;

        tokio::task::spawn_blocking(move || -> Result<(), SearchError> {
            let conn = pool.write_conn()?;
            index_document_sync(
                &conn,
                SyncIndexParams {
                    project_id,
                    source_doc_id: &source_doc_id,
                    title: &title,
                    content: &content,
                    chunks: &chunks,
                    chunk_embeddings: &chunk_embeddings,
                    meta: &meta,
                    strategy: &strategy,
                    last_indexed,
                },
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    pub async fn search_async(&self, query: &SearchQuery) -> Result<Vec<Hit>, SearchError> {
        let hits: Vec<SearchHit> = match query.mode {
            SearchMode::Fts => self.fts_search_async(query).await?,
            SearchMode::Vector => self.vector_search_async(query).await?,
            SearchMode::Hybrid => {
                let fts_hits = self.fts_search_async(query).await?;
                if !query.text.trim().is_empty() {
                    let vec_hits = self.vector_search_async(query).await.unwrap_or_default();
                    rrf_merge(fts_hits, vec_hits)
                } else {
                    fts_hits
                }
            }
        };

        let pool = self.conn.clone();
        let query_clone = query.clone();
        
        tokio::task::spawn_blocking(move || {
            let conn = pool.read_conn()?;
            let mut paged_hits: Vec<Hit> = hits.into_iter()
                .skip(query_clone.offset)
                .take(query_clone.limit)
                .map(Hit::from)
                .collect();
            
            if paged_hits.is_empty() { return Ok(paged_hits); }

            let scores: Vec<f64> = paged_hits.iter().map(|h| h.score).collect();
            let n = scores.len() as f64;
            let mean = scores.iter().sum::<f64>() / n;
            let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
            let sigma = variance.sqrt();
            let max_score = scores[0];

            let mut loaded_sections = HashMap::new();
            let mut total_chars = 0;
            const GLOBAL_CEILING: usize = 15000;

            for hit in paged_hits.iter_mut() {
                if total_chars >= GLOBAL_CEILING { break; }
                let is_high_confidence = hit.score >= (max_score - sigma);
                if is_high_confidence {
                    assemble_context_sync(&conn, hit, &mut loaded_sections, &mut total_chars, GLOBAL_CEILING)?;
                } else {
                    hit.context_content = hit.snippet.clone();
                    if let Some(ref s) = hit.context_content {
                        total_chars += s.len();
                    }
                }
            }
            Ok(paged_hits)
        }).await?
    }

    async fn fts_search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let pool = self.conn.clone();
        let query_clone = query.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.read_conn()?;
            fts_search_sync(&conn, &query_clone)
        }).await?
    }

    async fn vector_search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        if query.text.trim().is_empty() {
             return Ok(vec![]);
        }
        let embedding = self.embedder.embed(&[query.text.as_str()]).await?;
        let emb = embedding.into_iter().next().ok_or_else(|| SearchError::Embedding("empty".into()))?;
        let quantized = crate::embedding::quantize_to_i8(&emb);
        let emb_bytes: Vec<u8> = quantized.into_iter().map(|i| i as u8).collect();

        let pool = self.conn.clone();
        let query_clone = query.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.read_conn()?;
            vector_search_sync(&conn, &emb_bytes, &query_clone)
        }).await?
    }
}

pub(crate) fn index_document_sync(
    conn: &Connection,
    params: SyncIndexParams<'_>,
) -> Result<(), SearchError> {
    let project_id = params.project_id;
    let source_doc_id = params.source_doc_id;
    let title = params.title;
    let content = params.content;
    let meta = params.meta;
    let strategy = params.strategy;
    let chunks = params.chunks;
    let chunk_embeddings = params.chunk_embeddings;
    let last_indexed = params.last_indexed;
    let is_reference = strategy == "reference";
    // Even if content is empty, we should persist the document record to avoid repeated re-indexing.
    // We only skip if both content AND title are effectively empty.
    if content.trim().is_empty() && title.trim().is_empty() { 
        tracing::debug!("[Search-Engine][index] Skipping empty document: source_doc_id={}", source_doc_id);
        return Ok(()); 
    }

    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let metadata_json = serde_json::to_string(&meta.metadata).unwrap_or_else(|_| "{}".to_string());
    
    let now = chrono::Utc::now().timestamp();
    let created_at = meta.created_at.unwrap_or(now);
    let updated_at = meta.updated_at.unwrap_or(now);

    let project_path: Option<String> = conn.query_row("SELECT path FROM projects WHERE id = ?1", [project_id], |r| r.get(0)).ok();

    let full_file_path = if let (Some(base), Some(rel)) = (project_path, &meta.relative_path) {
        if base.starts_with("http") { Some(rel.clone()) }
        else {
            let path = std::path::PathBuf::from(base).join(rel);
            // Only materialize a cache file when the source file is not already on disk.
            // Obsidian vaults (and similar local sources) already own the file — overwriting
            // on every index run wastes I/O, pollutes the OS page cache, and can clobber
            // in-flight edits. For purely in-memory sources, we still cache so that
            // snippet/highlight paths can read back the content by byte range.
            if !path.exists() {
                if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
                let _ = std::fs::write(&path, content);
            }
            Some(path.to_string_lossy().to_string())
        }
    } else { None };

    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, title, url, content_hash, last_indexed, created_at, updated_at, metadata_json, file_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?10, ?6, ?7, ?8, ?9)
         ON CONFLICT(project_id, source_doc_id) DO UPDATE SET
            last_indexed = excluded.last_indexed,
            title = excluded.title, url = excluded.url, content_hash = excluded.content_hash,
            created_at = COALESCE(documents.created_at, excluded.created_at),
            updated_at = COALESCE(excluded.updated_at, documents.updated_at),
            metadata_json = excluded.metadata_json, file_path = COALESCE(excluded.file_path, documents.file_path)",
        params![project_id, source_doc_id, title, meta.url, content_hash, created_at, updated_at, metadata_json, full_file_path, last_indexed],
    )?;

    let doc_id: i64 = conn.query_row("SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2", params![project_id, source_doc_id], |row| row.get(0))?;

    conn.execute("DELETE FROM document_tags WHERE document_id = ?1", [doc_id])?;
    for tag in &meta.tags { conn.execute("INSERT OR IGNORE INTO document_tags (document_id, tag) VALUES (?1, ?2)", params![doc_id, tag])?; }
    conn.execute("DELETE FROM document_aliases WHERE document_id = ?1", [doc_id])?;
    for alias in &meta.aliases { conn.execute("INSERT OR IGNORE INTO document_aliases (document_id, alias) VALUES (?1, ?2)", params![doc_id, alias])?; }
    conn.execute("DELETE FROM document_metadata WHERE document_id = ?1", [doc_id])?;
    for (k, v) in &meta.metadata { conn.execute("INSERT OR REPLACE INTO document_metadata (document_id, key, value) VALUES (?1, ?2, ?3)", params![doc_id, k, v.to_string()])?; }

    conn.execute("DELETE FROM chunks WHERE document_id = ?1", [doc_id])?;
    for (i, chunk) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index, heading_path, start_byte, end_byte) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![doc_id, chunk.content, chunk.index as i64, chunk.heading_path, chunk.start_byte as i64, chunk.end_byte as i64],
        )?;
        let chunk_id: i64 = conn.last_insert_rowid();
        if is_reference { conn.execute("UPDATE chunks SET content = NULL WHERE id = ?1", [chunk_id])?; }
        if let Some(bytes) = chunk_embeddings.get(i) {
            conn.execute("INSERT OR REPLACE INTO chunk_embeddings(chunk_id, vector) VALUES (?1, vec_int8(?2))", params![chunk_id, bytes])?;
        }
    }
    Ok(())
}

fn fts_search_sync(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
    let fts_query = build_fts_query(&query.text);
    if fts_query.is_empty() && query.tags.is_empty() && query.project_ids.is_empty() {
        return Ok(vec![]);
    }

    let mut next_param = 1usize;
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 1. Core Selection Logic
    let (from_clause, match_clause, score_expr, order_by) = if !fts_query.is_empty() {
        let q = fts_query.clone();
        params_vec.push(Box::new(q));
        (
            "FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.rowid",
            format!("chunks_fts MATCH ?{}", next_param),
            "ABS(bm25(chunks_fts, 1.0, 3.0, 10.0))",
            "ORDER BY score DESC"
        )
    } else {
        (
            // Metadata-only: Group by document to avoid redundant chunk hits
            "FROM (SELECT id, document_id, content, heading_path, start_byte, end_byte FROM chunks GROUP BY document_id) c",
            "1=1".to_string(),
            "1.0",
            "ORDER BY d.updated_at DESC" // Stable default order for metadata search
        )
    };
    if !fts_query.is_empty() { next_param += 1; }

    let limit_val = (query.limit + query.offset) as i64;
    params_vec.push(Box::new(limit_val));
    let limit_param_idx = next_param;
    next_param += 1;

    // 2. Filter: Projects
    let (project_filter, projects_next_param) = if query.project_ids.is_empty() {
        ("AND p.status = 'active'".to_string(), next_param)
    } else {
        let start = next_param;
        let placeholders: Vec<String> = (0..query.project_ids.len())
            .map(|i| format!("?{}", start + i))
            .collect();
        for id in &query.project_ids { params_vec.push(Box::new(*id)); }
        (format!("AND d.project_id IN ({})", placeholders.join(", ")), next_param + query.project_ids.len())
    };
    next_param = projects_next_param;

    // 3. Filter: Tags (Stable OR logic with Case-Insensitivity)
    let mut tag_filter = String::new();
    if !query.tags.is_empty() {
        let start = next_param;
        let placeholders: Vec<String> = (0..query.tags.len())
            .map(|i| format!("?{}", start + i))
            .collect();
        for tag in &query.tags { params_vec.push(Box::new(tag.to_string())); }
        
        // Use a JOIN-based subquery for better SQLite optimization
        tag_filter = format!(
            "AND d.id IN (SELECT document_id FROM document_tags WHERE tag IN ({}) COLLATE NOCASE)",
            placeholders.join(", ")
        );
        next_param += query.tags.len();
    }

    // 4. Filter: Dates
    let mut date_filter = String::new();
    if let Some(v) = query.created_after  { date_filter.push_str(&format!(" AND d.created_at >= ?{}", next_param)); params_vec.push(Box::new(v)); next_param += 1; }
    if let Some(v) = query.created_before { date_filter.push_str(&format!(" AND d.created_at <= ?{}", next_param)); params_vec.push(Box::new(v)); next_param += 1; }
    if let Some(v) = query.updated_after  { date_filter.push_str(&format!(" AND d.updated_at >= ?{}", next_param)); params_vec.push(Box::new(v)); next_param += 1; }
    if let Some(v) = query.updated_before { date_filter.push_str(&format!(" AND d.updated_at <= ?{}", next_param)); params_vec.push(Box::new(v)); }

    let sql = format!(
        "SELECT d.id, d.source_doc_id, d.project_id, c.id, d.title, COALESCE(d.file_path, d.source_doc_id), c.heading_path,
                '' AS snippet, ({}) AS score,
                d.url, d.metadata_json, d.last_indexed,
                c.start_byte, c.end_byte, c.content,
                d.created_at, d.updated_at,
                (SELECT GROUP_CONCAT(tag) FROM document_tags WHERE document_id = d.id) as tags,
                p.name AS project_name,
                d.summary
         {}
         JOIN documents d ON d.id = c.document_id
         JOIN projects p ON p.id = d.project_id
         WHERE {}
         {project_filter}
         {date_filter}
         {tag_filter}
         {}
         LIMIT ?{}",
        score_expr, from_clause, match_clause, order_by, limit_param_idx
    );

    let keywords: Vec<String> = query.text.split_whitespace().map(|s| s.to_string()).collect();
    let highlighter = Highlighter::new(&keywords).unwrap_or_else(|_| Highlighter::new(&[]).unwrap());

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let hits = stmt.query_map(param_refs.as_slice(), |row| {
        let tags_str: Option<String> = row.get(17)?;
        let tags = tags_str.map(|s| s.split(',').map(|t| t.to_string()).collect()).unwrap_or_default();

        let mut hit = SearchHit {
            document_id: row.get(0)?,
            source_doc_id: row.get(1)?,
            project_id: row.get(2)?,
            chunk_id: row.get(3)?,
            title: row.get(4)?,
            file_path: row.get(5)?,
            heading_path: row.get(6)?,
            snippet: row.get(7)?,
            score: row.get::<_, f64>(8).unwrap_or(0.0),
            url: row.get(9)?,
            metadata_json: row.get(10)?,
            last_indexed: row.get(11)?,
            start_byte: row.get(12)?,
            end_byte: row.get(13)?,
            raw_content: row.get(14)?,
            context_content: None,
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
            tags,
            project_name: row.get(18).ok(),
            summary: row.get(19)?,
        };

        if let Some(ref path_str) = hit.file_path {
            let path = std::path::Path::new(path_str);
            if path.exists() {
                if let (Some(start), Some(end)) = (hit.start_byte, hit.end_byte) {
                    if let Ok(res) = highlighter.highlight_file(path, start as usize, end as usize, 50) {
                        hit.snippet = res.snippet;
                    }
                }
            } else if let Some(ref raw) = hit.raw_content {
                hit.snippet = highlighter.highlight_text(raw).snippet;
            }
        } else if let Some(ref raw) = hit.raw_content {
            hit.snippet = highlighter.highlight_text(raw).snippet;
        }

        Ok(hit)
    })?.collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(hits)
}

fn vector_search_sync(
    conn: &Connection,
    emb_bytes: &[u8],
    query: &SearchQuery,
) -> Result<Vec<SearchHit>, SearchError> {
    let k = (query.limit + query.offset) as i64;
    let mut next_param = 3usize;
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(emb_bytes.to_vec()), Box::new(k)];

    let (project_filter, projects_next_param) = if query.project_ids.is_empty() {
        ("AND p.status = 'active'".to_string(), next_param)
    } else {
        let start = next_param;
        let placeholders: Vec<String> = (0..query.project_ids.len()).map(|i| format!("?{}", start + i)).collect();
        for id in &query.project_ids { params_vec.push(Box::new(*id)); }
        (format!("AND d.project_id IN ({})", placeholders.join(", ")), next_param + query.project_ids.len())
    };
    next_param = projects_next_param;

    let mut tag_filter = String::new();
    if !query.tags.is_empty() {
        let start = next_param;
        let placeholders: Vec<String> = (0..query.tags.len()).map(|i| format!("?{}", start + i)).collect();
        for tag in &query.tags { params_vec.push(Box::new(tag.to_string())); }
        tag_filter = format!(
            "AND d.id IN (SELECT document_id FROM document_tags WHERE tag IN ({}))",
            placeholders.join(", ")
        );
    }

    let sql = format!(
        "SELECT c.id, c.document_id, d.project_id, d.source_doc_id, d.title, COALESCE(d.file_path, d.source_doc_id), c.heading_path, c.content, knn.distance, d.url, d.metadata_json, d.last_indexed, c.start_byte, c.end_byte, d.created_at, d.updated_at,
                (SELECT GROUP_CONCAT(tag) FROM document_tags WHERE document_id = d.id) as tags,
                p.name AS project_name,
                d.summary
         FROM (
             SELECT chunk_id, distance FROM chunk_embeddings
             WHERE vector MATCH vec_int8(?1) AND k = ?2
         ) knn
         JOIN chunks c ON knn.chunk_id = c.id
         JOIN documents d ON d.id = c.document_id
         JOIN projects p ON p.id = d.project_id
         WHERE 1=1 {project_filter} {tag_filter}
         ORDER BY knn.distance"
    );

    let keywords: Vec<String> = query.text.split_whitespace().map(|s| s.to_string()).collect();
    let highlighter = Highlighter::new(&keywords).unwrap_or_else(|_| Highlighter::new(&[]).unwrap());

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    
    let hits = stmt.query_map(param_refs.as_slice(), |row| {
        let distance: f64 = row.get(8)?;
        let tags_str: Option<String> = row.get(16)?;
        let tags = tags_str.map(|s| s.split(',').map(|t| t.to_string()).collect()).unwrap_or_default();

        let mut hit = SearchHit {
            chunk_id: row.get(0)?,
            document_id: row.get(1)?,
            project_id: row.get(2)?,
            source_doc_id: row.get(3)?,
            title: row.get(4)?,
            file_path: row.get(5)?,
            heading_path: row.get(6)?,
            url: row.get(9)?,
            snippet: String::new(),
            score: 1.0 / (RRF_K as f64 + distance),
            metadata_json: row.get(10)?,
            last_indexed: row.get(11)?,
            start_byte: row.get(12)?,
            end_byte: row.get(13)?,
            raw_content: row.get(7)?,
            context_content: None,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
            tags,
            project_name: row.get(17).ok(),
            summary: row.get(18)?,
        };

        if let Some(ref path_str) = hit.file_path {
            let path = std::path::Path::new(path_str);
            if path.exists() {
                if let (Some(start), Some(end)) = (hit.start_byte, hit.end_byte) {
                    if let Ok(res) = highlighter.highlight_file(path, start as usize, end as usize, 50) {
                        hit.snippet = res.snippet;
                    }
                }
            } else if let Some(ref raw) = hit.raw_content {
                hit.snippet = highlighter.highlight_text(raw).snippet;
            }
        } else if let Some(ref raw) = hit.raw_content {
            hit.snippet = highlighter.highlight_text(raw).snippet;
        }

        Ok(hit)
    })?.collect::<Result<Vec<_>, rusqlite::Error>>()?;

    let mut hits = hits;
    hits.retain(|h| {
        if h.score <= 0.0 { return false; }
        let distance = (1.0 / h.score) - RRF_K as f64;
        distance <= VECTOR_MAX_L2_DISTANCE
    });

    Ok(hits)
}

fn assemble_context_sync(
    conn: &Connection,
    hit: &mut Hit,
    loaded_sections: &mut HashMap<(i64, Option<String>), String>,
    total_chars: &mut usize,
    _global_ceiling: usize,
) -> Result<(), SearchError> {
    let key = (hit.document_id, hit.heading_path.clone());
    if let Some(cached) = loaded_sections.get(&key) {
        hit.context_content = Some(cached.clone());
        return Ok(());
    }

    let (_target_idx, target_content, start_byte, end_byte): (i32, Option<String>, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT chunk_index, content, start_byte, end_byte FROM chunks WHERE id = ?1",
        [hit.chunk_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;

    let actual_content = if let Some(c) = target_content { c }
    else if let (Some(path_str), Some(start), Some(end)) = (hit.file_path.as_ref(), start_byte, end_byte) {
        let path = std::path::Path::new(path_str);
        if path.exists() {
            let mut file = std::fs::File::open(path).map_err(|e| SearchError::Join(e.to_string()))?;
            use std::io::{Seek, SeekFrom, Read};
            file.seek(SeekFrom::Start(start as u64)).map_err(|e| SearchError::Join(e.to_string()))?;
            let mut buf = vec![0u8; (end - start) as usize];
            file.read_exact(&mut buf).map_err(|e| SearchError::Join(e.to_string()))?;
            String::from_utf8_lossy(&buf).to_string()
        } else { "[File not found]".to_string() }
    } else { "[Content unavailable]".to_string() };

    hit.context_content = Some(actual_content.clone());
    *total_chars += actual_content.len();
    loaded_sections.insert(key, actual_content);

    Ok(())
}

pub struct SyncSearchEngine<'a> { conn: &'a Connection }
impl<'a> SyncSearchEngine<'a> {
    pub fn from_conn(conn: &'a Connection) -> Self { Self { conn } }
    pub fn index_document(&self, project_id: i64, sid: &str, title: &str, content: &str, strategy: &str) -> Result<(), SearchError> {
        self.index_document_with_meta(project_id, sid, title, content, &DocMeta::default(), strategy)
    }
    pub fn index_document_with_meta(&self, project_id: i64, sid: &str, title: &str, content: &str, meta: &DocMeta, strategy: &str) -> Result<(), SearchError> {
        let sanitized = clean_markdown(content);
        let chunks = crate::chunker::split_chunks(&sanitized, crate::chunker::ChunkConfig { title: Some(title.to_string()), ..Default::default() });
        let now = chrono::Utc::now().timestamp();
        let meta = DocMeta {
            created_at: meta.created_at.or(Some(now)),
            updated_at: meta.updated_at.or(Some(now)),
            ..meta.clone()
        };
        index_document_sync(
            self.conn,
            SyncIndexParams {
                project_id,
                source_doc_id: sid,
                title,
                content: &sanitized,
                chunks: &chunks,
                chunk_embeddings: &[],
                meta: &meta,
                strategy,
                last_indexed: now,
            },
        )
    }
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        fts_search_sync(self.conn, query)
    }
}

#[allow(clippy::while_let_on_iterator)]
fn clean_markdown(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '<' && chars.peek() == Some(&'!') {
            let mut temp = chars.clone();
            temp.next(); // '!'
            if temp.next() == Some('-') && temp.next() == Some('-') {
                chars.next(); // '!'
                chars.next(); // '-'
                chars.next(); // '-'
                
                let mut hyphen_count = 0;
                while let Some(nc) = chars.next() {
                    if nc == '-' {
                        hyphen_count += 1;
                    } else if nc == '>' && hyphen_count >= 2 {
                        break;
                    } else {
                        hyphen_count = 0;
                    }
                }
                continue;
            }
        }
        result.push(c);
    }
    
    let mut cleaned = String::with_capacity(result.len());
    let mut last_was_space = false;
    for c in result.chars() {
        if c == ' ' || c == '\t' {
            if !last_was_space {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(c);
            last_was_space = false;
        }
    }
    cleaned
}
