use rusqlite::Connection;
use sha2::{Digest, Sha256};
use doxus_core::document::{parse_sections, replace_section, insert_section_after, delete_section};
use doxus_core::workspace::{
    ensure_default_workspace, get_workspace_project, WorkspaceProject,
};

fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn data_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".doxus")
}

// ── 워크스페이스 커맨드 ───────────────────────────────────────────────────────

/// 앱 시작 시 호출 — 디폴트 워크스페이스 보장
#[tauri::command]
pub async fn ensure_default_workspace_cmd(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let id = ensure_default_workspace(&conn, &data_dir()).map_err(|e| e.to_string())?;
    let ws = get_workspace_project(&conn, id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!(ws))
}

// ── 문서 커맨드 ──────────────────────────────────────────────────────────────

/// 디폴트(또는 지정) 워크스페이스의 project_id를 반환
fn resolve_project_id(conn: &Connection, workspace_id: Option<i64>) -> Result<i64, String> {
    match workspace_id {
        Some(id) => Ok(id),
        None => conn
            .query_row(
                "SELECT id FROM projects WHERE source_type='workspace' AND is_default=1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "활성 워크스페이스를 찾을 수 없습니다".to_string()),
    }
}

#[tauri::command]
pub async fn list_workspace_documents(
    state: tauri::State<'_, crate::AppState>,
    workspace_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let pid = resolve_project_id(&conn, workspace_id)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, substr(content, 1, 100) as preview, metadata_json
             FROM documents WHERE project_id=?1 ORDER BY created_at DESC LIMIT 100",
        )
        .map_err(|e| e.to_string())?;

    let docs: Vec<serde_json::Value> = stmt
        .query_map([pid], |r| {
            let meta: String = r.get::<_, String>(4).unwrap_or_else(|_| "{}".into());
            let meta_val: serde_json::Value = serde_json::from_str(&meta).unwrap_or_default();
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "title": r.get::<_, Option<String>>(1)?,
                "created_at": r.get::<_, i64>(2)?,
                "content_preview": r.get::<_, Option<String>>(3)?,
                "doc_type": meta_val["doc_type"],
                "status": meta_val["status"],
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!(docs))
}

#[tauri::command]
pub async fn create_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    title: String,
    template_id: Option<String>,
    workspace_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let pid = resolve_project_id(&conn, workspace_id)?;

    // 프로젝트 존재 확인
    let project_path: Option<String> = conn
        .query_row("SELECT path FROM projects WHERE id=?1", [pid], |r| r.get(0))
        .ok();
    let project_path = project_path.ok_or_else(|| format!("프로젝트를 찾을 수 없습니다: id={pid}"))?;

    // source_doc_id 생성
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = now_secs();
    let source_doc_id = format!("ws-doc-{now}-{seq:06}");
    let file_name = format!("{source_doc_id}.md");

    // 템플릿 내용 결정: DB 템플릿 우선, 없으면 빌트인
    let initial_content = resolve_template_content(&conn, template_id.as_deref());
    let content_hash = sha256_hex(&initial_content);
    let doc_type = template_id_to_doc_type(template_id.as_deref());

    // 파일 경로: project.path + file_name (경로 없으면 그냥 root)
    let file_path = format!("{}/{}", project_path, file_name);

    // 파일 시스템에 저장
    if let Ok(path) = std::path::PathBuf::from(&project_path).canonicalize().or_else(|_| {
        std::fs::create_dir_all(&project_path).map(|_| std::path::PathBuf::from(&project_path))
    }) {
        let _ = std::fs::write(path.join(&file_name), &initial_content);
    }

    let metadata = serde_json::json!({ "doc_type": doc_type, "status": "draft", "priority": "medium" }).to_string();

    conn.execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, indexing_status, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?8)",
        rusqlite::params![pid, source_doc_id, file_path, title, initial_content, content_hash, metadata, now],
    )
    .map_err(|e| e.to_string())?;

    let doc_id = conn.last_insert_rowid();

    // 즉시 재인덱싱 (비동기 백그라운드)
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({
        "id": doc_id,
        "title": title,
        "created_at": now,
        "content_preview": initial_content.chars().take(100).collect::<String>(),
    }))
}

