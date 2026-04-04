#[tauri::command]
pub async fn market_list_installed(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let plugins = state
        .plugin_manager
        .list_installed()
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "plugins": plugins }))
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
