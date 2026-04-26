//! Model downloader for ONNX embedding models.
//!
//! Downloads `multilingual-e5-small` model and tokenizer from HuggingFace to
//! a target directory, streaming the response so progress can be reported as
//! chunks arrive. Verifies SHA256 checksum of the model file after download
//! and cleans up partial files on any failure.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;


/// HuggingFace URL for the int8-quantized ONNX model (primary — 4x smaller, 2-4x faster on CPU).
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/intfloat/multilingual-e5-small/resolve/main/onnx/model_quantized.onnx";

/// HuggingFace URL for the fp32 ONNX model (fallback if quantized is unavailable).
pub const DEFAULT_MODEL_FP32_URL: &str =
    "https://huggingface.co/intfloat/multilingual-e5-small/resolve/main/onnx/model.onnx";

/// HuggingFace URL for the tokenizer.
pub const DEFAULT_TOKENIZER_URL: &str =
    "https://huggingface.co/intfloat/multilingual-e5-small/resolve/main/tokenizer.json";

/// File name of the int8 model on disk (matches `resolve_model_path` int8 preference).
pub const MODEL_FILE_NAME: &str = "multilingual-e5-small-int8.onnx";

/// File name of the tokenizer on disk.
pub const TOKENIZER_FILE_NAME: &str = "tokenizer.json";

#[derive(Debug, Error)]
pub enum ModelDownloadError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("server returned status {0}")]
    Status(u16),
}

impl From<std::io::Error> for ModelDownloadError {
    fn from(e: std::io::Error) -> Self {
        ModelDownloadError::Io(e.to_string())
    }
}

impl From<reqwest::Error> for ModelDownloadError {
    fn from(e: reqwest::Error) -> Self {
        ModelDownloadError::Http(e.to_string())
    }
}

/// Progress update emitted while downloading model files.
#[derive(Debug, Clone)]
pub struct ModelDownloadProgress {
    /// Which file is downloading: `"model"` or `"tokenizer"`.
    pub file: &'static str,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    /// 0.0 ~ 100.0 (based on total when known, else last known byte count).
    pub percent: f32,
}

/// Options for configuring the downloader (mainly for tests to inject URLs).
#[derive(Debug, Clone)]
pub struct ModelDownloadOptions {
    pub model_url: String,
    pub tokenizer_url: String,
    /// Expected SHA256 checksum of the model file (hex-encoded). `None` skips checksum check.
    pub model_sha256: Option<String>,
}

impl Default for ModelDownloadOptions {
    fn default() -> Self {
        Self {
            model_url: DEFAULT_MODEL_URL.to_string(),
            tokenizer_url: DEFAULT_TOKENIZER_URL.to_string(),
            // int8 quantized model: no pinned checksum (official HuggingFace file, no fp32 hash)
            model_sha256: None,
        }
    }
}

/// Download the ONNX model + tokenizer into `target_dir`, reporting progress via `on_progress`.
///
/// On any failure (HTTP, I/O, checksum) the partially written files are removed.
pub async fn download_model<F>(
    target_dir: &Path,
    on_progress: F,
) -> Result<(), ModelDownloadError>
where
    F: Fn(ModelDownloadProgress) + Send + Sync + 'static,
{
    download_model_with_options(target_dir, ModelDownloadOptions::default(), on_progress).await
}

/// Test-friendly variant that accepts custom URLs / checksum.
pub async fn download_model_with_options<F>(
    target_dir: &Path,
    opts: ModelDownloadOptions,
    on_progress: F,
) -> Result<(), ModelDownloadError>
where
    F: Fn(ModelDownloadProgress) + Send + Sync + 'static,
{
    tokio::fs::create_dir_all(target_dir).await?;

    let model_path = target_dir.join(MODEL_FILE_NAME);
    let tokenizer_path = target_dir.join(TOKENIZER_FILE_NAME);

    let client = reqwest::Client::builder()
        .user_agent("doxus-desktop/model-downloader")
        .build()
        .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

    // Step 1: model.onnx (verify checksum)
    let model_result = download_with_progress(
        &client,
        &opts.model_url,
        &model_path,
        "model",
        opts.model_sha256.as_deref(),
        &on_progress,
    )
    .await;

    if let Err(e) = model_result {
        cleanup(&[&model_path, &tokenizer_path]).await;
        return Err(e);
    }

    // Step 2: tokenizer.json (no checksum check; HF file is stable but no pinned hash)
    let tok_result = download_with_progress(
        &client,
        &opts.tokenizer_url,
        &tokenizer_path,
        "tokenizer",
        None,
        &on_progress,
    )
    .await;

    if let Err(e) = tok_result {
        cleanup(&[&model_path, &tokenizer_path]).await;
        return Err(e);
    }

    Ok(())
}

async fn download_with_progress<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    file_tag: &'static str,
    expected_sha256: Option<&str>,
    on_progress: &F,
) -> Result<(), ModelDownloadError>
where
    F: Fn(ModelDownloadProgress) + Send + Sync + 'static,
{
    use futures_util::StreamExt;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(ModelDownloadError::Status(resp.status().as_u16()));
    }
    let total_bytes = resp.content_length();

    let mut file = tokio::fs::File::create(dest).await?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();

    // Emit initial progress tick
    on_progress(ModelDownloadProgress {
        file: file_tag,
        bytes_downloaded: 0,
        total_bytes,
        percent: 0.0,
    });

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;

        let percent = match total_bytes {
            Some(total) if total > 0 => (downloaded as f64 / total as f64 * 100.0) as f32,
            _ => 0.0,
        };
        on_progress(ModelDownloadProgress {
            file: file_tag,
            bytes_downloaded: downloaded,
            total_bytes,
            percent,
        });
    }
    file.flush().await?;
    drop(file);

    if let Some(expected) = expected_sha256 {
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(ModelDownloadError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    // Final 100% tick (some servers don't send content_length).
    on_progress(ModelDownloadProgress {
        file: file_tag,
        bytes_downloaded: downloaded,
        total_bytes,
        percent: 100.0,
    });

    Ok(())
}

async fn cleanup(paths: &[&PathBuf]) {
    for p in paths {
        if tokio::fs::try_exists(p).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(p).await;
        }
    }
}

