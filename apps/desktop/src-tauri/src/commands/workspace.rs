use rusqlite::Connection;
use sha2::{Digest, Sha256};
use doxus_core::document::{parse_sections, replace_section, insert_section_after, delete_section};
use doxus_core::workspace::{
    ensure_default_workspace, get_workspace_project,
};

fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn data_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".doxus")
}

// ── 워크스페이스 커맨드 ───────────────────────────────────────────────────────

/// 앱 시작 시 호출 — 디폴트 워크스페이스 보장
#[tauri::command]
pub async fn ensure_default_workspace_cmd(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let id = ensure_default_workspace(&conn, &data_dir()).map_err(|e| e.to_string())?;
    let ws = get_workspace_project(&conn, id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!(ws))
}

// ── 문서 커맨드 ──────────────────────────────────────────────────────────────

/// 디폴트(또는 지정) 워크스페이스의 project_id를 반환
fn resolve_project_id(conn: &Connection, workspace_id: Option<i64>) -> Result<i64, String> {
    match workspace_id {
        Some(id) => Ok(id),
        None => conn
            .query_row(
                "SELECT id FROM projects WHERE source_type='workspace' AND is_default=1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "활성 워크스페이스를 찾을 수 없습니다".to_string()),
    }
}

pub fn 
 list_workspace_documents_impl(conn: &Connection, workspace_id: Option<i64>) -> Result<Vec<serde_json::Value>, String> {
    let pid = resolve_project_id(conn, workspace_id)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, substr(content, 1, 100) as preview, metadata_json
             FROM documents WHERE project_id=?1 ORDER BY created_at DESC LIMIT 100",
        )
        .map_err(|e| e.to_string())?;

    let docs: Vec<serde_json::Value> = stmt
        .query_map([pid], |r| {
            let meta: String = r.get::<_, String>(4).unwrap_or_else(|_| "{}".into());
            let meta_val: serde_json::Value = serde_json::from_str(&meta).unwrap_or_default();
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "title": r.get::<_, Option<String>>(1)?,
                "created_at": r.get::<_, i64>(2)?,
                "content_preview": r.get::<_, Option<String>>(3)?,
                "doc_type": meta_val["doc_type"],
                "status": meta_val["status"],
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(docs)
}

#[tauri::command]
pub async fn list_workspace_documents(
    state: tauri::State<'_, crate::AppState>,
    workspace_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let docs = list_workspace_documents_impl(&conn, workspace_id)?;
    Ok(serde_json::json!(docs))
}

pub fn create_workspace_document_impl(
    conn: &Connection,
    title: String,
    workspace_id: Option<i64>,
    base_path: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let pid = resolve_project_id(conn, workspace_id)?;

    // 프로젝트 존재 확인
    let project_path: Option<String> = conn
        .query_row("SELECT path FROM projects WHERE id=?1", [pid], |r| r.get(0))
        .ok();

    // 만약 프로젝트 경로가 비어있으면(신규 생성 등) base_path 하위로 지정
    let project_path = project_path.unwrap_or_else(|| {
        let name: String = conn.query_row("SELECT name FROM projects WHERE id=?1", [pid], |r| r.get(0)).unwrap_or_else(|_| "default".into());
        base_path.join(format!("ws-{}", name)).to_string_lossy().to_string()
    });


    // source_doc_id 생성
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = now_secs();
    // source_doc_id = 파일명 (Obsidian 플러그인이 vault 상대경로로 해석)
    let file_name = format!("ws-doc-{now}-{seq:06}.md");
    let source_doc_id = file_name.clone();

    // 템플릿 시스템 제거에 따라 기본 빈 문서로 생성
    let initial_content = "";
    let content_hash = sha256_hex(initial_content);
    let doc_type = "note";

    // 파일 경로: project.path + file_name (경로 없으면 그냥 root)
    let file_path = format!("{}/{}", project_path, file_name);

    // 파일 시스템에 저장
    if let Ok(path) = std::path::PathBuf::from(&project_path).canonicalize().or_else(|_| {
        std::fs::create_dir_all(&project_path).map(|_| std::path::PathBuf::from(&project_path))
    }) {
        let _ = std::fs::write(path.join(&file_name), initial_content);
    }

    let metadata = serde_json::json!({ "doc_type": doc_type, "status": "draft", "priority": "medium" }).to_string();

    conn.execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, indexing_status, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?8)",
        rusqlite::params![pid, source_doc_id, file_path, title, initial_content, content_hash, metadata, now],
    )
    .map_err(|e| e.to_string())?;

    let doc_id = conn.last_insert_rowid();

    Ok(serde_json::json!({
        "id": doc_id,
        "title": title,
        "created_at": now,
        "content_preview": "",
    }))
}

#[tauri::command]
pub async fn create_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    title: String,
    workspace_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let base_path = state.plugins_dir.parent().unwrap_or(&state.plugins_dir);
    let doc = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        create_workspace_document_impl(&conn, title, workspace_id, base_path)?
    };

    // 즉시 재인덱싱 (비동기 백그라운드)
    let doc_id = doc["id"].as_i64().unwrap_or(0);
    enqueue_reindex(state.inner(), doc_id);

    Ok(doc)
}

