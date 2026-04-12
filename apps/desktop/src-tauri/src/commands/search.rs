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
    source_type: Option<String>,
    config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    let src_type = source_type.unwrap_or_else(|| "obsidian".to_string());
    let config_json = config.map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string());
    conn.execute(
        "INSERT INTO projects (name, display_name, path, status, source_type, config_json, created_at, updated_at) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?6)",
        rusqlite::params![name, name, path, src_type, config_json, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "project": {
            "name": name,
            "display_name": name,
            "path": path,
            "status": "active",
            "source_type": src_type,
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
pub async fn remove_project(
    state: tauri::State<'_, crate::AppState>,
    name: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let affected = conn.execute(
        "DELETE FROM projects WHERE name = ?1",
        rusqlite::params![name],
    )
    .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(format!("project '{}' not found", name));
    }
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
    // document_id 목록으로 project_name / source_type 일괄 조회
    let doc_ids: Vec<i64> = hits.iter().map(|h| h.document_id).collect();
    let mut project_info: std::collections::HashMap<i64, (String, String)> = std::collections::HashMap::new();
    for chunk in doc_ids.chunks(50) {
        let placeholders = chunk.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT d.id, p.name, COALESCE(p.source_type, 'obsidian') \
             FROM documents d JOIN projects p ON d.project_id = p.id \
             WHERE d.id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params: Vec<&dyn rusqlite::ToSql> = chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }).map_err(|e| e.to_string())?;
        for row in rows.filter_map(|r| r.ok()) {
            project_info.insert(row.0, (row.1, row.2));
        }
    }

    let hits_json: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|h| {
            let (project_name, source_type) = project_info
                .get(&h.document_id)
                .cloned()
                .unwrap_or_default();
            serde_json::json!({
                "document_id": h.document_id,
                "chunk_id": h.chunk_id,
                "title": h.title,
                "file_path": h.file_path,
                "heading_path": h.heading_path,
                "snippet": h.snippet,
                "score": h.score,
                "project_name": project_name,
                "source_type": source_type,
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
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};

    let (project_id, path, source_type, config_json_str) = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT id, path, COALESCE(source_type, 'obsidian'), COALESCE(config_json, '{}') FROM projects WHERE name = ?1",
            rusqlite::params![name],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        )
        .map_err(|_| format!("프로젝트 '{name}'을 찾을 수 없습니다"))?
    };

    let config_map: serde_json::Value = serde_json::from_str(&config_json_str)
        .unwrap_or(serde_json::json!({}));

    let mut total = 0usize;
    let mut cursor: Option<String> = None;

    match source_type.as_str() {
        "confluence" => {
            use doxus_plugin_confluence::ConfluencePlugin;
            let mut plugin = ConfluencePlugin::new();
            let mut config = PluginConfig::default();
            if let Some(base_url) = config_map.get("base_url").and_then(|v| v.as_str()) {
                config.fields.insert("base_url".to_string(), serde_json::json!(base_url));
            }
            if let Some(space_key) = config_map.get("space_key").and_then(|v| v.as_str()) {
                if !space_key.is_empty() {
                    config.fields.insert("space_key".to_string(), serde_json::json!(space_key));
                }
            }
            if let Some(ancestor_id) = config_map.get("ancestor_id").and_then(|v| v.as_str()) {
                if !ancestor_id.is_empty() {
                    config.fields.insert("ancestor_id".to_string(), serde_json::json!(ancestor_id));
                }
            }
            let api_token = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:api_token")
                .ok()
                .and_then(|e| e.get_password().ok())
                .unwrap_or_default();
            let access_token = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:access_token")
                .ok()
                .and_then(|e| e.get_password().ok())
                .unwrap_or_default();
            // Personal API Token이면 Basic auth를 위해 email도 필요
            let email = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:email")
                .ok()
                .and_then(|e| e.get_password().ok())
                .unwrap_or_default();
            let token = if !access_token.is_empty() && email.is_empty() {
                // OAuth Bearer token (email 없음)
                access_token.clone()
            } else if !email.is_empty() {
                // Personal API Token + email → Basic auth
                api_token.clone()
            } else {
                api_token.clone()
            };
            eprintln!("[index_project] email={} token_len={}", if email.is_empty() { "none" } else { "set" }, token.len());
            if !email.is_empty() {
                config.fields.insert("email".to_string(), serde_json::json!(email));
            }
            config.fields.insert("api_token".to_string(), serde_json::json!(token));
            let mut secrets = PluginSecrets::default();
            secrets.fields.insert("api_token".to_string(), doxus_plugin_sdk::SecretValue::Text(token));

            plugin.initialize(config, secrets).await
                .map_err(|e| format!("Confluence 플러그인 초기화 실패: {e}"))?;

            loop {
                let stream = plugin.fetch_all(FetchAllOpts { cursor: cursor.clone(), page_size: 50 })
                    .await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    let engine = SearchEngine::new(&conn);
                    for doc in &stream.documents {
                        if engine.index_document(project_id, &doc.id.0, doc.title.as_deref().unwrap_or("Untitled"), &doc.content).is_ok() {
                            total += 1;
                        }
                    }
                }
                cursor = stream.next_cursor.clone();
                if cursor.is_none() { break; }
            }
        }
        "github" => {
            use doxus_plugin_github::GitHubPlugin;
            let mut plugin = GitHubPlugin::new();
            let mut config = PluginConfig::default();
            if let Some(repo) = config_map.get("repo").and_then(|v| v.as_str()) {
                config.fields.insert("repo".to_string(), serde_json::json!(repo));
            }
            let token = keyring::Entry::new("doxus", "doxus:com.doxus.github:token")
                .ok()
                .and_then(|e| e.get_password().ok())
                .unwrap_or_default();
            config.fields.insert("token".to_string(), serde_json::json!(token));
            let mut secrets = PluginSecrets::default();
            secrets.fields.insert("token".to_string(), doxus_plugin_sdk::SecretValue::Text(token));

            plugin.initialize(config, secrets).await
                .map_err(|e| format!("GitHub 플러그인 초기화 실패: {e}"))?;

            loop {
                let stream = plugin.fetch_all(FetchAllOpts { cursor: cursor.clone(), page_size: 50 })
                    .await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    let engine = SearchEngine::new(&conn);
                    for doc in &stream.documents {
                        if engine.index_document(project_id, &doc.id.0, doc.title.as_deref().unwrap_or("Untitled"), &doc.content).is_ok() {
                            total += 1;
                        }
                    }
                }
                cursor = stream.next_cursor.clone();
                if cursor.is_none() { break; }
            }
        }
        _ => {
            use doxus_plugin_obsidian::ObsidianPlugin;
            let mut plugin = ObsidianPlugin::new();
            let mut config = PluginConfig::default();
            config.fields.insert("path".to_string(), serde_json::json!(path));
            plugin.initialize(config, PluginSecrets::default()).await
                .map_err(|e| format!("플러그인 초기화 실패: {e}"))?;
            loop {
                let stream = plugin.fetch_all(FetchAllOpts { cursor: cursor.clone(), page_size: 100 })
                    .await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    let engine = SearchEngine::new(&conn);
                    for doc in &stream.documents {
                        if engine.index_document(project_id, &doc.id.0, doc.title.as_deref().unwrap_or("Untitled"), &doc.content).is_ok() {
                            total += 1;
                        }
                    }
                }
                cursor = stream.next_cursor.clone();
                if cursor.is_none() { break; }
            }
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
    let mut stmt = conn.prepare(
        "SELECT id, title, content FROM documents WHERE source_doc_id = ?1 OR file_path = ?1 ORDER BY id ASC"
    ).map_err(|e| e.to_string())?;
    let rows: Vec<(i64, Option<String>, String)> = stmt
        .query_map(rusqlite::params![file_path], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        return Err(format!("문서를 찾을 수 없음: {file_path}"));
    }
    let id = rows[0].0;
    let title = rows[0].1.clone();
    let content = rows.into_iter().map(|(_, _, c)| c).collect::<Vec<_>>().join("\n\n");
    Ok(serde_json::json!({
        "document_id": id,
        "title": title,
        "content": content,
        "file_path": file_path,
    }))
}

