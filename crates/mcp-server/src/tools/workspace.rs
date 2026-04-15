use crate::server::McpServer;
use crate::types::McpResponse;
use rusqlite::params;
use serde_json::{json, Value};
use doxus_plugin_sdk::{DocSource, PluginConfig, PluginSecrets, SourceDocId};
use doxus_core::search::{DocMeta, SyncSearchEngine};

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

/// 특정 프로젝트 명으로 ID와 초기화된 DocSource를 가져옴
async fn resolve_write_source(server: &McpServer, project_name: Option<&str>) -> Result<(i64, Box<dyn DocSource>), String> {
    let conn = server.conn();
    let (project_id, plugin_id, config_json) = if let Some(pname) = project_name {
        conn.query_row(
            "SELECT p.id, si.plugin_id, si.config_json 
             FROM projects p
             JOIN source_instances si ON p.id = si.project_id
             WHERE p.name = ?1",
            [pname],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        ).map_err(|e| format!("Project '{}' not found or source instance missing: {}", pname, e))?
    } else {
        let pid = workspace_project_id(server)?;
        conn.query_row(
            "SELECT p.id, si.plugin_id, si.config_json 
             FROM projects p
             JOIN source_instances si ON p.id = si.project_id
             WHERE p.id = ?1",
            [pid],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        ).map_err(|e| format!("Default workspace source not found: {}", e))?
    };

    let mut source = server.plugin_manager().get_source(&plugin_id)
        .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;
    
    let config: PluginConfig = serde_json::from_str(&config_json)
        .map_err(|e| format!("Invalid plugin config: {}", e))?;
    
    source.initialize(config, PluginSecrets::default()).await
        .map_err(|e| format!("Plugin initialization failed: {}", e))?;
    
    if !source.supports_write() {
        return Err(format!("Source '{}' does not support write operations", plugin_id));
    }

    Ok((project_id, source))
}

/// 쓰기 작업 후 즉시 동기화하여 DB를 최신화함
async fn immediate_sync(server: &McpServer, project_id: i64, source: &dyn DocSource, doc_id: &SourceDocId) -> Result<(), String> {
    let raw_doc = source.fetch_document(doc_id).await
        .map_err(|e| format!("Failed to fetch document after write: {}", e))?;
    
    let engine = SyncSearchEngine::from_conn(server.conn());
    engine.index_document_with_meta(
        project_id,
        &raw_doc.id.0,
        raw_doc.title.as_deref().unwrap_or(&raw_doc.id.0),
        &raw_doc.content,
        &DocMeta {
            created_at: raw_doc.created_at,
            updated_at: raw_doc.updated_at,
            tags: raw_doc.tags,
            aliases: raw_doc.aliases,
            metadata: raw_doc.metadata,
        }
    ).map_err(|e| format!("Immediate sync failed: {}", e))?;
    Ok(())
}

pub async fn create_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let title = match args["title"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: title"),
    };
    let project_name = args["project"].as_str();

    let (project_id, source) = match resolve_write_source(server, project_name).await {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32603, e),
    };

    // External sources might have different default content or metadata wrap
    let doc_type = args["doc_type"].as_str().unwrap_or("note");
    let content = format!("# {title}\n\n");

    // metadata 파싱
    let mut metadata_map = std::collections::HashMap::new();
    if let Some(obj) = args["metadata"].as_object() {
        for (k, v) in obj {
            metadata_map.insert(k.clone(), v.clone());
        }
    }
    // doc_type도 metadata에 포함
    metadata_map.entry("doc_type".to_string()).or_insert(serde_json::json!(doc_type));
    let metadata_opt = if metadata_map.is_empty() { None } else { Some(&metadata_map) };

    match source.create_document(title, &content, metadata_opt).await {
        Ok(source_doc_id) => {
            // 즉시 동기화 실행
            if let Err(e) = immediate_sync(server, project_id, source.as_ref(), &source_doc_id).await {
                return McpResponse::err(id, -32603, format!("Document created but sync failed: {}", e));
            }

            // DB에서 생성된 문서 ID 조회
            let doc_id: i64 = server.conn().query_row(
                "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                params![project_id, source_doc_id.0],
                |r| r.get(0)
            ).unwrap_or(0);

            McpResponse::ok(id, json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                    "id": doc_id,
                    "source_doc_id": source_doc_id.0,
                    "title": title,
                    "doc_type": doc_type,
                    "status": "created and synced"
                })).unwrap_or_default() }]
            }))
        }
        Err(e) => McpResponse::err(id, -32603, format!("Failed to create document: {}", e)),
    }
}