#[tauri::command]
pub async fn update_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
    title: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = now_secs();
    let content_hash = sha256_hex(&content);

    let affected = conn
        .execute(
            "UPDATE documents SET title=?1, content=?2, content_hash=?3, updated_at=?4, indexing_status='pending' WHERE id=?5",
            rusqlite::params![title, content, content_hash, now, id],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err(format!("문서를 찾을 수 없습니다: id={id}"));
    }

    // 파일 시스템 동기화
    sync_document_to_file(&conn, id, &content);

    // 즉시 재인덱싱
    enqueue_reindex(state.inner(), id);

    Ok(serde_json::json!({ "ok": true }))
}

pub fn 
 delete_workspace_document_impl(conn: &Connection, id: i64) -> Result<(), String> {
    let affected = conn
        .execute("DELETE FROM documents WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(format!("문서를 찾을 수 없습니다: id={id}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    delete_workspace_document_impl(&conn, id)?;
    Ok(serde_json::json!({ "ok": true }))
}

// ── 섹션 커맨드 ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let (title, content): (Option<String>, String) = conn
        .query_row(
            "SELECT title, content FROM documents WHERE id=?1",
            [doc_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;
    Ok(serde_json::json!({
        "id": doc_id,
        "title": title.unwrap_or_default(),
        "content": content,
    }))
}

#[tauri::command]
pub async fn get_document_sections(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let sections = parse_sections(&content);
    let result: Vec<serde_json::Value> = sections
        .iter()
        .map(|s| serde_json::json!({
            "heading": s.heading,
            "level": s.level,
            "content": s.content,
            "start_line": s.start_line,
            "end_line": s.end_line,
        }))
        .collect();

    Ok(serde_json::json!(result))
}

#[tauri::command]
pub async fn update_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    heading: String,
    new_content: String,
    occurrence: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = replace_section(&old_content, &heading, occurrence.unwrap_or(0), &new_content)
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn insert_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    after_heading: Option<String>,
    new_section_content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = insert_section_after(&old_content, after_heading.as_deref(), &new_section_content)
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn delete_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    heading: String,
    occurrence: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = delete_section(&old_content, &heading, occurrence.unwrap_or(0))
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

// ── 내부 헬퍼 ────────────────────────────────────────────────────────────────

/// 파일 시스템에 문서 내용 동기화 (실패해도 무시)
fn sync_document_to_file(conn: &Connection, doc_id: i64, content: &str) {
    if let Ok(file_path) = conn.query_row(
        "SELECT file_path FROM documents WHERE id=?1",
        [doc_id],
        |r| r.get::<_, String>(0),
    ) {
        let _ = std::fs::write(&file_path, content);
    }
}

/// 즉시 재인덱싱 요청 (백그라운드 tokio::spawn)
fn enqueue_reindex(state: &crate::AppState, doc_id: i64) {
    let conn_arc = state.conn.clone();
    let _embedder = state.embedder.clone();
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            reindex_document_sync(&conn, doc_id)
        })
        .await;
        if let Err(e) = result {
            eprintln!("[reindex] doc_id={doc_id} spawn error: {e}");
        }
    });
}

/// 단일 문서 동기 재인덱싱 (FTS5 + content_hash 업데이트)
fn reindex_document_sync(conn: &Connection, doc_id: i64) -> Result<(), String> {
    let (project_id, source_doc_id, title, content): (i64, String, String, String) = conn
        .query_row(
            "SELECT project_id, source_doc_id, COALESCE(title, ''), content FROM documents WHERE id=?1",
            [doc_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;

    // FTS5 upsert (chunks_fts)
    conn.execute(
        "INSERT OR REPLACE INTO chunks_fts(rowid, title, content, project_id, source_doc_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![doc_id, title, content, project_id, source_doc_id],
    )
    .map_err(|e| e.to_string())?;

    // indexing_status 업데이트
    conn.execute(
        "UPDATE documents SET indexing_status='indexed', last_indexed=?1 WHERE id=?2",
        rusqlite::params![now_secs(), doc_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

// ── 단위 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    fn seed_workspace(conn: &rusqlite::Connection) -> i64 {
        let tmp = tempfile::tempdir().unwrap();
        ensure_default_workspace(conn, tmp.path()).unwrap()
    }

    #[test]
    fn resolve_project_id_uses_default_workspace() {
        let conn = make_conn();
        let ws_id = seed_workspace(&conn);
        let resolved = resolve_project_id(&conn, None).unwrap();
        assert_eq!(resolved, ws_id);
    }

    #[test]
    fn resolve_project_id_uses_explicit_id() {
        let conn = make_conn();
        seed_workspace(&conn);
        let resolved = resolve_project_id(&conn, Some(42));
        // project 42가 없어도 id 그대로 반환
        assert_eq!(resolved.unwrap(), 42);
    }

    #[test]
    fn resolve_project_id_error_when_no_default() {
        let conn = make_conn();
        // 디폴트 워크스페이스 없는 상태
        let result = resolve_project_id(&conn, None);
        assert!(result.is_err());
    }

    #[test]
    fn reindex_document_sync_updates_fts() {
        let conn = make_conn();
        let ws_id = seed_workspace(&conn);

        conn.execute(
            "INSERT INTO documents(project_id, source_doc_id, title, content, content_hash, indexing_status, created_at, updated_at)
             VALUES (?1, 'test-doc', '테스트', '검색 가능한 내용', 'hash', 'pending', 1, 1)",
            [ws_id],
        ).unwrap();
        let doc_id = conn.last_insert_rowid();

        reindex_document_sync(&conn, doc_id).unwrap();

        let status: String = conn.query_row(
            "SELECT indexing_status FROM documents WHERE id=?1", [doc_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(status, "indexed");
    }
}
