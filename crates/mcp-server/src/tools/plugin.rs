use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};

pub fn list(server: &McpServer, id: Value) -> McpResponse {
    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let mut stmt = match conn_lock.prepare(
        "SELECT plugin_id, COUNT(*) as instances
         FROM source_instances
         GROUP BY plugin_id
         ORDER BY plugin_id",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<_>, _> = stmt
        .query_map([], |r: &rusqlite::Row<'_>| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) if rows.is_empty() => McpResponse::text(id, "No plugins installed."),
        Ok(rows) => {
            let items: Vec<Value> = rows
                .iter()
                .map(|(plugin_id, instances)| {
                    json!({ "plugin_id": plugin_id, "instances": instances })
                })
                .collect();
            McpResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&items).unwrap_or_default()
                    }]
                }),
            )
        }
    }
}

pub async fn install(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };
    let version = args["version"].as_str().unwrap_or("0.0.0");

    if let Some(registry_url) = args["registry_url"].as_str() {
        if !server.allow_file_scheme && !registry_url.starts_with("https://") {
            return McpResponse::err(id, -32602, "registry_url must use https://");
        }
        let client = match doxus_core::marketplace::registry::RegistryClient::new(registry_url) {
            Ok(c) => c,
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };
        let entry = match client.fetch_entry(plugin_id).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                return McpResponse::err(
                    id,
                    -32603,
                    format!("plugin '{plugin_id}' not found in registry"),
                )
            }
            Err(e) => return McpResponse::err(id, -32603, e.to_string()),
        };
        let installer =
            doxus_core::marketplace::installer::PluginInstaller::new(server.plugins_dir.clone());
        if let Err(e) =
            installer.install_from_url(plugin_id, &entry.download_url, Some(&entry.checksum_sha256))
        {
            return McpResponse::err(id, -32603, e.to_string());
        }
    } else if let Some(url) = args["url"].as_str() {
        let installer = if server.allow_file_scheme {
            doxus_core::marketplace::installer::PluginInstaller::new_with_file_scheme(
                server.plugins_dir.clone(),
            )
        } else {
            doxus_core::marketplace::installer::PluginInstaller::new(server.plugins_dir.clone())
        };
        let expected_sha256 = args["sha256"].as_str();
        if let Err(e) = installer.install_from_url(plugin_id, url, expected_sha256) {
            return McpResponse::err(id, -32603, e.to_string());
        }
    }

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let result = conn_lock.execute(
        "INSERT OR IGNORE INTO plugins(id, name, version, kind, installed_at)
         VALUES (?1, ?1, ?2, 'external', unixepoch())",
        params![plugin_id, version],
    );

    match result {
        Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' v{version} installed.")),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn remove(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };

    if !plugin_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return McpResponse::err(
            id,
            -32602,
            "invalid plugin_id: only alphanumeric, '.', '-', '_' allowed",
        );
    }

    {
        let plugins_dir = server.plugins_dir.clone();
        let wasm_path = plugins_dir.join(format!("{plugin_id}.wasm"));
        if let Ok(canonical) = wasm_path.canonicalize().or_else(|_| {
            plugins_dir
                .canonicalize()
                .map(|p| p.join(format!("{plugin_id}.wasm")))
        }) {
            let safe_prefix = plugins_dir.canonicalize().unwrap_or(plugins_dir.clone());
            if canonical.starts_with(&safe_prefix) && wasm_path.exists() {
                let _ = std::fs::remove_file(&wasm_path);
            }
        }
    }

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let db_result = conn_lock.execute("DELETE FROM plugins WHERE id=?1", params![plugin_id]);
    match db_result {
        Ok(0) => McpResponse::text(id, format!("Plugin '{plugin_id}' not found.")),
        Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' removed.")),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn update(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };
    let version = args["version"].as_str().unwrap_or("latest");

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let n = conn_lock.execute(
        "UPDATE plugins SET version=?2 WHERE id=?1",
        params![plugin_id, version],
    );
    match n {
        Ok(0) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
        Ok(_) => McpResponse::text(id, format!("Plugin '{plugin_id}' updated to v{version}.")),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn search(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let query = match args["query"].as_str() {
        Some(q) => q,
        None => return McpResponse::err(id, -32602, "missing required arg: query"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let mut stmt = match conn_lock.prepare(
        "SELECT id, name, version, kind, trust_level FROM plugins
         WHERE id LIKE ?1 OR name LIKE ?1
         ORDER BY name LIMIT 20",
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let pattern = format!("%{query}%");
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![pattern], |r: &rusqlite::Row<'_>| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "version": r.get::<_, String>(2)?,
                "kind": r.get::<_, String>(3)?,
                "trust_level": r.get::<_, String>(4)?,
            }))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(items) if items.is_empty() => {
            McpResponse::text(id, format!("No plugins matching '{query}'."))
        }
        Ok(items) => McpResponse::ok(
            id,
            json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            }),
        ),
    }
}

