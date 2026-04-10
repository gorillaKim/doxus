use doxus_core::search::{SearchEngine, SearchQuery};

/// Index all active projects using their registered plugin. Returns count of indexed documents.
pub(crate) fn run_reindex(conn: &rusqlite::Connection) -> Result<usize, String> {
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};

    let mut stmt = conn
        .prepare("SELECT id, name, path FROM projects WHERE status = 'active'")
        .map_err(|e| e.to_string())?;

    let projects: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let engine = SearchEngine::new(conn);
    let mut total = 0usize;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e: std::io::Error| e.to_string())?;

    for (project_id, name, path) in projects {
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".to_string(), serde_json::json!(path));

        if let Err(e) = rt.block_on(plugin.initialize(config, PluginSecrets::default())) {
            eprintln!("Failed to initialize plugin for {name}: {e}");
            continue;
        }

        let mut cursor = None;
        loop {
            let stream = match rt.block_on(plugin.fetch_all(FetchAllOpts {
                cursor: cursor.clone(),
                page_size: 100,
            })) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("fetch_all error for {name}: {e}");
                    break;
                }
            };

            for doc in &stream.documents {
                let source_doc_id = doc.id.0.as_str();
                let title = doc.title.as_deref().unwrap_or("Untitled");
                if let Err(e) =
                    engine.index_document(project_id, source_doc_id, title, &doc.content)
                {
                    eprintln!("index_document error: {e}");
                } else {
                    total += 1;
                }
            }

            cursor = stream.next_cursor.clone();
            if cursor.is_none() {
                break;
            }
        }
    }

    Ok(total)
}

#[derive(serde::Serialize)]
pub struct TopDocument {
    pub document_id: i64,
    pub title: String,
    pub file_path: String,
    pub count: i64,
}

pub(crate) fn increment_view_count_impl(conn: &rusqlite::Connection, document_id: i64) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO view_counts (document_id, count, last_viewed) VALUES (?1, 1, ?2)
         ON CONFLICT(document_id) DO UPDATE SET count = count + 1, last_viewed = ?2",
        rusqlite::params![document_id, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn get_top_documents_impl(conn: &rusqlite::Connection, limit: usize) -> Result<Vec<TopDocument>, String> {
    let mut stmt = conn.prepare(
        "SELECT d.id, COALESCE(d.title, 'Untitled') as title, d.source_doc_id, v.count
         FROM view_counts v
         JOIN documents d ON v.document_id = d.id
         ORDER BY v.count DESC
         LIMIT ?1"
    ).map_err(|e| e.to_string())?;
    let rows: Vec<TopDocument> = stmt.query_map(rusqlite::params![limit as i64], |r| {
        Ok(TopDocument {
            document_id: r.get(0)?,
            title: r.get(1)?,
            file_path: r.get(2)?,
            count: r.get(3)?,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();
    Ok(rows)
}

#[tauri::command]
pub async fn increment_view_count(
    state: tauri::State<'_, crate::AppState>,
    document_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    increment_view_count_impl(&conn, document_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn get_top_documents(
    state: tauri::State<'_, crate::AppState>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let tops = get_top_documents_impl(&conn, limit.unwrap_or(5))?;
    Ok(serde_json::json!({ "documents": tops }))
}

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
    fn search_engine_status_returns_doc_count() {
        let conn = make_conn();
        let total_documents: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap();
        let total_projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_documents, 0);
        assert_eq!(total_projects, 0);
    }

    #[test]
    fn trigger_reindex_indexes_obsidian_vault() {
        use std::fs;

        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir(&vault).unwrap();
        fs::write(vault.join("note.md"), "# Hello\nThis is a test note.").unwrap();

        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at)
             VALUES ('test-vault', 'Test Vault', ?1, 'active', ?2, ?2)",
            rusqlite::params![vault.to_str().unwrap(), now],
        )
        .unwrap();
        let project_id: i64 = conn
            .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
            .unwrap();

        let indexed = super::run_reindex(&conn).unwrap();
        assert!(indexed >= 1, "at least 1 document should be indexed");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE project_id = ?1",
                rusqlite::params![project_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn increment_view_count_upserts_correctly() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES ('p','p','/tmp','active',?1,?1)",
            rusqlite::params![now],
        ).unwrap();
        let project_id: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed) VALUES (?1,'d1','T','C','h',?2)",
            rusqlite::params![project_id, now],
        ).unwrap();
        let doc_id: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();

        // first increment → creates row with count=1
        super::increment_view_count_impl(&conn, doc_id).unwrap();
        let count: i64 = conn.query_row(
            "SELECT count FROM view_counts WHERE document_id = ?1",
            rusqlite::params![doc_id], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 1);

        // second increment → count=2
        super::increment_view_count_impl(&conn, doc_id).unwrap();
        let count: i64 = conn.query_row(
            "SELECT count FROM view_counts WHERE document_id = ?1",
            rusqlite::params![doc_id], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn get_top_documents_returns_ordered() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES ('p','p','/tmp','active',?1,?1)",
            rusqlite::params![now],
        ).unwrap();
        let pid: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();

        for (i, title) in ["Doc A", "Doc B", "Doc C"].iter().enumerate() {
            conn.execute(
                "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed) VALUES (?1,?2,?3,'C','h',?4)",
                rusqlite::params![pid, format!("d{}", i), title, now],
            ).unwrap();
            let did: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();
            let view_count = (i + 1) as i64; // A=1, B=2, C=3
            conn.execute(
                "INSERT INTO view_counts (document_id, count, last_viewed) VALUES (?1, ?2, ?3)",
                rusqlite::params![did, view_count, now],
            ).unwrap();
        }

        let tops = super::get_top_documents_impl(&conn, 3).unwrap();
        assert_eq!(tops.len(), 3);
        assert_eq!(tops[0].title, "Doc C"); // highest count first
        assert_eq!(tops[0].count, 3);
    }

    #[test]
    fn get_document_content_returns_content() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        conn.execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at) VALUES ('p','p','/tmp','active',?1,?1)",
            rusqlite::params![now],
        ).unwrap();
        let pid: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, last_indexed) VALUES (?1, '/path/to/note.md', 'My Note', '# Hello', 'h', ?2)",
            rusqlite::params![pid, now],
        ).unwrap();

        let result = super::get_document_content_impl(&conn, "/path/to/note.md").unwrap();
        assert_eq!(result["title"], "My Note");
        assert_eq!(result["content"], "# Hello");
        assert_eq!(result["file_path"], "/path/to/note.md");
    }

    #[test]
    fn get_document_content_not_found_returns_err() {
        let conn = make_conn();
        let err = super::get_document_content_impl(&conn, "/nonexistent.md").unwrap_err();
        assert!(err.contains("문서를 찾을 수 없음"));
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
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
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
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
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
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
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
pub async fn search_engine_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let total_documents: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let total_projects: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "total_documents": total_documents,
        "total_projects": total_projects,
        "index_size_bytes": 0
    }))
}

