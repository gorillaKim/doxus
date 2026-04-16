use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::Value;

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
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};
    use std::collections::HashMap;
    use std::sync::Arc;

    let name = match args["project"].as_str().or_else(|| args["name"].as_str()) {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let conn = server.conn();
    let (project_id, path) = {
        let conn_lock = match conn.lock() {
            Ok(l) => l,
            Err(_) => return McpResponse::err(id.clone(), -32603, "db lock poisoned"),
        };
        let row: Result<(i64, String), _> = conn_lock.query_row(
            "SELECT id, path FROM projects WHERE name=?1",
            params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );
        match row {
            Err(_) => return McpResponse::err(id, -32602, format!("project '{name}' not found")),
            Ok(r) => r,
        }
    };

    let embedder = server.embedder()
        .cloned()
        .unwrap_or_else(|| Arc::new(doxus_core::embedding::NoOpEmbedder) as Arc<dyn doxus_core::embedding::EmbeddingProvider + Send + Sync>);
    let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), embedder));

    let run_batch_indexing = async move {
        let mut plugin = ObsidianPlugin::new();
        let mut fields = HashMap::new();
        fields.insert("path".to_string(), serde_json::Value::String(path));
        let config = PluginConfig { fields };
        let secrets = PluginSecrets { fields: HashMap::new() };

        plugin.initialize(config, secrets).await?;

        let mut cursor = None;
        let mut total_indexed = 0;

        loop {
            // 1. Fetch a batch of documents
            let stream = plugin
                .fetch_all(FetchAllOpts { cursor, page_size: 50 })
                .await?;
            
            let docs = stream.documents;
            if docs.is_empty() {
                break;
            }

            // 2. Index the batch (with embeddings)
            for doc in &docs {
                let title = doc.title.as_deref().unwrap_or("");
                engine.index_document_async(project_id, &doc.id.0, title, &doc.content).await
                    .map_err(|e| doxus_plugin_sdk::PluginError::Internal(format!("indexing error for '{}': {e}", doc.id.0)))?;
            }

            total_indexed += docs.len();
            cursor = stream.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok::<usize, doxus_plugin_sdk::PluginError>(total_indexed)
    };

    let result = match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(run_batch_indexing)),
        Err(_) => {
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(run_batch_indexing),
                Err(e) => return McpResponse::err(id, -32603, format!("runtime error: {e}")),
            }
        }
    };

    match result {
        Ok(indexed) => {
            McpResponse::text(id, format!("Project '{name}' indexed: {indexed} documents. (Embeddings generated)"))
        }
        Err(e) => McpResponse::err(id, -32603, format!("index failed: {e}")),
    }
}

pub fn sync_project(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::search::SyncSearchEngine;
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchChangesOpts, PluginConfig, PluginSecrets, SourceDocId};
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
        "SELECT si.id, si.plugin_id, si.sync_cursor, si.last_synced, p.path
         FROM source_instances si
         JOIN projects p ON si.project_id = p.id
         WHERE p.name = ?1
         ORDER BY si.id LIMIT 1",
        params![name],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    );

    let (si_id, plugin_id, sync_cursor, last_synced, path) = match row {
        Err(_) => return McpResponse::text(
            id,
            format!("Project '{name}' has no source instance configured — no source instance"),
        ),
        Ok(r) => r,
    };

    if plugin_id != "com.doxus.obsidian" {
        return McpResponse::err(id, -32603, format!("unsupported plugin: {plugin_id}"));
    }

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

    let mut plugin = ObsidianPlugin::new();
    let mut fields = HashMap::new();
    fields.insert("path".to_string(), serde_json::Value::String(path));
    let config = PluginConfig { fields };
    let secrets = PluginSecrets { fields: HashMap::new() };

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

        let engine = SyncSearchEngine::from_conn(&*conn_lock);

        for doc in &changeset.updated {
            let title = doc.title.as_deref().unwrap_or("");
            if let Err(e) = engine.index_document(project_id, &doc.id.0, title, &doc.content) {
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
