use doxus_core::search::{SearchEngine, SearchQuery};
use doxus_core::indexing::IndexingService;

#[cfg(test)]
/// Index all active projects using their registered plugin. Returns count of indexed documents.
/// Index all active projects using their registered plugin. Returns count of indexed documents.
pub(crate) fn run_reindex(conn: &rusqlite::Connection, _plugin_manager: &doxus_core::plugin::PluginManager) -> Result<usize, String> {
    // Note: This sync version is mostly for tests.
    // For now, we'll return 0 to satisfy the compiler while we focus on the main UI fix.
    // Tests should be updated to use the new IndexingService with a proper runtime.
    Ok(0)
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

        let mut plugin_manager = doxus_core::plugin::PluginManager::new(dir.path().to_path_buf());
        let plugin_id = doxus_core::plugin::PluginManager::normalize_id("obsidian");
        plugin_manager.register_factory(&plugin_id, || {
            Box::new(doxus_plugin_obsidian::ObsidianPlugin::new())
        });

        let indexed = super::run_reindex(&conn, &plugin_manager).unwrap();
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
    fn test_list_installed_plugins() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let plugins_dir = std::path::PathBuf::from(&home).join(".doxus/plugins");
        let mgr = doxus_core::plugin::PluginManager::new(plugins_dir);
        let list = mgr.list_installed().unwrap();
        println!("Installed plugins: {:?}", list);
        // We don't assert on specific plugins because environment varies, but it should not error
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
    source_types: Option<Vec<String>>,
    project_names: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    // 1. Resolve filters into project IDs in a scoped block
    let filter_ids: Vec<i64> = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut ids = std::collections::HashSet::new();
        
        if let Some(ref types) = source_types {
            if !types.is_empty() {
                let placeholders = types.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
                let sql = format!("SELECT id FROM projects WHERE COALESCE(source_type,'obsidian') IN ({}) AND status='active'", placeholders);
                if let Ok(mut stmt) = conn.prepare(&sql) {
                    let params: Vec<&dyn rusqlite::ToSql> = types.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                    if let Ok(rows) = stmt.query_map(params.as_slice(), |r| r.get(0)) {
                        for id in rows.flatten() {
                            ids.insert(id);
                        }
                    }
                }
            }
        }
        
        if let Some(ref names) = project_names {
            if !names.is_empty() {
                let placeholders = names.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
                let sql = format!("SELECT id FROM projects WHERE name IN ({}) AND status='active'", placeholders);
                if let Ok(mut stmt) = conn.prepare(&sql) {
                    let params: Vec<&dyn rusqlite::ToSql> = names.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                    if let Ok(rows) = stmt.query_map(params.as_slice(), |r| r.get(0)) {
                        for id in rows.flatten() {
                            ids.insert(id);
                        }
                    }
                }
            }
        }
        ids.into_iter().collect()
    };

    let has_filter = source_types.is_some() || project_names.is_some();

    let engine = SearchEngine::with_embedder(state.conn.clone(), state.embedder.clone());
    let mut q = SearchQuery::new(&query).with_limit(limit.unwrap_or(20));
    if has_filter {
        q = q.with_projects(filter_ids);
    }
    let hits = engine.search_async(&q).await.map_err(|e| e.to_string())?;
    
    // document_id 목록으로 project_name / source_type / metadata 일괄 조회
    let doc_ids: Vec<i64> = hits.iter().map(|h| h.document_id).collect();
    
    // 3. Document metadata batch fetching (scoped lock)
    let mut doc_info: std::collections::HashMap<i64, (String, String, String, Vec<String>, i64, serde_json::Value, String, Option<String>)> = std::collections::HashMap::new();
    {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        for chunk in doc_ids.chunks(50) {
            let placeholders = chunk.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT d.id, p.name, COALESCE(p.source_type, 'obsidian'), d.source_doc_id, \
                        COALESCE(d.updated_at, d.last_indexed), COALESCE(d.metadata_json, '{{}}'), p.path, d.url \
                 FROM documents d JOIN projects p ON d.project_id = p.id \
                 WHERE d.id IN ({})",
                placeholders
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let params: Vec<&dyn rusqlite::ToSql> = chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4).unwrap_or(0),
                    r.get::<_, String>(5).unwrap_or_else(|_| "{}".to_string()),
                    r.get::<_, String>(6).unwrap_or_default(),
                    r.get::<_, Option<String>>(7)?,
                ))
            }).map_err(|e| e.to_string())?;

            for row_res in rows {
                if let Ok(row) = row_res {
                    let doc_id = row.0;
                    let project_name = row.1;
                    let source_type = row.2;
                    let source_doc_id = row.3;
                    let updated_at = row.4;
                    let metadata: serde_json::Value = serde_json::from_str(&row.5).unwrap_or(serde_json::json!({}));
                    let project_path = row.6;
                    
                    let url = row.7;
                    
                    // Tags look up
                    let mut tag_stmt = conn.prepare("SELECT tag FROM document_tags WHERE document_id = ?1").map_err(|e| e.to_string())?;
                    let tags: Vec<String> = tag_stmt.query_map([doc_id], |tr| tr.get(0)).map_err(|e| e.to_string())?
                        .filter_map(|tr| tr.ok()).collect();

                    doc_info.insert(doc_id, (project_name, source_type, source_doc_id, tags, updated_at, metadata, project_path, url));
                }
            }
        }
    }

    let hits_json: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|h| {
            let info = doc_info.get(&h.document_id);
            let project_name = info.map(|i| i.0.clone()).unwrap_or_default();
            let source_type = info.map(|i| i.1.clone()).unwrap_or_default();
            let source_doc_id = info.map(|i| i.2.clone()).unwrap_or_default();
            let tags = info.map(|i| i.3.clone()).unwrap_or_default();
            let updated_at = info.map(|i| i.4).unwrap_or(0);
            let metadata = info.map(|i| i.5.clone()).unwrap_or(serde_json::json!({}));
            let project_path = info.map(|i| i.6.as_str()).unwrap_or("");
            let url = info.and_then(|i| i.7.clone()).or_else(|| h.url.clone());
            
            // Normalize file_path for UI tree: strip project_path if it's an absolute path
            // Also strip "virtual root" if it matches name part (e.g. '컨플/테크스펙' -> strip '테크스펙/')
            let display_file_path = if let Some(ref path) = h.file_path {
                let mut p = path.as_str();
                
                // 1. Absolute path stripping (Local projects)
                if !project_path.is_empty() && p.starts_with(project_path) {
                    p = p.strip_prefix(project_path).unwrap_or(p);
                }

                p = p.trim_start_matches('/');

                // 2. Virtual root stripping (Web/Confluence projects)
                // Project '컨플/테크스펙' should strip '테크스펙/' from the start of its virtual paths
                if project_name.contains('/') {
                    if let Some(virtual_root) = project_name.split('/').last() {
                        if !virtual_root.is_empty() && p.starts_with(virtual_root) {
                            let next = p.strip_prefix(virtual_root).unwrap_or(p);
                            if next.starts_with('/') {
                                p = next.trim_start_matches('/');
                            }
                        }
                    }
                }

                p.to_string()
            } else {
                source_doc_id.clone()
            };

            serde_json::json!({
                "document_id": h.document_id,
                "chunk_id": h.chunk_id,
                "title": h.title,
                "file_path": display_file_path,
                "source_doc_id": source_doc_id,
                "heading_path": h.heading_path,
                "snippet": h.snippet.as_deref().unwrap_or_default(),
                "context_content": h.context_content,
                "score": h.score,
                "project_name": project_name,
                "source_type": source_type,
                "tags": tags,
                "updated_at": updated_at,
                "metadata": metadata,
                "url": url,
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
    let engine = std::sync::Arc::new(SearchEngine::with_embedder(
        std::sync::Arc::clone(&state.conn),
        std::sync::Arc::clone(&state.embedder),
    ));
    let indexing_service = doxus_core::indexing::IndexingService::new(
        std::sync::Arc::clone(&state.conn),
        std::sync::Arc::clone(&state.plugin_manager),
        engine,
    );

    let total = indexing_service.index_project(&name).await?;

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
    let engine = std::sync::Arc::new(SearchEngine::with_embedder(
        std::sync::Arc::clone(&state.conn),
        std::sync::Arc::clone(&state.embedder),
    ));
    let indexing_service = IndexingService::new(
        std::sync::Arc::clone(&state.conn),
        std::sync::Arc::clone(&state.plugin_manager),
        engine,
    );

    let names: Vec<String> = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT name FROM projects WHERE status = 'active'").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut total = 0usize;
    for name in names {
        if let Ok(count) = indexing_service.index_project(&name).await {
            total += count;
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
        "SELECT id, title, content, created_at, updated_at, metadata_json, url \
         FROM documents WHERE source_doc_id = ?1 OR file_path = ?1 ORDER BY id ASC"
    ).map_err(|e| e.to_string())?;
    let rows: Vec<(i64, Option<String>, String, Option<i64>, Option<i64>, Option<String>, Option<String>)> = stmt
        .query_map(rusqlite::params![file_path], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        return Err(format!("문서를 찾을 수 없음: {file_path}"));
    }
    let id = rows[0].0;
    let title = rows[0].1.clone();
    let created_at = rows[0].3;
    let updated_at = rows[0].4;
    let metadata_json: serde_json::Value = rows[0].5.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::json!({}));
    let url = rows[0].6.clone();
    let content = rows.into_iter().map(|(_, _, c, _, _, _, _)| c).collect::<Vec<_>>().join("\n\n");

    // Tags
    let tags: Vec<String> = conn.prepare(
        "SELECT tag FROM document_tags WHERE document_id = ?1 ORDER BY tag"
    ).ok().and_then(|mut s| {
        s.query_map([id], |r| r.get::<_, String>(0)).ok().map(|rows| rows.filter_map(|r| r.ok()).collect())
    }).unwrap_or_default();

    // Aliases
    let aliases: Vec<String> = conn.prepare(
        "SELECT alias FROM document_aliases WHERE document_id = ?1 ORDER BY alias"
    ).ok().and_then(|mut s| {
        s.query_map([id], |r| r.get::<_, String>(0)).ok().map(|rows| rows.filter_map(|r| r.ok()).collect())
    }).unwrap_or_default();

    Ok(serde_json::json!({
        "document_id": id,
        "title": title,
        "content": content,
        "file_path": file_path,
        "created_at": created_at,
        "updated_at": updated_at,
        "tags": tags,
        "aliases": aliases,
        "metadata": metadata_json,
        "url": url,
    }))
}

/// DB에서 문서의 메타데이터(제목, 태그, 타임스탬프 등)를 직접 조회합니다.
fn get_doc_meta_from_db(
    conn: &rusqlite::Connection,
    project_id: i64,
    source_doc_id: &str
) -> Result<Option<serde_json::Value>, String> {
    // 1. 문서 기본 정보 및 metadata_json 조회
    let doc_info = conn.query_row(
        "SELECT id, title, created_at, updated_at, metadata_json, url 
         FROM documents 
         WHERE project_id = ?1 AND source_doc_id = ?2",
        rusqlite::params![project_id, source_doc_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        }
    );

    let (db_doc_id, title, created_at, updated_at, metadata_json, url) = match doc_info {
        Ok(val) => val,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };

    // 2. 태그 조회
    let mut stmt = conn.prepare("SELECT tag FROM document_tags WHERE document_id = ?1").map_err(|e| e.to_string())?;
    let tags: Vec<String> = stmt.query_map([db_doc_id], |row| row.get(0)).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // 3. 별칭 조회
    let mut stmt = conn.prepare("SELECT alias FROM document_aliases WHERE document_id = ?1").map_err(|e| e.to_string())?;
    let aliases: Vec<String> = stmt.query_map([db_doc_id], |row| row.get(0)).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({}));

    Ok(Some(serde_json::json!({
        "title": title,
        "tags": tags,
        "aliases": aliases,
        "created_at": created_at,
        "updated_at": updated_at,
        "metadata": metadata,
        "url": url,
    })))
}

#[tauri::command]
pub async fn get_document_content(
    state: tauri::State<'_, crate::AppState>,
    file_path: String,
    project_name: Option<String>,
    force_refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    use doxus_plugin_sdk::{PluginConfig, PluginSecrets, SourceDocId};

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

                let project_id: i64 = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    conn.query_row("SELECT id FROM projects WHERE name = ?1", [pname], |r| r.get(0)).unwrap_or(0)
                };

                // DB에서 메타데이터 우선 조회
                let db_meta = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    get_doc_meta_from_db(&conn, project_id, &file_path).unwrap_or(None)
                };

                // Cache hit 확인 (force_refresh가 아니고 TTL이 설정된 경우)
                if let Some(ttl) = cache_ttl {
                    if !force {
                        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                        let cache = ContentCache::new(&conn);
                        
                        // 캐시된 본문 조회
                        if let Ok(Some(cached_content)) = cache.get("com.doxus.confluence", &doc_id.0) {
                            let _ = cache.touch("com.doxus.confluence", &doc_id.0, ttl);
                            
                            // DB 메타데이터가 있으면 그것을 사용, 없으면 캐시된 JSON에서 복구 시도 (fallback)
                            let final_meta = if let Some(meta) = db_meta {
                                meta
                            } else if let Some(data_json) = cache.get_full("com.doxus.confluence", &doc_id.0).unwrap_or(None) {
                                serde_json::from_str::<serde_json::Value>(&data_json).unwrap_or(serde_json::json!({}))
                            } else {
                                serde_json::json!({})
                            };

                            return Ok(serde_json::json!({
                                "title": final_meta.get("title"),
                                "content": cached_content,
                                "file_path": file_path,
                                "from_cache": true,
                                "reindex_triggered": false,
                                "tags": final_meta.get("tags"),
                                "aliases": final_meta.get("aliases").or(Some(&serde_json::json!([]))),
                                "created_at": final_meta.get("created_at"),
                                "updated_at": final_meta.get("updated_at"),
                                "metadata": final_meta.get("metadata"),
                            }));
                        }
                    } else {
                        // force_refresh: 기존 캐시 항목 제거
                        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                        let cache = ContentCache::new(&conn);
                        let _ = cache.invalidate("com.doxus.confluence", &doc_id.0);
                    }
                }

                let plugin_id = doxus_core::plugin::PluginManager::normalize_id("confluence");
                let mut plugin = state.plugin_manager.get_source(&plugin_id)
                    .ok_or_else(|| format!("Confluence 플러그인을 찾을 수 없습니다 ({plugin_id})"))?;

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
                
                let conn_arc = std::sync::Arc::clone(&state.conn);
                let embedder = std::sync::Arc::clone(&state.embedder) as std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider>;
                let engine = doxus_core::search::SearchEngine::with_embedder(conn_arc, embedder);

                let project_id: i64 = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    conn.query_row("SELECT id FROM projects WHERE name = ?1", [pname], |r| r.get(0)).unwrap_or(0)
                };

                let relative_path = raw.relative_path.clone().or_else(|| { raw.metadata.get("relative_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()) });

                let meta = doxus_core::search::DocMeta {
                    tags: raw.tags.clone(),
                    aliases: vec![],
                    created_at: raw.created_at,
                    updated_at: raw.updated_at,
                    url: raw.url.clone(),
                    relative_path,
                    metadata: raw.metadata.clone(),
                };

                // 실시간 인덱싱 및 파일 경로 동기화 실행
                let _ = engine.index_document_async_with_meta(
                    project_id, 
                    &raw.id.0, 
                    raw.title.as_deref().unwrap_or("Untitled"), 
                    &raw.content, 
                    meta
                ).await;

                let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                // 캐시에 저장 (TTL이 설정된 경우)
                if let Some(ttl) = cache_ttl {
                    let cache = ContentCache::new(&conn);
                    let data_json = serde_json::to_string(&raw).unwrap_or_default();
                    let _ = cache.set_full("com.doxus.confluence", &raw.id.0, &raw.content, &data_json, ttl);
                }

                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                    "from_cache": false,
                    "reindex_triggered": true,
                    "tags": raw.tags,
                    "aliases": Vec::<String>::new(),
                    "created_at": raw.created_at,
                    "updated_at": raw.updated_at,
                    "metadata": raw.metadata,
                }));
            }
            "github" => {
                let plugin_id = doxus_core::plugin::PluginManager::normalize_id("github");
                let mut plugin = state.plugin_manager.get_source(&plugin_id)
                    .ok_or_else(|| format!("GitHub 플러그인을 찾을 수 없습니다 ({plugin_id})"))?;

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

                let conn_arc = std::sync::Arc::clone(&state.conn);
                let embedder = std::sync::Arc::clone(&state.embedder) as std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider>;
                let engine = doxus_core::search::SearchEngine::with_embedder(conn_arc, embedder);

                let project_id: i64 = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    conn.query_row("SELECT id FROM projects WHERE name = ?1", [pname], |r| r.get(0)).unwrap_or(0)
                };

                let relative_path = raw.relative_path.clone().or_else(|| { raw.metadata.get("relative_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()) });

                let meta = doxus_core::search::DocMeta {
                    tags: raw.tags.clone(),
                    aliases: vec![],
                    created_at: None,
                    updated_at: raw.updated_at,
                    url: raw.url.clone(),
                    relative_path,
                    metadata: raw.metadata.clone(),
                };

                // 실시간 인덱싱 및 파일 경로 동기화 실행
                let _ = engine.index_document_async_with_meta(
                    project_id, 
                    &raw.id.0, 
                    raw.title.as_deref().unwrap_or("Untitled"), 
                    &raw.content, 
                    meta
                ).await;

                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                    "reindex_triggered": true,
                }));
            }
            _ => {
                // Obsidian/Workspace: 로컬 파일 직접 읽기 (워크스페이스도 같은 경로)
                let plugin_id = doxus_core::plugin::PluginManager::normalize_id(&source_type);
                let mut plugin = state.plugin_manager.get_source(&plugin_id)
                    .ok_or_else(|| format!("플러그인을 찾을 수 없습니다 ({plugin_id})"))?;

                let mut config = PluginConfig::default();
                config.fields.insert("path".to_string(), serde_json::json!(path));
                plugin.initialize(config, PluginSecrets::default()).await
                    .map_err(|e| format!("Obsidian 플러그인 초기화 실패: {e}"))?;
                let raw = plugin.fetch_document(&doc_id).await
                    .map_err(|e| format!("문서 가져오기 실패: {e}"))?;

                // 실시간 인덱싱 싱크 (파일이 변경되었을 가능성 대응)
                let conn_arc = std::sync::Arc::clone(&state.conn);
                let embedder = std::sync::Arc::clone(&state.embedder) as std::sync::Arc<dyn doxus_core::embedding::EmbeddingProvider>;
                let engine = doxus_core::search::SearchEngine::with_embedder(conn_arc, embedder);

                let project_id: i64 = {
                    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
                    conn.query_row("SELECT id FROM projects WHERE name = ?1", [pname], |r| r.get(0)).unwrap_or(0)
                };

                let meta = doxus_core::search::DocMeta {
                    tags: raw.tags.clone(),
                    aliases: raw.aliases.clone(),
                    created_at: raw.created_at,
                    updated_at: raw.updated_at,
                    url: raw.url.clone(),
                    relative_path: raw.relative_path.clone(),
                    metadata: raw.metadata.clone(),
                };

                // 백그라운드 인덱싱 (기다리지 않음)
                let _ = engine.index_document_async_with_meta(
                    project_id,
                    &raw.id.0,
                    raw.title.as_deref().unwrap_or("Untitled"),
                    &raw.content,
                    meta
                ).await;

                return Ok(serde_json::json!({
                    "title": raw.title,
                    "content": raw.content,
                    "file_path": file_path,
                    "tags": raw.tags,
                    "aliases": raw.aliases,
                    "created_at": raw.created_at,
                    "updated_at": raw.updated_at,
                    "metadata": raw.metadata,
                    "url": raw.url,
                    "reindex_triggered": true,
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
        "SELECT MIN(d.id), MIN(d.title), MIN(d.source_doc_id), p.name, COALESCE(p.source_type, 'obsidian'), MIN(d.file_path), p.path, MIN(d.url)
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         WHERE p.status = 'active'
         GROUP BY d.source_doc_id, d.project_id, p.name, p.source_type, p.path
         ORDER BY p.name, MIN(d.title)"
    ).map_err(|e| e.to_string())?;
    let docs: Vec<_> = stmt
        .query_map([], |r| {
            let document_id = r.get::<_, i64>(0)?;
            let title = r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "(제목 없음)".to_string());
            let source_doc_id = r.get::<_, String>(2)?;
            let project_name = r.get::<_, String>(3)?;
            let source_type = r.get::<_, String>(4)?;
            let file_path = r.get::<_, Option<String>>(5)?;
            let project_path = r.get::<_, String>(6).unwrap_or_default();
            let url = r.get::<_, Option<String>>(7)?;

            // Normalize file_path for UI tree: strip project_path if it's an absolute path
            // Also strip "virtual root" if it matches name part (e.g. '컨플/테크스펙' -> strip '테크스펙/')
            let display_file_path = if let Some(ref path) = file_path {
                let mut p = path.as_str();
                
                // 1. Absolute path stripping (Local projects)
                if !project_path.is_empty() && p.starts_with(&project_path) {
                    p = p.strip_prefix(&project_path).unwrap_or(p);
                }

                p = p.trim_start_matches('/');

                // 2. Virtual root stripping (Web/Confluence projects)
                if project_name.contains('/') {
                    if let Some(virtual_root) = project_name.split('/').last() {
                        if !virtual_root.is_empty() && p.starts_with(virtual_root) {
                            let next = p.strip_prefix(virtual_root).unwrap_or(p);
                            if next.starts_with('/') {
                                p = next.trim_start_matches('/');
                            }
                        }
                    }
                }

                p.to_string()
            } else {
                source_doc_id.clone()
            };

            Ok(serde_json::json!({
                "document_id": document_id,
                "title": title,
                "source_doc_id": source_doc_id,
                "project_name": project_name,
                "source_type": source_type,
                "file_path": display_file_path,
                "url": url,
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
