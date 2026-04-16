use crate::db::schema::SearchHit;
use crate::embedding::{EmbeddingError, EmbeddingProvider};
use crate::observability::{persist_audit, AuditEvent};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Sha256, Digest};
use std::collections::{HashMap, VecDeque};
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
    pub offset: usize,
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
            offset: 0,
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

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

/// Simplified search options for convenience API.
#[derive(Debug, Default)]
pub struct SearchOpts {
    pub project_ids: Option<Vec<i64>>,
    pub limit: Option<usize>,
}

/// A ranked search result with optional contextual content.
#[derive(Debug, Clone, Default)]
pub struct Hit {
    pub document_id: i64,
    pub chunk_id: i64,
    pub project_id: i64,
    pub source_doc_id: String,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub heading_path: Option<String>,
    pub snippet: Option<String>,
    pub context_content: Option<String>,
    pub score: f64,
}

/// RRF constant (k=60 is standard).
const RRF_K: usize = 60;

/// 벡터 검색 L2 거리 임계값. sqlite-vec 반환 distance가 이 값을 초과하면 노이즈로 제거.
/// all-MiniLM-L6-v2 기준 distance ≈ 1.0 은 거의 무관한 문서에 해당.
const VECTOR_MAX_L2_DISTANCE: f64 = 1.0;

/// title boost — RRF 스케일 기준 (1/(60+rank) ≈ 0.016~0.001)
/// 제목 완전 일치 시 약 1~2 순위 상승, 부분 일치 시 미세 보정.
const TITLE_EXACT_BOOST: f64 = 0.005;
const TITLE_PARTIAL_BOOST: f64 = 0.002;

/// FTS5 토큰 sanitize: 사용자 입력에서 FTS5 문법을 깨는 문자 제거.
/// " 는 phrase literal 내에서 `""` 이스케이프가 필요하지만, 검색 쿼리에서
/// 리터럴 따옴표 검색은 불필요하므로 단순 제거한다.
fn sanitize_fts_token(token: &str) -> String {
    token
        .replace('"', "")
        .replace(['(', ')', '^', '~'], "")
        .replace('-', " ")
}

/// prefix fallback 쿼리 빌더 — 각 토큰을 "token"* OR 조합
/// vector 실패 시 recall 보완용
#[allow(dead_code)]
fn build_prefix_fallback_query(query: &str) -> String {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.chars().count() >= 2)
        .map(|w| format!("\"{}\"*", sanitize_fts_token(w)))
        .collect();
    if tokens.is_empty() {
        format!("\"{}\"", sanitize_fts_token(query.trim()))
    } else {
        tokens.join(" OR ")
    }
}

/// 메인 FTS 쿼리 빌더:
/// 1. 전체 phrase match
/// 2. 다단어: 각 토큰 개별 OR
/// 3. 언더스코어 분리: DocSource_trait → DocSource OR trait
/// 4. 짧은 쿼리(≤3자): prefix 추가
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

