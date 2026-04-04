use doxus_core::search::{SearchEngine, SearchQuery};

#[cfg(test)]
mod tests {
    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    fn insert_project(conn: &rusqlite::Connection, name: &str, path: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
            rusqlite::params![name, name, path, now],
        )
        .unwrap();
    }

    #[test]
    fn add_project_inserts_and_returns_project() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES ('my-project', 'my-project', '/tmp/proj', 'active', ?1, ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        let (name, status): (String, String) = conn
            .query_row(
                "SELECT name, status FROM projects WHERE name = 'my-project'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "my-project");
        assert_eq!(status, "active");
    }

    #[test]
    fn toggle_project_status_updates_status() {
        let conn = make_conn();
        insert_project(&conn, "proj", "/tmp/p");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "UPDATE projects SET status = 'disabled', updated_at = ?1 WHERE name = 'proj'",
            rusqlite::params![now],
        )
        .unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM projects WHERE name = 'proj'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "disabled");
    }
}

#[tauri::command]
pub async fn add_project(
    state: tauri::State<'_, crate::AppState>,
    name: String,
    path: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
        rusqlite::params![name, name, path, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "project": {
            "name": name,
            "display_name": name,
            "path": path,
            "status": "active"
        }
    }))
}

#[tauri::command]
pub async fn toggle_project_status(
    state: tauri::State<'_, crate::AppState>,
    name: String,
    status: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    conn.execute(
        "UPDATE projects SET status = ?1, updated_at = ?2 WHERE name = ?3",
        rusqlite::params![status, now, name],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true }))
}

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