pub fn status(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let row: Result<(String, String, i64), _> = conn_lock.query_row(
        "SELECT version, trust_level, enabled FROM plugins WHERE id=?1",
        params![plugin_id],
        |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    );

    match row {
        Err(_) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
        Ok((version, trust, enabled)) => {
            let instances: i64 = conn_lock
                .query_row(
                    "SELECT COUNT(*) FROM source_instances WHERE plugin_id=?1",
                    params![plugin_id],
                    |r: &rusqlite::Row<'_>| r.get(0),
                )
                .unwrap_or(0);
            let status = json!({
                "id": plugin_id,
                "version": version,
                "trust_level": trust,
                "enabled": enabled != 0,
                "instances": instances,
            });
            McpResponse::ok(
                id,
                json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&status).unwrap_or_default() }]
                }),
            )
        }
    }
}

pub fn logs(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };
    let level_filter = args["level"].as_str().unwrap_or("info");
    let limit = args["limit"].as_u64().unwrap_or(50) as i64;

    let levels = match level_filter {
        "error" => vec!["error"],
        "warn" => vec!["error", "warn"],
        "info" => vec!["error", "warn", "info"],
        "debug" => vec!["error", "warn", "info", "debug"],
        _ => vec!["error", "warn", "info", "debug", "trace"],
    };
    let placeholders = levels
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT level, message, occurred_at FROM plugin_logs
         WHERE plugin_id = ?1 AND level IN ({placeholders})
         ORDER BY occurred_at DESC LIMIT ?"
    );

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };

    let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(plugin_id.to_string())];
    for l in &levels {
        all_params.push(Box::new(l.to_string()));
    }
    all_params.push(Box::new(limit));

    let mut stmt = match conn_lock.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|b| b.as_ref()).collect();
    let rows: Result<Vec<_>, _> = stmt
        .query_map(refs.as_slice(), |r: &rusqlite::Row<'_>| {
            Ok(json!({
                "level": r.get::<_, String>(0)?,
                "message": r.get::<_, String>(1)?,
                "occurred_at": r.get::<_, i64>(2)?,
            }))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(items) if items.is_empty() => {
            McpResponse::text(id, format!("No logs for plugin '{plugin_id}'."))
        }
        Ok(items) => McpResponse::ok(
            id,
            json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
            }),
        ),
    }
}

pub fn info(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let plugin_id = match args["id"].as_str() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id.clone(), -32603, format!("db pool error: {e}")),
    };
    let row: Result<(String, String, String, String, i64, i64), _> = conn_lock.query_row(
        "SELECT version, kind, trust_level, manifest_json, enabled, installed_at FROM plugins WHERE id=?1",
        params![plugin_id],
        |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    );

    match row {
        Err(_) => McpResponse::err(id, -32602, format!("plugin '{plugin_id}' not found")),
        Ok((version, kind, trust, manifest, enabled, installed_at)) => {
            let manifest_val: Value = serde_json::from_str(&manifest).unwrap_or(json!({}));
            let info = json!({
                "id": plugin_id,
                "version": version,
                "kind": kind,
                "trust_level": trust,
                "enabled": enabled != 0,
                "installed_at": installed_at,
                "manifest": manifest_val,
            });
            McpResponse::ok(
                id,
                json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default() }]
                }),
            )
        }
    }
}
