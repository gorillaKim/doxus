use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::Value;
use doxus_core::indexing::IndexingService;

pub fn list_projects(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let mut stmt = match conn_lock.prepare(
        "SELECT name, display_name, status, path FROM projects ORDER BY name",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map([], |r| {
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

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let result = conn_lock.execute(
        "INSERT INTO projects(name, display_name, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, unixepoch(), unixepoch())",
        params![name, display_name, path],
    );

    match result {
        Ok(_) => McpResponse::text(
            id,
            format!("Project '{name}' added. Run doxus_index_project to index it."),
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
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let pid: Result<i64, _> = conn_lock
        .query_row("SELECT id FROM projects WHERE name=?1", params![name], |r| r.get(0));

    match pid {
        Err(_) => McpResponse::err(id, -32602, format!("project '{name}' not found")),
        Ok(pid) => {
            let _ = conn_lock
                .execute("DELETE FROM source_instances WHERE project_id=?1", [pid]);
            match conn_lock.execute("DELETE FROM projects WHERE id=?1", [pid]) {
                Ok(_) => McpResponse::text(
                    id,
                    format!("Project '{name}' removed (index data deleted, original files untouched)."),
                ),
                Err(e) => McpResponse::err(id, -32603, e.to_string()),
            }
        }
    }
}

pub fn index_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::SearchEngine;
    use std::sync::Arc;

    let name = match args["project"].as_str().or_else(|| args["name"].as_str()) {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };
    let full = args["full"].as_bool().unwrap_or(false);

    let conn = server.conn().clone();
    let plugin_manager = server.plugin_manager().clone();
    let embedder = server.embedder()
        .unwrap_or_else(|| Arc::new(doxus_core::embedding::NoOpEmbedder) as Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync>);
    let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), embedder));

    let indexing_service = IndexingService::new(conn, plugin_manager, engine);

    let run_indexing = async move {
        indexing_service.index_project(&name, full).await
    };

    let result = match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(run_indexing)),
        Err(_) => {
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(run_indexing),
                Err(e) => return McpResponse::err(id, -32603, format!("runtime error: {e}")),
            }
        }
    };

    match result {
        Ok(indexed) => {
            McpResponse::text(id, format!("Project '{name}' indexed: {indexed} documents. (Unified Indexing Service)"))
        }
        Err(e) => McpResponse::err(id, -32603, format!("index failed: {e}")),
    }
}

