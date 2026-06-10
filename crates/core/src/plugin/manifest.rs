use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    pub plugin_id: String,
    pub display_name: String,
    pub version: String,
    pub abi_version: u32,
    pub http_domains: Vec<String>,
    pub kv_namespaces: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
}

impl PluginManifest {
    pub fn from_file(path: &std::path::Path) -> Result<Self, ManifestError> {
        let content =
            std::fs::read_to_string(path).map_err(|e| ManifestError::Io(e.to_string()))?;
        toml::from_str(&content).map_err(|e| ManifestError::Parse(e.to_string()))
    }

    pub fn is_domain_allowed(&self, url: &str) -> bool {
        if self.http_domains.is_empty() {
            return false;
        }
        let host = match Url::parse(url) {
            Ok(u) => u.host_str().unwrap_or("").to_lowercase(),
            Err(_) => return false,
        };
        self.http_domains.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix("*.") {
                let suffix_lc = suffix.to_lowercase();
                host.ends_with(&format!(".{suffix_lc}"))
            } else {
                host == pattern.to_lowercase()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_domains(domains: Vec<&str>) -> PluginManifest {
        PluginManifest {
            plugin_id: "com.test".into(),
            display_name: "Test".into(),
            version: "1.0.0".into(),
            abi_version: 1,
            http_domains: domains.into_iter().map(String::from).collect(),
            kv_namespaces: vec![],
            secrets: vec![],
        }
    }

    #[test]
    fn domain_wildcard_matches() {
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(m.is_domain_allowed("https://foo.atlassian.net/wiki"));
    }

    #[test]
    fn domain_exact_matches() {
        let m = manifest_with_domains(vec!["example.com"]);
        assert!(m.is_domain_allowed("https://example.com/api"));
    }

    #[test]
    fn empty_domains_denies_all() {
        let m = manifest_with_domains(vec![]);
        assert!(!m.is_domain_allowed("https://example.com"));
    }

    #[test]
    fn unlisted_domain_denied() {
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(!m.is_domain_allowed("https://evil.com/steal"));
    }

    #[test]
    fn query_param_bypass_denied() {
        // SSRF bypass: domain in query param should NOT be allowed
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(!m.is_domain_allowed("https://evil.com/?x=foo.atlassian.net"));
    }

    #[test]
    fn path_bypass_denied() {
        // SSRF bypass: domain in path should NOT be allowed
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(!m.is_domain_allowed("https://evil.com/foo.atlassian.net/"));
    }

    #[test]
    fn subdomain_suffix_bypass_denied() {
        // bypass: evil.com subdomain that ends with atlassian.net should NOT match
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(!m.is_domain_allowed("https://foo.atlassian.net.evil.com/"));
    }

    #[test]
    fn wildcard_requires_subdomain() {
        // bare domain itself should NOT match wildcard (host must have .suffix, not == suffix)
        let m = manifest_with_domains(vec!["*.atlassian.net"]);
        assert!(!m.is_domain_allowed("https://atlassian.net/"));
    }

    #[test]
    fn exact_match_no_substring() {
        // "notexample.com" should NOT match exact pattern "example.com"
        let m = manifest_with_domains(vec!["example.com"]);
        assert!(!m.is_domain_allowed("https://notexample.com/"));
    }
}
