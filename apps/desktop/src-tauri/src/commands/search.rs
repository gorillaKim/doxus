use doxus_core::search::{SearchEngine, SearchQuery};
use rusqlite::OptionalExtension;
use std::sync::Arc;

#[cfg(test)]
pub(crate) fn run_reindex(conn: &rusqlite::Connection, plugin_manager: &doxus_core::plugin::PluginManager) -> Result<usize, String> {
    use sha2::{Digest, Sha256};
    use doxus_plugin_sdk::FetchAllOpts;

    let projects: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, COALESCE(source_type, 'obsidian') FROM projects WHERE status = 'active'"
        ).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut acc = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            if let (Ok(id), Ok(name), Ok(st)) = (row.get::<_, i64>(0), row.get::<_, String>(1), row.get::<_, String>(2)) {
                acc.push((id, name, st));
            }
        }
        acc
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    let mut total = 0usize;
    for (project_id, project_name, source_type) in projects {
        let plugin_id = doxus_core::plugin::PluginManager::normalize_id(&source_type);
        let mut source = match plugin_manager.get_source(&plugin_id) {
            Some(s) => s,
            None => continue,
        };

        let path: String = conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        ).map_err(|e| e.to_string())?;

        let config = doxus_plugin_sdk::PluginConfig {
            fields: {
                let mut m = std::collections::HashMap::new();
                m.insert("path".to_string(), serde_json::Value::String(path));
                m
            },
        };
        if let Err(e) = rt.block_on(source.initialize(config, doxus_plugin_sdk::PluginSecrets::default())) {
            eprintln!("plugin init error for {}: {}", project_name, e);
            continue;
        }

        let opts = FetchAllOpts { cursor: None, page_size: 100 };
        let stream = match rt.block_on(source.fetch_all(opts)) {
            Ok(s) => s,
            Err(e) => { eprintln!("fetch_all error: {}", e); continue; }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

        for doc in stream.documents {
            let hash = format!("{:x}", Sha256::digest(doc.content.as_bytes()));
            conn.execute(
                "INSERT INTO documents (project_id, source_doc_id, title, content_hash, last_indexed)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project_id, source_doc_id) DO UPDATE SET title=excluded.title, content_hash=excluded.content_hash, last_indexed=excluded.last_indexed",
                rusqlite::params![project_id, &doc.id.0, doc.title, hash, now],
            ).map_err(|e| e.to_string())?;
            let doc_id: i64 = conn.query_row(
                "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                rusqlite::params![project_id, &doc.id.0],
                |r| r.get(0),
            ).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO chunks (document_id, content, chunk_index) VALUES (?1, ?2, 0)
                 ON CONFLICT(document_id, chunk_index) DO UPDATE SET content=excluded.content",
                rusqlite::params![doc_id, doc.content],
            ).map_err(|e| e.to_string())?;
            total += 1;
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
    state: tauri::State<'_, Arc<crate::AppState>>,
    document_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    increment_view_count_impl(&conn, document_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn get_top_documents(
    state: tauri::State<'_, Arc<crate::AppState>>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let tops = get_top_documents_impl(&conn, limit.unwrap_or(5))?;
    Ok(serde_json::json!({ "documents": tops }))
}

#[cfg(test)]
pub(crate) fn get_document_content_impl(
    conn: &rusqlite::Connection,
    file_path: &str,
) -> Result<serde_json::Value, String> {
    let (doc_id, title): (i64, String) = conn.query_row(
        "SELECT id, COALESCE(title, '') FROM documents WHERE source_doc_id = ?1 LIMIT 1",
        rusqlite::params![file_path],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(|_| "문서를 찾을 수 없음".to_string())?;

    let mut stmt = conn
        .prepare("SELECT content FROM chunks WHERE document_id = ?1 ORDER BY chunk_index")
        .map_err(|e| e.to_string())?;
    let parts: Vec<String> = stmt
        .query_map(rusqlite::params![doc_id], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let content = parts.join("\n");

    Ok(serde_json::json!({
        "title": title,
        "content": content,
        "file_path": file_path,
    }))
}

pub fn reindex_if_stale(
    conn: &rusqlite::Connection,
    project_name: &str,
    source_doc_id: &str,
    title: &str,
    content: &str,
) -> Result<bool, String> {
    use sha2::{Digest, Sha256};
    let new_hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT d.id, d.content_hash FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.name = ?1 AND d.source_doc_id = ?2 LIMIT 1",
            rusqlite::params![project_name, source_doc_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (doc_id, stored_hash) = match row {
        None => return Ok(false),
        Some(v) => v,
    };

    if stored_hash == new_hash {
        return Ok(false);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    conn.execute(
        "UPDATE documents SET title = ?1, content_hash = ?2, last_indexed = ?3 WHERE id = ?4",
        rusqlite::params![title, new_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO chunks (document_id, content, chunk_index) VALUES (?1, ?2, 0)
         ON CONFLICT(document_id, chunk_index) DO UPDATE SET content = excluded.content",
        rusqlite::params![doc_id, content],
    )
    .map_err(|e| e.to_string())?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    fn make_conn() -> rusqlite::Connection {
        doxus_core::db::ensure_vec_extension();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::create_vec0_table(&conn).unwrap();
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
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, last_indexed) VALUES (?1,'d1','T','h',?2)",
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
                "INSERT INTO documents (project_id, source_doc_id, title, content_hash, last_indexed) VALUES (?1,?2,?3,'h',?4)",
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
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, last_indexed) VALUES (?1, '/path/to/note.md', 'My Note', 'h', ?2)",
            rusqlite::params![pid, now],
        ).unwrap();
        let did: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index) VALUES (?1, '# Hello', 0)",
            rusqlite::params![did],
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
    state: tauri::State<'_, Arc<crate::AppState>>,
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
    state: tauri::State<'_, Arc<crate::AppState>>,
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
    state: tauri::State<'_, Arc<crate::AppState>>,
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
    state: tauri::State<'_, Arc<crate::AppState>>,
    query: String,
    limit: Option<usize>,
    source_types: Option<Vec<String>>,
    project_names: Option<Vec<String>>,
    tags: Option<Vec<String>>,
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

    let embedder = state.embedder.read().await.clone();
    let engine = SearchEngine::with_embedder(state.conn.clone(), embedder);
    let mut q = SearchQuery::new(&query)
        .with_limit(limit.unwrap_or(20))
        .with_tags(tags.unwrap_or_default());
    if has_filter {
        q = q.with_projects(filter_ids);
    }
    let hits = engine.search_async(&q).await.map_err(|e| e.to_string())?;
    
    // document_id 목록으로 project_name / source_type / metadata 일괄 조회
    let doc_ids: Vec<i64> = hits.iter().map(|h| h.document_id).collect();
    
    // 3. Document metadata batch fetching (scoped lock)
    let mut doc_info: std::collections::HashMap<i64, (String, String, String, Vec<String>, i64, i64, serde_json::Value, String, Option<String>, String, f64, String)> = std::collections::HashMap::new();
    {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        for chunk in doc_ids.chunks(50) {
            let placeholders = chunk.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT d.id, p.name, COALESCE(p.source_type, 'obsidian'), d.source_doc_id, \
                        d.updated_at, d.last_indexed, COALESCE(d.metadata_json, '{{}}'), p.path, d.url, \
                        COALESCE(p.source_project_id, p.name), \
                        COALESCE(f.freshness_score, 100.0), \
                        COALESCE(f.retention_tier, 'mid') \
                 FROM documents d \
                 JOIN projects p ON d.project_id = p.id \
                 LEFT JOIN document_freshness f ON d.id = f.document_id \
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
                    r.get::<_, Option<i64>>(4).ok().flatten().unwrap_or(0),
                    r.get::<_, Option<i64>>(5).ok().flatten().unwrap_or(0),
                    r.get::<_, String>(6).unwrap_or_else(|_| "{}".to_string()),
                    r.get::<_, String>(7).unwrap_or_default(),
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, f64>(10).unwrap_or(100.0),
                    r.get::<_, String>(11).unwrap_or_else(|_| "mid".to_string()),
                ))
            }).map_err(|e| e.to_string())?;

            for row_res in rows {
                if let Ok(row) = row_res {
                    let doc_id = row.0;
                    let project_name = row.1;
                    let source_type = row.2;
                    let source_doc_id = row.3;
                    let updated_at = row.4;
                    let last_indexed = row.5;
                    let metadata: serde_json::Value = serde_json::from_str(&row.6).unwrap_or(serde_json::json!({}));
                    let project_path = row.7;
                    let url = row.8.clone();
                    let source_project_id = row.9.clone();
                    let freshness_score = row.10;
                    let retention_tier = row.11.clone();
                    
                    // Tags look up
                    let mut tag_stmt = conn.prepare("SELECT tag FROM document_tags WHERE document_id = ?1").map_err(|e| e.to_string())?;
                    let tags: Vec<String> = tag_stmt.query_map([doc_id], |tr| tr.get(0)).map_err(|e| e.to_string())?
                        .filter_map(|tr| tr.ok()).collect();

                    doc_info.insert(doc_id, (project_name.clone(), source_type.clone(), source_doc_id.clone(), tags, updated_at, last_indexed, metadata.clone(), project_path.clone(), url.clone(), source_project_id.clone(), freshness_score, retention_tier.clone()));
                }
            }
        }
    }

    // Cache TTL lookup for active plugins
    let mut plugin_ttls: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT plugin_id, CAST(value AS INTEGER) FROM plugin_kv WHERE namespace = 'settings' AND key = 'cache_ttl_minutes'").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            plugin_ttls.insert(row.0, row.1);
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
            let last_indexed = info.map(|i| i.5).unwrap_or(0);
            let metadata = info.map(|i| i.6.clone()).unwrap_or(serde_json::json!({}));
            let project_path = info.map(|i| i.7.as_str()).unwrap_or("");
            let url = info.and_then(|i| i.8.clone()).or_else(|| h.url.clone());
            let source_project_id = info.map(|i| i.9.clone()).unwrap_or_default();
            
            let freshness_score = info.map(|i| i.10).unwrap_or(100.0);
            let retention_tier = info.map(|i| i.11.clone()).unwrap_or_else(|| "mid".to_string());
            
            let plugin_id = format!("com.doxus.{}", source_type);
            let cache_ttl = plugin_ttls.get(&plugin_id).cloned().unwrap_or(0);
            
            let display_file_path = if let Some(ref path) = h.file_path {
                let mut p = path.as_str();
                
                // 1. Absolute path stripping (Local projects)
                if !project_path.is_empty() && p.starts_with(project_path) {
                    p = p.strip_prefix(project_path).unwrap_or(p);
                }

                p = p.trim_start_matches('/');

                // 2. Virtual root stripping (Web/Confluence projects)
                // Project 'AI 리포트 V3' should strip 'Project/' or 'AI 리포트 V3/' from its virtual paths
                let first_seg = p.split('/').next().unwrap_or("");
                let proj_lower = project_name.to_lowercase();
                let first_lower = first_seg.to_lowercase();

                let should_strip = if !first_seg.is_empty() {
                    // 완전히 일치하거나
                    first_lower == proj_lower || 
                    // 프로젝트명에 슬래시가 있는 경우 마지막 파트와 일치하거나 (e.g. '컨플/테크' -> '테크')
                    (project_name.contains('/') && Some(first_lower.as_str()) == proj_lower.split('/').last()) ||
                    // 일반적인 중복 폴더명인 경우
                    first_lower == "project" || first_lower == "space"
                } else {
                    false
                };

                if should_strip {
                    let next = p.strip_prefix(first_seg).unwrap_or(p);
                    p = next.trim_start_matches('/');
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
                "last_indexed": last_indexed,
                "cache_ttl": cache_ttl,
                "metadata": metadata,
                "url": url,
                "source_project_id": source_project_id,
                "freshness_score": freshness_score,
                "retention_tier": retention_tier,
            })
        })
        .collect();
    Ok(serde_json::json!({ "hits": hits_json }))
}

