use anyhow::Result;
use doxus_mcp::{sync_loop::spawn_sync_loop, McpRequest, McpResponse, McpServer};
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

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

    // Main connection used by McpServer.
    let conn = doxus_core::db::open(&db_path)?;

    // Separate connection for the background sync loop (SQLite WAL mode supports
    // concurrent readers; the sync loop only reads due-instance metadata).
    let sync_conn = doxus_core::db::open(&db_path)?;
    let sync_conn = Arc::new(Mutex::new(sync_conn));

    // Default sync interval: 3600 s (1 hour).  Override via DOXUS_SYNC_INTERVAL_SECS.
    let interval_secs: u64 = std::env::var("DOXUS_SYNC_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let plugins_dir = std::path::PathBuf::from(&home).join(".doxus/plugins");
    let mut plugin_manager = doxus_core::plugin::PluginManager::new(plugins_dir);
    plugin_manager.register_factory("com.doxus.obsidian", || {
        Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
    });
    let plugin_manager = std::sync::Arc::new(plugin_manager);

    let sync_handle = spawn_sync_loop(sync_conn, plugin_manager, interval_secs);

    // Attempt to initialize OnnxEmbedder; fall back gracefully if model is absent.
    let model_path = std::path::PathBuf::from(&home)
        .join(".doxus/models/all-MiniLM-L6-v2/model.onnx");
    let embedder: Option<std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider>> =
        match doxus_core::embedding::OnnxEmbedder::new(&model_path) {
            Ok(e) => {
                tracing::info!(
                    "OnnxEmbedder loaded from {}; hybrid search enabled.",
                    model_path.display()
                );
                Some(std::sync::Arc::new(e))
            }
            Err(e) => {
                tracing::warn!(
                    "OnnxEmbedder unavailable ({}); running in FTS-only mode. \
                     Run scripts/download-model.sh to enable vector search.",
                    e
                );
                None
            }
        };

    let plugins_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".doxus/plugins");
    let server = McpServer::new(conn, embedder, plugins_dir);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    // Run the stdio MCP loop; exit on EOF or ctrl-c.
    let result: Result<()> = tokio::select! {
        r = async {
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
            Ok::<(), anyhow::Error>(())
        } => r,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("ctrl-c received, shutting down");
            Ok(())
        }
    };

    // Graceful shutdown of the sync loop.
    sync_handle.shutdown().await;

    result
}