#[tauri::command]
pub async fn index_project(
    state: tauri::State<'_, crate::AppState>,
    name: String,
) -> Result<serde_json::Value, String> {
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};

    // DB 락을 최소한으로 유지: 프로젝트 정보만 조회 후 즉시 해제
    let (project_id, path) = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT id, path FROM projects WHERE name = ?1",
            rusqlite::params![name],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(|_| format!("프로젝트 '{name}'을 찾을 수 없습니다"))?
    };

    let mut plugin = ObsidianPlugin::new();
    let mut config = PluginConfig::default();
    config.fields.insert("path".to_string(), serde_json::json!(path));

    // async 컨텍스트이므로 .await 직접 사용
    plugin.initialize(config, PluginSecrets::default()).await
        .map_err(|e| format!("플러그인 초기화 실패: {e}"))?;

    let mut total = 0usize;
    let mut cursor = None;
    loop {
        let stream = plugin
            .fetch_all(FetchAllOpts { cursor: cursor.clone(), page_size: 100 })
            .await
            .map_err(|e| format!("문서 가져오기 실패: {e}"))?;

        // 배치 인덱싱: 락을 배치 단위로 짧게 유지
        {
            let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
            let engine = SearchEngine::new(&conn);
            for doc in &stream.documents {
                let source_doc_id = doc.id.0.as_str();
                let title = doc.title.as_deref().unwrap_or("Untitled");
                if engine.index_document(project_id, source_doc_id, title, &doc.content).is_ok() {
                    total += 1;
                }
            }
        }

        cursor = stream.next_cursor.clone();
        if cursor.is_none() {
            break;
        }
    }

    Ok(serde_json::json!({
        "status": "ok",
        "indexed": total,
        "message": format!("{total}개 문서 인덱싱 완료")
    }))
}

#[tauri::command]
pub async fn trigger_reindex(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};

    // active 프로젝트 목록만 락으로 가져온 후 즉시 해제
    let projects: Vec<(i64, String, String)> = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare("SELECT id, name, path FROM projects WHERE status = 'active'")
            .map_err(|e| e.to_string())?;
        let rows: Vec<_> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    let mut total = 0usize;
    for (project_id, name, path) in projects {
        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".to_string(), serde_json::json!(path));

        if let Err(e) = plugin.initialize(config, PluginSecrets::default()).await {
            eprintln!("Failed to initialize plugin for {name}: {e}");
            continue;
        }

        let mut cursor = None;
        loop {
            let stream = match plugin.fetch_all(FetchAllOpts { cursor: cursor.clone(), page_size: 100 }).await {
                Ok(s) => s,
                Err(e) => { eprintln!("fetch_all error for {name}: {e}"); break; }
            };

            {
                let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                let engine = SearchEngine::new(&conn);
                for doc in &stream.documents {
                    let source_doc_id = doc.id.0.as_str();
                    let title = doc.title.as_deref().unwrap_or("Untitled");
                    if engine.index_document(project_id, source_doc_id, title, &doc.content).is_ok() {
                        total += 1;
                    }
                }
            }

            cursor = stream.next_cursor.clone();
            if cursor.is_none() { break; }
        }
    }

    Ok(serde_json::json!({
        "status": "ok",
        "indexed": total,
        "message": format!("{total}개 문서 재인덱싱 완료")
    }))
}

pub(crate) fn get_document_content_impl(conn: &rusqlite::Connection, file_path: &str) -> Result<serde_json::Value, String> {
    let result = conn.query_row(
        "SELECT id, title, content FROM documents WHERE source_doc_id = ?1 LIMIT 1",
        rusqlite::params![file_path],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?)),
    );
    match result {
        Ok((id, title, content)) => Ok(serde_json::json!({
            "document_id": id,
            "title": title,
            "content": content,
            "file_path": file_path,
        })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!("문서를 찾을 수 없음: {file_path}")),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn get_document_content(
    state: tauri::State<'_, crate::AppState>,
    file_path: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    get_document_content_impl(&conn, &file_path)
}

#[tauri::command]
pub async fn list_projects(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
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
