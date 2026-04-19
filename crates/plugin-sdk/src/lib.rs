#[cfg(feature = "native")]
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub mod wasm_types;
pub mod path_utils;

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
    pub aliases: Vec<String>,
    pub links: Vec<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    #[default]
    Markdown,
    PlainText,
    Html,
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
    /// IDs previously known to the caller; plugin uses this to detect deletions.
    pub known_ids: Vec<SourceDocId>,
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

// ── SSRF protection ───────────────────────────────────────────────────────────

/// Validates that a base URL is safe to use as a plugin endpoint.
///
/// Blocks:
/// - Non-HTTP(S) schemes
/// - HTTP (only HTTPS allowed)
/// - Loopback: 127.0.0.0/8, ::1
/// - RFC 1918 private: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
/// - Link-local / AWS metadata: 169.254.0.0/16, fe80::/10
/// - IPv6 unique-local: fc00::/7
/// - Hostnames: localhost, *.local
pub fn validate_base_url(url: &str) -> Result<(), PluginError> {
    // Require HTTPS scheme
    if !url.starts_with("https://") {
        return Err(PluginError::PermissionDenied(
            "base_url must use HTTPS".into(),
        ));
    }

    // Extract host (strip port if present)
    let after_scheme = url.trim_start_matches("https://");
    let authority = after_scheme.split('/').next().unwrap_or("");
    // Handle IPv6 bracket notation: [::1] or [::1]:port
    let host = if authority.starts_with('[') {
        // bracketed IPv6 — extract content between [ and ]
        authority
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or("")
    } else if authority.matches(':').count() >= 2 {
        // bare IPv6 (no brackets, no path yet) — use the whole authority
        authority
    } else {
        // hostname or IPv4 — strip port
        authority.split(':').next().unwrap_or("")
    }
    .trim();

    if host.is_empty() {
        return Err(PluginError::PermissionDenied(
            "base_url host is empty".into(),
        ));
    }

    // Block hostname-based private addresses
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".local")
        || host.eq_ignore_ascii_case(".local")
    {
        return Err(PluginError::PermissionDenied(format!(
            "base_url host is not allowed: {host}"
        )));
    }

    // Try to parse as IP address
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if is_ip_blocked(ip) {
            return Err(PluginError::PermissionDenied(format!(
                "base_url IP address is not allowed: {host}"
            )));
        }
    }

    Ok(())
}

fn is_ip_blocked(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => is_ipv4_blocked(v4),
        IpAddr::V6(v6) => is_ipv6_blocked(v6),
    }
}

fn is_ipv4_blocked(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 127.0.0.0/8 — loopback
    if octets[0] == 127 {
        return true;
    }
    // 10.0.0.0/8 — RFC 1918
    if octets[0] == 10 {
        return true;
    }
    // 172.16.0.0/12 — RFC 1918 (172.16.x.x – 172.31.x.x)
    if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
        return true;
    }
    // 192.168.0.0/16 — RFC 1918
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    // 169.254.0.0/16 — link-local / AWS metadata
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    // 0.0.0.0
    if octets == [0, 0, 0, 0] {
        return true;
    }
    false
}

fn is_ipv6_blocked(ip: std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    // ::1 — loopback
    if ip == std::net::Ipv6Addr::LOCALHOST {
        return true;
    }
    // fc00::/7 — unique-local (fc00:: and fd00::)
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // fe80::/10 — link-local
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    false
}

// ── DocSource trait ───────────────────────────────────────────────────────────

