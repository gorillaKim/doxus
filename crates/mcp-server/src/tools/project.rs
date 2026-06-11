use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::Value;

pub fn list_projects(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let mut stmt = match conn_lock
        .prepare("SELECT name, display_name, status, path FROM projects ORDER BY name")
    {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map([], |r: &rusqlite::Row<'_>| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) if rows.is_empty() => {
            McpResponse::text(id, "No projects found. Add one with doxus_add_project.")
        }
        Ok(rows) => {
            let mut lines =
                vec!["NAME                 DISPLAY              STATUS    PATH".to_string()];
            lines.push("-".repeat(80));
            for (name, display, status, path) in &rows {
                lines.push(format!("{name:<20} {display:<20} {status:<9} {path}"));
            }
            McpResponse::text(id, lines.join("\n"))
        }
    }
}

pub fn add_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let name = match args["name"].as_str() {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: name"),
    };
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: path"),
    };
    let display_name = args["display_name"].as_str().unwrap_or(name);
    let source_type = args["source_type"].as_str().unwrap_or("obsidian");
    let config_json = args["config"]
        .as_object()
        .map(|m| serde_json::Value::Object(m.clone()).to_string())
        .unwrap_or_else(|| "{}".to_string());

    let plugin_id = match source_type {
        "obsidian" | "confluence" | "github" => format!("com.doxus.{source_type}"),
        other => other.to_string(),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    // projects + source_instances 동시 INSERT (atomic)
    let result: Result<(), rusqlite::Error> = (|| {
        // plugins FK 충족: 해당 plugin_id 행이 없으면 upsert
        conn_lock.execute(
            "INSERT OR IGNORE INTO plugins(id, name, version, kind, installed_at)
             VALUES (?1, ?1, '0.0.0', 'builtin', unixepoch())",
            params![plugin_id],
        )?;
        conn_lock.execute(
            "INSERT INTO projects(name, display_name, path, source_type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch(), unixepoch())",
            params![name, display_name, path, source_type],
        )?;
        let project_id = conn_lock.last_insert_rowid();
        conn_lock.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())",
            params![plugin_id, project_id, name, config_json],
        )?;
        Ok(())
    })();

    match result {
        Ok(_) => McpResponse::text(
            id,
            format!("Project '{name}' added (source_type: {source_type}). Run doxus_index_project to index it."),
        ),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn remove_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let name = match args["name"].as_str() {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: name"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let pid: Result<i64, _> = conn_lock.query_row(
        "SELECT id FROM projects WHERE name=?1",
        params![name],
        |r: &rusqlite::Row<'_>| r.get(0),
    );

    match pid {
        Err(_) => McpResponse::err(id, -32602, format!("project '{name}' not found")),
        Ok(pid) => {
            let _ = conn_lock.execute("DELETE FROM source_instances WHERE project_id=?1", [pid]);
            match conn_lock.execute("DELETE FROM projects WHERE id=?1", [pid]) {
                Ok(_) => McpResponse::text(
                    id,
                    format!(
                        "Project '{name}' removed (index data deleted, original files untouched)."
                    ),
                ),
                Err(e) => McpResponse::err(id, -32603, e.to_string()),
            }
        }
    }
}

fn get_project_id(conn: &rusqlite::Connection, name: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM projects WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )
}

fn insert_reindex_history(
    conn: &rusqlite::Connection,
    project_id: i64,
    scope: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO reindex_history(project_id, scope, status, total_docs, processed_docs, started_at)
         VALUES (?1, ?2, 'running', 0, 0, unixepoch())",
        params![project_id, scope],
    )?;
    Ok(conn.last_insert_rowid())
}

fn update_reindex_history(
    conn: &rusqlite::Connection,
    history_id: i64,
    status: &str,
    total: i64,
    processed: i64,
    error_msg: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE reindex_history
         SET status = ?1, total_docs = ?2, processed_docs = ?3, error_message = ?4, completed_at = unixepoch()
         WHERE id = ?5",
        params![status, total, processed, error_msg, history_id],
    )?;
    Ok(())
}

