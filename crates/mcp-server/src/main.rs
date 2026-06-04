use anyhow::Result;
use doxus_mcp::{http_server::build_router, sync_loop::spawn_sync_loop, McpRequest, McpResponse, McpServer};
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::Arc;


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

    // Main connection pool used by McpServer.
    let conn = doxus_core::db::create_pool(&db_path)?;
    
    // Cleanup expired content cache on startup
    {
        // For ContentCache we need a temporary connection from pool
        let temp_conn = conn.get()?;
        let cache = doxus_core::cache::ContentCache::new(&temp_conn);
        if let Ok(count) = cache.cleanup_expired() {
            if count > 0 {
                tracing::info!("Purged {} expired cache entries on startup", count);
            }
        }
    }

    // Separate connection pool for the background sync loop (SQLite WAL mode supports
    // concurrent readers; the sync loop only reads due-instance metadata).
    let sync_conn = doxus_core::db::create_pool(&db_path)?;

    // Default sync interval: 10800 s (3 hours).  Override via DOXUS_SYNC_INTERVAL_SECS.
    let interval_secs: u64 = std::env::var("DOXUS_SYNC_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10800);

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let plugins_dir = std::path::PathBuf::from(&home).join(".doxus/plugins");
    let mut plugin_manager = doxus_core::plugin::PluginManager::new(plugins_dir);
    plugin_manager.register_factory("com.doxus.obsidian", || {
        Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
    });
    plugin_manager.register_factory("com.doxus.confluence", || {
        Box::new(doxus_plugin_confluence::ConfluencePlugin::new())
    });
    plugin_manager.register_factory("com.doxus.github", || {
        Box::new(doxus_plugin_github::GitHubPlugin::new())
    });
    let plugin_manager = std::sync::Arc::new(plugin_manager);



    // Initialize with None to ensure fast handshake.
    let plugins_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".doxus/plugins");
    let server = McpServer::new(conn.clone(), db_path.clone(), None, Arc::clone(&plugin_manager), plugins_dir);
    let embedder_handle = server.embedder_arc();
    
    // Background thread to load default embedder (only if enabled via env)
    if std::env::var("DOXUS_ENABLE_ONNX").is_ok() {
        std::thread::spawn(move || {
            tracing::info!("[MCP] Resolving default embedder...");
            let embedder = doxus_core::embedding::get_default_embedder();
            if embedder.dimension() > 0 {
                match embedder_handle.lock() {
                    Ok(mut guard) => {
                        *guard = Some(embedder);
                        tracing::info!("[MCP] Embedder load complete. Hybrid search enabled.");
                    }
                    Err(err) => {
                        tracing::error!("[MCP] embedder mutex poisoned, cannot register embedder: {err}");
                    }
                }
            } else {
                tracing::warn!("[MCP] No active embedding model available. Vector search disabled.");
            }
        });
    } else {
        tracing::info!("[MCP] ONNX auto-load disabled. Vector search tools will be unavailable.");
    }

    // Sync loop (only if enabled via env)
    let sync_handle_opt = if std::env::var("DOXUS_ENABLE_SYNC").is_ok() {
        Some(spawn_sync_loop(sync_conn, server.embedder_arc(), Arc::clone(&plugin_manager), interval_secs))
    } else {
        tracing::info!("[MCP] Background sync disabled in sidecar.");
        None
    };

    // Parse --http [PORT] and --token TOKEN flags
    let args: Vec<String> = std::env::args().collect();
    let http_port = args
        .windows(2)
        .find(|w| w[0] == "--http")
        .and_then(|w| w[1].parse::<u16>().ok());
    let token = args
        .windows(2)
        .find(|w| w[0] == "--token")
        .map(|w| w[1].clone())
        .or_else(|| std::env::var("DOXUS_MCP_TOKEN").ok())
        .unwrap_or_default();

    if let Some(port) = http_port {
        let server = Arc::new(server);
        // Bind early so we can report the actual port before blocking on serve.
        let listener =
            tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
        let actual_port = listener.local_addr()?.port();

        // Print PORT= line so callers (e.g. Tauri) can discover the bound port.
        println!("PORT={actual_port}");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        tracing::info!("doxus-mcp HTTP server listening on port {actual_port}");

        let app = build_router(Arc::clone(&server), token);

        // Graceful shutdown of sync loop on ctrl-c.
        tokio::select! {
            r = axum::serve(listener, app) => {
                r?;
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("ctrl-c received, shutting down HTTP server");
            }
        }

        if let Some(h) = sync_handle_opt {
            h.shutdown().await;
        }
        return Ok(());
    }

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
                        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
                        server.dispatch(&req.method, id, req.params.as_ref()).await
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
    if let Some(h) = sync_handle_opt {
        h.shutdown().await;
    }

    result
}