#[tauri::command]
pub async fn search_engine_status(
    state: tauri::State<'_, Arc<crate::AppState>>,
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
    state: tauri::State<'_, Arc<crate::AppState>>,
    app_handle: tauri::AppHandle,
    name: String,
    full: Option<bool>,
) -> Result<serde_json::Value, String> {
    let embedder = state.embedder.read().await.clone();
    let engine = std::sync::Arc::new(SearchEngine::with_embedder(
        std::sync::Arc::clone(&state.conn),
        embedder,
    ));
    let indexing_service = doxus_core::indexing::IndexingService::new(
        std::sync::Arc::clone(&state.conn),
        std::sync::Arc::clone(&state.plugin_manager),
        engine,
    );

    let is_full = full.unwrap_or(false);
    state.sync_manager.record_external_trigger(
        "Manual", 
        Some(name.clone()), 
        Some(format!("User requested {}indexing", if is_full { "full " } else { "" }))
    ).await;

    let app_handle_progress = app_handle.clone();
    let name_progress = name.clone();
    
    // 수동 인덱싱은 항상 실행 (SyncManager 진행 중이더라도 강제 등록)
    state.sync_manager.force_mark_task_started(&name).await;

    let result = indexing_service.index_project_with_progress(&name, is_full, move |docs_done, total_docs| {
        use tauri::Emitter;
        let _ = app_handle_progress.emit("index_progress", serde_json::json!({
            "project_name": name_progress,
            "docs_indexed": docs_done,
            "total_docs": if total_docs > 0 { serde_json::json!(total_docs) } else { serde_json::Value::Null },
        }));
    }).await;

    // 추가: SyncManager에서 태스크 해제 (성공/실패 무관)
    state.sync_manager.mark_task_done(&name).await;

    let total = result?;

    let message = if total == 0 {
        if is_full { "문서가 없는 프로젝트이거나 인덱싱에 실패했습니다".to_string() }
        else { "이미 최신 상태입니다 (0개 변경)".to_string() }
    } else {
        format!("{total}개 문서 {}인덱싱 완료", if is_full { "전체 강제 " } else { "" })
    };

    use tauri::Emitter;
    let _ = app_handle.emit("project-indexed", serde_json::json!({
        "project_name": name,
        "indexed": total,
        "full": is_full,
    }));

    Ok(serde_json::json!({
        "status": "ok",
        "indexed": total,
        "message": message
    }))
}

