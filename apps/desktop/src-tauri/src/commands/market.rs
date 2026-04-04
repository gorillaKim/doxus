#[tauri::command]
pub async fn market_list_installed(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let installed_ids = state
        .plugin_manager
        .list_installed()
        .map_err(|e| e.to_string())?;

    // Built-in plugins (always available)
    let builtin = vec![
        serde_json::json!({
            "id": "com.doxus.obsidian",
            "name": "Obsidian",
            "version": "1.0.0",
            "trust": "official",
            "description": "Obsidian vault integration (built-in, local folder)",
            "installed": true,
            "builtin": true
        }),
        serde_json::json!({
            "id": "com.doxus.confluence",
            "name": "Confluence",
            "version": "1.0.0",
            "trust": "official",
            "description": "Confluence Cloud/Server REST API integration",
            "installed": installed_ids.contains(&"com.doxus.confluence".to_string()),
            "builtin": false
        }),
        serde_json::json!({
            "id": "com.doxus.github",
            "name": "GitHub",
            "version": "1.0.0",
            "trust": "official",
            "description": "GitHub Issues, Wiki, Discussions",
            "installed": installed_ids.contains(&"com.doxus.github".to_string()),
            "builtin": false
        }),
    ];

    // User-installed plugins not in built-in list
    let builtin_ids = ["com.doxus.obsidian", "com.doxus.confluence", "com.doxus.github"];
    let user_installed: Vec<serde_json::Value> = installed_ids
        .iter()
        .filter(|id| !builtin_ids.contains(&id.as_str()))
        .map(|id| serde_json::json!({
            "id": id,
            "name": id,
            "version": "unknown",
            "trust": "unverified",
            "description": "User-installed plugin",
            "installed": true,
            "builtin": false
        }))
        .collect();

    let mut all = builtin;
    all.extend(user_installed);
    Ok(serde_json::json!({ "plugins": all }))
}

#[tauri::command]
pub async fn get_system_status() -> Result<serde_json::Value, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{}/.doxus/db/doxus.db", home);
    let db_exists = std::path::Path::new(&db_path).exists();

    // CLI 바이너리 존재 여부 확인
    let cli_candidates = [
        format!("{}/.cargo/bin/doxus-cli", home),
        "/usr/local/bin/doxus-cli".to_string(),
    ];
    let cli_path = cli_candidates.iter().find(|p| std::path::Path::new(p).exists());

    // MCP 서버 포트 확인 (7700번 기본 포트)
    let mcp_running = std::net::TcpStream::connect("127.0.0.1:7700").is_ok();

    Ok(serde_json::json!({
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "status": "running"
        },
        "database": {
            "path": db_path,
            "exists": db_exists,
            "status": if db_exists { "connected" } else { "not found" }
        },
        "mcp": {
            "status": if mcp_running { "running" } else { "not started" },
            "note": "MCP 서버는 별도 프로세스로 실행됩니다 (포트 7700)"
        },
        "cli": {
            "status": if cli_path.is_some() { "installed" } else { "not installed" },
            "path": cli_path.cloned().unwrap_or_default()
        },
        "agent": {
            "status": "not started",
            "note": "Agent sidecar는 Phase 3에서 구현됩니다"
        }
    }))
}

#[tauri::command]
pub async fn get_workspaces(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let repo = doxus_core::workspace::WorkspaceRepo::new(&conn);
    let workspaces = repo.list().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "workspaces": workspaces }))
}