pub fn index_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let name = match args["project"].as_str().or_else(|| args["name"].as_str()) {
        Some(n) => n.to_string(),
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let full = args["full"].as_bool().unwrap_or(false);

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let project_id = match get_project_id(&conn_lock, &name) {
        Ok(pid) => pid,
        Err(_) => return McpResponse::err(id, -32602, format!("project '{}' not found", name)),
    };

    let history_id = match insert_reindex_history(
        &conn_lock,
        project_id,
        if full { "full" } else { "incremental" },
    ) {
        Ok(hid) => hid,
        Err(e) => return McpResponse::err(id, -32603, format!("failed to insert history: {e}")),
    };

    let indexing_service = server.indexer();
    let conn_clone = conn.clone();
    let name_clone = name.clone();

    tokio::spawn(async move {
        let result = indexing_service.index_project(&name_clone, full).await;
        if let Ok(c) = conn_clone.get() {
            match result {
                Ok(indexed) => {
                    let _ = update_reindex_history(
                        &c,
                        history_id,
                        "completed",
                        indexed as i64,
                        indexed as i64,
                        None,
                    );
                }
                Err(e) => {
                    let _ = update_reindex_history(
                        &c,
                        history_id,
                        "failed",
                        0,
                        0,
                        Some(&e.to_string()),
                    );
                }
            }
        }
    });

    McpResponse::text(
        id,
        format!("Indexing for project '{name}' has been started in the background. History ID: {history_id}. Use doxus_reindex_status to check status."),
    )
}

