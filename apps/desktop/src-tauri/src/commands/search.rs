use doxus_core::search::{SearchEngine, SearchQuery};

#[tauri::command]
pub async fn search_documents(
    state: tauri::State<'_, crate::AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let engine = SearchEngine::new(&conn);
    let q = SearchQuery::new(&query).with_limit(limit.unwrap_or(20));
    let hits = engine.search(&q).map_err(|e| e.to_string())?;
    let hits_json: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|h| {
            serde_json::json!({
                "document_id": h.document_id,
                "chunk_id": h.chunk_id,
                "title": h.title,
                "file_path": h.file_path,
                "heading_path": h.heading_path,
                "snippet": h.snippet,
                "score": h.score,
            })
        })
        .collect();
    Ok(serde_json::json!({ "hits": hits_json }))
}

#[tauri::command]
pub async fn list_projects(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT name, display_name, path, status FROM projects ORDER BY name")
        .map_err(|e| e.to_string())?;
    let projects: Vec<_> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "name": r.get::<_, String>(0)?,
                "display_name": r.get::<_, String>(1)?,
                "path": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "projects": projects }))
}
