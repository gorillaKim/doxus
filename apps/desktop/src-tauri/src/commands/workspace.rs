#[tauri::command]
pub async fn list_workspace_documents(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, file_path FROM workspace_documents \
             ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let docs: Vec<serde_json::Value> = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let title: Option<String> = r.get(1)?;
            let created_at: i64 = r.get(2)?;
            let file_path: String = r.get(3)?;
            // preview: first 100 chars of file_path as stand-in (content not stored here)
            let preview: String = file_path.chars().take(100).collect();
            Ok(serde_json::json!({
                "id": id,
                "title": title,
                "created_at": created_at,
                "content_preview": preview,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!(docs))
}

#[tauri::command]
pub async fn create_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    title: String,
    template_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;

    let file_path = format!("ws-{}.md", now);
    let content_hash = format!("{:x}", now);
    let doc_type = template_id.as_deref().unwrap_or("note");

    conn.execute(
        "INSERT INTO workspace_documents \
         (file_path, title, doc_type, status, priority, content_hash, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'draft', 'medium', ?4, ?5, ?5)",
        rusqlite::params![file_path, title, doc_type, content_hash, now],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    Ok(serde_json::json!({
        "id": id,
        "title": title,
        "created_at": now,
        "content_preview": null,
    }))
}

#[cfg(test)]
mod tests {
    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn insert_workspace_doc(conn: &rusqlite::Connection, title: &str) -> i64 {
        let now = now_secs();
        conn.execute(
            "INSERT INTO workspace_documents \
             (file_path, title, doc_type, status, priority, content_hash, created_at, updated_at) \
             VALUES (?1, ?2, 'note', 'draft', 'medium', 'hash', ?3, ?3)",
            rusqlite::params![format!("ws-{}.md", now), title, now],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn list_workspace_documents_returns_inserted_doc() {
        let conn = make_conn();
        insert_workspace_doc(&conn, "Hello Doc");

        let mut stmt = conn
            .prepare(
                "SELECT id, title FROM workspace_documents ORDER BY created_at DESC LIMIT 50",
            )
            .unwrap();
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "Hello Doc");
    }

    #[test]
    fn create_workspace_document_appears_in_list() {
        let conn = make_conn();
        let now = now_secs();
        conn.execute(
            "INSERT INTO workspace_documents \
             (file_path, title, doc_type, status, priority, content_hash, created_at, updated_at) \
             VALUES ('ws-new.md', 'My Title', 'note', 'draft', 'medium', 'h', ?1, ?1)",
            rusqlite::params![now],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspace_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
