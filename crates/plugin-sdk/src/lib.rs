use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ── Shared error type ─────────────────────────────────────────────────────────

#[derive(Debug, Error, Serialize, Deserialize, Clone)]
pub enum PluginError {
    #[error("config invalid: {0}")]
    ConfigInvalid(String),
    #[error("auth required")]
    AuthRequired,
    #[error("auth expired")]
    AuthExpired,
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// ── Shared types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Builtin,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginSecrets {
    pub fields: HashMap<String, SecretValue>,
}

#[derive(Debug, Clone)]
pub enum SecretValue {
    Text(String),
    Token {
        value: String,
        expires_at: Option<i64>,
        refresh_token: Option<String>,
    },
}

/// Opaque cursor for plugin pagination — never parsed by core.
pub type Cursor = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDocId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawDocument {
    pub id: SourceDocId,
    pub title: Option<String>,
    pub content: String,
    pub content_type: ContentType,
    pub url: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub tags: Vec<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    #[default]
    Markdown,
    PlainText,
}

#[derive(Debug, Clone)]
pub struct FetchAllOpts {
    pub cursor: Option<Cursor>,
    pub page_size: usize,
}

#[derive(Debug)]
pub struct DocumentStream {
    pub documents: Vec<RawDocument>,
    pub next_cursor: Option<Cursor>,
    pub estimated_total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FetchChangesOpts {
    pub since: i64,
    pub cursor: Option<Cursor>,
    pub page_size: usize,
}

#[derive(Debug)]
pub struct ChangeSet {
    pub updated: Vec<RawDocument>,
    pub deleted_ids: Vec<SourceDocId>,
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub incremental_sync: bool,
    pub oauth: bool,
    pub native_search: bool,
}

// ── DocSource trait ───────────────────────────────────────────────────────────

#[async_trait]
pub trait DocSource: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;
    fn capabilities(&self) -> Capabilities;

    async fn validate_config(&self, config: &PluginConfig) -> Result<(), PluginError>;

    async fn initialize(
        &mut self,
        config: PluginConfig,
        secrets: PluginSecrets,
    ) -> Result<(), PluginError>;

    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError>;

    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Err(PluginError::Internal(
            "incremental sync not supported by this plugin".to_string(),
        ))
    }

    async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError>;

    async fn health_check(&self) -> HealthStatus;

    async fn oauth_start(&self) -> Option<String> {
        None
    }
    async fn oauth_exchange(&mut self, _code: &str, _state: &str) -> Result<(), PluginError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_error_is_serializable() {
        let e = PluginError::RateLimited { retry_after_secs: 60 };
        let json = serde_json::to_string(&e).unwrap();
        let back: PluginError = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, PluginError::RateLimited { retry_after_secs: 60 }));
    }

    #[test]
    fn raw_document_roundtrip() {
        let doc = RawDocument {
            id: SourceDocId("doc1".into()),
            title: Some("Test".into()),
            content: "# Hello".into(),
            content_type: ContentType::Markdown,
            url: None,
            metadata: HashMap::new(),
            tags: vec!["rust".into()],
            updated_at: Some(1700000000),
        };
        let json = serde_json::to_string(&doc).unwrap();
        let back: RawDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id.0, "doc1");
        assert_eq!(back.tags, vec!["rust"]);
    }

    #[test]
    fn doc_source_is_object_safe() {
        let _: Option<Box<dyn DocSource>> = None;
    }
}
