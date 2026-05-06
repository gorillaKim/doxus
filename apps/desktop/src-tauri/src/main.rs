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

/// doxus-mcp 바이너리 경로 탐색
fn find_doxus_mcp_bin() -> Option<std::path::PathBuf> {
    find_doxus_mcp_bin_in(std::env::current_exe().ok()?.parent()?)
        .or_else(|| find_doxus_mcp_bin_in_target())
        .or_else(|| find_doxus_mcp_bin_in_path())
}

/// 지정 디렉토리에서 `doxus-mcp` prefix를 가진 실행 파일을 탐색
/// Tauri externalBin은 OS/아키텍처별 트리플 suffix를 붙여 번들링하므로
/// 정확한 이름 대신 prefix 매칭으로 탐색 (OS 확장 시에도 수정 불필요)
fn find_doxus_mcp_bin_in(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    // 정확한 이름 우선 (dev/local 빌드)
    let exact = dir.join("doxus-mcp");
    if exact.is_file() { return Some(exact); }

    // Tauri 번들 바이너리: "doxus-mcp-<triple>" prefix 스캔
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let is_mcp = name_str == "doxus-mcp"
                || (name_str.starts_with("doxus-mcp-") && !name_str.ends_with(".d"));
            if is_mcp && entry.path().is_file() {
                return Some(entry.path());
            }
        }
    }
    None
}

fn find_doxus_mcp_bin_in_target() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.as_path();
    loop {
        if dir.file_name().map(|n| n == "target").unwrap_or(false) {
            let d = dir.join("debug/doxus-mcp");
            if d.exists() { return Some(d); }
            let r = dir.join("release/doxus-mcp");
            if r.exists() { return Some(r); }
            break;
        }
        match dir.parent() { Some(p) => dir = p, None => break }
    }
    None
}

fn find_doxus_mcp_bin_in_path() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).find_map(|d| {
            let c = d.join("doxus-mcp");
            if c.exists() { Some(c) } else { None }
        })
    })
}

/// 단일 공유 HTTP 서버 포트 (모든 Claude 세션이 이 포트를 공유)
const DOXUS_MCP_HTTP_PORT: u16 = 1421;

/// 지정한 포트가 이미 사용 중인지 확인 (bind 실패 = 사용 중)
fn is_port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// doxus HTTP MCP 엔드포인트를 설정 파일에 기록 (테스트 가능한 내부 구현)
fn write_doxus_http_config_to(port: u16, token: &str, config_path: &std::path::Path) {
    let mut config: serde_json::Value = if config_path.exists() {
        match std::fs::read_to_string(config_path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
            Err(e) => {
                eprintln!("[mcp] Failed to read settings.json: {e}");
                return;
            }
        }
    } else {
        serde_json::json!({})
    };

    let mcp_entry = serde_json::json!({
        "type": "http",
        "url": format!("http://127.0.0.1:{port}/mcp"),
        "headers": { "Authorization": format!("Bearer {token}") }
    });

    if config.get("mcpServers").is_none() {
        if let Some(obj) = config.as_object_mut() {
            obj.insert("mcpServers".to_string(), serde_json::json!({}));
        }
    }
    if let Some(servers) = config.get_mut("mcpServers").and_then(|m| m.as_object_mut()) {
        servers.insert("doxus".to_string(), mcp_entry);
    }

    match serde_json::to_string_pretty(&config) {
        Ok(content) => {
            if let Err(e) = std::fs::write(config_path, content) {
                eprintln!("[mcp] Failed to write settings.json: {e}");
            } else {
                eprintln!("[mcp] ~/.claude/settings.json updated → doxus HTTP port {port}");
            }
        }
        Err(e) => eprintln!("[mcp] Failed to serialize settings.json: {e}"),
    }
}

/// ~/.claude/settings.json 의 doxus 항목을 HTTP 타입으로 갱신
fn write_doxus_http_config(port: u16, token: &str) {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("[mcp] HOME not set, skipping settings update");
            return;
        }
    };
    let config_path = std::path::PathBuf::from(home).join(".claude/settings.json");
    write_doxus_http_config_to(port, token, &config_path);
}

