#[tauri::command]
pub async fn market_list_installed(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<String>, String> {
    state
        .plugin_manager
        .list_installed()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_workspaces(
    _state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<doxus_core::workspace::Workspace>, String> {
    // Workspace wiring is deferred
    Ok(vec![])
}
