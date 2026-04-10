use extism_pdk::*;
use serde::Serialize;

#[derive(Serialize)]
struct DocumentStreamWasm {
    documents: Vec<serde_json::Value>,
    next_cursor: Option<String>,
    estimated_total: Option<u64>,
}

#[derive(Serialize)]
struct ChangeSetWasm {
    updated: Vec<serde_json::Value>,
    deleted: Vec<String>,
    next_cursor: Option<String>,
}

#[plugin_fn]
pub fn fetch_all(_input: String) -> FnResult<Json<DocumentStreamWasm>> {
    Ok(Json(DocumentStreamWasm {
        documents: vec![],
        next_cursor: None,
        estimated_total: Some(0),
    }))
}

#[plugin_fn]
pub fn fetch_document(_input: String) -> FnResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "id": "test-doc-1",
        "title": "Test Document",
        "content": "# Hello from WASM",
        "content_type": "markdown",
        "url": null,
        "metadata": {},
        "tags": [],
        "updated_at": null
    })))
}

#[plugin_fn]
pub fn fetch_changes(_input: String) -> FnResult<Json<ChangeSetWasm>> {
    Ok(Json(ChangeSetWasm {
        updated: vec![],
        deleted: vec![],
        next_cursor: None,
    }))
}

#[plugin_fn]
pub fn health_check(_input: String) -> FnResult<String> {
    Ok("Healthy".to_string())
}