pub fn sync_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::{SyncSearchEngine, DocMeta};
    use doxus_plugin_sdk::{FetchChangesOpts, PluginConfig, PluginSecrets, SourceDocId};
    use std::collections::HashMap;

    let name = match args["project"].as_str() {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let conn = server.conn();
    let conn_lock = match conn.lock() {
        Ok(l) => l,
        Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
    };
    let row: Result<(i64, String, Option<String>, Option<i64>, String), _> = conn_lock.query_row(
        "SELECT si.id, si.plugin_id, si.sync_cursor, si.last_synced, si.config_json
         FROM source_instances si
         JOIN projects p ON si.project_id = p.id
         WHERE p.name = ?1
         ORDER BY si.id LIMIT 1",
        params![name],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    );

    let (si_id, plugin_id, sync_cursor, last_synced, config_json) = match row {
        Err(_) => return McpResponse::text(
            id,
            format!("Project '{name}' has no source instance configured — no source instance"),
        ),
        Ok(r) => r,
    };

    let project_id: i64 = match conn_lock.query_row(
        "SELECT p.id FROM projects p
         JOIN source_instances si ON si.project_id = p.id
         WHERE si.id = ?1",
        params![si_id],
        |r| r.get(0),
    ) {
        Ok(pid) => pid,
        Err(e) => return McpResponse::err(id, -32603, format!("project lookup: {e}")),
    };

    let known_ids: Vec<SourceDocId> = {
        let mut stmt = match conn_lock.prepare(
            "SELECT source_doc_id FROM documents WHERE project_id = ?1",
        ) {
            Ok(s) => s,
            Err(e) => return McpResponse::err(id, -32603, format!("prepare known_ids: {e}")),
        };
        let ids: Result<Vec<String>, _> = stmt
            .query_map(params![project_id], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query known_ids: {e}"))
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string()));
        match ids {
            Ok(v) => v.into_iter().map(SourceDocId).collect(),
            Err(e) => return McpResponse::err(id, -32603, e),
        }
    };

    let since = last_synced.unwrap_or(0);
    let cursor = sync_cursor;

    let plugin = server.plugin_manager().get_source(&plugin_id)
        .ok_or_else(|| McpResponse::err(id.clone(), -32603, format!("plugin not found: {plugin_id}")));
    
    let mut plugin = match plugin {
        Ok(p) => p,
        Err(e) => return e,
    };

    let fields: HashMap<String, serde_json::Value> = serde_json::from_str(&config_json)
        .unwrap_or_default();
    let mut config = PluginConfig { fields };
    let mut secrets = PluginSecrets { fields: HashMap::new() };

    // Inject keychain auth (Wait synchronously in sync function)
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(doxus_core::auth::inject_keychain_auth(&plugin_id, &mut config, &mut secrets)));
    } else {
        let _ = tokio::runtime::Runtime::new().map(|rt| rt.block_on(doxus_core::auth::inject_keychain_auth(&plugin_id, &mut config, &mut secrets)));
    }

    let run_async = async move {
        plugin.initialize(config, secrets).await?;
        let changeset = plugin
            .fetch_changes(FetchChangesOpts { since, cursor, page_size: 1000, known_ids })
            .await?;
        Ok::<_, doxus_plugin_sdk::PluginError>(changeset)
    };

    let changeset = match tokio::runtime::Handle::try_current() {
        Ok(handle) => match tokio::task::block_in_place(|| handle.block_on(run_async)) {
            Ok(c) => c,
            Err(e) => return McpResponse::err(id, -32603, format!("fetch_changes error: {e}")),
        },
        Err(_) => {
            match tokio::runtime::Runtime::new()
                .map_err(|e| format!("runtime error: {e}"))
                .and_then(|rt| rt.block_on(run_async).map_err(|e| e.to_string()))
            {
                Ok(c) => c,
                Err(e) => return McpResponse::err(id, -32603, e),
            }
        }
    };

    let n_updated = changeset.updated.len();
    let n_deleted = changeset.deleted_ids.len();

    let result: Result<(), String> = (|| {
        conn_lock.execute_batch("BEGIN").map_err(|e| format!("begin: {e}"))?;

        let strategy: String = conn_lock.query_row(
            "SELECT storage_strategy FROM projects WHERE id = ?1",
            [project_id],
            |r| r.get(0)
        ).unwrap_or_else(|_| "full".to_string());

        let engine = SyncSearchEngine::from_conn(&*conn_lock);

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
            if let Err(e) = engine.index_document_with_meta(project_id, &doc.id.0, title, &doc.content, &meta, &strategy) {
                let _ = conn_lock.execute_batch("ROLLBACK");
                return Err(format!("index error for '{}': {e}", doc.id.0));
            }
        }

        for del_id in &changeset.deleted_ids {
            if let Err(e) = conn_lock.execute(
                "DELETE FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                params![project_id, &del_id.0],
            ) {
                let _ = conn_lock.execute_batch("ROLLBACK");
                return Err(format!("delete error for '{}': {e}", del_id.0));
            }
        }

        let new_cursor: Option<&str> = changeset.next_cursor.as_deref();
        if let Err(e) = conn_lock.execute(
            "UPDATE source_instances SET sync_cursor = ?1, last_synced = unixepoch() WHERE id = ?2",
            params![new_cursor, si_id],
        ) {
            let _ = conn_lock.execute_batch("ROLLBACK");
            return Err(format!("update cursor: {e}"));
        }

        conn_lock.execute_batch("COMMIT").map_err(|e| format!("commit: {e}"))?;
        Ok(())
    })();

    match result {
        Ok(()) => McpResponse::text(
            id,
            format!("Synced project '{name}': {n_updated} updated, {n_deleted} deleted."),
        ),
        Err(e) => McpResponse::err(id, -32603, format!("sync failed (rolled back): {e}")),
    }
}
pub fn setup_project_agent(_server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let path_str = match args["path"].as_str() {
        Some(p) => p,
        None => ".", // Default to current directory
    };
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
            Err(e) => return McpResponse::err(id, -32603, format!("Failed to read CLAUDE.md: {e}")),
        }
    } else {
        "# Project Instructions\n\n".to_string()
    };

    if content.contains(instr_header) {
        return McpResponse::text(id, "Doxus agent instructions already present in CLAUDE.md.");
    }

    content.push_str("\n\n");
    content.push_str(instr_header);
    content.push_str("\n");
    content.push_str(instr_body);

    if let Err(e) = std::fs::write(&claude_md_path, content) {
        return McpResponse::err(id, -32603, format!("Failed to write CLAUDE.md: {e}"));
    }

    McpResponse::text(id, format!("Doxus agent instructions successfully added to {}.", claude_md_path.display()))
}
