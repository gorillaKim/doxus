// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use doxus_desktop_lib::AppState;

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
    let state = AppState::new(conn, plugins_dir);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_oauth::init())
        .setup(|app| {
            // dev 모드에서 doxus:// 스킴을 macOS에 런타임 등록
            #[cfg(debug_assertions)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register("doxus").ok();
            }
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            doxus_desktop_lib::commands::market::market_list_installed,
            doxus_desktop_lib::commands::market::get_workspaces,
            doxus_desktop_lib::commands::market::get_system_status,
            doxus_desktop_lib::commands::market::get_plugin_logs,
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
            doxus_desktop_lib::commands::search::search_engine_status,
            doxus_desktop_lib::commands::search::trigger_reindex,
            doxus_desktop_lib::commands::workspace::list_workspace_documents,
            doxus_desktop_lib::commands::workspace::create_workspace_document,
            doxus_desktop_lib::commands::agent::agent_send_message,
            doxus_desktop_lib::commands::agent::agent_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