#[tauri::command]
pub async fn update_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
    title: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = now_secs();
    let content_hash = sha256_hex(&content);

    let affected = conn
        .execute(
            "UPDATE documents SET title=?1, content=?2, content_hash=?3, updated_at=?4, indexing_status='pending' WHERE id=?5",
            rusqlite::params![title, content, content_hash, now, id],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err(format!("문서를 찾을 수 없습니다: id={id}"));
    }

    // 파일 시스템 동기화
    sync_document_to_file(&conn, id, &content);

    // 즉시 재인덱싱
    enqueue_reindex(state.inner(), id);

    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn delete_workspace_document(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let affected = conn
        .execute("DELETE FROM documents WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(format!("문서를 찾을 수 없습니다: id={id}"));
    }
    Ok(serde_json::json!({ "ok": true }))
}

// ── 섹션 커맨드 ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_document_sections(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let sections = parse_sections(&content);
    let result: Vec<serde_json::Value> = sections
        .iter()
        .map(|s| serde_json::json!({
            "heading": s.heading,
            "level": s.level,
            "content": s.content,
            "start_line": s.start_line,
            "end_line": s.end_line,
        }))
        .collect();

    Ok(serde_json::json!(result))
}

#[tauri::command]
pub async fn update_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    heading: String,
    new_content: String,
    occurrence: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = replace_section(&old_content, &heading, occurrence.unwrap_or(0), &new_content)
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn insert_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    after_heading: Option<String>,
    new_section_content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = insert_section_after(&old_content, after_heading.as_deref(), &new_section_content)
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn delete_document_section(
    state: tauri::State<'_, crate::AppState>,
    doc_id: i64,
    heading: String,
    occurrence: Option<usize>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let old_content: String = conn
        .query_row("SELECT content FROM documents WHERE id=?1", [doc_id], |r| r.get(0))
        .map_err(|_| format!("문서를 찾을 수 없습니다: id={doc_id}"))?;

    let updated = delete_section(&old_content, &heading, occurrence.unwrap_or(0))
        .map_err(|e| e.to_string())?;

    let now = now_secs();
    let content_hash = sha256_hex(&updated);
    conn.execute(
        "UPDATE documents SET content=?1, content_hash=?2, updated_at=?3, indexing_status='pending' WHERE id=?4",
        rusqlite::params![updated, content_hash, now, doc_id],
    )
    .map_err(|e| e.to_string())?;

    sync_document_to_file(&conn, doc_id, &updated);
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({ "ok": true }))
}

// ── 템플릿 커맨드 ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_templates(
    state: tauri::State<'_, crate::AppState>,
    project_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    use doxus_core::workspace::TemplateEngine;

    // 내장 템플릿 (12종)
    let engine = TemplateEngine::with_builtins();
    let mut items: Vec<serde_json::Value> = engine.list_templates().into_iter().map(|t| serde_json::json!({
        "name": t.name,
        "description": t.description,
        "source": "builtin",
    })).collect();

    // DB 사용자 정의 템플릿 (전체 필드 포함)
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, doc_type, content, description, project_id, created_at FROM templates WHERE project_id IS NULL OR project_id=?1 ORDER BY name",
    ) {
        let db_items: Vec<serde_json::Value> = stmt
            .query_map([project_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "doc_type": r.get::<_, String>(2)?,
                    "content": r.get::<_, String>(3)?,
                    "description": r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    "project_id": r.get::<_, Option<i64>>(5)?,
                    "created_at": r.get::<_, i64>(6)?,
                    "source": "custom",
                }))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        items.extend(db_items);
    }

    Ok(serde_json::json!({ "templates": items }))
}

/// 특정 템플릿 상세 조회: content + frontmatter_fields + body_variables 반환
#[tauri::command]
pub async fn get_template(
    state: tauri::State<'_, crate::AppState>,
    name: String,
) -> Result<serde_json::Value, String> {
    use doxus_core::workspace::{TemplateEngine, extract_frontmatter_variables, extract_body_variables};

    let engine = TemplateEngine::with_builtins();

    // 내장 먼저, 없으면 DB 조회
    let source = if let Some(src) = engine.get_template_source(&name) {
        src
    } else {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT content FROM templates WHERE name=?1",
            [&name],
            |r| r.get::<_, String>(0),
        ).map_err(|_| format!("템플릿을 찾을 수 없습니다: {name}"))?
    };

    let frontmatter_fields = extract_frontmatter_variables(&source);
    let body_variables = extract_body_variables(&source);

    Ok(serde_json::json!({
        "name": name,
        "content": source,
        "frontmatter_fields": frontmatter_fields,
        "body_variables": body_variables,
    }))
}

