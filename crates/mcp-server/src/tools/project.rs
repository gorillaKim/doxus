use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

pub fn list_projects(server: &McpServer, id: Value) -> McpResponse {
    let mut stmt = match server.conn().prepare(
        "SELECT name, display_name, status, path FROM projects WHERE source_type != 'workspace' ORDER BY name",
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

    let result = server.conn().execute(
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

    let pid: Result<i64, _> = server
        .conn()
        .query_row("SELECT id FROM projects WHERE name=?1", params![name], |r| r.get(0));

    match pid {
        Err(_) => McpResponse::err(id, -32602, format!("project '{name}' not found")),
        Ok(pid) => {
            let _ = server
                .conn()
                .execute("DELETE FROM source_instances WHERE project_id=?1", [pid]);
            match server.conn().execute("DELETE FROM projects WHERE id=?1", [pid]) {
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
    use doxus_core::search::SyncSearchEngine;
    use doxus_plugin_obsidian::ObsidianPlugin;
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};
    use std::collections::HashMap;

    let name = match args["project"].as_str().or_else(|| args["name"].as_str()) {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let row: Result<(i64, String), _> = server.conn().query_row(
        "SELECT id, path FROM projects WHERE name=?1",
        params![name],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    let (project_id, path) = match row {
        Err(_) => return McpResponse::err(id, -32602, format!("project '{name}' not found")),
        Ok(r) => r,
    };

    let mut plugin = ObsidianPlugin::new();
    let mut fields = HashMap::new();
    fields.insert("path".to_string(), serde_json::Value::String(path));
    let config = PluginConfig { fields };
    let secrets = PluginSecrets { fields: HashMap::new() };

    let run_async = async move {
        plugin.initialize(config, secrets).await?;
        let mut all_docs = Vec::new();
        let mut cursor = None;
        loop {
            let stream = plugin
                .fetch_all(FetchAllOpts { cursor, page_size: 1000 })
                .await?;
            all_docs.extend(stream.documents);
            match stream.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok::<_, doxus_plugin_sdk::PluginError>(all_docs)
    };

    let docs = match tokio::runtime::Handle::try_current() {
        Ok(handle) => match tokio::task::block_in_place(|| handle.block_on(run_async)) {
            Ok(d) => d,
            Err(e) => return McpResponse::err(id, -32603, format!("fetch error: {e}")),
        },
        Err(_) => {
            match tokio::runtime::Runtime::new()
                .map_err(|e| format!("runtime error: {e}"))
                .and_then(|rt| rt.block_on(run_async).map_err(|e| e.to_string()))
            {
                Ok(d) => d,
                Err(e) => return McpResponse::err(id, -32603, e),
            }
        }
    };

    // Index all documents inside a single transaction
    let result: Result<usize, String> = (|| {
        server
            .conn()
            .execute_batch("BEGIN")
            .map_err(|e| format!("begin transaction: {e}"))?;

        let engine = SyncSearchEngine::from_conn(server.conn());
        let mut indexed = 0usize;

        for doc in &docs {
            let title = doc.title.as_deref().unwrap_or("");
            if let Err(e) = engine.index_document(project_id, &doc.id.0, title, &doc.content) {
                let _ = server.conn().execute_batch("ROLLBACK");
                return Err(format!("indexing error for '{}': {e}", doc.id.0));
            }
            indexed += 1;

            if let Some(links_val) = doc.metadata.get("links") {
                if let Some(links_arr) = links_val.as_array() {
                    let doc_id: i64 = match server.conn().query_row(
                        "SELECT id FROM documents WHERE project_id=?1 AND source_doc_id=?2",
                        params![project_id, &doc.id.0],
                        |r| r.get(0),
                    ) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = server.conn().execute_batch("ROLLBACK");
                            return Err(format!("doc id lookup: {e}"));
                        }
                    };

                    if let Err(e) = server.conn().execute(
                        "DELETE FROM document_links WHERE source_id=?1",
                        [doc_id],
                    ) {
                        let _ = server.conn().execute_batch("ROLLBACK");
                        return Err(format!("delete links: {e}"));
                    }

                    for link in links_arr {
                        if let Some(target_raw) = link.as_str() {
                            if let Err(e) = server.conn().execute(
                                "INSERT INTO document_links (source_id, target_raw, link_type) VALUES (?1, ?2, 'wikilink')",
                                params![doc_id, target_raw],
                            ) {
                                let _ = server.conn().execute_batch("ROLLBACK");
                                return Err(format!("insert link: {e}"));
                            }
                        }
                    }
                }
            }
        }

        server
            .conn()
            .execute_batch("COMMIT")
            .map_err(|e| format!("commit transaction: {e}"))?;

        Ok(indexed)
    })();

    match result {
        Ok(indexed) => {
            McpResponse::text(id, format!("Project '{name}' indexed: {indexed} documents."))
        }
        Err(e) => McpResponse::err(id, -32603, format!("index failed (rolled back): {e}")),
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

    let row: Result<(i64, String, Option<String>, Option<i64>, String), _> = server.conn().query_row(
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

    let project_id: i64 = match server.conn().query_row(
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
        let mut stmt = match server.conn().prepare(
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
        server.conn().execute_batch("BEGIN").map_err(|e| format!("begin: {e}"))?;

        let engine = SyncSearchEngine::from_conn(server.conn());

        for doc in &changeset.updated {
            let title = doc.title.as_deref().unwrap_or("");
            if let Err(e) = engine.index_document(project_id, &doc.id.0, title, &doc.content) {
                let _ = server.conn().execute_batch("ROLLBACK");
                return Err(format!("index error for '{}': {e}", doc.id.0));
            }
        }

        for del_id in &changeset.deleted_ids {
            if let Err(e) = server.conn().execute(
                "DELETE FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                params![project_id, &del_id.0],
            ) {
                let _ = server.conn().execute_batch("ROLLBACK");
                return Err(format!("delete error for '{}': {e}", del_id.0));
            }
        }

        let new_cursor: Option<&str> = changeset.next_cursor.as_deref();
        if let Err(e) = server.conn().execute(
            "UPDATE source_instances SET sync_cursor = ?1, last_synced = unixepoch() WHERE id = ?2",
            params![new_cursor, si_id],
        ) {
            let _ = server.conn().execute_batch("ROLLBACK");
            return Err(format!("update cursor: {e}"));
        }

        server.conn().execute_batch("COMMIT").map_err(|e| format!("commit: {e}"))?;
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