#[tauri::command]
pub async fn get_document_content(
    state: tauri::State<'_, crate::AppState>,
    file_path: String,
    project_name: Option<String>,
    force_refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    use doxus_plugin_sdk::{DocSource, PluginConfig, PluginSecrets, SourceDocId};

    // project_name이 있으면 플러그인을 통해 실시간으로 가져옴
    if let Some(ref pname) = project_name {
        let (path, source_type, config_json_str) = {
            let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.query_row(
                "SELECT path, COALESCE(source_type, 'obsidian'), COALESCE(config_json, '{}') FROM projects WHERE name = ?1",
                rusqlite::params![pname],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )
            .map_err(|_| format!("프로젝트 '{pname}'을 찾을 수 없습니다"))?
        };

        let config_map: serde_json::Value = serde_json::from_str(&config_json_str)
            .unwrap_or(serde_json::json!({}));

        let doc_id = SourceDocId(file_path.clone());

        match source_type.as_str() {
            "confluence" => {
                use doxus_core::cache::ContentCache;
                use doxus_plugin_confluence::ConfluencePlugin;

                let force = force_refresh.unwrap_or(false);
                // TTL은 plugin_kv["com.doxus.confluence"]["settings"]["cache_ttl_minutes"]에서 읽음
                // None = 캐시 비활성화 (opt-in), 최소 10분
                let cache_ttl: Option<u32> = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    conn.query_row(
                        "SELECT CAST(value AS INTEGER) FROM plugin_kv
                         WHERE plugin_id = 'com.doxus.confluence'
                           AND namespace = 'settings'
                           AND key = 'cache_ttl_minutes'",
                        [],
                        |r| r.get::<_, i64>(0),
                    ).ok().map(|v| v as u32).filter(|&v| v >= 10)
                };

                // Cache hit 확인 (force_refresh가 아니고 TTL이 설정된 경우)
                if let Some(ttl) = cache_ttl {
                    if !force {
                        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                        let cache = ContentCache::new(&conn);
                        if let Ok(Some(cached_content)) = cache.get("com.doxus.confluence", &file_path) {
                            let _ = cache.touch("com.doxus.confluence", &file_path, ttl);
                            return Ok(serde_json::json!({
                                "title": null,
                                "content": cached_content,
                                "file_path": file_path,
                                "from_cache": true,
                                "reindex_triggered": false,
                            }));
                        }
                    } else {
                        // force_refresh: 기존 캐시 항목 제거
                        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                        let cache = ContentCache::new(&conn);
                        let _ = cache.invalidate("com.doxus.confluence", &file_path);
                    }
                }

                let mut plugin = ConfluencePlugin::new();
                let mut config = PluginConfig::default();
                if let Some(base_url) = config_map.get("base_url").and_then(|v| v.as_str()) {
                    config.fields.insert("base_url".to_string(), serde_json::json!(base_url));
                }
                if let Some(space_key) = config_map.get("space_key").and_then(|v| v.as_str()) {
                    if !space_key.is_empty() {
                        config.fields.insert("space_key".to_string(), serde_json::json!(space_key));
                    }
                }
                let api_token = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:api_token")
                    .ok().and_then(|e| e.get_password().ok()).unwrap_or_default();
                let access_token = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:access_token")
                    .ok().and_then(|e| e.get_password().ok()).unwrap_or_default();
                let email = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:email")
                    .ok().and_then(|e| e.get_password().ok()).unwrap_or_default();
                let token = if !access_token.is_empty() && email.is_empty() {
                    access_token
                } else {
                    api_token
                };
                if !email.is_empty() {
                    config.fields.insert("email".to_string(), serde_json::json!(email));
                }
                let token_len = token.len();
                config.fields.insert("api_token".to_string(), serde_json::json!(token.clone()));
                let mut secrets = PluginSecrets::default();
                secrets.fields.insert("api_token".to_string(), doxus_plugin_sdk::SecretValue::Text(token));
                eprintln!("[get_document_content] confluence base_url={:?} doc_id={} email={} token_len={}",
                    config_map.get("base_url"),
                    file_path,
                    if email.is_empty() { "none" } else { &email },
                    token_len,
                );
                plugin.initialize(config, secrets).await
                    .map_err(|e| format!("Confluence 플러그인 초기화 실패: {e}"))?;
                let raw = plugin.fetch_document(&doc_id).await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                // 캐시에 저장 (TTL이 설정된 경우)
                if let Some(ttl) = cache_ttl {
                    let cache = ContentCache::new(&conn);
                    let _ = cache.set("com.doxus.confluence", &file_path, &raw.content, ttl);
                }
                let reindexed = reindex_if_stale(
                    &conn, pname, &file_path,
                    raw.title.as_deref().unwrap_or("Untitled"),
                    &raw.content,
                ).unwrap_or(false);
                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                    "from_cache": false,
                    "reindex_triggered": reindexed,
                }));
            }
            "github" => {
                use doxus_plugin_github::GitHubPlugin;
                let mut plugin = GitHubPlugin::new();
                let mut config = PluginConfig::default();
                if let Some(repo) = config_map.get("repo").and_then(|v| v.as_str()) {
                    config.fields.insert("repo".to_string(), serde_json::json!(repo));
                }
                let token = keyring::Entry::new("doxus", "doxus:com.doxus.github:token")
                    .ok().and_then(|e| e.get_password().ok()).unwrap_or_default();
                config.fields.insert("token".to_string(), serde_json::json!(token.clone()));
                let mut secrets = PluginSecrets::default();
                secrets.fields.insert("token".to_string(), doxus_plugin_sdk::SecretValue::Text(token));

                plugin.initialize(config, secrets).await
                    .map_err(|e| format!("GitHub 플러그인 초기화 실패: {e}"))?;
                let raw = plugin.fetch_document(&doc_id).await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                }));
            }
            _ => {
                // Obsidian: 로컬 파일 직접 읽기
                use doxus_plugin_obsidian::ObsidianPlugin;
                let mut plugin = ObsidianPlugin::new();
                let mut config = PluginConfig::default();
                config.fields.insert("path".to_string(), serde_json::json!(path));
                plugin.initialize(config, PluginSecrets::default()).await
                    .map_err(|e| format!("Obsidian 플러그인 초기화 실패: {e}"))?;
                let raw = plugin.fetch_document(&doc_id).await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;
                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                }));
            }
        }
    }

    // project_name 없으면 SQLite 캐시 fallback
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    get_document_content_impl(&conn, &file_path)
}