/// 템플릿 적용하여 문서 생성: frontmatter + variables 분리 수신
#[tauri::command]
pub async fn apply_template(
    state: tauri::State<'_, crate::AppState>,
    template: String,
    frontmatter: serde_json::Value,
    variables: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use doxus_core::workspace::TemplateEngine;
    use sha2::{Sha256, Digest};

    // frontmatter + variables 병합 (렌더링 컨텍스트)
    let mut ctx = variables.clone();
    if let (Some(fm_obj), Some(ctx_obj)) = (frontmatter.as_object(), ctx.as_object_mut()) {
        for (k, v) in fm_obj {
            ctx_obj.entry(k).or_insert(v.clone());
        }
    }

    let mut engine = TemplateEngine::with_builtins();

    // 날짜 자동 주입 + DB 사용자 정의 템플릿도 로드
    {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        let now: String = conn.query_row("SELECT date('now')", [], |r| r.get(0))
            .unwrap_or_else(|_| "2026-01-01".to_string());
        if let Some(ctx_obj) = ctx.as_object_mut() {
            ctx_obj.entry("created").or_insert(serde_json::json!(now));
            ctx_obj.entry("updated").or_insert(serde_json::json!(now));
        }
        if let Ok(src) = conn.query_row(
            "SELECT content FROM templates WHERE name=?1",
            [&template],
            |r| r.get::<_, String>(0),
        ) {
            engine.register(&template, &src).ok();
        }
    }

    let content = engine.render(&template, &ctx)
        .map_err(|e| e.to_string())?;

    let title = ctx["title"].as_str().unwrap_or(&template).to_string();
    let slug: String = title.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let file_path = format!("workspace/{slug}.md");

    // frontmatter 파싱 (응답용)
    let parsed = doxus_core::document::parse_frontmatter(&content);
    let frontmatter_obj: serde_json::Map<String, serde_json::Value> = parsed.fields.iter()
        .map(|(k, v)| {
            let v = v.trim().trim_matches('"').to_string();
            (k.clone(), serde_json::Value::String(v))
        })
        .collect();

    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let project_id: i64 = conn.query_row(
        "SELECT id FROM projects WHERE source_type='workspace' AND is_default=1 LIMIT 1",
        [],
        |r| r.get(0),
    ).map_err(|_| "기본 워크스페이스를 찾을 수 없습니다".to_string())?;

    let source_doc_id = format!("ws-tpl-{slug}-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());

    conn.execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), unixepoch())",
        rusqlite::params![project_id, source_doc_id, file_path, title, content, hash,
            serde_json::json!({"doc_type": template}).to_string()],
    ).map_err(|e| e.to_string())?;

    let new_id = conn.last_insert_rowid();
    Ok(serde_json::json!({
        "id": new_id,
        "file_path": file_path,
        "title": title,
        "content": content,
        "frontmatter": frontmatter_obj,
        "body": parsed.body,
    }))
}

