// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use doxus_desktop_lib::AppState;
use tauri::{Emitter, Manager};
use std::sync::Arc;


fn find_bundle_plugins_dir() -> Option<std::path::PathBuf> {
    // macOS 프로덕션 번들: MacOS/../Resources/
    let base_res = std::env::current_exe().ok()
        .and_then(|exe| exe.parent()?.parent().map(|p| p.join("Resources")))?;
    
    if !base_res.exists() { return None; }

    // Resources 폴더 내에서 'crates/plugins'가 포함된 경로를 검색합니다. (Tauri의 _up_ 핸들링 대응)
    fn find_recursive(dir: &std::path::Path) -> Option<std::path::PathBuf> {
        if dir.ends_with("crates/plugins") {
            return Some(dir.to_path_buf());
        }
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(found) = find_recursive(&entry.path()) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    find_recursive(&base_res)
}

/// 내장된 플러그인들을 ~/.doxus/plugins 폴더로 복사합니다.
fn ensure_plugins(target_dir: &std::path::Path) {
    std::fs::create_dir_all(target_dir).ok();

    if let Some(bundle_dir) = find_bundle_plugins_dir() {
        if !bundle_dir.exists() { return; }
        
        // 재귀적으로 .wasm 및 .manifest.toml 파일을 찾습니다.
        fn visit_dirs(dir: &std::path::Path, target_dir: &std::path::Path) -> std::io::Result<()> {
            if dir.is_dir() {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_dir() {
                        visit_dirs(&path, target_dir)?;
                    } else {
                        let ext = path.extension().and_then(|e| e.to_str());

                        if ext == Some("wasm") {
                            // 1. WASM 파일 복사
                            let target_wasm = target_dir.join(path.file_name().unwrap());
                            if !target_wasm.exists() {
                                let _ = std::fs::copy(&path, &target_wasm);
                                eprintln!("[plugins] Installed WASM: {}", target_wasm.display());
                            }
                            
                            // 2. 동반 매니페스트 확인 및 복사 (foo.wasm -> foo.manifest.toml)
                            let companion_manifest = path.with_extension("manifest.toml");
                            if companion_manifest.exists() {
                                let target_manifest = target_dir.join(companion_manifest.file_name().unwrap());
                                let _ = std::fs::copy(&companion_manifest, &target_manifest);
                                eprintln!("[plugins] Installed companion: {}", target_manifest.display());
                            } else {
                                // 3. 폴더 내 generic manifest.toml이 있는지 확인 (하위 호환성)
                                let generic_manifest = path.parent().unwrap().join("manifest.toml");
                                if generic_manifest.exists() {
                                    let target_manifest = target_dir.join(format!("{}.manifest.toml", path.file_stem().unwrap().to_str().unwrap()));
                                    let _ = std::fs::copy(&generic_manifest, &target_manifest);
                                    eprintln!("[plugins] Installed generic as companion: {}", target_manifest.display());
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        let _ = visit_dirs(&bundle_dir, target_dir);
    }
}

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

/// 브릿지 토큰을 생성하거나 로드합니다.
fn ensure_bridge_token() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let token_path = std::path::PathBuf::from(home).join(".doxus/.bridge_token");
    
    if token_path.exists() {
        if let Ok(token) = std::fs::read_to_string(&token_path) {
            let token = token.trim();
            if !token.is_empty() {
                return token.to_string();
            }
        }
    }

    // 새로운 랜덤 토큰 생성 (32바이트)
    use rand::Rng;
    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    
    std::fs::create_dir_all(token_path.parent().unwrap()).ok();
    if let Err(e) = std::fs::write(&token_path, &token) {
        eprintln!("[bridge] Failed to save token: {}", e);
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600)).ok();
        }
        eprintln!("[bridge] New token generated and saved to ~/.doxus/.bridge_token");
    }
    
    token
}

fn get_macos_idle_seconds() -> Result<f64, String> {
    use std::process::Command;
    let output = Command::new("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("HIDIdleTime") {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() >= 2 {
                let ns_str = parts[1].trim();
                if let Ok(ns) = ns_str.parse::<u64>() {
                    return Ok(ns as f64 / 1_000_000_000.0);
                }
            }
        }
    }
    Err("HIDIdleTime not found in ioreg output".into())
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let config_path = std::path::PathBuf::from(&home).join(".doxus/config.toml");
    
    // Load settings and initialize debug tags
    let mut settings = doxus_desktop_lib::commands::settings::load_settings_from_path(&config_path)
        .unwrap_or_default();
    doxus_core::observability::set_debug_tags(settings.debug_tags.clone());
    let keychain_migrated_init = settings.keychain_migrated;

    let db_path = std::env::var("DOXUS_DB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let p = std::path::PathBuf::from(&home).join(".doxus/db");
            std::fs::create_dir_all(&p).ok();
            p.join("doxus.db")
        });
    let conn = doxus_core::db::open(&db_path).expect("failed to open db");
    let plugins_dir = std::path::PathBuf::from(&home).join(".doxus/plugins");
    
    // Ensure plugins are synced from bundle to plugins_dir
    ensure_plugins(&plugins_dir);

    let sidecar_script = find_sidecar_script();
    let bridge_token = ensure_bridge_token();
    let embedder: std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync> =
        doxus_core::embedding::OnnxEmbedder::from_default_path()
            .map(|e| std::sync::Arc::new(e) as std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync>)
            .unwrap_or_else(|e| {
                eprintln!("[embedding] ONNX load failed: {e}, falling back to no-op");
                std::sync::Arc::new(doxus_core::embedding::NoOpEmbedder)
            });
            
    let (state_arc, rx) = AppState::new(conn, plugins_dir, sidecar_script, embedder, keychain_migrated_init);
    let state_arc = Arc::new(state_arc);
    let manager = state_arc.sync_manager.clone();
    
    // If migration was triggered (flag was false), mark as done in config for next time
    if !keychain_migrated_init {
        settings.keychain_migrated = true;
        let _ = doxus_desktop_lib::commands::settings::save_settings_to_path(&settings, &config_path);
    }
    state_arc.sidecar.set_debug(doxus_core::observability::is_debug_enabled("agent"));
    let conn_arc = state_arc.conn.clone();

    let state_for_tauri = state_arc.clone();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_oauth::init())
        .setup(move |app| {
            let state = app.state::<Arc<AppState>>();
            let scheduler = state.scheduler_manager.clone();
            let handler = Arc::new(doxus_desktop_lib::scheduler_handler::TauriAgentHandler {
                state: state.inner().clone(),
                app_handle: app.handle().clone(),
            });
            scheduler.set_agent_handler(handler);

            #[cfg(debug_assertions)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register("doxus").ok();
            }

            // Spawn background cache cleanup task (every 30 minutes)
            let handle = app.handle().clone();
            let conn_arc_inner = conn_arc.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(30 * 60));
                interval.tick().await; // skip immediate tick (startup cleanup done in AppState::new)
                loop {
                    interval.tick().await;
                    if let Ok(conn) = conn_arc_inner.lock() {
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

            // Start SyncManager background loop
            let manager_inner = manager.clone();
            tauri::async_runtime::spawn(async move {
                manager_inner.init_watchers().await;
                manager_inner.start_loop(rx).await;
            });

            // Start SchedulerManager tick loop
            let scheduler = state_arc.scheduler_manager.clone();
            tauri::async_runtime::spawn(async move {
                scheduler.ensure_defaults();
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;

                    // OS 레벨 유휴 상태 체크 (macOS 전용)
                    let is_idle = match get_macos_idle_seconds() {
                        Ok(seconds) => {
                            // 5분(300초) 이상 입력이 없으면 유휴 상태로 판단
                            seconds > 300.0
                        },
                        Err(_) => false,
                    };

                    scheduler.tick(is_idle).await;
                }
            });

            // Start Auth Bridge server (localhost:14201)
            let store = state_arc.secret_store.clone();
            tauri::async_runtime::spawn(async move {
                eprintln!("[bridge] Starting server on port 14201...");
                doxus_desktop_lib::bridge::run_bridge_server(store, 14201, bridge_token).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(true) = event {
                let state = window.state::<Arc<AppState>>();
                let manager = state.sync_manager.clone();
                tauri::async_runtime::spawn(async move {
                    manager.trigger(doxus_core::sync_manager::SyncTrigger::Focus).await;
                });
            }
        })
        .manage(state_for_tauri)
        .invoke_handler(tauri::generate_handler![
            doxus_desktop_lib::commands::market::market_list_installed,
            doxus_desktop_lib::commands::market::market_fetch_registry,
            doxus_desktop_lib::commands::market::market_fetch_guide,
            doxus_desktop_lib::commands::market::plugin_get_cache_ttl,
            doxus_desktop_lib::commands::market::plugin_set_cache_ttl,
            doxus_desktop_lib::commands::market::get_system_status,
            doxus_desktop_lib::commands::market::get_plugin_logs,
            doxus_desktop_lib::commands::market::clear_audit_log,
            doxus_desktop_lib::commands::market::get_embedding_status,
            doxus_desktop_lib::commands::market::get_sync_status,
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
            doxus_desktop_lib::commands::search::search_engine_repair_index,
            doxus_desktop_lib::commands::search::trigger_reindex,
            doxus_desktop_lib::commands::search::index_project,
            doxus_desktop_lib::commands::search::increment_view_count,
            doxus_desktop_lib::commands::search::get_top_documents,
            doxus_desktop_lib::commands::search::get_document_content,
            doxus_desktop_lib::commands::search::list_all_documents,
            doxus_desktop_lib::commands::search::count_all_documents,
            doxus_desktop_lib::commands::agent::chat_start_session,
            doxus_desktop_lib::commands::agent::chat_send_message,
            doxus_desktop_lib::commands::agent::chat_cancel,
            doxus_desktop_lib::commands::agent::agent_status,
            doxus_desktop_lib::commands::agent::detect_cli_path,
            doxus_desktop_lib::commands::agent::get_claude_mcp_config,
            doxus_desktop_lib::commands::agent::get_claude_md_template,
            doxus_desktop_lib::commands::agent::upsert_claude_mcp_config,
            doxus_desktop_lib::commands::agent::remove_claude_mcp_config,
            doxus_desktop_lib::commands::agent::generate_project_claude_md,
            doxus_desktop_lib::commands::agent::generate_global_claude_md,
            doxus_desktop_lib::commands::settings::load_settings,
            doxus_desktop_lib::commands::system::get_resource_usage,
            doxus_desktop_lib::commands::system::check_model_status,
            doxus_desktop_lib::commands::system::download_onnx_model,
            doxus_desktop_lib::commands::graph::get_graph_data,
            doxus_desktop_lib::commands::freshness::get_freshness_dashboard,
            doxus_desktop_lib::commands::freshness::get_stale_documents,
            doxus_desktop_lib::commands::freshness::update_freshness_mark,
            doxus_desktop_lib::commands::freshness::update_sensitivity_mode,
            doxus_desktop_lib::commands::scheduler::list_scheduled_jobs,
            doxus_desktop_lib::commands::scheduler::create_scheduled_job,
            doxus_desktop_lib::commands::scheduler::delete_scheduled_job,
            doxus_desktop_lib::commands::scheduler::get_job_history,
            doxus_desktop_lib::commands::scheduler::update_scheduled_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
