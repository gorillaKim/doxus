use doxus_core::freshness::FreshnessService;
use serde_json::json;
use std::sync::Arc;

#[tauri::command]
pub async fn get_freshness_dashboard(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.clone();
    let service = FreshnessService::new(conn);

    let report = service
        .get_project_freshness_report(project_id)
        .map_err(|e| format!("Failed to get freshness report: {e}"))?;

    Ok(serde_json::to_value(report).unwrap_or(json!({})))
}

#[tauri::command]
pub async fn get_stale_documents(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
    limit: Option<u32>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let current_limit = limit.unwrap_or(20);

    let mut sql = "SELECT d.title, d.source_doc_id, p.name, df.freshness_score, d.updated_at 
                   FROM document_freshness df 
                   JOIN documents d ON df.document_id = d.id 
                   JOIN projects p ON d.project_id = p.id
                   WHERE df.status IN ('stale', 'aging') AND p.status = 'active'"
        .to_string();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND d.project_id = ?1");
        params.push(rusqlite::types::Value::Integer(pid));
    }

    sql.push_str(" ORDER BY df.freshness_score ASC LIMIT ?");
    params.push(rusqlite::types::Value::Integer(current_limit as i64));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let docs: Vec<_> = stmt
        .query_map(rusqlite::params_from_iter(params), |row| {
            Ok(json!({
                "title": row.get::<_, Option<String>>(0)?.unwrap_or_else(|| "Untitled".to_string()),
                "source_doc_id": row.get::<_, String>(1)?,
                "project_name": row.get::<_, String>(2)?,
                "freshness_score": row.get::<_, f64>(3)?,
                "updated_at": row.get::<_, Option<i64>>(4).unwrap_or(None),
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();

    Ok(json!({ "documents": docs }))
}

#[tauri::command]
pub async fn update_freshness_mark(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
    source_doc_id: String,
    tier: String,
) -> Result<serde_json::Value, String> {
    let pid = project_id.ok_or_else(|| "project_id is required".to_string())?;

    let conn = state.conn.clone();
    let service = FreshnessService::new(conn);

    let updated = service
        .update_document_freshness_config(pid, &source_doc_id, Some(&tier))
        .map_err(|e| format!("Failed to update freshness: {e}"))?;

    if updated {
        Ok(json!({ "ok": true }))
    } else {
        Err("Document not found".to_string())
    }
}

#[tauri::command]
pub async fn update_sensitivity_mode(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
    mode: String,
) -> Result<serde_json::Value, String> {
    // Phase 1 implementation uses a project-wide sensitivity mode.
    // However, the current DB migration `V33__freshness_config.sql` says:
    // "ALTER TABLE projects ADD COLUMN freshness_policy_json TEXT;"
    let conn = state.conn.get().map_err(|e| e.to_string())?;

    let policy_str = json!({
        "sensitivity_mode": mode,
        "default_tier": "mid",
        "thresholds": { "fresh": 70.0, "aging": 40.0 }
    })
    .to_string();

    let service = FreshnessService::new(state.conn.clone());

    if let Some(pid) = project_id {
        conn.execute(
            "UPDATE projects SET freshness_policy_json = ?1 WHERE id = ?2",
            rusqlite::params![policy_str, pid],
        )
        .map_err(|e| e.to_string())?;

        // 즉시 재계산 트리거
        drop(conn); // Lock 해제
        let _ = service.recalculate_project(pid);

        Ok(json!({ "ok": true }))
    } else {
        conn.execute(
            "UPDATE projects SET freshness_policy_json = ?1",
            rusqlite::params![policy_str],
        )
        .map_err(|e| e.to_string())?;

        // 즉시 전수 재계산 트리거
        drop(conn); // Lock 해제
        let _ = service.recalculate_all();

        Ok(json!({ "ok": true, "global": true }))
    }
}