pub async fn update_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let doc_id_str = match args["id"].as_str() {
        Some(s) => s,
        None => return McpResponse::err(id, -32602, "missing required arg: id (string)"),
    };
    let content = args["content"].as_str();
    let metadata_val = args["metadata"].as_object();
    let project_name = args["project"].as_str();

    let (project_id, source) = match resolve_write_source(server, project_name).await {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32603, e),
    };

    // metadata 변환 (Map<String, Value> -> HashMap<String, Value>)
    let mut metadata_map = std::collections::HashMap::new();
    if let Some(obj) = metadata_val {
        for (k, v) in obj {
            metadata_map.insert(k.clone(), v.clone());
        }
    }
    let metadata_opt = if metadata_map.is_empty() { None } else { Some(&metadata_map) };

    match source.update_document(&SourceDocId(doc_id_str.into()), content, metadata_opt).await {
        Ok(_) => {
            let source_doc_id = SourceDocId(doc_id_str.into());
            if let Err(e) = immediate_sync(server, project_id, source.as_ref(), &source_doc_id).await {
                return McpResponse::err(id, -32603, format!("Document updated but sync failed: {}", e));
            }
            McpResponse::text(id, format!("Document '{}' updated and synced.", doc_id_str))
        }
        Err(e) => McpResponse::err(id, -32603, format!("Failed to update document: {}", e)),
    }
}

pub async fn delete_document(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let doc_id_str = match args["id"].as_str() {
        Some(s) => s,
        None => return McpResponse::err(id, -32602, "missing required arg: id (string)"),
    };
    let project_name = args["project"].as_str();

    let (project_id, source) = match resolve_write_source(server, project_name).await {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32603, e),
    };

    match source.delete_document(&SourceDocId(doc_id_str.into())).await {
        Ok(_) => {
            // 로컬 DB에서도 삭제
            let _ = server.conn().execute(
                "DELETE FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                params![project_id, doc_id_str],
            );
            McpResponse::text(id, format!("Document '{}' deleted from source and local DB.", doc_id_str))
        }
        Err(e) => McpResponse::err(id, -32603, format!("Failed to delete document: {}", e)),
    }
}

pub fn list_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let type_filter = args["doc_type"].as_str();
    let project_name = args["project"].as_str();

    let project_id_limit = if let Some(pname) = project_name {
        let pid = server.conn().query_row(
            "SELECT id FROM projects WHERE name = ?1",
            [pname],
            |r| r.get::<_, i64>(0)
        ).map_err(|_| format!("Project '{}' not found", pname));
        
        match pid {
            Ok(id) => format!("AND d.project_id = {}", id),
            Err(e) => return McpResponse::err(id, -32603, e),
        }
    } else {
        match workspace_project_id(server) {
            Ok(pid) => format!("AND d.project_id = {}", pid),
            Err(e) => return McpResponse::err(id, -32603, e),
        }
    };

    let sql = if let Some(dt) = type_filter {
        format!(
            "SELECT d.id, d.file_path, d.title, d.metadata_json, d.created_at
             FROM documents d
             WHERE 1=1 {project_id_limit}
             AND json_extract(d.metadata_json, '$.doc_type') = '{dt}'
             ORDER BY d.created_at DESC"
        )
    } else {
        format!(
            "SELECT d.id, d.file_path, d.title, d.metadata_json, d.created_at
             FROM documents d
             WHERE 1=1 {project_id_limit}
             ORDER BY d.created_at DESC"
        )
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
        Ok(items) if items.is_empty() => McpResponse::text(id, "No documents found."),
        Ok(items) => McpResponse::ok(id, json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap_or_default() }]
        })),
    }
}

pub async fn apply_template(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let template_name = match args["template"].as_str() {
        Some(t) => t,
        None => return McpResponse::err(id, -32602, "missing required arg: template"),
    };
    let mut variables = args.get("variables").cloned().unwrap_or(json!({}));
    let project_name = args["project"].as_str();

    let (project_id, source) = match resolve_write_source(server, project_name).await {
        Ok(res) => res,
        Err(e) => return McpResponse::err(id, -32603, e),
    };

    let title = variables["title"].as_str().unwrap_or(template_name).to_string();

    let now: String = server.conn()
        .query_row("SELECT date('now')", [], |r| r.get(0))
        .unwrap_or_else(|_| "2024-01-01".to_string());
    if variables["created"].is_null() { variables["created"] = json!(now); }
    if variables["updated"].is_null() { variables["updated"] = json!(now); }

    let mut engine = doxus_core::workspace::TemplateEngine::with_builtins();
    if let Ok(db_src) = server.conn().query_row("SELECT content FROM templates WHERE name=?1", params![template_name], |r| r.get::<_, String>(0)) {
        engine.register(template_name, &db_src).ok();
    }

    let content = match engine.render(template_name, &variables) {
        Ok(rendered) => rendered,
        Err(_) => format!("# {title}\n\n"),
    };

    // metadata 파싱
    let mut metadata_map = std::collections::HashMap::new();
    if let Some(obj) = variables["metadata"].as_object() {
        for (k, v) in obj {
            metadata_map.insert(k.clone(), v.clone());
        }
    }
    let metadata_opt = if metadata_map.is_empty() { None } else { Some(&metadata_map) };

    match source.create_document(&title, &content, metadata_opt).await {
        Ok(source_doc_id) => {
            if let Err(e) = immediate_sync(server, project_id, source.as_ref(), &source_doc_id).await {
                return McpResponse::err(id, -32603, format!("Template applied but sync failed: {}", e));
            }
            McpResponse::text(id, format!("Template '{}' applied to project. New doc: {}", template_name, source_doc_id.0))
        }
        Err(e) => McpResponse::err(id, -32603, format!("Failed to apply template: {}", e)),
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
