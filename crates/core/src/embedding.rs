use async_trait::async_trait;
use ort::{session::Session, value::TensorRef};
use std::sync::Mutex;
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
    #[error("empty input")]
    EmptyInput,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub dimension: usize,
    pub max_tokens: usize,
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

/// ONNX-backed embedding provider using all-MiniLM-L6-v2
pub struct OnnxEmbedder {
    info: ModelInfo,
    /// `None` when constructed with an invalid model file.
    session: Option<Mutex<Session>>,
    /// `None` when constructed without a tokenizer.json.
    tokenizer: Option<Tokenizer>,
}

impl OnnxEmbedder {
    /// Create an embedder from a model `.onnx` file.
    /// The tokenizer is loaded from `<model_dir>/tokenizer.json`.
    /// If the model file is not a valid ONNX model or tokenizer.json is missing,
    /// returns a partially-initialized embedder that returns `Inference` errors on `embed()`.
    pub fn new(model_path: impl Into<std::path::PathBuf>) -> Result<Self, EmbeddingError> {
        let path = model_path.into();
        if !path.exists() {
            return Err(EmbeddingError::ModelLoad(format!(
                "model not found at {}",
                path.display()
            )));
        }

        let session = Session::builder()
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
            .commit_from_file(&path)
            .ok()
            .map(Mutex::new);

        // Tokenizer lives next to the model file as tokenizer.json
        let tokenizer_path = path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("tokenizer.json");

        let tokenizer = Tokenizer::from_file(&tokenizer_path).ok();

        Ok(Self {
            info: ModelInfo {
                name: "all-MiniLM-L6-v2".to_string(),
                dimension: 384,
                max_tokens: 256,
            },
            session,
            tokenizer,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OnnxEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }

        let batch_size = texts.len();

        let tokenizer = self.tokenizer.as_ref().ok_or_else(|| {
            EmbeddingError::Tokenizer("tokenizer not loaded".to_string())
        })?;
        let mut session = self
            .session
            .as_ref()
            .ok_or_else(|| EmbeddingError::Inference("ONNX session not loaded".to_string()))?
            .lock()
            .map_err(|e| EmbeddingError::Inference(format!("session lock poisoned: {e}")))?;

        // Batch tokenize
        let encodings = tokenizer
            .encode_batch(
                texts.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
                true,
            )
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
        let dim = self.info.dimension;
        let seq_len = hidden_shape[1] as usize;
        let mut embeddings = Vec::with_capacity(batch_size);

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
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    model: String,
}

impl OllamaEmbedder {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let model = model.into();
        Self {
            info: ModelInfo {
                name: model.clone(),
                dimension: 384,
                max_tokens: 512,
            },
            base_url: base_url.into(),
            model,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        // TODO: implement HTTP call to Ollama in Phase 1
        Err(EmbeddingError::Inference(
            "Ollama embedding not yet implemented".to_string(),
        ))
    }

    fn dimension(&self) -> usize {
        self.info.dimension
    }

    fn model_info(&self) -> &ModelInfo {
        &self.info
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

    #[test]
    fn onnx_embedder_dimension_is_384() {
        // Create a dummy file so the constructor doesn't fail on path check
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let embedder = OnnxEmbedder::new(&path).unwrap();
        assert_eq!(embedder.dimension(), 384);
    }

    #[test]
    fn onnx_embedder_model_info_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let embedder = OnnxEmbedder::new(&path).unwrap();
        assert_eq!(embedder.model_info().name, "all-MiniLM-L6-v2");
    }

    #[tokio::test]
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
        let e = OllamaEmbedder::new("http://localhost:11434", "all-minilm");
        assert_eq!(e.dimension(), 384);
    }

    #[tokio::test]
    async fn ollama_embedder_empty_input_errors() {
        let e = OllamaEmbedder::new("http://localhost:11434", "all-minilm");
        let result = e.embed(&[]).await;
        assert!(matches!(result, Err(EmbeddingError::EmptyInput)));
    }

    // ── trait object usage test ───────────────────────────────────────────────

    #[test]
    fn embedding_provider_is_object_safe() {
        // Verify the trait can be used as a trait object (dyn EmbeddingProvider)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        std::fs::write(&path, b"dummy").unwrap();
        let _provider: Box<dyn EmbeddingProvider> =
            Box::new(OnnxEmbedder::new(&path).unwrap());
    }

    #[tokio::test]
    #[ignore = "requires ONNX model file"]
    async fn onnx_embedder_produces_384_dim_vectors() {
        let path = "models/all-MiniLM-L6-v2.onnx";
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
    #[ignore = "requires ONNX model file"]
    async fn onnx_embedder_similar_texts_high_cosine() {
        let path = "models/all-MiniLM-L6-v2.onnx";
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
}