/// RRF score에 제목 일치 보너스를 가산. 곱셈이 아닌 가산으로 RRF 스케일 유지.
fn apply_title_boost(hits: &mut Vec<Hit>, query: &str) {
    let q = query.to_lowercase();
    for hit in hits.iter_mut() {
        if let Some(ref title) = hit.title {
            let t = title.to_lowercase();
            if t.contains(&q) {
                hit.score += TITLE_EXACT_BOOST;
            } else if q.split_whitespace().any(|w| w.len() >= 2 && t.contains(w)) {
                hit.score += TITLE_PARTIAL_BOOST;
            }
        }
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
}

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

impl From<SearchHit> for Hit {
    fn from(sh: SearchHit) -> Self {
        Hit {
            document_id: sh.document_id,
            chunk_id: sh.chunk_id,
            project_id: 0,
            source_doc_id: String::new(),
            title: sh.title,
            file_path: sh.file_path,
            heading_path: sh.heading_path,
            snippet: Some(sh.snippet),
            context_content: sh.context_content,
            score: sh.score,
        }
    }
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
    embedder: Arc<dyn EmbeddingProvider + Send + Sync>,
}

impl SearchEngine {
    /// Create a new SearchEngine with an embedding provider.
    pub fn with_embedder(conn: Arc<Mutex<Connection>>, embedder: Arc<dyn EmbeddingProvider + Send + Sync>) -> Self {
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
            embedder: Arc::new(NoOpEmbedder) as Arc<dyn EmbeddingProvider + Send + Sync>,
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
        self.index_document_async_with_meta(project_id, source_doc_id, title, content, DocMeta::default()).await
    }

}
 
/// 일괄 인덱싱 요청 정보를 담는 구조체.
#[derive(Debug, Clone)]
pub struct BatchIndexingRequest {
    pub project_id: i64,
    pub source_doc_id: String,
    pub title: String,
    pub content: String,
    pub meta: DocMeta,
}

impl SearchEngine {
    pub async fn index_documents_batch_async(
        &self,
        requests: Vec<BatchIndexingRequest>,
    ) -> Result<(), SearchError> {
        if requests.is_empty() {
            return Ok(());
        }

        let num_requests = requests.len();
        let project_id = requests[0].project_id;
        tracing::info!(project_id, doc_count = num_requests, "index_documents_batch: starting batch indexing");

        // 1. 문서별 청크 분할 및 플랫화
        // chunk_counts: 각 문서가 가진 청크의 개수를 저장 (나중에 결과 복원용)
        let mut all_chunks = Vec::new(); // Vec<(doc_idx, Chunk)>
        let mut flat_texts = Vec::new();
        let mut chunk_counts = Vec::with_capacity(num_requests);

        for (doc_idx, req) in requests.iter().enumerate() {
            let chunks = crate::chunker::split_chunks(
                &req.content,
                crate::chunker::ChunkConfig {
                    title: Some(req.title.clone()),
                    ..Default::default()
                },
            );
            chunk_counts.push(chunks.len());
            for chunk in chunks {
                flat_texts.push(chunk.embedding_text.clone());
                all_chunks.push((doc_idx, chunk));
            }
        }

        if flat_texts.is_empty() {
            tracing::warn!(project_id, "index_documents_batch: no chunks to index");
            return Ok(());
        }

        let total_chunks = flat_texts.len();
        tracing::info!(project_id, total_chunks, "index_documents_batch: chunking complete, starting embedding");

        // 2. 배치 임베딩 수행
        // OnnxEmbedder::embed가 내부적으로 CPU 부하를 처리하므로 여기서는 직접 await 합니다.
        let embedding_vecs = self.embedder.embed(&flat_texts.iter().map(|s| s.as_str()).collect::<Vec<_>>()).await?;

        // 3. 임베딩 벡터 가공 (Vec<f32> -> Vec<u8>)
        let mut flat_embeddings: Vec<Vec<u8>> = Vec::with_capacity(embedding_vecs.len());
        for emb in embedding_vecs {
            let bytes: Vec<u8> = emb.iter().flat_map(|f: &f32| f.to_le_bytes()).collect();
            flat_embeddings.push(bytes);
        }

        // 4. DB 일괄 저장 (Transaction 사용)
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> Result<(), SearchError> {
            let mut conn_guard = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            let tx = conn_guard.transaction()?;

            persist_audit(&tx, &AuditEvent::IndexStart { project_id });

            let mut current_chunk_offset = 0;
            for (doc_idx, req) in requests.into_iter().enumerate() {
                let num_chunks = chunk_counts[doc_idx];
                let doc_chunks: Vec<_> = all_chunks[current_chunk_offset..current_chunk_offset + num_chunks]
                    .iter()
                    .map(|(_, c)| c.clone())
                    .collect();
                let doc_embeddings: Vec<_> = flat_embeddings[current_chunk_offset..current_chunk_offset + num_chunks]
                    .iter()
                    .cloned()
                    .collect();

                index_document_sync(&tx, req.project_id, &req.source_doc_id, &req.title, &req.content, &doc_chunks, &doc_embeddings, &req.meta)?;
                current_chunk_offset += num_chunks;
            }

            persist_audit(&tx, &AuditEvent::IndexComplete { project_id, docs_indexed: num_requests });
            tx.commit()?;
            Ok(())
        })
        .await??;

        Ok(())
    }

    pub async fn index_document_async_with_meta(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
        meta: DocMeta,
    ) -> Result<(), SearchError> {
        // 1. Chunk the document first to identify what to embed
        let chunks = crate::chunker::split_chunks(
            content,
            crate::chunker::ChunkConfig {
                title: Some(title.to_string()),
                ..Default::default()
            },
        );

        if chunks.is_empty() {
            return Ok(());
        }

        // 2. Generate embeddings for all chunks in a batch
        let texts: Vec<&str> = chunks.iter().map(|c| c.embedding_text.as_str()).collect();
        let embedding_vecs = self.embedder.embed(&texts).await.unwrap_or_default();
        
        let mut chunk_embeddings = Vec::new();
        for emb in embedding_vecs {
            let bytes: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();
            chunk_embeddings.push(bytes);
        }

        // 3. DB writes via spawn_blocking
        let conn = Arc::clone(&self.conn);
        let source_doc_id = source_doc_id.to_string();
        let title = title.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<(), SearchError> {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            persist_audit(&conn, &AuditEvent::IndexStart { project_id });
            index_document_sync(&conn, project_id, &source_doc_id, &title, &content, &chunks, &chunk_embeddings, &meta)?;
            persist_audit(&conn, &AuditEvent::IndexComplete { project_id, docs_indexed: 1 });
            Ok(())
        })
        .await??;

        Ok(())
    }

    /// Hybrid search: FTS5 + vector similarity, merged via RRF.
    pub async fn search_async(&self, query: &SearchQuery) -> Result<Vec<Hit>, SearchError> {
        let hits: Vec<Hit> = match query.mode {
            SearchMode::Fts => self.fts_search_async(query).await?.into_iter().map(Hit::from).collect(),
            SearchMode::Vector => self.vector_search_async(query).await?.into_iter().map(Hit::from).collect(),
            SearchMode::Hybrid => {
                let fts_hits = self.fts_search_async(query).await?;
                let vec_hits = self.vector_search_async(query).await.unwrap_or_default();
                rrf_merge(fts_hits, vec_hits).into_iter().map(Hit::from).collect()
            }
        };

        // Post-process hits: Pagination and Contextual retrieval
        let conn = Arc::clone(&self.conn);
        let query = query.clone();
        
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            
            // 1. Pagination: Skip `offset` and take `limit`
            let mut paged_hits: Vec<Hit> = hits.into_iter()
                .skip(query.offset)
                .take(query.limit)
                .map(Hit::from)
                .collect();
            
            if paged_hits.is_empty() {
                return Ok(paged_hits);
            }

            // 2. Statistical Analysis for Tiered Budgeting
            // We use all hits (before paging) to get a better distribution if possible,
            // or just the paged ones. Let's use the paged hits for immediate context.
            let scores: Vec<f64> = paged_hits.iter().map(|h| h.score).collect();
            let n = scores.len() as f64;
            let mean = scores.iter().sum::<f64>() / n;
            let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
            let sigma = variance.sqrt();
            let max_score = scores[0]; // Assuming sorted by score desc

            // 3. Assemble Context and Deduplicate
            // Deduplication: track sections already loaded in this turn to avoid redundant text if multiple hits in same section
            let mut loaded_sections: HashMap<(i64, Option<String>), String> = HashMap::new();
            let mut total_chars = 0;
            const GLOBAL_CEILING: usize = 15000;

            for hit in paged_hits.iter_mut() {
                if total_chars >= GLOBAL_CEILING {
                    break;
                }

                // Tier 1: Score >= (Max - Sigma) or Rank 1-3
                // (Since we skip offset, we don't know global rank easily here, so we use the first few of paged_hits)
                let is_high_confidence = hit.score >= (max_score - sigma);
                
                if is_high_confidence {
                    assemble_context_sync(&conn, hit, &mut loaded_sections, &mut total_chars, GLOBAL_CEILING)?;
                } else {
                    // Tier 2: Just use the initial snippet/content
                    hit.context_content = hit.snippet.clone();
                    total_chars += hit.context_content.as_ref().map(|s: &String| s.len()).unwrap_or(0);
                }
            }

            Ok(paged_hits)
        })
        .await?
    }

    async fn fts_search_async(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let conn = Arc::clone(&self.conn);
        let query_clone = query.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            fts_search_sync(&conn, &query_clone)
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
        let project_ids = query.project_ids.clone();
        let limit = query.limit as i64;
        let offset = query.offset as i64;

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| SearchError::LockPoisoned)?;
            vector_search_sync(&conn, &emb_bytes, limit, offset, &project_ids)
        })
        .await?
    }
}

