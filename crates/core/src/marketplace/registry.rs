use semver::{Version, VersionReq};

/// Returns true if `candidate` satisfies the semver `requirement`.
///
/// `requirement` may be an exact version (`"1.0.0"`) or a range (`"^1.2.0"`, `"~1.2.0"`).
pub fn matches_version(requirement: &str, candidate: &str) -> Result<bool, RegistryError> {
    // If requirement starts with a digit, treat it as an exact match (prefix with `=`).
    let normalized = if requirement.starts_with(|c: char| c.is_ascii_digit()) {
        format!("={requirement}")
    } else {
        requirement.to_string()
    };
    let req = VersionReq::parse(&normalized)
        .map_err(|e| RegistryError::Parse(format!("invalid version requirement '{requirement}': {e}")))?;
    let ver = Version::parse(candidate)
        .map_err(|e| RegistryError::Parse(format!("invalid version '{candidate}': {e}")))?;
    Ok(req.matches(&ver))
}

/// Returns the entry with the highest version that satisfies `requirement`, or `None`.
///
/// Uses the same bare-version normalization as [`matches_version`]: a
/// requirement that starts with a digit is treated as an exact match (`=X.Y.Z`).
pub fn find_best_match<'a>(
    entries: &'a [RegistryEntry],
    requirement: &str,
) -> Result<Option<&'a RegistryEntry>, RegistryError> {
    let normalized = if requirement.starts_with(|c: char| c.is_ascii_digit()) {
        format!("={requirement}")
    } else {
        requirement.to_string()
    };
    let req = VersionReq::parse(&normalized)
        .map_err(|e| RegistryError::Parse(format!("invalid version requirement '{requirement}': {e}")))?;
    Ok(entries
        .iter()
        .filter_map(|e| {
            Version::parse(&e.version)
                .ok()
                .filter(|v| req.matches(v))
                .map(|v| (v, e))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, e)| e))
}

/// A single entry in the plugin registry.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RegistryEntry {
    pub plugin_id: String,
    pub version: String,
    pub display_name: String,
    pub download_url: String,
    pub checksum_sha256: String,
    pub public_key_hex: String,
    /// Authentication type: "none" | "api_token" | "oauth"
    #[serde(default)]
    pub auth_type: String,
    /// URL to the plugin's guide markdown file
    #[serde(default)]
    pub guide_url: String,
}

/// Errors from registry operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub struct RegistryClient {
    base_url: String,
    client: reqwest::Client,
}

impl RegistryClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self, RegistryError> {
        let client = reqwest::ClientBuilder::new()
            .user_agent("doxus-registry-client/0.1.0")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| RegistryError::Network(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Fetch plugin registry entries from the registry server.
    pub async fn fetch_entries(&self) -> Result<Vec<RegistryEntry>, RegistryError> {
        let url = format!("{}/plugins.json", self.base_url);
        let resp = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Network(format!("HTTP {}", resp.status())));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| RegistryError::Network(e.to_string()))?;
        Self::parse_entries(&text)
    }

    /// Fetch a single registry entry by plugin_id.
    pub async fn fetch_entry(
        &self,
        plugin_id: &str,
    ) -> Result<Option<RegistryEntry>, RegistryError> {
        let entries = self.fetch_entries().await?;
        Ok(entries.into_iter().find(|e| e.plugin_id == plugin_id))
    }

    pub fn parse_entries(json: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        serde_json::from_str(json).map_err(|e| RegistryError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_json() -> &'static str {
        r#"[
            {
                "plugin_id": "com.doxus.confluence",
                "version": "1.0.0",
                "display_name": "Confluence",
                "download_url": "https://registry.doxus.io/confluence-1.0.0.wasm",
                "checksum_sha256": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                "public_key_hex": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
            }
        ]"#
    }

    #[test]
    fn parse_entries_valid_json() {
        let entries = RegistryClient::parse_entries(sample_json()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].plugin_id, "com.doxus.confluence");
        assert_eq!(entries[0].version, "1.0.0");
    }

    #[test]
    fn parse_entries_empty_array() {
        let entries = RegistryClient::parse_entries("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_entries_invalid_json() {
        let result = RegistryClient::parse_entries("not json at all");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_entries_parses_registry_json() {
        let server = MockServer::start().await;
        let body = serde_json::json!([{
            "plugin_id": "com.test.plugin",
            "version": "1.0.0",
            "display_name": "Test Plugin",
            "download_url": "https://example.com/plugin.wasm",
            "checksum_sha256": "abc123",
            "public_key_hex": "deadbeef"
        }]);
        Mock::given(method("GET"))
            .and(path("/plugins.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = RegistryClient::new(server.uri()).unwrap();
        let entries = client.fetch_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].plugin_id, "com.test.plugin");
    }

    #[tokio::test]
    async fn fetch_entries_returns_error_on_http_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let client = RegistryClient::new(server.uri()).unwrap();
        let result = client.fetch_entries().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_entries_trims_trailing_slash_from_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/plugins.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let url_with_slash = format!("{}/", server.uri());
        let client = RegistryClient::new(url_with_slash).unwrap();
        let entries = client.fetch_entries().await.unwrap();
        assert!(entries.is_empty());
    }
}
