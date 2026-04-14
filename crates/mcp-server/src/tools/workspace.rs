use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 디폴트 워크스페이스의 project_id 조회 (없으면 생성)
pub(crate) fn workspace_project_id(server: &McpServer) -> Result<i64, String> {
    server.conn().query_row(
        "SELECT id FROM projects WHERE source_type='workspace' AND is_default=1 LIMIT 1",
        [],
        |r| r.get(0),
    ).or_else(|_| {
        server.conn().query_row(
            "SELECT id FROM projects WHERE source_type='workspace' LIMIT 1",
            [],
            |r| r.get(0),
        )
    }).map_err(|e| format!("workspace project not found: {e}. Run ensure_default_workspace first."))
}

pub fn create_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let title = match args["title"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: title"),
    };
    let doc_type = args["doc_type"].as_str().unwrap_or("note");

    let project_id = match workspace_project_id(server) {
        Ok(pid) => pid,
        Err(e) => return McpResponse::err(id, -32603, e),
    };

    let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let file_path = format!("workspace/{slug}.md");
    let source_doc_id = format!("ws-{slug}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
    let content = format!("# {title}\n\n");
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let metadata = serde_json::json!({"doc_type": doc_type}).to_string();

    let result = server.conn().execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), unixepoch())",
        params![project_id, source_doc_id, file_path, title, content, hash, metadata],
    );

    match result {
        Ok(_) => {
            let new_id = server.conn().last_insert_rowid();
            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                    "id": new_id,
                    "file_path": file_path,
                    "title": title,
                    "doc_type": doc_type,
                    "content": content,
                })).unwrap_or_default() }]
            }))
        }
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn update_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let doc_id: i64 = match args["id"].as_i64() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id (integer)"),
    };
    let content = match args["content"].as_str() {
        Some(c) => c,
        None => return McpResponse::err(id, -32602, "missing required arg: content"),
    };
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    let n = server.conn().execute(
        "UPDATE documents SET content=?2, content_hash=?3, updated_at=unixepoch() WHERE id=?1",
        params![doc_id, content, hash],
    );

    match n {
        Ok(0) => McpResponse::err(id, -32602, format!("workspace document #{doc_id} not found")),
        Ok(_) => McpResponse::text(id, format!("Document #{doc_id} updated.")),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn delete_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let doc_id: i64 = match args["id"].as_i64() {
        Some(i) => i,
        None => return McpResponse::err(id, -32602, "missing required arg: id (integer)"),
    };

    let n = server.conn().execute("DELETE FROM documents WHERE id=?1", params![doc_id]);
    match n {
        Ok(0) => McpResponse::err(id, -32602, format!("workspace document #{doc_id} not found")),
        Ok(_) => McpResponse::text(id, format!("Document #{doc_id} deleted.")),
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn list_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let type_filter = args["doc_type"].as_str();

    let (sql, _use_filter) = if let Some(dt) = type_filter {
        (format!(
            "SELECT d.id, d.file_path, d.title, d.metadata_json, d.created_at
             FROM documents d
             JOIN projects p ON d.project_id = p.id
             WHERE p.source_type='workspace'
             AND json_extract(d.metadata_json, '$.doc_type') = '{dt}'
             ORDER BY d.created_at DESC"
        ), true)
    } else {
        ("SELECT d.id, d.file_path, d.title, d.metadata_json, d.created_at
          FROM documents d
          JOIN projects p ON d.project_id = p.id
          WHERE p.source_type='workspace'
          ORDER BY d.created_at DESC".to_string(), false)
    };

    let mut stmt = match server.conn().prepare(&sql) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Value> {
        let meta: String = r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "{}".to_string());
        let meta_val: Value = serde_json::from_str(&meta).unwrap_or(json!({}));
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "file_path": r.get::<_, Option<String>>(1)?,
            "title": r.get::<_, Option<String>>(2)?,
            "doc_type": meta_val["doc_type"],
            "created_at": r.get::<_, Option<i64>>(4)?,
        }))
    };

    let rows: Result<Vec<_>, _> = stmt.query_map([], map_row).and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(items) if items.is_empty() => McpResponse::text(id, "No workspace documents found."),
        Ok(items) => McpResponse::ok(id, json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
        })),
    }
}

