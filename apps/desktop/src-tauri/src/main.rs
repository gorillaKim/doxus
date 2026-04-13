// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use doxus_desktop_lib::AppState;
use tauri::Emitter;


fn find_sidecar_script() -> std::path::PathBuf {
    // 1. 환경변수 오버라이드 (개발/테스트용)
    if let Ok(p) = std::env::var("DOXUS_SIDECAR_PATH") {
        let path = std::path::PathBuf::from(p);
        if path.exists() { return path; }
    }

    let mut candidates = vec![
        // macOS 프로덕션 번들: MacOS/../Resources/sidecar/
        std::env::current_exe().ok()
            .and_then(|exe| exe.parent()?.parent().map(|p| p.join("Resources/sidecar/agent-bridge.mjs")))
            .unwrap_or_default(),
        // dev: src-tauri 기준 상대 경로
        std::path::PathBuf::from("sidecar/agent-bridge.mjs"),
        // dev: workspace root 기준
        std::path::PathBuf::from("apps/desktop/src-tauri/sidecar/agent-bridge.mjs"),
        // Tauri dev cwd
        std::env::current_dir().unwrap_or_default()
            .join("apps/desktop/src-tauri/sidecar/agent-bridge.mjs"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            eprintln!("[sidecar] found: {}", candidate.display());
            return candidate.clone();
        }
    }

    eprintln!("[sidecar] WARNING: agent-bridge.mjs not found, using fallback path");
    candidates.remove(1) // sidecar/agent-bridge.mjs — best guess
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let db_path = std::env::var("DOXUS_DB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let p = std::path::PathBuf::from(&home).join(".doxus/db");
            std::fs::create_dir_all(&p).ok();
            p.join("doxus.db")
        });
    let conn = doxus_core::db::open(&db_path).expect("failed to open db");
    let plugins_dir = std::path::PathBuf::from(&home).join(".doxus/plugins");
    let sidecar_script = find_sidecar_script();
    let embedder: std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync> =
        doxus_core::embedding::OnnxEmbedder::from_default_path()
            .map(|e| std::sync::Arc::new(e) as std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync>)
            .unwrap_or_else(|e| {
                eprintln!("[embedding] ONNX load failed: {e}, falling back to no-op");
                std::sync::Arc::new(doxus_core::embedding::NoOpEmbedder)
            });
    let state = AppState::new(conn, plugins_dir, sidecar_script, embedder);
    let conn_arc = state.conn.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_oauth::init())
        .setup(move |app| {
            #[cfg(debug_assertions)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register("doxus").ok();
            }

            // Spawn background cache cleanup task (every 30 minutes)
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(30 * 60));
                interval.tick().await; // skip immediate tick (startup cleanup done in AppState::new)
                loop {
                    interval.tick().await;
                    if let Ok(conn) = conn_arc.lock() {
                        let cache = doxus_core::cache::ContentCache::new(&conn);
                        match cache.cleanup_expired() {
                            Ok(n) if n > 0 => {
                                eprintln!("[cache] scheduler removed {n} expired entries");
                                handle.emit("cache:cleanup", serde_json::json!({ "count": n })).ok();
                            }
                            Err(e) => eprintln!("[cache] cleanup error: {e}"),
                            _ => {}
                        }
                    }
                }
            });

            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            doxus_desktop_lib::commands::market::market_list_installed,
            doxus_desktop_lib::commands::market::market_fetch_registry,
            doxus_desktop_lib::commands::market::market_fetch_guide,
            doxus_desktop_lib::commands::market::get_workspaces,
            doxus_desktop_lib::commands::market::plugin_get_cache_ttl,
            doxus_desktop_lib::commands::market::plugin_set_cache_ttl,
            doxus_desktop_lib::commands::market::get_system_status,
            doxus_desktop_lib::commands::market::get_plugin_logs,
            doxus_desktop_lib::commands::market::clear_audit_log,
            doxus_desktop_lib::commands::market::get_embedding_status,
            doxus_desktop_lib::commands::market::trigger_sync,
            doxus_desktop_lib::commands::market::market_install_plugin,
            doxus_desktop_lib::commands::market::market_uninstall_plugin,
            doxus_desktop_lib::commands::market::plugin_save_auth,
            doxus_desktop_lib::commands::market::plugin_get_auth_status,
            doxus_desktop_lib::commands::market::plugin_start_oauth,
            doxus_desktop_lib::commands::market::plugin_oauth_exchange,
            doxus_desktop_lib::commands::market::plugin_validate_config,
            doxus_desktop_lib::commands::market::plugin_open_url,
            doxus_desktop_lib::commands::market::check_claude_status,
            doxus_desktop_lib::commands::market::check_gemini_status,
            doxus_desktop_lib::commands::search::search_documents,
            doxus_desktop_lib::commands::search::list_projects,
            doxus_desktop_lib::commands::search::add_project,
            doxus_desktop_lib::commands::search::toggle_project_status,
            doxus_desktop_lib::commands::search::remove_project,
            doxus_desktop_lib::commands::search::search_engine_status,
            doxus_desktop_lib::commands::search::trigger_reindex,
            doxus_desktop_lib::commands::search::index_project,
            doxus_desktop_lib::commands::search::increment_view_count,
            doxus_desktop_lib::commands::search::get_top_documents,
            doxus_desktop_lib::commands::search::get_document_content,
            doxus_desktop_lib::commands::search::list_all_documents,
            doxus_desktop_lib::commands::workspace::ensure_default_workspace_cmd,
            doxus_desktop_lib::commands::workspace::list_workspace_documents,
            doxus_desktop_lib::commands::workspace::create_workspace_document,
            doxus_desktop_lib::commands::workspace::update_workspace_document,
            doxus_desktop_lib::commands::workspace::delete_workspace_document,
            doxus_desktop_lib::commands::workspace::get_document_sections,
            doxus_desktop_lib::commands::workspace::update_document_section,
            doxus_desktop_lib::commands::workspace::insert_document_section,
            doxus_desktop_lib::commands::workspace::delete_document_section,
            doxus_desktop_lib::commands::workspace::list_templates,
            doxus_desktop_lib::commands::workspace::get_template,
            doxus_desktop_lib::commands::workspace::apply_template,
            doxus_desktop_lib::commands::workspace::create_template,
            doxus_desktop_lib::commands::workspace::update_template,
            doxus_desktop_lib::commands::workspace::delete_template,
            doxus_desktop_lib::commands::workspace::create_document_from_template,
            doxus_desktop_lib::commands::agent::chat_start_session,
            doxus_desktop_lib::commands::agent::chat_send_message,
            doxus_desktop_lib::commands::agent::chat_cancel,
            doxus_desktop_lib::commands::agent::agent_status,
            doxus_desktop_lib::commands::agent::detect_cli_path,
            doxus_desktop_lib::commands::settings::save_settings,
            doxus_desktop_lib::commands::settings::load_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