pub fn sync_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::{DocMeta, SyncSearchEngine};
    use doxus_plugin_sdk::{DocSource, FetchChangesOpts, PluginConfig, PluginSecrets, SourceDocId};
    use std::collections::HashMap;
    use std::sync::Arc;

    let name = match args["project"].as_str() {
        Some(n) => n.to_string(),
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let project_id = match get_project_id(&conn_lock, &name) {
        Ok(pid) => pid,
        Err(_) => return McpResponse::err(id, -32602, format!("project '{}' not found", name)),
    };

    let history_id = match insert_reindex_history(&conn_lock, project_id, "sync") {
        Ok(hid) => hid,
        Err(e) => return McpResponse::err(id, -32603, format!("failed to insert history: {e}")),
    };

    let conn_clone = conn.clone();
    let name_clone = name.clone();
    let plugin_manager_clone = Arc::clone(server.plugin_manager());

    tokio::spawn(async move {
        let result = async {
            let conn_lock = conn_clone.get().map_err(|e| format!("db pool error: {e}"))?;

            type SourceRow = (i64, String, Option<String>, Option<i64>, String);
            let row: Result<SourceRow, _> = conn_lock.query_row(
                "SELECT si.id, si.plugin_id, si.sync_cursor, si.last_synced, si.config_json
                 FROM source_instances si
                 JOIN projects p ON si.project_id = p.id
                 WHERE p.name = ?1
                 ORDER BY si.id LIMIT 1",
                params![name_clone],
                |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            );

            let (si_id, plugin_id, sync_cursor, last_synced, config_json) = match row {
                Err(_) => {
                    return Err(format!("Project '{name_clone}' has no source instance configured"))
                }
                Ok(r) => r,
            };

            let project_id: i64 = conn_lock.query_row(
                "SELECT p.id FROM projects p
                 JOIN source_instances si ON si.project_id = p.id
                 WHERE si.id = ?1",
                params![si_id],
                |r: &rusqlite::Row<'_>| r.get(0),
            ).map_err(|e| format!("project lookup: {e}"))?;

            let known_ids: Vec<SourceDocId> = {
                let mut stmt = conn_lock.prepare("SELECT source_doc_id FROM documents WHERE project_id = ?1")
                    .map_err(|e| format!("prepare known_ids: {e}"))?;
                let ids: Result<Vec<String>, _> = stmt
                    .query_map(params![project_id], |r: &rusqlite::Row<'_>| {
                        r.get::<_, String>(0)
                    })
                    .map_err(|e| format!("query known_ids: {e}"))
                    .and_then(|rows| {
                        rows.collect::<Result<Vec<_>, _>>()
                            .map_err(|e| e.to_string())
                    });
                ids.map_err(|e| e.to_string())?.into_iter().map(SourceDocId).collect()
            };

            let since = last_synced.unwrap_or(0);
            let cursor = sync_cursor;

            let mut plugin = plugin_manager_clone
                .get_source(&plugin_id)
                .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;

            let fields: HashMap<String, serde_json::Value> =
                serde_json::from_str(&config_json).unwrap_or_default();
            let mut config = PluginConfig { fields };
            let mut secrets = PluginSecrets {
                fields: HashMap::new(),
            };

            // Inject keychain auth (async)
            doxus_core::auth::inject_keychain_auth(&plugin_id, &mut config, &mut secrets).await;

            plugin.initialize(config, secrets).await
                .map_err(|e| format!("plugin init error: {e}"))?;

            let changeset = plugin
                .fetch_changes(FetchChangesOpts {
                    since,
                    cursor,
                    page_size: 1000,
                    known_ids,
                })
                .await
                .map_err(|e| format!("fetch_changes error: {e}"))?;

            let n_updated = changeset.updated.len();
            let n_deleted = changeset.deleted_ids.len();

            let mut conn_mut = conn_clone.get().map_err(|e| format!("db pool error: {e}"))?;
            let tx = conn_mut.transaction().map_err(|e| format!("tx begin: {e}"))?;

            let db_tx_result: Result<(), String> = (|| {
                let strategy: String = tx
                    .query_row(
                        "SELECT storage_strategy FROM projects WHERE id = ?1",
                        [project_id],
                        |r: &rusqlite::Row<'_>| r.get(0),
                    )
                    .unwrap_or_else(|_| "full".to_string());

                let engine = SyncSearchEngine::from_conn(&tx);

                for doc in &changeset.updated {
                    let title = doc.title.as_deref().unwrap_or("");
                    let meta = DocMeta {
                        url: doc.url.clone(),
                        tags: doc.tags.clone(),
                        metadata: doc.metadata.clone(),
                        created_at: doc.created_at,
                        updated_at: doc.updated_at,
                        relative_path: doc.relative_path.clone(),
                        ..Default::default()
                    };
                    engine.index_document_with_meta(
                        project_id,
                        &doc.id.0,
                        title,
                        &doc.content,
                        &meta,
                        &strategy,
                    ).map_err(|e| format!("index error for '{}': {e}", doc.id.0))?;
                }

                for del_id in &changeset.deleted_ids {
                    tx.execute(
                        "DELETE FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                        params![project_id, &del_id.0],
                    ).map_err(|e| format!("delete error for '{}': {e}", del_id.0))?;
                }

                let new_cursor: Option<&str> = changeset.next_cursor.as_deref();
                tx.execute(
                    "UPDATE source_instances SET sync_cursor = ?1, last_synced = unixepoch() WHERE id = ?2",
                    params![new_cursor, si_id],
                ).map_err(|e| format!("update cursor: {e}"))?;

                Ok(())
            })();

            match db_tx_result {
                Ok(_) => {
                    tx.commit().map_err(|e| format!("commit: {e}"))?;
                }
                Err(e) => {
                    let _ = tx.rollback();
                    return Err(e);
                }
            }

            Ok::<_, String>((n_updated, n_deleted))
        }.await;

        if let Ok(c) = conn_clone.get() {
            match result {
                Ok((updated, deleted)) => {
                    let _ = update_reindex_history(
                        &c,
                        history_id,
                        "completed",
                        (updated + deleted) as i64,
                        updated as i64,
                        None,
                    );
                }
                Err(e) => {
                    let _ = update_reindex_history(&c, history_id, "failed", 0, 0, Some(&e));
                }
            }
        }
    });

    McpResponse::text(
        id,
        format!("Synchronization for project '{name}' has been started in the background. History ID: {history_id}. Use doxus_reindex_status to check status."),
    )
}

