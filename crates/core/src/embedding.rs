use async_trait::async_trait;
use ort::session::builder::GraphOptimizationLevel;
use ort::{session::Session, value::TensorRef};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokenizers::Tokenizer;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("model load failed: {0}")]
    ModelLoad(String),
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("empty input")]
    EmptyInput,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub dimension: usize,
    pub max_tokens: usize,
    /// 물리적 모델 파일 경로 (있을 경우)
    pub path: Option<String>,
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    fn dimension(&self) -> usize;
    fn model_info(&self) -> &ModelInfo;
}

/// Compute cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "vectors must have equal length");
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Quantizes an L2-normalized f32 vector to i8.
/// Maps [-1.0, 1.0] to [-128, 127].
pub fn quantize_to_i8(v: &[f32]) -> Vec<i8> {
    v.iter()
        .map(|&x| {
            let scaled = (x * 128.0).round();
            scaled.clamp(-128.0, 127.0) as i8
        })
        .collect()
}

#[cfg(test)]
mod quantization_tests {
    use super::*;

    #[test]
    fn test_quantize_to_i8_basic() {
        // L2 normalized vectors are in [-1, 1]
        let input = vec![0.0f32, 1.0, -1.0, 0.5, -0.25];
        let expected = vec![0i8, 127, -128, 64, -32];
        let result = quantize_to_i8(&input);
        assert_eq!(result, expected);
    }
}

/// Mini-batch size for ONNX inference. Keeps per-call tensor memory bounded:
/// `EMBED_BATCH_SIZE * max_tokens * dim * 4 bytes` ≈ 32 * 512 * 384 * 4 ≈ 24 MB peak.
const EMBED_BATCH_SIZE: usize = 32;

/// ONNX-backed embedding provider using all-MiniLM-L6-v2
pub struct OnnxEmbedder {
    info: ModelInfo,
    model_path: std::path::PathBuf,
    session: Arc<Mutex<Option<Session>>>,
    tokenizer: Option<Tokenizer>,
    last_used: Arc<Mutex<std::time::Instant>>,
    session_ttl: std::time::Duration,
}

