use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchAllOptsWasm {
    pub cursor: Option<String>,
    pub page_size: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawDocumentWasm {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "title")]
    pub title: Option<String>,
    #[serde(rename = "content")]
    pub content: String,
    #[serde(rename = "content_type")]
    pub content_type: String,
    #[serde(rename = "url")]
    pub url: Option<String>,
    #[serde(rename = "metadata")]
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
    #[serde(rename = "created_at")]
    pub created_at: Option<i64>,
    #[serde(rename = "updated_at")]
    pub updated_at: Option<i64>,
    #[serde(rename = "relative_path")]
    pub relative_path: Option<String>,
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
