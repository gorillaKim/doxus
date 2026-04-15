use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchAllOptsWasm {
    pub cursor: Option<String>,
    pub page_size: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RawDocumentWasm {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub content_type: String,
    pub url: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub tags: Vec<String>,
    pub updated_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DocumentStreamWasm {
    pub documents: Vec<RawDocumentWasm>,
    pub next_cursor: Option<String>,
    pub estimated_total: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchDocumentOptsWasm {
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchChangesOptsWasm {
    pub since: i64,
    pub cursor: Option<String>,
    pub page_size: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChangeSetWasm {
    pub updated: Vec<RawDocumentWasm>,
    pub deleted: Vec<String>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateDocumentOptsWasm {
    pub title: String,
    pub content: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateDocumentResultWasm {
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateDocumentOptsWasm {
    pub id: String,
    pub content: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteDocumentOptsWasm {
    pub id: String,
}