impl OnnxEmbedder {
    pub fn with_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.session_ttl = ttl;
        self
    }

    /// Helper method to load ONNX runtime session on demand.
    fn load_session(path: &std::path::Path) -> Result<Session, EmbeddingError> {
        let cpu_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8);

        Session::builder()
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
            .with_intra_threads(cpu_threads)
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
            .with_config_entry("session.use_memory_arena", "0")
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
            .commit_from_file(path)
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))
    }

    /// Create an embedder from a model `.onnx` file.
    /// The tokenizer is loaded from `<model_dir>/tokenizer.json`.
    /// If the model file is not a valid ONNX model or tokenizer.json is missing,
    /// returns a partially-initialized embedder that returns `Inference` errors on `embed()`.
    pub fn new(model_path: impl Into<std::path::PathBuf>) -> Result<Self, EmbeddingError> {
        // Suppress ORT INFO logs — they pollute stdout
        ort::init().commit();
        if let Ok(env) = ort::environment::current() {
            env.set_log_level(ort::logging::LogLevel::Error);
        }

        let path = model_path.into();
        let path_clone = path.clone();
        if !path.exists() {
            return Err(EmbeddingError::ModelLoad(format!(
                "model not found at {}",
                path.display()
            )));
        }

        let tokenizer_path = path.parent().unwrap_or_else(|| std::path::Path::new(".")).join("tokenizer.json");
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path).ok();
        if let Some(ref mut t) = tokenizer {
            t.with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .expect("failed to set truncation");
        }

        Ok(Self {
            info: ModelInfo {
                name: "multilingual-e5-small".to_string(),
                dimension: 384,
                max_tokens: 512,
                path: Some(path_clone.to_string_lossy().to_string()),
            },
            model_path: path_clone,
            session: Arc::new(Mutex::new(None)),
            tokenizer,
            last_used: Arc::new(Mutex::new(std::time::Instant::now())),
            session_ttl: std::time::Duration::from_secs(300),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OnnxEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }

        let tokenizer = self.tokenizer.as_ref().ok_or_else(|| {
            EmbeddingError::Tokenizer("tokenizer not loaded".to_string())
        })?;

        let mut session_guard = self
            .session
            .lock()
            .map_err(|e| EmbeddingError::Inference(format!("session lock poisoned: {e}")))?;

        if session_guard.is_none() {
            let loaded_session = Self::load_session(&self.model_path)?;
            *session_guard = Some(loaded_session);

            let session_clone = Arc::clone(&self.session);
            let last_used_clone = Arc::clone(&self.last_used);
            let ttl = self.session_ttl;

            tokio::spawn(async move {
                let check_interval = ttl.min(std::time::Duration::from_millis(50));
                loop {
                    tokio::time::sleep(check_interval).await;

                    let last_used_val = match last_used_clone.lock() {
                        Ok(guard) => *guard,
                        Err(_) => break, // poisoned
                    };

                    if last_used_val.elapsed() >= ttl {
                        let mut session_guard = match session_clone.lock() {
                            Ok(guard) => guard,
                            Err(_) => break, // poisoned
                        };
                        *session_guard = None;
                        tracing::debug!("ONNX session dropped due to inactivity (TTL: {:?})", ttl);
                        break;
                    }
                }
            });
        }

        let session = session_guard
            .as_mut()
            .ok_or_else(|| EmbeddingError::Inference("ONNX session not loaded".to_string()))?;

        {
            let mut last_used = self.last_used.lock().map_err(|e| {
                EmbeddingError::Inference(format!("last_used lock poisoned: {e}"))
            })?;
            *last_used = std::time::Instant::now();
        }

        let dim = self.info.dimension;
        let mut embeddings = Vec::with_capacity(texts.len());

        // Process in mini-batches to keep peak tensor memory bounded.
        // A single call with hundreds of chunks would allocate
        // `batch * max_len * dim * 4 bytes` which can reach hundreds of MB.
        //
        // block_in_place signals tokio that this thread will perform CPU-bound blocking
        // work (ONNX inference), allowing the runtime to move other tasks to free threads.
        tokio::task::block_in_place(|| -> Result<(), EmbeddingError> {
        for chunk in texts.chunks(EMBED_BATCH_SIZE) {
            let batch_size = chunk.len();

            // Batch tokenize (avoiding to_string() clones)
            let encodings = tokenizer
                .encode_batch(chunk.to_vec(), true)
                .map_err(|e| EmbeddingError::Tokenizer(e.to_string()))?;

            let max_len = encodings
                .iter()
                .map(|e| e.get_ids().len())
                .max()
                .unwrap_or(0);

            // Build flat i64 buffers: input_ids, attention_mask, token_type_ids
            let mut input_ids = vec![0i64; batch_size * max_len];
            let mut attention_mask = vec![0i64; batch_size * max_len];
            let mut token_type_ids = vec![0i64; batch_size * max_len];

            for (i, enc) in encodings.iter().enumerate() {
                let ids = enc.get_ids();
                let mask = enc.get_attention_mask();
                let type_ids = enc.get_type_ids();
                for j in 0..ids.len() {
                    input_ids[i * max_len + j] = ids[j] as i64;
                    attention_mask[i * max_len + j] = mask[j] as i64;
                    token_type_ids[i * max_len + j] = type_ids[j] as i64;
                }
            }

            // Build TensorRef from (shape, &[T]) tuples — avoids ndarray version conflicts
            let shape = [batch_size, max_len];
            let ids_ref = TensorRef::<i64>::from_array_view((shape, input_ids.as_slice()))
                .map_err(|e| EmbeddingError::Inference(e.to_string()))?;
            let mask_ref = TensorRef::<i64>::from_array_view((shape, attention_mask.as_slice()))
                .map_err(|e| EmbeddingError::Inference(e.to_string()))?;
            let type_ref = TensorRef::<i64>::from_array_view((shape, token_type_ids.as_slice()))
                .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

            let outputs = session
                .run(ort::inputs![
                    "input_ids" => ids_ref,
                    "attention_mask" => mask_ref,
                    "token_type_ids" => type_ref,
                ])
                .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

            // last_hidden_state shape: [batch, seq_len, 384]
            // Use try_extract_tensor which returns (&Shape, &[f32]) — no ndarray needed
            let (hidden_shape, hidden_data) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

            // hidden_shape is [batch, seq_len, dim]
            let seq_len = hidden_shape[1] as usize;

            for i in 0..batch_size {
                let mask_sum: f32 = attention_mask[i * max_len..(i + 1) * max_len]
                    .iter()
                    .map(|&m| m as f32)
                    .sum();
                let denom = mask_sum.max(1e-9);

                // Mean pooling: sum(token_vec * mask) / sum(mask)
                let mut pooled = vec![0f32; dim];
                for j in 0..seq_len {
                    let mask_val = if j < max_len {
                        attention_mask[i * max_len + j] as f32
                    } else {
                        0.0
                    };
                    if mask_val > 0.0 {
                        let base = (i * seq_len + j) * dim;
                        for k in 0..dim {
                            pooled[k] += hidden_data[base + k] * mask_val;
                        }
                    }
                }
                for v in &mut pooled {
                    *v /= denom;
                }

                // L2 normalize
                let l2: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
                if l2 > 0.0 {
                    for v in &mut pooled {
                        *v /= l2;
                    }
                }

                embeddings.push(pooled);
            }
        }
        Ok(())
        })?;

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.info.dimension
    }

    fn model_info(&self) -> &ModelInfo {
        &self.info
    }
}