/// doxus-mcp를 HTTP 모드로 실행하고 실제 포트를 반환
fn spawn_mcp_http_server(
    bin: &std::path::Path,
    token: &str,
    port: u16,
) -> Result<(std::process::Child, u16), String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let mut child = Command::new(bin)
        .args(["--http", &port.to_string()])
        .env("DOXUS_MCP_TOKEN", token)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    // stdout에서 "PORT=<num>" 읽기 — 별도 스레드 + 채널로 블로킹 방지 (최대 2초)
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = std::sync::mpsc::channel::<u16>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 { break; }
            if let Some(p) = line.trim().strip_prefix("PORT=") {
                if let Ok(port) = p.parse::<u16>() {
                    let _ = tx.send(port);
                    break;
                }
            }
            line.clear();
        }
    });

    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(p) => Ok((child, p)),
        Err(_) => {
            let _ = child.kill();
            Err("doxus-mcp did not print PORT= within timeout".into())
        }
    }
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
    doxus_core::observability::init_tracing();
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

    // Create completion channel for SyncManager background indexing events
    let (indexed_tx, mut indexed_rx) = tokio::sync::mpsc::channel::<(String, usize)>(32);
    {
        let manager_clone = manager.clone();
        tauri::async_runtime::block_on(async move {
            manager_clone.set_event_sender(indexed_tx).await;
        });
    }
    
    // If migration was triggered (flag was false), mark as done in config for next time
    if !keychain_migrated_init {
        settings.keychain_migrated = true;
        let _ = doxus_desktop_lib::commands::settings::save_settings_to_path(&settings, &config_path);
    }
    state_arc.sidecar.set_debug(doxus_core::observability::is_debug_enabled("agent"));

    // doxus-mcp HTTP 서버 시작 — 포트 1421 고정, 이미 실행 중이면 재사용
    {
        let mcp_token = bridge_token.clone();
        let endpoint = format!("http://127.0.0.1:{}/mcp", DOXUS_MCP_HTTP_PORT);

        if is_port_in_use(DOXUS_MCP_HTTP_PORT) {
            // 이미 실행 중 (이전 앱 인스턴스 또는 동일 프로세스) → 재사용
            eprintln!("[mcp] HTTP server already running on port {DOXUS_MCP_HTTP_PORT}, reusing");
            *state_arc.mcp_endpoint.lock().unwrap() = Some(endpoint.clone());
            *state_arc.mcp_token.lock().unwrap() = mcp_token.clone();
        } else if let Some(mcp_bin) = find_doxus_mcp_bin() {
            match spawn_mcp_http_server(&mcp_bin, &mcp_token, DOXUS_MCP_HTTP_PORT) {
                Ok((child, port)) => {
                    let endpoint = format!("http://127.0.0.1:{}/mcp", port);
                    eprintln!("[mcp] HTTP server started on {}", endpoint);
                    *state_arc.mcp_process.lock().unwrap() = Some(child);
                    *state_arc.mcp_endpoint.lock().unwrap() = Some(endpoint.clone());
                    *state_arc.mcp_token.lock().unwrap() = mcp_token.clone();
                }
                Err(e) => eprintln!("[mcp] Failed to start HTTP server: {}", e),
            }
        } else {
            eprintln!("[mcp] doxus-mcp binary not found, HTTP server disabled");
        }

        // ~/.claude/settings.json 을 HTTP 타입으로 자동 갱신
        write_doxus_http_config(DOXUS_MCP_HTTP_PORT, &mcp_token);
    }

    let conn_arc = state_arc.conn.clone();

    let state_for_tauri = state_arc.clone();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_oauth::init())
        .setup(move |app| {
            let state = app.state::<Arc<AppState>>();

            // Post-update migration: detect version change and run hooks
            {
                let hook = doxus_desktop_lib::update_manager::TauriReindexHook::new(
                    app.handle().clone(),
                    conn_arc.clone(),
                    state_arc.sync_manager.indexing_service(),
                );
                match conn_arc.lock() {
                    Ok(conn_guard) => {
                        match doxus_desktop_lib::update_manager::detect_and_migrate(
                            &conn_guard,
                            Some(&hook),
                            env!("CARGO_PKG_VERSION"),
                        ) {
                            Ok(r) => tracing::info!(
                                outcome = ?r.outcome,
                                reindex_triggered = r.reindex_triggered,
                                "post-update migration complete"
                            ),
                            Err(e) => tracing::error!(error = %e, "post-update migration failed"),
                        }
                    }
                    Err(e) => tracing::error!(
                        error = %e,
                        "post-update migration skipped: db mutex poisoned"
                    ),
                }
            }

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

            // Start SyncManager background loop (init_watchers runs first, then loop starts)
            let manager_inner = manager.clone();
            tauri::async_runtime::spawn(async move {
                manager_inner.init_watchers().await;
                manager_inner.start_loop(rx).await;
            });

            // Register SyncManager progress callback to emit index_progress events
            {
                let handle_progress = app.handle().clone();
                let manager_progress = state_arc.sync_manager.clone();
                tauri::async_runtime::spawn(async move {
                    manager_progress.set_progress_callback(move |project_name, docs_done, total_docs| {
                        use tauri::Emitter;
                        let _ = handle_progress.emit("index_progress", serde_json::json!({
                            "project_name": project_name,
                            "docs_indexed": docs_done,
                            "total_docs": if total_docs > 0 { serde_json::json!(total_docs) } else { serde_json::Value::Null },
                        }));
                    }).await;
                });
            }

            // Forward SyncManager completion events to frontend as "project-indexed"
            let handle_indexed = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some((project_name, count)) = indexed_rx.recv().await {
                    handle_indexed.emit("project-indexed", serde_json::json!({
                        "project_name": project_name,
                        "indexed": count,
                        "full": false
                    })).ok();
                }
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
            match event {
                tauri::WindowEvent::Focused(true) => {
                    let state = window.state::<Arc<AppState>>();
                    let manager = state.sync_manager.clone();
                    tauri::async_runtime::spawn(async move {
                        manager.trigger(doxus_core::sync_manager::SyncTrigger::Focus).await;
                    });
                }
                _ => {}
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
            doxus_desktop_lib::commands::agent::chat_close_session,
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
            doxus_desktop_lib::update_manager::relaunch_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let state: Arc<AppState> = app_handle.state::<Arc<AppState>>().inner().clone();
                std::thread::spawn(move || {
                    let _ = state.sidecar.send_request(&serde_json::json!({"type": "close"}));
                    if let Ok(mut proc) = state.mcp_process.lock() {
                        if let Some(ref mut child) = *proc {
                            let _ = child.kill();
                            eprintln!("[mcp] HTTP server stopped on app exit");
                        }
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_port_in_use_when_bound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(is_port_in_use(port), "bound port should be in use");
        drop(listener);
        assert!(!is_port_in_use(port), "released port should be free");
    }

    #[test]
    fn test_write_doxus_http_config_creates_http_entry() {
        let dir = std::env::temp_dir().join(format!(
            "doxus-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("settings.json");
        std::fs::write(&config_path, r#"{"mcpServers": {}}"#).unwrap();

        write_doxus_http_config_to(1421, "tok123", &config_path);

        let content = std::fs::read_to_string(&config_path).unwrap();
        let config: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(config["mcpServers"]["doxus"]["type"], "http");
        assert_eq!(config["mcpServers"]["doxus"]["url"], "http://127.0.0.1:1421/mcp");
        assert_eq!(
            config["mcpServers"]["doxus"]["headers"]["Authorization"],
            "Bearer tok123"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_doxus_http_config_overwrites_stdio_entry() {
        let dir = std::env::temp_dir().join(format!(
            "doxus-test2-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("settings.json");
        std::fs::write(
            &config_path,
            r#"{"mcpServers": {"doxus": {"type": "stdio", "command": "/old/path"}}}"#,
        )
        .unwrap();

        write_doxus_http_config_to(1421, "newtoken", &config_path);

        let content = std::fs::read_to_string(&config_path).unwrap();
        let config: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(config["mcpServers"]["doxus"]["type"], "http");
        assert!(config["mcpServers"]["doxus"]["command"].is_null());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_find_doxus_mcp_bin_in_exact_name() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("doxus-mcp");
        std::fs::write(&bin, b"").unwrap();
        assert_eq!(find_doxus_mcp_bin_in(dir.path()), Some(bin));
    }

    #[test]
    fn test_find_doxus_mcp_bin_in_triple_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("doxus-mcp-aarch64-apple-darwin");
        std::fs::write(&bin, b"").unwrap();
        assert_eq!(find_doxus_mcp_bin_in(dir.path()), Some(bin));
    }

    #[test]
    fn test_find_doxus_mcp_bin_in_prefers_exact_over_triple() {
        let dir = tempfile::tempdir().unwrap();
        let exact = dir.path().join("doxus-mcp");
        let triple = dir.path().join("doxus-mcp-aarch64-apple-darwin");
        std::fs::write(&exact, b"").unwrap();
        std::fs::write(&triple, b"").unwrap();
        assert_eq!(find_doxus_mcp_bin_in(dir.path()), Some(exact));
    }

    #[test]
    fn test_find_doxus_mcp_bin_in_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_doxus_mcp_bin_in(dir.path()), None);
    }
}