#[cfg(test)]
mod downloader_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;

    fn test_target() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // Pre-computed SHA256 of b"fake-onnx-model-bytes"
    const FAKE_MODEL_BODY: &[u8] = b"fake-onnx-model-bytes";
    // Known-good computed SHA256 for the body above.
    fn fake_model_sha256() -> String {
        let mut h = Sha256::new();
        h.update(FAKE_MODEL_BODY);
        hex::encode(h.finalize())
    }

    #[tokio::test]
    async fn download_reports_progress_callbacks() {
        let mut server = mockito::Server::new_async().await;
        let model_mock = server
            .mock("GET", "/model.onnx")
            .with_status(200)
            .with_header("content-length", &FAKE_MODEL_BODY.len().to_string())
            .with_body(FAKE_MODEL_BODY)
            .create_async()
            .await;
        let tok_mock = server
            .mock("GET", "/tokenizer.json")
            .with_status(200)
            .with_header("content-length", "2")
            .with_body("{}")
            .create_async()
            .await;

        let target = test_target();
        let updates = Arc::new(Mutex::new(Vec::<ModelDownloadProgress>::new()));
        let updates_cb = updates.clone();

        let opts = ModelDownloadOptions {
            model_url: format!("{}/model.onnx", server.url()),
            tokenizer_url: format!("{}/tokenizer.json", server.url()),
            model_sha256: Some(fake_model_sha256()),
        };

        download_model_with_options(target.path(), opts, move |p| {
            updates_cb.lock().unwrap().push(p);
        })
        .await
        .expect("download should succeed");

        model_mock.assert_async().await;
        tok_mock.assert_async().await;

        let updates = updates.lock().unwrap();
        assert!(!updates.is_empty(), "expected at least one progress callback");
        assert!(updates.iter().any(|p| p.file == "model"), "expected model progress");
        assert!(updates.iter().any(|p| p.file == "tokenizer"), "expected tokenizer progress");
        // At least one update hit 100%.
        assert!(
            updates.iter().any(|p| (p.percent - 100.0).abs() < 0.01),
            "expected a 100% progress tick"
        );

        assert!(target.path().join(MODEL_FILE_NAME).exists());
        assert!(target.path().join(TOKENIZER_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn download_fails_on_checksum_mismatch() {
        let mut server = mockito::Server::new_async().await;
        let _model_mock = server
            .mock("GET", "/model.onnx")
            .with_status(200)
            .with_body(FAKE_MODEL_BODY)
            .create_async()
            .await;
        // Tokenizer mock isn't hit because model fails first, but define it for safety
        let _tok_mock = server
            .mock("GET", "/tokenizer.json")
            .with_status(200)
            .with_body("{}")
            .expect(0)
            .create_async()
            .await;

        let target = test_target();
        let opts = ModelDownloadOptions {
            model_url: format!("{}/model.onnx", server.url()),
            tokenizer_url: format!("{}/tokenizer.json", server.url()),
            model_sha256: Some(
                "0000000000000000000000000000000000000000000000000000000000000000".into(),
            ),
        };

        let result = download_model_with_options(target.path(), opts, |_| {}).await;
        assert!(matches!(result, Err(ModelDownloadError::ChecksumMismatch { .. })));
        // Partial file should be cleaned up.
        assert!(
            !target.path().join(MODEL_FILE_NAME).exists(),
            "partial model.onnx should be removed on checksum failure"
        );
    }

    #[tokio::test]
    async fn download_cleans_up_partial_files_on_failure() {
        let mut server = mockito::Server::new_async().await;
        // Model OK but tokenizer returns 500 — model.onnx was written, tokenizer.json was not.
        // After failure BOTH must be removed (cleanup contract).
        let _model_mock = server
            .mock("GET", "/model.onnx")
            .with_status(200)
            .with_body(FAKE_MODEL_BODY)
            .create_async()
            .await;
        let _tok_mock = server
            .mock("GET", "/tokenizer.json")
            .with_status(500)
            .with_body("nope")
            .create_async()
            .await;

        let target = test_target();
        let opts = ModelDownloadOptions {
            model_url: format!("{}/model.onnx", server.url()),
            tokenizer_url: format!("{}/tokenizer.json", server.url()),
            model_sha256: Some(fake_model_sha256()),
        };

        let result = download_model_with_options(target.path(), opts, |_| {}).await;
        assert!(result.is_err(), "expected error from tokenizer 500");
        match result {
            Err(ModelDownloadError::Status(s)) => assert_eq!(s, 500),
            other => panic!("expected Status(500) error, got {other:?}"),
        }

        assert!(
            !target.path().join(MODEL_FILE_NAME).exists(),
            "model.onnx must be removed after tokenizer download failure"
        );
        assert!(
            !target.path().join(TOKENIZER_FILE_NAME).exists(),
            "tokenizer.json must not remain"
        );
    }
}