/// Ollama-backed embedding provider (optional fallback)
pub struct OllamaEmbedder {
    info: ModelInfo,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, dimension: usize) -> Self {
        let model = model.into();
        Self {
            info: ModelInfo {
                name: model.clone(),
                dimension,
                max_tokens: 512,
                path: None,
            },
            base_url: base_url.into(),
            model,
            client: reqwest::Client::new(),
        }
    }

    /// Convenience constructor for nomic-embed-text (768 dimensions)
    pub fn default_nomic() -> Self {
        Self::new("http://localhost:11434", "nomic-embed-text", 768)
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }

        let url = format!("{}/api/embeddings", self.base_url);
        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            let body = serde_json::json!({
                "model": self.model,
                "prompt": text,
            });

            let resp = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| EmbeddingError::Http(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(EmbeddingError::Http(format!(
                    "Ollama returned status {}",
                    resp.status()
                )));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| EmbeddingError::Http(e.to_string()))?;

            let embedding: Vec<f32> = json["embedding"]
                .as_array()
                .ok_or_else(|| EmbeddingError::Http("missing 'embedding' field".to_string()))?
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();

            results.push(embedding);
        }

        Ok(results)
    }

    fn dimension(&self) -> usize {
        self.info.dimension
    }

    fn model_info(&self) -> &ModelInfo {
        &self.info
    }
}

pub const MULTILINGUAL_E5_SMALL_SHA256: &str = "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665";