#[tauri::command]
pub async fn create_template(
    state: tauri::State<'_, crate::AppState>,
    name: String,
    description: Option<String>,
    doc_type: String,
    content: String,
    project_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;

    // project_id 지정 시 프로젝트 존재 확인
    if let Some(pid) = project_id {
        let exists: bool = conn
            .query_row("SELECT COUNT(*) FROM projects WHERE id=?1", [pid], |r| r.get::<_, i64>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        if !exists {
            return Err(format!("프로젝트를 찾을 수 없습니다: id={pid}"));
        }
    }

    let now = now_secs();
    conn.execute(
        "INSERT INTO templates(project_id, name, description, doc_type, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![project_id, name, description, doc_type, content, now],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    Ok(serde_json::json!({ "id": id, "name": name, "created_at": now }))
}

#[tauri::command]
pub async fn update_template(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
    name: String,
    description: Option<String>,
    doc_type: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let affected = conn
        .execute(
            "UPDATE templates SET name=?1, description=?2, doc_type=?3, content=?4 WHERE id=?5",
            rusqlite::params![name, description, doc_type, content, id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(format!("템플릿을 찾을 수 없습니다: id={id}"));
    }
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn delete_template(
    state: tauri::State<'_, crate::AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let affected = conn
        .execute("DELETE FROM templates WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(format!("템플릿을 찾을 수 없습니다: id={id}"));
    }
    Ok(serde_json::json!({ "ok": true }))
}

/// 템플릿으로 문서 생성 (플러그인 프로젝트에서도 호출 가능)
#[tauri::command]
pub async fn create_document_from_template(
    state: tauri::State<'_, crate::AppState>,
    template_id: i64,
    project_id: i64,
    path: Option<String>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;

    // 프로젝트 존재 확인
    let project_path: Option<String> = conn
        .query_row("SELECT path FROM projects WHERE id=?1", [project_id], |r| r.get(0))
        .ok();
    let project_path = project_path
        .ok_or_else(|| format!("프로젝트를 찾을 수 없습니다: id={project_id}"))?;

    // 템플릿 조회
    let (tmpl_name, tmpl_content, tmpl_doc_type): (String, String, String) = conn
        .query_row(
            "SELECT name, content, doc_type FROM templates WHERE id=?1",
            [template_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| format!("템플릿을 찾을 수 없습니다: id={template_id}"))?;

    // 저장 경로 결정: project.path + 요청 path (없으면 root)
    let base_dir = match &path {
        Some(p) if !p.is_empty() => {
            let full = std::path::PathBuf::from(&project_path).join(p);
            // 없는 경로는 자동 생성
            std::fs::create_dir_all(&full).map_err(|e| format!("경로 생성 실패: {e}"))?;
            full
        }
        _ => std::path::PathBuf::from(&project_path),
    };

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = now_secs();
    let source_doc_id = format!("tmpl-doc-{now}-{seq:06}");
    let file_name = format!("{source_doc_id}.md");
    let file_path = base_dir.join(&file_name);
    let file_path_str = file_path.to_string_lossy().to_string();

    // 파일 저장
    std::fs::write(&file_path, &tmpl_content).map_err(|e| format!("파일 저장 실패: {e}"))?;

    let content_hash = sha256_hex(&tmpl_content);
    let metadata = serde_json::json!({ "doc_type": tmpl_doc_type, "status": "draft" }).to_string();

    conn.execute(
        "INSERT INTO documents(project_id, source_doc_id, file_path, title, content, content_hash, indexing_status, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?8)",
        rusqlite::params![project_id, source_doc_id, file_path_str, tmpl_name, tmpl_content, content_hash, metadata, now],
    )
    .map_err(|e| e.to_string())?;

    let doc_id = conn.last_insert_rowid();
    enqueue_reindex(state.inner(), doc_id);

    Ok(serde_json::json!({
        "id": doc_id,
        "title": tmpl_name,
        "file_path": file_path_str,
        "created_at": now,
    }))
}

// ── 내부 헬퍼 ────────────────────────────────────────────────────────────────

/// 파일 시스템에 문서 내용 동기화 (실패해도 무시)
fn sync_document_to_file(conn: &Connection, doc_id: i64, content: &str) {
    if let Ok(file_path) = conn.query_row(
        "SELECT file_path FROM documents WHERE id=?1",
        [doc_id],
        |r| r.get::<_, String>(0),
    ) {
        let _ = std::fs::write(&file_path, content);
    }
}

/// 즉시 재인덱싱 요청 (백그라운드 tokio::spawn)
fn enqueue_reindex(state: &crate::AppState, doc_id: i64) {
    let conn_arc = state.conn.clone();
    let embedder = state.embedder.clone();
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            reindex_document_sync(&conn, doc_id)
        })
        .await;
        if let Err(e) = result {
            eprintln!("[reindex] doc_id={doc_id} spawn error: {e}");
        }
    });
}

/// 단일 문서 동기 재인덱싱 (FTS5 + content_hash 업데이트)
fn reindex_document_sync(conn: &Connection, doc_id: i64) -> Result<(), String> {
    let (project_id, source_doc_id, title, content): (i64, String, String, String) = conn
        .query_row(
            "SELECT project_id, source_doc_id, COALESCE(title, ''), content FROM documents WHERE id=?1",
            [doc_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;

    // FTS5 upsert (chunks_fts)
    conn.execute(
        "INSERT OR REPLACE INTO chunks_fts(rowid, title, content, project_id, source_doc_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![doc_id, title, content, project_id, source_doc_id],
    )
    .map_err(|e| e.to_string())?;

    // indexing_status 업데이트
    conn.execute(
        "UPDATE documents SET indexing_status='indexed', last_indexed=?1 WHERE id=?2",
        rusqlite::params![now_secs(), doc_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn resolve_template_content(conn: &Connection, template_id: Option<&str>) -> String {
    // DB에서 먼저 조회
    if let Some(tid) = template_id {
        if let Ok(content) = conn.query_row(
            "SELECT content FROM templates WHERE name=?1 LIMIT 1",
            [tid],
            |r| r.get::<_, String>(0),
        ) {
            if !content.is_empty() {
                return content;
            }
        }
    }
    // 빌트인 폴백
    builtin_template_content(template_id.unwrap_or("")).to_string()
}

fn builtin_template_content(template_id: &str) -> &'static str {
    match template_id {
        "todo" => "# TODO\n\n## 오늘\n- [ ] \n\n## 이번 주\n- [ ] \n\n## 백로그\n- [ ] \n",
        "techspec" => "# [기능명] 기술 명세서\n\n## 개요\n> 한 줄 요약\n\n## 요구사항\n### 기능 요구사항\n- [ ] FR-01:\n\n### 비기능 요구사항\n- [ ] NFR-01:\n\n## 상세 구현 계획\n### 아키텍처\n### API 설계\n### DB 스키마 변경\n### 테스트 계획\n\n## 리스크 및 미결 사항\n",
        "meeting" => "# 회의록\n\n**일시**: \n**참석자**: \n\n## 안건\n\n## 결정 사항\n\n## 액션 아이템\n- [ ] \n",
        "decision" => "# 의사결정 기록 (ADR)\n\n## 상태\ndraft\n\n## 컨텍스트\n\n## 결정\n\n## 결과\n",
        "journal" => "# 일지\n\n**날짜**: \n\n## 오늘 한 일\n\n## 배운 것\n\n## 내일 할 일\n",
        "retrospective" => "# 스프린트 회고\n\n## 잘 된 것\n\n## 개선할 것\n\n## 액션 아이템\n- [ ] \n",
        _ => "",
    }
}

fn template_id_to_doc_type(template_id: Option<&str>) -> &'static str {
    match template_id {
        Some("meeting") => "meeting",
        Some("decision") => "decision",
        Some("journal") => "journal",
        Some("retrospective") => "other",
        Some("todo") => "other",
        Some("techspec") => "other",
        _ => "note",
    }
}

// ── 단위 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    fn seed_workspace(conn: &rusqlite::Connection) -> i64 {
        let tmp = tempfile::tempdir().unwrap();
        ensure_default_workspace(conn, tmp.path()).unwrap()
    }

    #[test]
    fn resolve_project_id_uses_default_workspace() {
        let conn = make_conn();
        let ws_id = seed_workspace(&conn);
        let resolved = resolve_project_id(&conn, None).unwrap();
        assert_eq!(resolved, ws_id);
    }

    #[test]
    fn resolve_project_id_uses_explicit_id() {
        let conn = make_conn();
        seed_workspace(&conn);
        let resolved = resolve_project_id(&conn, Some(42));
        // project 42가 없어도 id 그대로 반환
        assert_eq!(resolved.unwrap(), 42);
    }

    #[test]
    fn resolve_project_id_error_when_no_default() {
        let conn = make_conn();
        // 디폴트 워크스페이스 없는 상태
        let result = resolve_project_id(&conn, None);
        assert!(result.is_err());
    }

    #[test]
    fn reindex_document_sync_updates_fts() {
        let conn = make_conn();
        let ws_id = seed_workspace(&conn);

        conn.execute(
            "INSERT INTO documents(project_id, source_doc_id, title, content, content_hash, indexing_status, created_at, updated_at)
             VALUES (?1, 'test-doc', '테스트', '검색 가능한 내용', 'hash', 'pending', 1, 1)",
            [ws_id],
        ).unwrap();
        let doc_id = conn.last_insert_rowid();

        reindex_document_sync(&conn, doc_id).unwrap();

        let status: String = conn.query_row(
            "SELECT indexing_status FROM documents WHERE id=?1", [doc_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(status, "indexed");
    }

    #[test]
    fn builtin_todo_template_has_checkbox() {
        let content = builtin_template_content("todo");
        assert!(content.contains("- [ ]"));
    }

    #[test]
    fn builtin_unknown_template_returns_empty() {
        let content = builtin_template_content("nonexistent");
        assert!(content.is_empty());
    }
}