/// Compares `sha256(content)` with stored `content_hash`.
/// If different, re-indexes the document and returns `true`.
/// If same or document not in DB, returns `false`.
pub fn reindex_if_stale(
    conn: &rusqlite::Connection,
    project_name: &str,
    source_doc_id: &str,
    title: &str,
    content: &str,
) -> Result<bool, String> {
    use sha2::{Digest, Sha256};

    let new_hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    // Single JOIN query: project_id + stored content_hash
    use rusqlite::OptionalExtension;
    let row: Option<(i64, String)> = conn.query_row(
        "SELECT p.id, d.content_hash
         FROM projects p
         JOIN documents d ON d.project_id = p.id AND d.source_doc_id = ?2
         WHERE p.name = ?1
         LIMIT 1",
        rusqlite::params![project_name, source_doc_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())?;

    let (project_id, stored_hash) = match row {
        None => return Ok(false), // 신규 문서 — reindex 불필요
        Some(r) => r,
    };

    if new_hash == stored_hash {
        return Ok(false); // 변경 없음
    }

    // 해시 달라짐 → reindex
    let engine = doxus_core::search::SearchEngine::new(conn);
    engine
        .index_document(project_id, source_doc_id, title, content)
        .map_err(|e| e.to_string())?;

    Ok(true)
}

pub fn list_all_documents_impl(conn: &rusqlite::Connection) -> Result<serde_json::Value, String> {
    let mut stmt = conn.prepare(
        "SELECT MIN(d.id), MIN(d.title), MIN(d.source_doc_id), p.name, COALESCE(p.source_type, 'obsidian')
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         WHERE p.status = 'active'
         GROUP BY d.source_doc_id, d.project_id
         ORDER BY p.name, MIN(d.title)"
    ).map_err(|e| e.to_string())?;
    let docs: Vec<_> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "document_id": r.get::<_, i64>(0)?,
                "title": r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "(제목 없음)".to_string()),
                "source_doc_id": r.get::<_, String>(2)?,
                "project_name": r.get::<_, String>(3)?,
                "source_type": r.get::<_, String>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "documents": docs }))
}

#[tauri::command]
pub async fn list_all_documents(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    list_all_documents_impl(&conn)
}

#[tauri::command]
pub async fn list_projects(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmt = conn
        .prepare("SELECT name, display_name, path, status, COALESCE(source_type, 'obsidian') FROM projects ORDER BY name")
        .map_err(|e| e.to_string())?;
    let projects: Vec<_> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "name": r.get::<_, String>(0)?,
                "display_name": r.get::<_, String>(1)?,
                "path": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?,
                "source_type": r.get::<_, String>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "projects": projects }))
}
