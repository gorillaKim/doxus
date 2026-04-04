use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    pub plugin_id: String,
    pub display_name: String,
    pub version: String,
    pub abi_version: u32,
    pub http_domains: Vec<String>,
    pub kv_namespaces: Vec<String>,
}

impl PluginManifest {
    pub fn is_domain_allowed(&self, url: &str) -> bool {
        if self.http_domains.is_empty() {
            return false;
        }
        self.http_domains.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix("*.") {
                url.contains(suffix)
            } else {
                url.contains(pattern.as_str())
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
}