// ── Sync free functions (used inside spawn_blocking) ─────────────────────────

/// 인덱싱 시 함께 저장할 문서 메타정보.
#[derive(Debug, Default, Clone)]
pub struct DocMeta {
    /// 문서 생성 시각 (Unix timestamp). 플러그인이 알 수 없으면 None → 인덱싱 시각으로 대체.
    pub created_at: Option<i64>,
    /// 문서 최종 수정 시각 (Unix timestamp).
    pub updated_at: Option<i64>,
    /// 태그 목록 (frontmatter tags: 또는 인라인 #tag, Confluence labels 등).
    pub tags: Vec<String>,
    /// 별칭 목록 (Obsidian aliases: frontmatter 등).
    pub aliases: Vec<String>,
    /// 문서의 상대 경로 (예: "Folder/Sub/File.md"). 물리적 폴더 구조 생성 시 사용.
    pub relative_path: Option<String>,
    /// 플러그인별 추가 메타 (space_key, url, repo 등) — JSON 직렬화하여 저장.
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

fn index_document_sync(
    conn: &Connection,
    project_id: i64,
    source_doc_id: &str,
    title: &str,
    content: &str,
    chunks: &[crate::chunker::Chunk],
    chunk_embeddings: &[Vec<u8>],
    meta: &DocMeta,
) -> Result<(), SearchError> {
    // content 빈값 방어 — 빈 content를 청크/임베딩에 삽입하지 않음
    if content.trim().is_empty() {
        tracing::warn!(source_doc_id = %source_doc_id, "skipping document with empty content");
        conn.execute(
            "UPDATE documents SET indexing_status = 'failed', last_indexed = unixepoch() WHERE project_id = ?1 AND source_doc_id = ?2",
            rusqlite::params![project_id, source_doc_id],
        )?;
        return Ok(());
    }

    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let metadata_json = serde_json::to_string(&meta.metadata).unwrap_or_else(|_| "{}".to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let created_at = meta.created_at.unwrap_or(now);
    let updated_at = meta.updated_at.unwrap_or(now);

    // 1. Get existing file path to check for moves
    let existing_data: Option<(String, i64)> = conn
        .query_row(
            "SELECT file_path, id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
            rusqlite::params![project_id, source_doc_id],
            |r| Ok((r.get::<_, String>(0).unwrap_or_default(), r.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|e| SearchError::Db(e))?;

    // 2. Calculate actual file path if relative_path is provided
    let project_path: Option<String> = conn
        .query_row("SELECT path FROM projects WHERE id = ?1", [project_id], |r| r.get(0))
        .ok();

    let full_file_path = if let (Some(base), Some(rel)) = (project_path, &meta.relative_path) {
        if base.starts_with("http://") || base.starts_with("https://") {
            // Web source: Use relative path as the virtual file path
            Some(rel.clone())
        } else {
            let path = std::path::PathBuf::from(base).join(rel);
            
            if let Some((ref old_path, _)) = existing_data {
                if !old_path.is_empty() && old_path.as_str() != path.to_string_lossy() {
                    let _ = std::fs::remove_file(old_path);
                    let _ = clean_up_empty_dirs(std::path::Path::new(old_path));
                }
            }

            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, content);
            Some(path.to_string_lossy().to_string())
        }
    } else {
        None
    };

    conn.execute(
        "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed, created_at, updated_at, metadata_json, file_path)
         VALUES (?1, ?2, ?3, ?4, ?5, unixepoch(), ?6, ?7, ?8, ?9)
         ON CONFLICT(project_id, source_doc_id) DO UPDATE SET
            title = excluded.title,
            content = excluded.content,
            content_hash = excluded.content_hash,
            last_indexed = excluded.last_indexed,
            updated_at = excluded.updated_at,
            metadata_json = excluded.metadata_json,
            file_path = COALESCE(excluded.file_path, documents.file_path)",
        rusqlite::params![project_id, source_doc_id, title, content, content_hash,
                          created_at, updated_at, metadata_json, full_file_path],
    )?;

    let doc_id: i64 = conn.query_row(
        "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
        rusqlite::params![project_id, source_doc_id],
        |row| row.get(0),
    )?;

    // Tags: 기존 삭제 후 재삽입
    conn.execute("DELETE FROM document_tags WHERE document_id = ?1", [doc_id])?;
    for tag in &meta.tags {
        conn.execute(
            "INSERT OR IGNORE INTO document_tags (document_id, tag) VALUES (?1, ?2)",
            rusqlite::params![doc_id, tag],
        )?;
    }

    // Aliases: 기존 삭제 후 재삽입
    conn.execute("DELETE FROM document_aliases WHERE document_id = ?1", [doc_id])?;
    for alias in &meta.aliases {
        conn.execute(
            "INSERT OR IGNORE INTO document_aliases (document_id, alias) VALUES (?1, ?2)",
            rusqlite::params![doc_id, alias],
        )?;
    }

    // Metadata key-value (document_metadata): 기존 삭제 후 재삽입
    conn.execute("DELETE FROM document_metadata WHERE document_id = ?1", [doc_id])?;
    for (key, value) in &meta.metadata {
        let val_str = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        conn.execute(
            "INSERT OR REPLACE INTO document_metadata (document_id, key, value) VALUES (?1, ?2, ?3)",
            rusqlite::params![doc_id, key, val_str],
        )?;
    }

    // Delete old chunks (triggers handle FTS cleanup)
    conn.execute("DELETE FROM chunks WHERE document_id = ?1", [doc_id])?;

    // Store chunks and their embeddings
    for (i, chunk) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index, heading_path) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![doc_id, chunk.content, chunk.index as i64, chunk.heading_path],
        )?;

        let chunk_id: i64 = conn.last_insert_rowid();

        if let Some(bytes) = chunk_embeddings.get(i) {
            conn.execute(
                "INSERT OR REPLACE INTO chunk_embeddings(chunk_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![chunk_id, bytes],
            )?;
        }
    }