/// Resolve the ONNX model path from multiple candidate locations.
///
/// Priority order:
/// 1. `DOXUS_MODEL_PATH` environment variable (if file + tokenizer.json exist)
/// 2. macOS app bundle `{exe}/../Resources/models/multilingual-e5-small.onnx`
/// 3. `~/.doxus/models/multilingual-e5-small.onnx` (shared install path for MCP/CLI)
/// 4. Dev workspace: `{exe_ancestry}/crates/core/models/multilingual-e5-small.onnx`
///
/// Each candidate is accepted only if both the `.onnx` file and `tokenizer.json`
/// exist in the same directory.
pub fn resolve_model_path() -> Option<std::path::PathBuf> {
    let skip_onnx = std::env::var("DOXUS_SKIP_ONNX").unwrap_or_default();
    if skip_onnx == "1" || skip_onnx == "true" {
        tracing::debug!("resolve_model_path: DOXUS_SKIP_ONNX is set, skipping ONNX model resolution");
        return None;
    }

    // Prefer int8-quantized model (~120MB) over fp32 (~448MB) for faster CPU inference.
    let home_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let int8_exists = home_dir.join(".doxus/models/multilingual-e5-small-int8.onnx").exists();
    
    let model_name = if int8_exists {
        "multilingual-e5-small-int8.onnx"
    } else {
        "multilingual-e5-small.onnx"
    };

    // Helper: accept path only if both model and tokenizer.json exist alongside it
    let valid = |p: &std::path::PathBuf| -> bool {
        if !p.exists() {
            return false;
        }
        let tokenizer = p
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("tokenizer.json");
        tokenizer.exists()
    };

    // 1. DOXUS_MODEL_PATH env var
    if let Ok(p) = std::env::var("DOXUS_MODEL_PATH") {
        let path = std::path::PathBuf::from(p);
        if valid(&path) {
            tracing::debug!("resolve_model_path: using DOXUS_MODEL_PATH {:?}", path);
            return Some(path);
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));

    let candidates: Vec<std::path::PathBuf> = [
        // macOS bundle: {exe}/../Resources/models/
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()?
                    .parent()
                    .map(|p| p.join("Resources/models").join(model_name))
            }),
        // Shared install path (MCP + CLI share this)
        Some(home.join(".doxus/models").join(model_name)),
        // Dev: exe is in target/{debug,release}/, go up 3 levels to workspace root
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()?
                    .parent()?
                    .parent()
                    .map(|root| root.join("crates/core/models").join(model_name))
            }),
        // Dev: relative to cwd
        Some(std::path::PathBuf::from("crates/core/models").join(model_name)),
    ]
    .into_iter()
    .flatten()
    .collect();

    for candidate in &candidates {
        if valid(candidate) {
            tracing::debug!("resolve_model_path: found {:?}", candidate);
            return Some(candidate.clone());
        }
    }

    tracing::debug!("resolve_model_path: no model found in any candidate location");
    None
}

/// Verify the SHA256 checksum of the model file.
pub fn verify_model_checksum(path: &std::path::Path, expected_hex: &str) -> bool {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return false,
        };
        hasher.update(&buffer[..n]);
    }
    let actual_hex = hex::encode(hasher.finalize());
    actual_hex == expected_hex
}

impl OnnxEmbedder {
    /// Create an embedder using the default model path resolution (see `resolve_model_path`).
    /// Also verifies the SHA256 checksum of the model file.
    pub fn from_default_path() -> Result<Self, EmbeddingError> {
        let path = resolve_model_path().ok_or_else(|| {
            EmbeddingError::ModelLoad(
                "no model found in any default location; \
                 place multilingual-e5-small.onnx + tokenizer.json in ~/.doxus/models/"
                    .into(),
            )
        })?;

        // Verify checksum (only for the standard model)
        if path.file_name().and_then(|n| n.to_str()) == Some("multilingual-e5-small.onnx")
            && !verify_model_checksum(&path, MULTILINGUAL_E5_SMALL_SHA256)
        {
            return Err(EmbeddingError::ModelLoad(format!(
                "model checksum mismatch at {}; the file may be corrupt",
                path.display()
            )));
        }

        Self::new(path)
    }
}

/// No-op embedding provider — returns errors on embed(), used as fallback when model is unavailable.
pub struct NoOpEmbedder;

#[async_trait::async_trait]
impl EmbeddingProvider for NoOpEmbedder {
    async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(vec![])
    }
    fn dimension(&self) -> usize { 0 }
    fn model_info(&self) -> &ModelInfo {
        static INFO: std::sync::LazyLock<ModelInfo> = std::sync::LazyLock::new(|| {
            ModelInfo { name: "noop".into(), dimension: 0, max_tokens: 0, path: None }
        });
        &INFO
    }
}

