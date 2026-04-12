use std::path::PathBuf;

use crate::marketplace::{
    installer::{InstallerError, PluginInstaller},
    registry::{RegistryClient, RegistryEntry, RegistryError},
    signing::sha256_hex,
};

#[derive(Debug, thiserror::Error)]
pub enum PluginRegistryError {
    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),
    #[error("network error: {0}")]
    Network(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("installer error: {0}")]
    Installer(#[from] InstallerError),
}

pub struct PluginRegistry {
    client: RegistryClient,
    installer: PluginInstaller,
    http: reqwest::Client,
}

impl PluginRegistry {
    pub fn new(base_url: &str, plugins_dir: PathBuf) -> Result<Self, PluginRegistryError> {
        let client = RegistryClient::new(base_url)?;
        let installer = PluginInstaller::new(plugins_dir);
        let http = reqwest::ClientBuilder::new()
            .user_agent("doxus-plugin-registry/0.1.0")
            .build()
            .map_err(|e| PluginRegistryError::Network(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { client, installer, http })
    }

    /// Fetch the list of available plugins from the registry.
    pub async fn list_plugins(&self) -> Result<Vec<RegistryEntry>, PluginRegistryError> {
        Ok(self.client.fetch_entries().await?)
    }

    /// Download the WASM binary for `entry`, verify its SHA-256 checksum, and install it to
    /// `plugins_dir/{plugin_id}.wasm`. Returns the installed path on success.
    /// On checksum mismatch the partially-written file is removed before returning the error.
    pub async fn download_and_install(
        &self,
        entry: &RegistryEntry,
    ) -> Result<PathBuf, PluginRegistryError> {
        let resp = self
            .http
            .get(&entry.download_url)
            .send()
            .await
            .map_err(|e| PluginRegistryError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(PluginRegistryError::Network(format!(
                "HTTP {} downloading plugin {}",
                resp.status(),
                entry.plugin_id
            )));
        }

        let wasm_bytes = resp
            .bytes()
            .await
            .map_err(|e| PluginRegistryError::Network(e.to_string()))?;

        let actual = sha256_hex(&wasm_bytes);
        if actual != entry.checksum_sha256 {
            return Err(PluginRegistryError::ChecksumMismatch {
                expected: entry.checksum_sha256.clone(),
                actual,
            });
        }

        Ok(self.installer.install(entry, &wasm_bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::signing::sha256_hex;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn make_registry(server: &MockServer, tmp: &TempDir) -> PluginRegistry {
        PluginRegistry::new(&server.uri(), tmp.path().to_path_buf()).unwrap()
    }

    fn make_entry(server_uri: &str, plugin_id: &str, wasm: &[u8]) -> RegistryEntry {
        RegistryEntry {
            plugin_id: plugin_id.to_string(),
            version: "1.0.0".into(),
            display_name: "Test Plugin".into(),
            download_url: format!("{}/{}.wasm", server_uri, plugin_id),
            checksum_sha256: sha256_hex(wasm),
            public_key_hex: "deadbeef".into(),
            auth_type: "none".into(),
            guide_url: String::new(),
        }
    }

    #[tokio::test]
    async fn list_plugins_returns_entries_from_registry() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        let body = serde_json::json!([{
            "plugin_id": "com.doxus.confluence",
            "version": "1.0.0",
            "display_name": "Confluence",
            "download_url": "https://example.com/confluence.wasm",
            "checksum_sha256": "abc123",
            "public_key_hex": "deadbeef"
        }]);
        Mock::given(method("GET"))
            .and(path("/plugins.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let registry = make_registry(&server, &tmp).await;
        let entries = registry.list_plugins().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].plugin_id, "com.doxus.confluence");
    }

    #[tokio::test]
    async fn download_and_install_writes_wasm_to_plugins_dir() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        let wasm = b"fake wasm bytes for confluence";
        let entry = make_entry(&server.uri(), "com.doxus.confluence", wasm);

        Mock::given(method("GET"))
            .and(path("/com.doxus.confluence.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm.as_ref()))
            .mount(&server)
            .await;

        let registry = make_registry(&server, &tmp).await;
        let installed_path = registry.download_and_install(&entry).await.unwrap();

        assert!(installed_path.exists());
        assert_eq!(std::fs::read(&installed_path).unwrap(), wasm);
        assert_eq!(
            installed_path.file_name().unwrap().to_str().unwrap(),
            "com.doxus.confluence.wasm"
        );
    }

    #[tokio::test]
    async fn download_and_install_rejects_checksum_mismatch_and_does_not_install() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        let wasm = b"correct wasm bytes";
        let tampered = b"tampered wasm bytes";

        // Entry has checksum of `wasm` but server returns `tampered`
        let entry = make_entry(&server.uri(), "com.doxus.confluence", wasm);

        Mock::given(method("GET"))
            .and(path("/com.doxus.confluence.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tampered.as_ref()))
            .mount(&server)
            .await;

        let registry = make_registry(&server, &tmp).await;
        let err = registry.download_and_install(&entry).await.unwrap_err();

        assert!(
            matches!(err, PluginRegistryError::ChecksumMismatch { .. }),
            "expected ChecksumMismatch, got: {err}"
        );
        // File must not be present after mismatch
        let dest = tmp.path().join("com.doxus.confluence.wasm");
        assert!(!dest.exists(), "partially-written file should be removed on mismatch");
    }

    #[tokio::test]
    async fn download_and_install_returns_error_on_http_failure() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        let wasm = b"wasm bytes";
        let entry = make_entry(&server.uri(), "com.doxus.confluence", wasm);

        Mock::given(method("GET"))
            .and(path("/com.doxus.confluence.wasm"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let registry = make_registry(&server, &tmp).await;
        let err = registry.download_and_install(&entry).await.unwrap_err();
        assert!(matches!(err, PluginRegistryError::Network(_)));
    }
}