pub fn setup_project_agent(_server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let path_str = args["path"].as_str().unwrap_or(".");
    let project_path = std::path::PathBuf::from(path_str);

    // Attempt to resolve absolute path for clarity
    let abs_path = std::fs::canonicalize(&project_path).unwrap_or(project_path.clone());
    let claude_md_path = abs_path.join("CLAUDE.md");

    let instr_header = "## AI 에이전트 도구 (Doxus)";
    let instr_body = r#"이 프로젝트의 지식과 문서는 Doxus에 의해 인덱싱되어 있습니다. 에이전트는 다음 MCP 도구를 사용하여 문서를 검색하고 맥락을 파악할 수 있습니다:
- `doxus_search`: 하이브리드 검색을 통해 관련 문서 및 코드 조각을 찾습니다.
- `doxus_get_document`: 문서의 전체 내용을 읽어옵니다.
- `doxus_agent_summary`: 현재 인덱싱된 프로젝트의 전체 상태와 주요 태그를 파악합니다.
- `doxus_get_backlinks`: 문서 간의 연관 관계 및 참고 자료를 추적합니다.

지식 검색이 필요한 경우 가장 먼저 `doxus_search`를 호출하십시오."#;

    let mut content = if claude_md_path.exists() {
        match std::fs::read_to_string(&claude_md_path) {
            Ok(c) => c,
            Err(e) => {
                return McpResponse::err(id, -32603, format!("Failed to read CLAUDE.md: {e}"))
            }
        }
    } else {
        "# Project Instructions\n\n".to_string()
    };

    if content.contains(instr_header) {
        return McpResponse::text(id, "Doxus agent instructions already present in CLAUDE.md.");
    }

    content.push_str("\n\n");
    content.push_str(instr_header);
    content.push('\n');
    content.push_str(instr_body);

    if let Err(e) = std::fs::write(&claude_md_path, content) {
        return McpResponse::err(id, -32603, format!("Failed to write CLAUDE.md: {e}"));
    }

    McpResponse::text(
        id,
        format!(
            "Doxus agent instructions successfully added to {}.",
            claude_md_path.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServer;
    use doxus_core::db::TestDb;
    use std::sync::{Arc, Mutex};

    fn make_server(db: TestDb) -> McpServer {
        let conn = doxus_core::db::create_pool(std::path::Path::new(":memory:")).unwrap();
        // TestDb의 conn에 이미 마이그레이션이 다 적용되어 있지만, McpServer::new에는 pool이 들어가야 하므로
        // 임시 db 파일로 풀을 만들어서 전달하는 편이 더 낫다.
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = doxus_core::db::create_pool(&db_path).unwrap();
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(
            std::path::PathBuf::from("/tmp/plugins"),
        ));
        McpServer::new(
            pool,
            db_path,
            None,
            pm,
            std::path::PathBuf::from("/tmp/plugins"),
        )
    }

    #[test]
    fn test_add_project_creates_source_instances_row() {
        let db = TestDb::new();
        let server = make_server(db);

        let args = serde_json::json!({
            "name": "my-confluence",
            "path": "https://example.atlassian.net",
            "source_type": "confluence"
        });
        let resp = add_project(&server, serde_json::json!(1), &args);
        assert!(
            resp.result.is_some(),
            "add_project should succeed: {:?}",
            resp.error
        );

        let count: i64 = server
            .conn()
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM source_instances si
                 JOIN projects p ON si.project_id = p.id
                 WHERE p.name = 'my-confluence'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1, "add_project는 source_instances 행을 생성해야 함");
    }

    #[test]
    fn test_add_project_obsidian_default_source_type() {
        let db = TestDb::new();
        let server = make_server(db);

        // source_type 없으면 obsidian 기본값
        let args = serde_json::json!({ "name": "my-vault", "path": "/Users/me/vault" });
        let resp = add_project(&server, serde_json::json!(2), &args);
        assert!(
            resp.result.is_some(),
            "add_project should succeed: {:?}",
            resp.error
        );

        let plugin_id: String = server
            .conn()
            .get()
            .unwrap()
            .query_row(
                "SELECT si.plugin_id FROM source_instances si
                 JOIN projects p ON si.project_id = p.id
                 WHERE p.name = 'my-vault'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plugin_id, "com.doxus.obsidian");
    }
}
