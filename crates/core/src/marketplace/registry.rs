/// A single entry in the plugin registry.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RegistryEntry {
    pub plugin_id: String,
    pub version: String,
    pub display_name: String,
    pub download_url: String,
    pub checksum_sha256: String,
    pub public_key_hex: String,
}

pub struct RegistryClient {
    pub registry_url: String,
}

impl RegistryClient {
    pub fn new(registry_url: impl Into<String>) -> Self {
        Self { registry_url: registry_url.into() }
    }

    pub fn parse_entries(json: &str) -> Result<Vec<RegistryEntry>, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
