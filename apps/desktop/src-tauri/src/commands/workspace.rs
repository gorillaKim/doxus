use rusqlite::Connection;

// ── Pure DB helpers (pub for integration tests) ───────────────────────────────

pub fn list_workspace_documents_in_conn(
    conn: &Connection,
) -> Result<Vec<serde_json::Value>, String> {
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
    Ok(docs)
}

pub fn create_workspace_document_in_conn(
    conn: &Connection,
    title: &str,
    template_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;

    // Atomic counter ensures unique file_path even when multiple docs are created
    // within the same second (e.g. parallel tests or rapid UI clicks).
    static COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file_path = format!("ws-{}-{:06}.md", now, seq);
    let content_hash = format!("{:x}", now);
    // Map template IDs to the allowed doc_type values
    let doc_type = match template_id.as_deref() {
        Some("note") | None => "note",
        Some("meeting") => "meeting",
        Some("decision") => "decision",
        Some("journal") => "journal",
        _ => "other",
    };
    let initial_content = template_id
        .as_deref()
        .and_then(initial_content_for_template)
        .unwrap_or("");

    conn.execute(
        "INSERT INTO workspace_documents \
         (file_path, title, doc_type, status, priority, content_hash, content, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'draft', 'medium', ?4, ?5, ?6, ?6)",
        rusqlite::params![file_path, title, doc_type, content_hash, initial_content, now],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    let preview: String = initial_content.chars().take(100).collect();
    Ok(serde_json::json!({
        "id": id,
        "title": title,
        "created_at": now,
        "content_preview": if preview.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(preview) },
    }))
}

pub fn delete_workspace_document_in_conn(
    conn: &Connection,
    id: i64,
) -> Result<(), String> {
    let rows = conn
        .execute(
            "DELETE FROM workspace_documents WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| e.to_string())?;
    if rows == 0 {
        return Err(format!("document {} not found", id));
    }
    Ok(())
}

fn initial_content_for_template(template_id: &str) -> Option<&'static str> {
    match template_id {
        "todo" => Some(
            "# TODO\n\n## 오늘\n- [ ] \n\n## 이번 주\n- [ ] \n\n## 백로그\n- [ ] \n",
        ),
        "techspec" => Some(
            "# [기능명] 기술 명세서\n\n## 개요\n> 한 줄 요약\n\n## 요구사항\n### 기능 요구사항\n- [ ] FR-01:\n\n### 비기능 요구사항\n- [ ] NFR-01:\n\n## 상세 구현 계획\n### 아키텍처\n### API 설계\n### DB 스키마 변경\n### 테스트 계획\n\n## 리스크 및 미결 사항\n",
        ),
        _ => None,
    }
}

// ── Tauri IPC commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_workspace_documents(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let docs = list_workspace_documents_in_conn(&conn)?;
    Ok(serde_json::json!(docs))
}

#[tauri::command]
pub async fn create_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    title: String,
    template_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    create_workspace_document_in_conn(&conn, &title, template_id)
}

#[tauri::command]
pub async fn update_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
    title: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    conn.execute(
        "UPDATE workspace_documents SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![title, content, now, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn delete_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    delete_workspace_document_in_conn(&conn, id)?;
    Ok(serde_json::json!({ "ok": true }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

        let docs = list_workspace_documents_in_conn(&conn).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0]["title"].as_str().unwrap(), "Hello Doc");
    }

    #[test]
    fn create_workspace_document_appears_in_list() {
        let conn = make_conn();
        create_workspace_document_in_conn(&conn, "My Title", None).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspace_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn update_workspace_document_modifies_record() {
        let conn = make_conn();
        let id = insert_workspace_doc(&conn, "Original Title");

        let new_now = now_secs();
        conn.execute(
            "UPDATE workspace_documents SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params!["Updated Title", "New content body", new_now, id],
        )
        .unwrap();

        let (title, content): (String, Option<String>) = conn
            .query_row(
                "SELECT title, content FROM workspace_documents WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert_eq!(title, "Updated Title");
        assert_eq!(content.as_deref(), Some("New content body"));
    }

    #[test]
    fn delete_workspace_document_removes_record() {
        let conn = make_conn();
        let id = insert_workspace_doc(&conn, "To Delete");

        delete_workspace_document_in_conn(&conn, id).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspace_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_nonexistent_document_returns_error() {
        let conn = make_conn();
        let result = delete_workspace_document_in_conn(&conn, 9999);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn initial_content_for_todo_template() {
        let content = initial_content_for_template("todo").unwrap();
        assert!(content.contains("# TODO"));
        assert!(content.contains("- [ ]"));
    }

    #[test]
    fn initial_content_for_techspec_template() {
        let content = initial_content_for_template("techspec").unwrap();
        assert!(content.contains("기술 명세서"));
        assert!(content.contains("FR-01"));
    }

    #[test]
    fn initial_content_unknown_template_is_none() {
        assert!(initial_content_for_template("unknown").is_none());
    }
}