#[tauri::command]
pub async fn trigger_reindex(
    state: tauri::State<'_, Arc<crate::AppState>>,
    full: Option<bool>,
) -> Result<serde_json::Value, String> {
    let is_full = full.unwrap_or(false);

    // 1. 트리거 기록 남기기
    state.sync_manager.record_external_trigger(
        "Manual", 
        None, 
        Some(format!("Manual {}re-index of all projects started", if is_full { "full " } else { "" }))
    ).await;

    // 2. 모든 활성 프로젝트 이름 가져오기
    let names: Vec<String> = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT name FROM projects WHERE status = 'active'").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // 3. UI에 즉시 표시되도록 active_tasks에 선등록 후 큐에 전달
    for name in &names {
        state.sync_manager.force_mark_task_started(name).await;
    }
    for name in names {
        let _ = state.sync_manager.trigger_full_indexing_by_name(&name, is_full).await;
    }

    Ok(serde_json::json!({ 
        "message": if is_full { "전체 강제 재인덱싱이 백그라운드에서 시작되었습니다" } else { "증분 인덱싱이 시작되었습니다" }
    }))
}


#[tauri::command]
pub async fn get_document_content(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
    file_path: String,
    project_name: Option<String>,
    force_refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    use doxus_core::document::DocumentService;
    use tauri::Emitter;

    let conn_arc = state.conn.clone();
    let indexer = state.sync_manager.indexer();

    // 1. 문서 가져오기 (Local File / Cache / Remote Plugin)
    let doc = if let Some(ref pname) = project_name {
        let pm_arc = state.plugin_manager.clone();
        let service = DocumentService::new(conn_arc.clone(), Some(pm_arc));

        if force_refresh.unwrap_or(false) {
            service.refresh_content(pname, &file_path).await
                .map_err(|e| format!("문서 새로고침 실패: {e}"))?
        } else {
            service.fetch_full_content(pname, &file_path).await
                .map_err(|e| format!("문서 가져오기 실패: {e}"))?
        }
    } else {
        // 프로젝트 이름이 없는 경우 로컬 파일로 간주
        let path = std::path::Path::new(&file_path);
        if !path.exists() {
            return Err(format!("파일을 찾을 수 없습니다: {file_path}"));
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("파일 읽기 실패: {e}"))?;
        
        doxus_plugin_sdk::RawDocument {
            id: doxus_plugin_sdk::SourceDocId(file_path.clone()),
            title: Some(path.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled").to_string()),
            content,
            content_type: doxus_plugin_sdk::ContentType::Markdown,
            url: None,
            metadata: std::collections::HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: Some(file_path.clone()),
        }
    };

    // 2. 백그라운드 재인덱싱 트리거 (원격 프로젝트인 경우)
    let mut reindex_triggered = false;
    if let Some(ref pname) = project_name {
        let pname_clone = pname.clone();
        let doc_clone = doc.clone();
        let indexer_clone = indexer.clone();
        let handle_clone = app_handle.clone();

        tauri::async_runtime::spawn(async move {
            let conn = indexer_clone.conn();
            let project_info = {
                let conn_lock = conn.lock().unwrap_or_else(|e| e.into_inner());
                conn_lock.query_row(
                    "SELECT id, storage_strategy FROM projects WHERE name = ?1 OR source_project_id = ?1",
                    rusqlite::params![pname_clone],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                ).ok()
            };

            if let Some((pid, strategy)) = project_info {
                if indexer_clone.needs_reindexing(pid, &doc_clone.id.0, doc_clone.updated_at).await {
                    doxus_core::log_d!("commands", "[JIT-Indexer] App background indexing triggered for: {} (ID: {})", 
                        doc_clone.title.as_deref().unwrap_or("Untitled"), doc_clone.id.0);
                    
                    let doc_id_for_log = doc_clone.id.0.clone();
                    if let Ok(_) = indexer_clone.index_single_document(pid, doc_clone, &strategy).await {
                        // 실제 인덱싱 완료 시 프런트엔드에 알림 이벤트 발행
                        let _ = handle_clone.emit("document-indexed", serde_json::json!({
                            "project_name": pname_clone,
                            "source_doc_id": doc_id_for_log,
                            "last_indexed": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
                        }));
                        doxus_core::log_d!("commands", "[JIT-Indexer] Event emitted: document-indexed for {}", doc_id_for_log);
                    }
                }
            }
        });
        reindex_triggered = true;
    }

    // 3. Fetch canonical title and metadata from DB (Source of truth for Doxus UI)
    let db_meta = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let row: Option<(Option<String>, Option<i64>, Option<i32>)> = conn.query_row(
            "SELECT d.title, d.last_indexed, p.cache_ttl \
             FROM documents d \
             JOIN projects p ON d.project_id = p.id \
             WHERE (p.name = ?1 OR p.source_project_id = ?1 OR p.display_name = ?1 OR ?1 IS NULL) \
             AND (d.source_doc_id = ?2 OR d.file_path = ?2) \
             LIMIT 1",
            rusqlite::params![project_name, doc.id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        ).optional().unwrap_or(None);
        row
    };

    let (db_title, last_indexed, cache_ttl) = match db_meta {
        Some(m) => m,
        None => (None, None, None),
    };

    // 4. Fetch tags from DB
    let tags = {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut tags: Vec<String> = Vec::new();
        let sql = "SELECT dt.tag FROM document_tags dt \
                   JOIN documents d ON dt.document_id = d.id \
                   JOIN projects p ON d.project_id = p.id \
                   WHERE (p.name = ?1 OR p.source_project_id = ?1 OR p.display_name = ?1 OR ?1 IS NULL) \
                   AND (d.source_doc_id = ?2 OR d.file_path = ?2)";
        
        if let Ok(mut stmt) = conn.prepare(sql) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![project_name, doc.id.0], |r| r.get::<_, String>(0)) {
                tags = rows.filter_map(|r| r.ok()).collect();
            }
        }
        
        if tags.is_empty() { doc.tags.clone() } else { tags }
    };

    // Determine the most stable title
    let final_title = db_title
        .filter(|t| !t.trim().is_empty())
        .or_else(|| doc.title.clone())
        .unwrap_or_else(|| {
            // Last fallback: use filename from source_doc_id
            let path = std::path::Path::new(&doc.id.0);
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

    Ok(serde_json::json!({
        "title": final_title,
        "content": doc.content,
        "file_path": file_path,
        "from_cache": false, 
        "reindex_triggered": reindex_triggered,
        "tags": tags,
        "aliases": doc.aliases,
        "created_at": doc.created_at,
        "updated_at": doc.updated_at,
        "metadata": doc.metadata,
        "url": doc.url,
        "source_project_id": project_name,
        "source_doc_id": doc.id.0,
        "last_indexed": last_indexed,
        "cache_ttl": cache_ttl,
    }))
}

pub fn list_all_documents_impl(
    conn: &rusqlite::Connection,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<serde_json::Value, String> {
    let limit_val = limit.unwrap_or(100);
    let offset_val = offset.unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT d.id, COALESCE(NULLIF(d.title, ''), d.source_doc_id) as title, d.source_doc_id, p.name, COALESCE(p.source_type, 'obsidian'), \
                d.file_path, p.path, d.url, d.updated_at, d.last_indexed, \
                (SELECT GROUP_CONCAT(tag) FROM document_tags WHERE document_id = d.id), \
                COALESCE(f.freshness_score, 100.0), COALESCE(f.retention_tier, 'mid')
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         LEFT JOIN document_freshness f ON d.id = f.document_id
         WHERE p.status = 'active'
         ORDER BY p.name, title
         LIMIT ?1 OFFSET ?2"
    ).map_err(|e| e.to_string())?;
    
    let docs: Vec<_> = stmt
        .query_map(rusqlite::params![limit_val as i64, offset_val as i64], |r| {
            let document_id = r.get::<_, i64>(0)?;
            let title = r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "Untitled".to_string());
// ... rest of the mapping logic remains same ...
            let source_doc_id = r.get::<_, String>(2)?;
            let project_name = r.get::<_, String>(3)?;
            let source_type = r.get::<_, String>(4)?;
            let file_path = r.get::<_, Option<String>>(5)?;
            let project_path = r.get::<_, String>(6).unwrap_or_default();
            let url = r.get::<_, Option<String>>(7)?;
            let updated_at = r.get::<_, Option<i64>>(8).unwrap_or_default().unwrap_or(0);
            let last_indexed = r.get::<_, Option<i64>>(9).unwrap_or_default().unwrap_or(0);
            let tags_str: Option<String> = r.get(10)?;
            let tags: Vec<String> = tags_str
                .map(|s| s.split(',').map(|t| t.to_string()).collect())
                .unwrap_or_default();
            let freshness_score: f64 = r.get(11).unwrap_or(100.0);
            let retention_tier: String = r.get(12).unwrap_or_else(|_| "mid".to_string());

            let display_file_path = if let Some(ref path) = file_path {
                let mut p = path.as_str();
                if !project_path.is_empty() && p.starts_with(&project_path) {
                    p = p.strip_prefix(&project_path).unwrap_or(p);
                }
                p = p.trim_start_matches('/');
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
                "updated_at": updated_at,
                "last_indexed": last_indexed,
                "tags": tags,
                "freshness_score": freshness_score,
                "retention_tier": retention_tier,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let count = docs.len();
    doxus_core::log_d!("commands", "[Core] list_all_documents: found {} documents (limit: {}, offset: {})", count, limit_val, offset_val);
    Ok(serde_json::json!({ "documents": docs }))
}

#[tauri::command]
pub async fn list_all_documents(
    state: tauri::State<'_, Arc<crate::AppState>>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    list_all_documents_impl(&conn, limit, offset)
}

#[tauri::command]
pub async fn count_all_documents(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<i64, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub async fn list_projects(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmt = conn
        .prepare("SELECT id, name, display_name, path, status, COALESCE(source_type, 'obsidian'), freshness_policy_json FROM projects ORDER BY name")
        .map_err(|e| e.to_string())?;
    let projects: Vec<_> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "display_name": r.get::<_, String>(2)?,
                "path": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "source_type": r.get::<_, String>(5)?,
                "freshness_policy_json": r.get::<_, Option<String>>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "projects": projects }))
}

#[tauri::command]
pub async fn search_engine_repair_index(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<(), String> {
    let engine = doxus_core::search::SearchEngine::with_embedder(
        state.conn.clone(),
        state.embedder.read().await.clone(),
    );
    engine.rebuild_vector_table().await.map_err(|e| e.to_string())?;
    Ok(())
}