pub fn apply_template(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let template_name = match args["template"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: template"),
    };
    let frontmatter_in = args.get("frontmatter").cloned().unwrap_or(json!({}));
    let mut variables = args.get("variables").cloned().unwrap_or(json!({}));
    
    if let Some(fm_obj) = frontmatter_in.as_object() {
        if let Some(vars_obj) = variables.as_object_mut() {
            for (k, v) in fm_obj {
                vars_obj.entry(k).or_insert(v.clone());
            }
        }
    }
    let title = variables["title"].as_str().unwrap_or(template_name).to_string();

    let now: String = server.conn()
        .query_row("SELECT date('now')", [], |r| r.get(0))
        .unwrap_or_else(|_| "2026-01-01".to_string());
    if variables["created"].is_null() || variables["created"].as_str().map(|s| s.is_empty()).unwrap_or(false) {
        variables["created"] = json!(now);
    }
    if variables["updated"].is_null() || variables["updated"].as_str().map(|s| s.is_empty()).unwrap_or(false) {
        variables["updated"] = json!(now);
    }

    let mut engine = doxus_core::workspace::TemplateEngine::with_builtins();

    let db_content: Option<String> = server.conn().query_row(
        "SELECT content FROM templates WHERE name=?1",
        params![template_name],
        |r| r.get(0),
    ).ok();
    if let Some(ref src) = db_content {
        engine.register(template_name, src).ok();
    }

    let content = match engine.render(template_name, &variables) {
        Ok(rendered) => rendered,
        Err(_) => format!("# {title}\n\n<!-- template: {template_name} -->\n\n"),
    };

    let parsed = doxus_core::document::parse_frontmatter(&content);
    let frontmatter_obj: serde_json::Map<String, Value> = parsed.fields.iter()
        .map(|(k, v)| {
            let v = v.trim().trim_matches('"').to_string();
            (k.clone(), Value::String(v))
        })
        .collect();

    let slug: String = title.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let file_path = format!("workspace/{slug}.md");
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    let doc_type = template_name;
    let metadata = serde_json::json!({"doc_type": doc_type}).to_string();

    let project_id = match workspace_project_id(server) {
        Ok(pid) => pid,
        Err(e) => return McpResponse::err(id, -32603, e),
    };
    let source_doc_id = format!("ws-tpl-{slug}-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());

    let result = server.conn().execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), unixepoch())",
        params![project_id, source_doc_id, file_path, title, content, hash, metadata],
    );
    match result {
        Ok(_) => {
            let new_id = server.conn().last_insert_rowid();
            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                    "id": new_id,
                    "file_path": file_path,
                    "title": title,
                    "content": content,
                    "frontmatter": frontmatter_obj,
                    "body": parsed.body,
                })).unwrap_or_default() }]
            }))
        }
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
    }
}

pub fn list_templates(server: &McpServer, id: Value) -> McpResponse {
    use doxus_core::workspace::TemplateEngine;

    let engine = TemplateEngine::with_builtins();
    let mut items: Vec<Value> = engine.list_templates().into_iter().map(|t| json!({
        "name": t.name,
        "description": t.description,
        "source": "builtin",
    })).collect();

    if let Ok(mut stmt) = server.conn().prepare(
        "SELECT name, description FROM templates ORDER BY name"
    ) {
        let db_items: Vec<Value> = stmt.query_map([], |r| {
            Ok(json!({
                "name": r.get::<_, String>(0)?,
                "description": r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                "source": "custom",
            }))
        }).ok()
          .and_then(|rows| rows.collect::<Result<Vec<_>, _>>().ok())
          .unwrap_or_default();
        items.extend(db_items);
    }

    McpResponse::ok(id, json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({"templates": items})).unwrap_or_default() }]
    }))
}

pub fn get_template(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    use doxus_core::workspace::{TemplateEngine, extract_frontmatter_variables, extract_body_variables};

    let name = match args["name"].as_str() {
        Some(n) => n,
        None => return McpResponse::err(id, -32602, "missing required arg: name"),
    };

    let engine = TemplateEngine::with_builtins();

    let source = if let Some(src) = engine.get_template_source(name) {
        src
    } else {
        match server.conn().query_row(
            "SELECT content FROM templates WHERE name=?1",
            params![name],
            |r| r.get::<_, String>(0),
        ) {
            Ok(src) => src,
            Err(_) => return McpResponse::err(id, -32602, format!("template '{name}' not found")),
        }
    };

    let frontmatter_fields = extract_frontmatter_variables(&source);
    let body_variables = extract_body_variables(&source);

    McpResponse::ok(id, json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
            "name": name,
            "content": source,
            "frontmatter_fields": frontmatter_fields,
            "body_variables": body_variables,
        })).unwrap_or_default() }]
    }))
}
