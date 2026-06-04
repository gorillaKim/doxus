use std::sync::Arc;
use serde_json::json;
use doxus_core::scheduler::{SchedulerDb, ScheduledJob, Schedule, Executor};

#[tauri::command]
pub async fn list_scheduled_jobs(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let sdb = SchedulerDb::new(&conn);
    
    let jobs = sdb.list_jobs(project_id).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(jobs).unwrap_or_default())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_scheduled_job(
    state: tauri::State<'_, Arc<crate::AppState>>,
    project_id: Option<i64>,
    job_name: String,
    description: Option<String>,
    executor: String,
    action: String,
    action_config: serde_json::Value,
    schedule_json: serde_json::Value,
    run_on_idle: bool,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let sdb = SchedulerDb::new(&conn);
    
    let exec = if executor == "system" { Executor::System } else { Executor::Agent };
    let sched: Schedule = serde_json::from_value(schedule_json).map_err(|e| e.to_string())?;
    
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let next_run = sched.next_run_after(now);

    let job = ScheduledJob {
        id: 0,
        project_id,
        job_name,
        description,
        executor: exec,
        action,
        action_config,
        schedule: sched,
        enabled: true,
        run_on_idle,
        is_immutable: false,
        last_run_at: None,
        next_run_at: next_run,
        created_by: "user".to_string(),
    };

    let new_id = sdb.insert_job(&job).map_err(|e| e.to_string())?;
    Ok(json!({ "id": new_id }))
}

#[tauri::command]
pub async fn delete_scheduled_job(
    state: tauri::State<'_, Arc<crate::AppState>>,
    job_id: i64,
    disable_only: Option<bool>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let sdb = SchedulerDb::new(&conn);

    if disable_only.unwrap_or(false) {
        sdb.disable_job(job_id).map_err(|e| e.to_string())?;
    } else {
        sdb.delete_job(job_id).map_err(|e| e.to_string())?;
    }
    
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub async fn get_job_history(
    state: tauri::State<'_, Arc<crate::AppState>>,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    // get_job_history is not yet fully implemented in SchedulerDb, 
    // but we can query it directly here as a stopgap
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, started_at, finished_at, status, result_text, error_text 
         FROM job_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT 50"
    ).map_err(|e| e.to_string())?;

    let iter = stmt.query_map([job_id], |row: &rusqlite::Row<'_>| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "started_at": row.get::<_, i64>(1)?,
            "finished_at": row.get::<_, Option<i64>>(2)?,
            "status": row.get::<_, String>(3)?,
            "result_text": row.get::<_, Option<String>>(4)?,
            "error_text": row.get::<_, Option<String>>(5)?,
        }))
    }).map_err(|e| e.to_string())?;

    let mut rows = Vec::new();
    for v in iter.flatten() {
        rows.push(v);
    }

    Ok(json!({ "history": rows }))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn update_scheduled_job(
    state: tauri::State<'_, Arc<crate::AppState>>,
    job_id: i64,
    project_id: Option<i64>,
    job_name: String,
    description: Option<String>,
    executor: String,
    action: String,
    action_config: serde_json::Value,
    schedule_json: serde_json::Value,
    run_on_idle: bool,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.get().map_err(|e| e.to_string())?;
    let sdb = SchedulerDb::new(&conn);
    
    let exec = if executor == "system" { Executor::System } else { Executor::Agent };
    let sched: Schedule = serde_json::from_value(schedule_json).map_err(|e| e.to_string())?;

    let job = ScheduledJob {
        id: job_id,
        project_id,
        job_name,
        description,
        executor: exec,
        action,
        action_config,
        schedule: sched,
        enabled: true,
        run_on_idle,
        is_immutable: false,
        last_run_at: None,
        next_run_at: 0,
        created_by: "user".to_string(),
    };

    sdb.update_job(job_id, &job).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true }))
}