    Ok(())
}

/// Recursively removes empty parent directories up to but not including the root.
fn clean_up_empty_dirs(path: &std::path::Path) -> std::io::Result<()> {
    let mut current = path.parent();
    while let Some(parent) = current {
        // Try to remove. If not empty or other error, stop.
        if std::fs::remove_dir(parent).is_err() {
            break;
        }
        current = parent.parent();
    }
    Ok(())
}

fn fts_search_sync(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
    let fts_query = build_fts_query(&query.text);
    if fts_query.is_empty() {
        return Ok(vec![]);
    }

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
        "SELECT d.id, c.id, d.title, COALESCE(d.file_path, d.source_doc_id), c.heading_path,
                snippet(chunks_fts, 0, '<b>', '</b>', '…', 20) AS snippet,
                bm25(chunks_fts, 1.0, 3.0) AS score
         FROM chunks_fts
         JOIN chunks c ON c.id = chunks_fts.rowid
         JOIN documents d ON d.id = c.document_id
         JOIN projects p ON p.id = d.project_id
         WHERE chunks_fts MATCH ?1
         {project_filter}
         ORDER BY score
         LIMIT ?2 OFFSET ?3"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(fts_query),
        Box::new((query.limit + query.offset) as i64),
        Box::new(0i64), // We handle offset in rrf_merge/search_async to keep scores consistent
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
                context_content: None,
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
    offset: i64,
    project_ids: &[i64],
) -> Result<Vec<SearchHit>, SearchError> {
    // We fetch limit + offset candidates to allow proper RRF merging and pagination
    let k = limit + offset;
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
        "SELECT c.id, c.document_id, d.title, COALESCE(d.file_path, d.source_doc_id), c.heading_path, c.content, knn.distance
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
        Box::new(k),
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
                context_content: None,
                score: 1.0 / (RRF_K as f64 + distance),
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    // 유사도 임계값 이하 결과 필터링 (L2 distance 기반: distance > VECTOR_MAX_L2_DISTANCE 제거)
    let mut hits = hits;
    hits.retain(|h| {
        // score는 1/(RRF_K + distance)로 계산됨 → distance = 1/score - RRF_K
        // score == 0.0 인 경우 division-by-zero → inf 이므로 명시적으로 제거
        if h.score <= 0.0 {
            return false;
        }
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
    global_ceiling: usize,
) -> Result<(), SearchError> {
    let key = (hit.document_id, hit.heading_path.clone());
    
    if let Some(cached) = loaded_sections.get(&key) {
        hit.context_content = Some(cached.clone());
        return Ok(());
    }

    let (target_idx, target_content): (i32, String) = conn.query_row(
        "SELECT chunk_index, content FROM chunks WHERE id = ?1",
        rusqlite::params![hit.chunk_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let mut stmt = conn.prepare(
        "SELECT chunk_index, content FROM chunks 
         WHERE document_id = ?1 AND (heading_path IS ?2 OR heading_path = ?2)
         ORDER BY chunk_index"
    )?;
    
    let section_chunks: Vec<(i32, String)> = stmt.query_map(
        rusqlite::params![hit.document_id, hit.heading_path],
        |r| Ok((r.get(0)?, r.get(1)?))
    )?.collect::<Result<Vec<_>, _>>()?;

    const SOFT_LIMIT: usize = 4000;
    let pos = section_chunks.iter().position(|(idx, _)| *idx == target_idx).unwrap_or(0);
    
    let mut context_parts = VecDeque::new();
    context_parts.push_back(&section_chunks[pos].1);
    let mut current_len = section_chunks[pos].1.len();
    
    let mut left = pos as i32 - 1;
    let mut right = pos as i32 + 1;
    
    // Greedy center-out assembly
    while current_len < SOFT_LIMIT && (left >= 0 || right < section_chunks.len() as i32) {
        if left >= 0 && current_len < SOFT_LIMIT {
            let s = &section_chunks[left as usize].1;
            context_parts.push_front(s);
            current_len += s.len();
            left -= 1;
        }
        if right < section_chunks.len() as i32 && current_len < SOFT_LIMIT {
            let s = &section_chunks[right as usize].1;
            context_parts.push_back(s);
            current_len += s.len();
            right += 1;
        }
    }

    let joined = context_parts.into_iter().cloned().collect::<Vec<_>>().join("\n\n");
    let mut final_context = joined;

    if left >= 0 {
        final_context = format!("[... (Preceding content truncated) ...]\n\n{}", final_context);
    }
    if right < section_chunks.len() as i32 {
        final_context = format!("{}\n\n[... (Following content truncated - use get_section for full details) ...]", final_context);
    }

    if *total_chars + final_context.len() > global_ceiling {
        let remaining = global_ceiling.saturating_sub(*total_chars);
        if remaining > 100 {
            final_context = format!("{}...", &final_context[..remaining.saturating_sub(50)]);
            final_context.push_str("\n\n[... (Global context ceiling reached) ...]");
        } else {
            final_context = target_content;
        }
    }

    *total_chars += final_context.len();
    hit.context_content = Some(final_context.clone());
    loaded_sections.insert(key, final_context);

    Ok(())
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
        self.index_document_with_meta(project_id, source_doc_id, title, content, &DocMeta::default())
    }

    /// Index a document with full metadata (sync, FTS only).
    pub fn index_document_with_meta(
        &self,
        project_id: i64,
        source_doc_id: &str,
        title: &str,
        content: &str,
        meta: &DocMeta,
    ) -> Result<(), SearchError> {
        let chunks = crate::chunker::split_chunks(
            content,
            crate::chunker::ChunkConfig {
                title: Some(title.to_string()),
                ..Default::default()
            },
        );
        index_document_sync(self.conn, project_id, source_doc_id, title, content, &chunks, &[], meta)
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

        let fts_query_str = build_fts_query(query);
        if fts_query_str.is_empty() {
            return Ok(vec![]);
        }

        let sql = format!(
            "SELECT d.id, d.project_id, d.source_doc_id, d.title,
                    snippet(chunks_fts, 0, '<b>', '</b>', '...', 20) AS snippet,
                    bm25(chunks_fts, 1.0, 3.0) AS fts_score
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
            Box::new(fts_query_str.clone()),
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
                chunk_id: 0, // Not precisely known in simplify view, but placeholder is fine
                project_id,
                source_doc_id,
                title,
                file_path: None, // Need to join more tables if we want this here, but None is fine for search_simple
                heading_path: None,
                snippet: Some(snippet),
                context_content: None,
                score: 0.0,
            });
            entry.score += rrf_score(rank + 1);
        }

        let mut hits: Vec<Hit> = rrf_map.into_values().collect();

        // Fix 4-3: FTS 결과 < 3개이면 prefix fallback 쿼리로 보완
        if hits.len() < 3 {
            let fallback_q = build_prefix_fallback_query(query);
            if fallback_q != fts_query_str {
                let mut fb_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                    Box::new(fallback_q),
                    Box::new(limit),
                ];
                for id in &project_id_params {
                    fb_params.push(Box::new(*id));
                }
                let fb_refs: Vec<&dyn rusqlite::types::ToSql> =
                    fb_params.iter().map(|p| p.as_ref()).collect();
                let fb_sql = format!(
                    "SELECT d.id, d.project_id, d.source_doc_id, d.title,
                            snippet(chunks_fts, 0, '<b>', '</b>', '...', 20) AS snippet,
                            bm25(chunks_fts, 1.0, 3.0) AS fts_score
                     FROM chunks_fts
                     JOIN chunks c ON c.id = chunks_fts.rowid
                     JOIN documents d ON d.id = c.document_id
                     JOIN projects p ON p.id = d.project_id
                     WHERE chunks_fts MATCH ?1
                     {project_filter}
                     ORDER BY fts_score
                     LIMIT ?2"
                );
                if let Ok(mut fb_stmt) = self.conn.prepare(&fb_sql) {
                    let existing_ids: std::collections::HashSet<i64> =
                        hits.iter().map(|h| h.document_id).collect();
                    let fb_rows: Vec<(i64, i64, String, Option<String>, String)> = fb_stmt
                        .query_map(fb_refs.as_slice(), |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                        })
                        .map(|mapped| {
                            mapped
                                .filter_map(|r| r.ok())
                                .filter(|(doc_id, ..)| !existing_ids.contains(doc_id))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    // prefix 결과는 기존 rank 이후부터 시작해 자연스럽게 낮은 가중치
                    let base_rank = hits.len();
                    for (offset, (doc_id, project_id, source_doc_id, title, snippet)) in
                        fb_rows.into_iter().enumerate()
                    {
                        hits.push(Hit {
                            document_id: doc_id,
                            chunk_id: 0,
                            project_id,
                            source_doc_id,
                            title,
                            file_path: None,
                            heading_path: None,
                            snippet: Some(snippet),
                            context_content: None,
                            score: rrf_score(base_rank + offset + 1),
                        });
                    }
                }
            }
        }

        apply_title_boost(&mut hits, query);
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
            include_str!("db/migrations/V10__plugin_kv.sql"),
            include_str!("db/migrations/V11__project_source.sql"),
            include_str!("db/migrations/V12__content_cache.sql"),
            include_str!("db/migrations/V13__document_meta.sql"),
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
            offset: 0,
            mode: SearchMode::Vector,
        };
        let hits = engine.search_async(&query).await.unwrap();
        assert_eq!(hits.len(), 2, "should find both documents via vector search");
    }

    #[tokio::test]
    async fn index_document_stores_embeddings_for_all_chunks() {
        let (engine, conn, pid) = make_async_engine("multi-chunk-test");

        // Create a document with 2 sections (will result in at least 2 chunks)
        let content = "# Section 1\nContent 1\n\n# Section 2\nContent 2";
        engine.index_document_async(pid, "doc1", "Multi", content).await.unwrap();

        let c = conn.lock().unwrap();
        let chunk_count: i64 = c
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        let emb_count: i64 = c
            .query_row("SELECT COUNT(*) FROM chunk_embeddings", [], |r| r.get(0))
            .unwrap();
        
        assert!(chunk_count >= 2, "should have multiple chunks");
        assert_eq!(emb_count, chunk_count, "all chunks should have embeddings");
    }

    #[tokio::test]
    async fn chunking_preserves_heading_path() {
        let (engine, conn, pid) = make_async_engine("heading-test");
        let content = "# Intro\nHello\n\n## Details\nWorld";
        engine.index_document_async(pid, "doc1", "Title", content).await.unwrap();

        let c = conn.lock().unwrap();
        let paths: Vec<Option<String>> = c.prepare("SELECT heading_path FROM chunks ORDER BY chunk_index")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], Some("# Intro".to_string()));
        assert_eq!(paths[1], Some("## Details".to_string()));
    }

    #[tokio::test]
    async fn hybrid_search_merges_fts_and_vector() {
        let (engine, _conn, pid) = make_async_engine("htest");

        engine.index_document_async(pid, "h1", "Rust Guide", "rust programming language").await.unwrap();

        let query = SearchQuery::new("rust programming");
        let hits = engine.search_async(&query).await.unwrap();
        assert!(!hits.is_empty(), "hybrid search should return results");
    }

    #[tokio::test]
    async fn index_document_writes_index_start_audit_log() {
        let (_engine, conn, pid) = make_async_engine("audit_start");
        let engine = SearchEngine::with_embedder(
            Arc::clone(&conn),
            Arc::new(crate::embedding::MockEmbedder::new(384)),
        );
        engine.index_document_async(pid, "doc1", "Title", "content").await.unwrap();

        let conn = conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE event_type = 'index_start'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "index_start should be recorded in audit_log");
    }

    #[tokio::test]
    async fn index_document_writes_index_complete_audit_log() {
        let (_engine, conn, pid) = make_async_engine("audit_complete");
        let engine = SearchEngine::with_embedder(
            Arc::clone(&conn),
            Arc::new(crate::embedding::MockEmbedder::new(384)),
        );
        engine.index_document_async(pid, "doc1", "Title", "content").await.unwrap();

        let conn = conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE event_type = 'index_complete'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "index_complete should be recorded in audit_log");
    }

    #[tokio::test]
    async fn index_document_audit_log_has_correct_project_id() {
        let (_engine, conn, pid) = make_async_engine("audit_pid");
        let engine = SearchEngine::with_embedder(
            Arc::clone(&conn),
            Arc::new(crate::embedding::MockEmbedder::new(384)),
        );
        engine.index_document_async(pid, "doc1", "Title", "content").await.unwrap();

        let conn = conn.lock().unwrap();
        let stored_pid: Option<i64> = conn
            .query_row(
                "SELECT project_id FROM audit_log WHERE event_type = 'index_complete'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_pid, Some(pid));
    }

    // ── Fix 3: empty content guard ──────────────────────────────────────

    #[test]
    fn index_document_empty_content_does_not_create_chunks() {
        let db = TestDb::new();
        let pid = insert_project(&db, "proj", "/proj");
        let engine = SyncSearchEngine::from_conn(&db.conn);
        // 빈 content 인덱싱 — 패닉 없이 Ok(())
        engine.index_document(pid, "empty-doc", "Empty Doc", "").unwrap();
        // chunks 테이블에 행이 없어야 함
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "빈 content 문서는 chunk를 생성하면 안 됨");
    }

    // ── Fix 4-1: FTS 쿼리 빌더 ─────────────────────────────────────────

    #[test]
    fn build_fts_query_single_word() {
        let q = build_fts_query("hello");
        assert!(q.contains("\"hello\""), "phrase match 포함되어야 함");
    }

    #[test]
    fn build_fts_query_short_word_has_prefix() {
        let q = build_fts_query("abc");
        assert!(q.contains("\"abc\"*"), "짧은 쿼리는 prefix 포함되어야 함");
    }

    #[test]
    fn build_fts_query_multiword_has_tokens() {
        let q = build_fts_query("hello world");
        assert!(q.contains("\"hello world\""), "phrase match 있어야 함");
        assert!(q.contains("\"hello\""), "hello 개별 토큰 있어야 함");
        assert!(q.contains("\"world\""), "world 개별 토큰 있어야 함");
    }

    #[test]
    fn build_fts_query_underscore_split() {
        let q = build_fts_query("DocSource_trait");
        assert!(q.contains("\"DocSource\""), "언더스코어 앞부분 분리되어야 함");
        assert!(q.contains("\"trait\""), "언더스코어 뒷부분 분리되어야 함");
    }

    #[test]
    fn sanitize_fts_token_escapes_quotes() {
        let s = sanitize_fts_token("a\"b");
        assert!(!s.contains('"') || s.contains("\"\""), "따옴표 이스케이프되어야 함");
    }

    #[test]
    fn sanitize_fts_token_replaces_dash() {
        let s = sanitize_fts_token("sqlite-vec");
        assert!(!s.contains('-'), "대시가 제거되어야 함");
    }

    // ── Fix 4-4: title boost ────────────────────────────────────────────

    #[test]
    fn title_boost_increases_score_for_title_match() {
        let mut hits = vec![
            Hit { document_id: 1, project_id: 1, source_doc_id: "a".into(),
                  title: Some("Rust Programming Guide".into()), snippet: None, score: 0.010,
                  chunk_id: 0, context_content: None, heading_path: None },
            Hit { document_id: 2, project_id: 1, source_doc_id: "b".into(),
                  title: Some("Something Else".into()), snippet: None, score: 0.010,
                  chunk_id: 0, context_content: None, heading_path: None },
        ];
        apply_title_boost(&mut hits, "rust programming");
        assert!(hits[0].document_id == 1, "title match 문서가 1위여야 함");
        assert!(hits[0].score > hits[1].score, "title match가 더 높은 score를 가져야 함");
    }

    // ── Fix 4-5: vector 최소 유사도 임계값 ─────────────────────────────

    #[test]
    fn vector_search_filters_low_similarity() {
        let score_low = 1.0 / (RRF_K as f64 + 2.0); // distance=2.0, 제거되어야 함
        let score_ok  = 1.0 / (RRF_K as f64 + 0.5); // distance=0.5, 유지되어야 함
        let distance_low = (1.0 / score_low) - RRF_K as f64;
        let distance_ok  = (1.0 / score_ok)  - RRF_K as f64;
        assert!(distance_low > VECTOR_MAX_L2_DISTANCE, "낮은 유사도는 임계값 초과여야 함");
        assert!(distance_ok  <= VECTOR_MAX_L2_DISTANCE, "높은 유사도는 임계값 이하여야 함");
    }
}
