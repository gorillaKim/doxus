use async_trait::async_trait;
use thiserror::Error;

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
    // ort session and tokenizer will be added when ort compiles
    // Placeholder to keep the struct compilable for now
    _model_path: std::path::PathBuf,
}

impl OnnxEmbedder {
    pub fn new(model_path: impl Into<std::path::PathBuf>) -> Result<Self, EmbeddingError> {
        let path = model_path.into();
        if !path.exists() {
            return Err(EmbeddingError::ModelLoad(format!(
                "model not found at {}",
                path.display()
            )));
        }
        Ok(Self {
            info: ModelInfo {
                name: "all-MiniLM-L6-v2".to_string(),
                dimension: 384,
                max_tokens: 256,
            },
            _model_path: path,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OnnxEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        // TODO: implement actual ONNX inference in Phase 0-A
        // For now return deterministic dummy vectors for compilation
        Err(EmbeddingError::Inference(
            "ONNX inference not yet implemented — model loading pending".to_string(),
        ))
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
}