#[cfg(feature = "native")]
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

    /// Returns true if the source supports active write operations (create/update/delete).
    fn supports_write(&self) -> bool {
        false
    }

    /// Creates a new document in the target system.
    async fn create_document(
        &self,
        _title: &str,
        _content: &str,
        _folder: Option<&str>,
        _metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<SourceDocId, PluginError> {
        Err(PluginError::Internal(
            "create_document not supported by this plugin".to_string(),
        ))
    }

    /// Updates an existing document. Content and metadata can be updated independently.
    async fn update_document(
        &self,
        _id: &SourceDocId,
        _content: Option<&str>,
        _metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(), PluginError> {
        Err(PluginError::Internal(
            "update_document not supported by this plugin".to_string(),
        ))
    }

    /// Deletes a document from the target system.
    async fn delete_document(&self, _id: &SourceDocId) -> Result<(), PluginError> {
        Err(PluginError::Internal(
            "delete_document not supported by this plugin".to_string(),
        ))
    }

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
    fn raw_document_hash_is_stable() {
        // Same content produces the same content_hash value when computed consistently.
        // RawDocument itself doesn't auto-compute hash; callers set content_hash.
        // We verify that two docs built with identical content strings share the hash.
        let content = "# Hello\nworld".to_string();
        let hash = format!("{:x}", {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            content.hash(&mut h);
            h.finish()
        });
        let doc1 = RawDocument {
            id: SourceDocId("a".into()),
            title: None,
            content: content.clone(),
            content_type: ContentType::Markdown,
            url: None,
            metadata: HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: None,
        };
        let doc2 = RawDocument {
            id: SourceDocId("b".into()),
            title: None,
            content: content.clone(),
            content_type: ContentType::Markdown,
            url: None,
            metadata: HashMap::new(),
            tags: vec![],
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: None,
            relative_path: None,
        };
        // Same content → same hash when computed the same way
        let hash2 = format!("{:x}", {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            doc2.content.hash(&mut h);
            h.finish()
        });
        assert_eq!(hash, hash2);
        assert_eq!(doc1.content, doc2.content);
    }

    #[test]
    fn document_stream_empty() {
        let stream = DocumentStream {
            documents: vec![],
            next_cursor: None,
            estimated_total: None,
        };
        assert!(stream.documents.is_empty());
        assert!(stream.next_cursor.is_none());
    }

    #[test]
    fn plugin_error_display() {
        assert_eq!(
            PluginError::ConfigInvalid("bad field".into()).to_string(),
            "config invalid: bad field"
        );
        assert_eq!(PluginError::AuthRequired.to_string(), "auth required");
        assert_eq!(
            PluginError::NetworkError("timeout".into()).to_string(),
            "network error: timeout"
        );
        assert_eq!(
            PluginError::RateLimited { retry_after_secs: 30 }.to_string(),
            "rate limited, retry after 30s"
        );
    }

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
            aliases: vec![],
            links: vec![],
            created_at: None,
            updated_at: Some(1700000000),
            relative_path: Some("notes/test.md".into()),
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

    // ── validate_base_url tests ───────────────────────────────────────────────

    #[test]
    fn validate_base_url_accepts_public_https() {
        assert!(validate_base_url("https://api.github.com").is_ok());
        assert!(validate_base_url("https://mycompany.atlassian.net").is_ok());
        assert!(validate_base_url("https://example.com/base").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_http() {
        let err = validate_base_url("http://example.com").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn validate_base_url_rejects_non_http_schemes() {
        assert!(matches!(
            validate_base_url("ftp://example.com"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("file:///etc/passwd"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_localhost() {
        assert!(matches!(
            validate_base_url("https://localhost/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://localhost:8080/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_dot_local() {
        assert!(matches!(
            validate_base_url("https://myhost.local/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_loopback_ipv4() {
        assert!(matches!(
            validate_base_url("https://127.0.0.1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://127.255.255.255/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_rfc1918_10_slash_8() {
        assert!(matches!(
            validate_base_url("https://10.0.0.1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://10.255.255.255/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_rfc1918_172_16_slash_12() {
        assert!(matches!(
            validate_base_url("https://172.16.0.1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://172.31.255.255/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        // 172.15.x.x is NOT in the range — should be allowed
        assert!(validate_base_url("https://172.15.0.1/api").is_ok());
        // 172.32.x.x is NOT in the range — should be allowed
        assert!(validate_base_url("https://172.32.0.1/api").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_rfc1918_192_168_slash_16() {
        assert!(matches!(
            validate_base_url("https://192.168.0.1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://192.168.255.255/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_link_local_aws_metadata() {
        assert!(matches!(
            validate_base_url("https://169.254.169.254/latest/meta-data"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://169.254.0.1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_ipv6_loopback() {
        assert!(matches!(
            validate_base_url("https://::1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_ipv6_unique_local() {
        assert!(matches!(
            validate_base_url("https://fc00::1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(matches!(
            validate_base_url("https://fd12:3456:789a::1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }

    #[test]
    fn validate_base_url_rejects_ipv6_link_local() {
        assert!(matches!(
            validate_base_url("https://fe80::1/api"),
            Err(PluginError::PermissionDenied(_))
        ));
    }
}
