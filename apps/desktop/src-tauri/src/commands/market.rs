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
pub async fn get_plugin_logs(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, event_type, payload, occurred_at \
             FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let logs: Vec<serde_json::Value> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "project_id": r.get::<_, Option<i64>>(1)?,
                "event_type": r.get::<_, String>(2)?,
                "payload": r.get::<_, Option<String>>(3)?,
                "occurred_at": r.get::<_, i64>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "logs": logs }))
}

#[tauri::command]
pub async fn market_install_plugin(
    _state: tauri::State<'_, crate::AppState>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    eprintln!("market_install_plugin requested for: {}", plugin_id);
    Ok(serde_json::json!({
        "status": "ok",
        "message": format!("플러그인 설치가 요청됐습니다: {}. 실제 설치는 Phase 4에서 구현됩니다.", plugin_id)
    }))
}

#[tauri::command]
pub async fn market_uninstall_plugin(
    _state: tauri::State<'_, crate::AppState>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    eprintln!("market_uninstall_plugin requested for: {}", plugin_id);
    Ok(serde_json::json!({
        "status": "ok",
        "message": format!("플러그인 제거가 요청됐습니다: {}. 실제 제거는 Phase 4에서 구현됩니다.", plugin_id)
    }))
}

#[cfg(test)]
mod tests {
    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn get_plugin_logs_returns_empty_array_when_no_logs() {
        let conn = make_conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, event_type, payload, occurred_at \
                 FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
            )
            .unwrap();
        let logs: Vec<serde_json::Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "project_id": r.get::<_, Option<i64>>(1)?,
                    "event_type": r.get::<_, String>(2)?,
                    "payload": r.get::<_, Option<String>>(3)?,
                    "occurred_at": r.get::<_, i64>(4)?,
                }))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(logs.is_empty());
    }

    #[test]
    fn get_plugin_logs_returns_recent_entries() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO audit_log (event_type, payload, occurred_at) VALUES ('plugin_error', '{\"msg\":\"test\"}', ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, event_type, payload, occurred_at \
                 FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
            )
            .unwrap();
        let logs: Vec<serde_json::Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "project_id": r.get::<_, Option<i64>>(1)?,
                    "event_type": r.get::<_, String>(2)?,
                    "payload": r.get::<_, Option<String>>(3)?,
                    "occurred_at": r.get::<_, i64>(4)?,
                }))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0]["event_type"], "plugin_error");
    }
}

#[tauri::command]
pub async fn check_claude_status() -> Result<serde_json::Value, String> {
    let claude_in_path = std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let api_key_set = std::env::var("ANTHROPIC_API_KEY").is_ok();
    Ok(serde_json::json!({
        "status": if claude_in_path || api_key_set { "ok" } else { "warn" },
        "claude_cli": claude_in_path,
        "api_key": api_key_set,
        "message": if claude_in_path { "Claude Code CLI 감지됨" } else if api_key_set { "API 키 설정됨" } else { "Claude를 찾을 수 없습니다" }
    }))
}

#[tauri::command]
pub async fn check_gemini_status() -> Result<serde_json::Value, String> {
    let gemini_in_path = std::process::Command::new("which")
        .arg("gemini")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let api_key_set = std::env::var("GEMINI_API_KEY").is_ok();
    Ok(serde_json::json!({
        "status": if gemini_in_path || api_key_set { "ok" } else { "warn" },
        "gemini_cli": gemini_in_path,
        "api_key": api_key_set,
        "message": if gemini_in_path { "Gemini CLI 감지됨" } else if api_key_set { "API 키 설정됨" } else { "Gemini를 찾을 수 없습니다" }
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