/// Mock embedding provider for tests — returns deterministic vectors
pub struct MockEmbedder {
    dimension: usize,
    info: ModelInfo,
}

impl MockEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            info: ModelInfo {
                name: "mock".to_string(),
                dimension,
                max_tokens: 512,
                path: None,
            },
        }
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        Ok(texts.iter().map(|_| vec![0.1f32; self.dimension]).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_info(&self) -> &ModelInfo {
        &self.info
    }
}

/// Create an embedding provider based on environment config with fallback options.
pub fn get_default_embedder() -> Arc<dyn EmbeddingProvider + Send + Sync> {
    let provider_env = std::env::var("DOXUS_EMBEDDING_PROVIDER").unwrap_or_default();
    
    if provider_env == "ollama" {
        let url = std::env::var("DOXUS_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("DOXUS_OLLAMA_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        let dim = std::env::var("DOXUS_OLLAMA_DIM")
            .ok()
            .and_then(|d| d.parse::<usize>().ok())
            .unwrap_or(768);
        tracing::info!("Using Ollama embedder ({}) at {}", model, url);
        return Arc::new(OllamaEmbedder::new(url, model, dim));
    } else if provider_env == "noop" {
        tracing::info!("Using NoOp embedder (FTS-only)");
        return Arc::new(NoOpEmbedder);
    } else if provider_env == "onnx" {
        match OnnxEmbedder::from_default_path() {
            Ok(e) => return Arc::new(e),
            Err(err) => {
                tracing::error!("ONNX embedder explicitly requested but failed to load: {}", err);
                return Arc::new(NoOpEmbedder);
            }
        }
    }

    // Default resolution (no explicit provider specified)
    let skip_onnx = std::env::var("DOXUS_SKIP_ONNX").unwrap_or_default();
    if skip_onnx == "1" || skip_onnx == "true" {
        tracing::info!("DOXUS_SKIP_ONNX is set. Skipping ONNX and falling back to Ollama.");
        let url = std::env::var("DOXUS_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("DOXUS_OLLAMA_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        let dim = std::env::var("DOXUS_OLLAMA_DIM")
            .ok()
            .and_then(|d| d.parse::<usize>().ok())
            .unwrap_or(768);
        return Arc::new(OllamaEmbedder::new(url, model, dim));
    }

    match OnnxEmbedder::from_default_path() {
        Ok(e) => Arc::new(e),
        Err(err) => {
            tracing::warn!("ONNX load failed: {}. Falling back to Ollama.", err);
            let url = std::env::var("DOXUS_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
            let model = std::env::var("DOXUS_OLLAMA_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
            let dim = std::env::var("DOXUS_OLLAMA_DIM")
                .ok()
                .and_then(|d| d.parse::<usize>().ok())
                .unwrap_or(768);
            Arc::new(OllamaEmbedder::new(url, model, dim))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── cosine_similarity unit tests ─────────────────────────────────────────

    #[test]
    fn cosine_identical_vectors_is_one() {
        let v = vec![1.0f32, 0.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_vectors_is_zero() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_opposite_vectors_is_minus_one() {
        let a = vec![1.0f32, 0.0];
        let b = vec![-1.0f32, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_zero_vector_returns_zero() {
        let a = vec![0.0f32, 0.0];
        let b = vec![1.0f32, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    // ── EmbeddingProvider trait object tests ─────────────────────────────────

    #[test]
    fn onnx_embedder_rejects_missing_model() {
        let result = OnnxEmbedder::new("/nonexistent/model.onnx");
        assert!(matches!(result, Err(EmbeddingError::ModelLoad(_))));
    }

    #[tokio::test]
    async fn embedding_provider_is_object_safe_and_send_sync() {
        use std::sync::Arc;
        // Arc<dyn + Send + Sync> — Tauri State처럼 실사용 패턴 검증
        let provider: Arc<dyn EmbeddingProvider + Send + Sync> = Arc::new(NoOpEmbedder);
        assert_eq!(provider.dimension(), 0);
        let result = provider.embed(&["hello"]).await.unwrap();
        assert!(result.is_empty(), "NoOpEmbedder should return no vectors");
        // Send bound: tokio::spawn으로 스레드 이동 가능한지 확인
        let p = Arc::clone(&provider);
        let dim = tokio::spawn(async move { p.dimension() }).await.unwrap();
        assert_eq!(dim, 0);
    }

    #[test]
    #[ignore = "requires real ONNX model"]
    fn onnx_embedder_dimension_is_384() {
        // Create a dummy file so the constructor doesn't fail on path check
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let embedder = OnnxEmbedder::new(&path).unwrap();
        assert_eq!(embedder.dimension(), 384);
    }

    #[test]
    #[ignore = "requires real ONNX model"]
    fn onnx_embedder_model_info_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let embedder = OnnxEmbedder::new(&path).unwrap();
        assert_eq!(embedder.model_info().name, "multilingual-e5-small");
    }

    #[tokio::test]
    #[ignore = "requires real ONNX model"]
    async fn onnx_embedder_empty_input_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let embedder = OnnxEmbedder::new(&path).unwrap();
        let result = embedder.embed(&[]).await;
        assert!(matches!(result, Err(EmbeddingError::EmptyInput)));
    }

    #[test]
    fn ollama_embedder_dimension_is_384() {
        let e = OllamaEmbedder::new("http://localhost:11434", "all-minilm", 384);
        assert_eq!(e.dimension(), 384);
    }

    #[tokio::test]
    async fn ollama_embedder_empty_input_errors() {
        let e = OllamaEmbedder::new("http://localhost:11434", "all-minilm", 384);
        let result = e.embed(&[]).await;
        assert!(matches!(result, Err(EmbeddingError::EmptyInput)));
    }

    // ── TDD: 5 required tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn mock_embedder_returns_correct_dimension() {
        let embedder = MockEmbedder::new(64);
        let result = embedder.embed(&["a", "b", "c"]).await.unwrap();
        assert_eq!(result.len(), 3);
        for vec in &result {
            assert_eq!(vec.len(), 64);
        }
    }

    #[tokio::test]
    async fn mock_embedder_is_deterministic() {
        let embedder = MockEmbedder::new(32);
        let r1 = embedder.embed(&["hello"]).await.unwrap();
        let r2 = embedder.embed(&["hello"]).await.unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn embedding_error_display() {
        assert_eq!(
            EmbeddingError::ModelLoad("bad path".to_string()).to_string(),
            "model load failed: bad path"
        );
        assert_eq!(
            EmbeddingError::Inference("segfault".to_string()).to_string(),
            "inference failed: segfault"
        );
        assert_eq!(
            EmbeddingError::Http("timeout".to_string()).to_string(),
            "HTTP error: timeout"
        );
    }

    #[test]
    fn ollama_embedder_default_nomic_dimension() {
        let e = OllamaEmbedder::default_nomic();
        assert_eq!(e.dimension(), 768);
    }

    #[tokio::test]
    async fn batch_embed_returns_one_per_input() {
        let embedder = MockEmbedder::new(16);
        let texts = ["a", "b", "c", "d", "e"];
        let result = embedder.embed(&texts).await.unwrap();
        assert_eq!(result.len(), 5);
    }

    // ── resolve_model_path TDD tests ─────────────────────────────────────────

    #[test]
    #[serial_test::serial(doxus_model_path_env)]
    fn resolve_model_path_env_override() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.onnx");
        let tokenizer = dir.path().join("tokenizer.json");
        std::fs::write(&model, b"dummy").unwrap();
        std::fs::write(&tokenizer, b"{}").unwrap();

        std::env::set_var("DOXUS_MODEL_PATH", &model);
        let result = resolve_model_path();
        std::env::remove_var("DOXUS_MODEL_PATH");

        assert_eq!(result, Some(model));
    }

    #[test]
    #[serial_test::serial(doxus_model_path_env)]
    fn resolve_model_path_requires_tokenizer_colocated() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.onnx");
        std::fs::write(&model, b"dummy").unwrap();
        // No tokenizer.json — should NOT return this path

        std::env::set_var("DOXUS_MODEL_PATH", &model);
        let result = resolve_model_path();
        std::env::remove_var("DOXUS_MODEL_PATH");

        assert_ne!(result, Some(model));
    }

    #[test]
    #[serial_test::serial(doxus_model_path_env)]
    fn resolve_model_path_with_tokenizer_returns_path() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.onnx");
        let tokenizer = dir.path().join("tokenizer.json");
        std::fs::write(&model, b"dummy").unwrap();
        std::fs::write(&tokenizer, b"{}").unwrap();

        std::env::set_var("DOXUS_MODEL_PATH", &model);
        let result = resolve_model_path();
        std::env::remove_var("DOXUS_MODEL_PATH");

        assert_eq!(result, Some(model));
    }

    #[test]
    #[serial_test::serial(doxus_model_path_env)]
    fn from_default_path_errors_when_env_points_to_missing_file_and_no_model_installed() {
        // Point DOXUS_MODEL_PATH at nonexistent path; if no real model exists anywhere,
        // from_default_path must return Err (not panic).
        std::env::set_var("DOXUS_MODEL_PATH", "/nonexistent/no-such-model.onnx");
        let result = OnnxEmbedder::from_default_path();
        std::env::remove_var("DOXUS_MODEL_PATH");
        // If a real model happens to be installed on this machine it returns Ok, that's fine.
        // We only verify: no panic, and if Err then it's ModelLoad variant.
        if let Err(e) = result {
            assert!(matches!(e, EmbeddingError::ModelLoad(_)), "unexpected error: {e}");
        }
    }

    // ── trait object usage test ───────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "requires model file at ~/.doxus/models/all-MiniLM-L6-v2/model.onnx — run scripts/download-model.sh"]
    async fn onnx_embedder_produces_384_dim_vectors() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let path = format!("{home}/.doxus/models/all-MiniLM-L6-v2/model.onnx");
        let embedder = OnnxEmbedder::new(path).unwrap();
        let result = embedder
            .embed(&["hello world", "foo bar", "test"])
            .await
            .unwrap();
        assert_eq!(result.len(), 3);
        for vec in &result {
            assert_eq!(vec.len(), 384);
        }
    }

    #[tokio::test]
    #[ignore = "requires model file at ~/.doxus/models/all-MiniLM-L6-v2/model.onnx — run scripts/download-model.sh"]
    async fn onnx_embedder_similar_texts_high_cosine() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let path = format!("{home}/.doxus/models/all-MiniLM-L6-v2/model.onnx");
        let embedder = OnnxEmbedder::new(path).unwrap();
        let result = embedder
            .embed(&[
                "The cat sat on the mat",
                "A cat is sitting on a mat",
                "Completely different topic about rockets",
            ])
            .await
            .unwrap();

        let sim_similar = cosine_similarity(&result[0], &result[1]);
        let sim_different = cosine_similarity(&result[0], &result[2]);

        assert!(
            sim_similar > 0.8,
            "similar texts should have high similarity: {sim_similar}"
        );
        assert!(
            sim_similar > sim_different,
            "similar pair should be more similar"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn onnx_embedder_lazy_loading_and_drop() {
        let path = resolve_model_path();
        if path.is_none() {
            println!("Skipping onnx_embedder_lazy_loading_and_drop test: real model not found");
            return;
        }
        let path = path.unwrap();

        // 1. 생성 시점 검증 (Lazy Loading)
        let mut embedder = OnnxEmbedder::new(&path).unwrap();
        embedder = embedder.with_ttl(std::time::Duration::from_millis(100));

        // 생성 직후에는 세션이 None이어야 함 (실패 지점 1)
        {
            let session_guard = embedder.session.lock().unwrap();
            assert!(session_guard.is_none(), "Session should be None initially (Lazy Loading)");
        }

        // 2. embed 호출 시점 검증 (On-demand loading)
        let result = embedder.embed(&["hello world"]).await;
        assert!(result.is_ok(), "Embedding should succeed with real model");

        // 호출 직후에는 세션이 Some이어야 함
        {
            let session_guard = embedder.session.lock().unwrap();
            assert!(session_guard.is_some(), "Session should be loaded (Some) after embed call");
        }

        // 3. TTL 이후 드롭 검증 (자동 드롭)
        // 100ms TTL이 지나도록 200ms 대기
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // 세션이 None으로 드롭되어 있어야 함 (실패 지점 2)
        {
            let session_guard = embedder.session.lock().unwrap();
            assert!(session_guard.is_none(), "Session should be dropped (None) after inactivity TTL");
        }
    }

    fn get_current_rss_kb() -> Option<usize> {
        let pid = std::process::id();
        let output = std::process::Command::new("ps")
            .args(&["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let rss_str = String::from_utf8_lossy(&output.stdout);
        rss_str.trim().parse::<usize>().ok()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn onnx_embedder_rss_memory_monitoring() {
        let path = resolve_model_path();
        if path.is_none() {
            println!("Skipping onnx_embedder_rss_memory_monitoring test: real model not found");
            return;
        }
        let path = path.unwrap();

        // 1. 임베더를 아주 짧은 TTL로 생성
        let mut embedder = OnnxEmbedder::new(&path).unwrap();
        embedder = embedder.with_ttl(std::time::Duration::from_millis(100));

        let rss_start = get_current_rss_kb().unwrap_or(0);
        println!("RSS Start: {} KB", rss_start);

        // 2. 대량의 임베딩 연산 수행하여 모델 로드 유도
        let dummy_texts: Vec<String> = (0..100)
            .map(|i| format!("This is dummy text paragraph number {} to perform bulk embedding memory testing.", i))
            .collect();
        let text_refs: Vec<&str> = dummy_texts.iter().map(|s| s.as_str()).collect();

        let result = embedder.embed(&text_refs).await;
        assert!(result.is_ok(), "Bulk embedding should succeed");

        let rss_loaded = get_current_rss_kb().unwrap_or(0);
        println!("RSS Loaded: {} KB (Diff: {} KB)", rss_loaded, rss_loaded.saturating_sub(rss_start));

        // 3. TTL 만료 및 드롭 대기 (200ms)
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // 세션이 드롭되어 있어야 함
        {
            let session_guard = embedder.session.lock().unwrap();
            assert!(session_guard.is_none(), "Session should be dropped after TTL");
        }

        let rss_dropped = get_current_rss_kb().unwrap_or(0);
        println!("RSS Dropped: {} KB (Diff from Loaded: {} KB)", rss_dropped, (rss_dropped as i64) - (rss_loaded as i64));
    }

    #[test]
    #[serial_test::serial(doxus_embedder_env)]
    fn test_get_default_embedder_resolution() {
        // 1. DOXUS_EMBEDDING_PROVIDER=noop 인 경우 NoOpEmbedder 반환 검증
        std::env::set_var("DOXUS_EMBEDDING_PROVIDER", "noop");
        let embedder = get_default_embedder();
        std::env::remove_var("DOXUS_EMBEDDING_PROVIDER");
        assert_eq!(embedder.dimension(), 0, "NoOpEmbedder should have 0 dimension");

        // 2. DOXUS_SKIP_ONNX=1 인 경우 OllamaEmbedder로 폴백하는지 검증
        std::env::set_var("DOXUS_SKIP_ONNX", "1");
        std::env::set_var("DOXUS_OLLAMA_DIM", "768");
        let embedder = get_default_embedder();
        std::env::remove_var("DOXUS_SKIP_ONNX");
        std::env::remove_var("DOXUS_OLLAMA_DIM");
        assert_eq!(embedder.dimension(), 768, "Ollama fallback should return 768 dimension");
    }
}
