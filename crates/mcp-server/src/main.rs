use anyhow::Result;
use doxus_mcp::{McpRequest, McpResponse, McpServer};
use serde_json::json;
use std::io::{BufRead, Write};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info")
        .init();

    tracing::info!("doxus-mcp starting on stdio");

    let db_path = std::env::var("DOXUS_DB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".doxus/db/doxus.db")
        });

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = doxus_core::db::open(&db_path)?;

    // Attempt to initialize OnnxEmbedder; fall back gracefully if model is absent.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let model_path = std::path::PathBuf::from(&home)
        .join(".doxus/models/all-MiniLM-L6-v2/model.onnx");
    // TODO: Pass embedder to McpServer once it accepts an EmbeddingProvider.
    // Currently McpServer::new() only takes a Connection; vector search requires
    // extending the constructor (tracked for Phase 1).
    match doxus_core::embedding::OnnxEmbedder::new(&model_path) {
        Ok(_embedder) => {
            tracing::info!(
                "OnnxEmbedder loaded from {} but McpServer does not yet accept an embedder; \
                 running in FTS-only mode. Vector search will be enabled once McpServer is extended.",
                model_path.display()
            );
        }
        Err(e) => {
            tracing::warn!(
                "OnnxEmbedder unavailable ({}); vector search disabled. \
                 Run scripts/download-model.sh to enable.",
                e
            );
        }
    }

    let server = McpServer::new(conn);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        // Notifications (no "id" field) are fire-and-forget — skip silently.
        let is_notification = serde_json::from_str::<serde_json::Value>(&line)
            .map(|v| v.get("id").is_none())
            .unwrap_or(false);
        if is_notification {
            continue;
        }

        let response = match serde_json::from_str::<McpRequest>(&line) {
            Ok(req) => {
                let id = req.id.clone();
                server.dispatch(&req.method, id, req.params.as_ref())
            }
            Err(e) => McpResponse::err(json!(null), -32700, format!("parse error: {e}")),
        };

        let json = serde_json::to_string(&response)?;
        writeln!(out, "{json}")?;
        out.flush()?;
    }

    Ok(())
}
