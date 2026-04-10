use std::path::PathBuf;

use doxus_plugin_sdk::DocSource;

use crate::marketplace::{
    installer::{InstallerError, PluginInstaller},
    registry::RegistryEntry,
    signing::{verify_plugin, SignedPlugin, SigningError},
};

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("plugin not found: {0}")]
    NotFound(String),
    #[error("signature verification failed: {0}")]
    SignatureInvalid(#[from] SigningError),
    #[error("installer error: {0}")]
    Installer(#[from] InstallerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PluginManager {
    pub plugins_dir: PathBuf,
    installer: PluginInstaller,
}

impl PluginManager {
    pub fn new(plugins_dir: PathBuf) -> Self {
        let installer = PluginInstaller::new(plugins_dir.clone());
        Self { plugins_dir, installer }
    }

    /// Verifies the ED25519 signature before installing.
    /// Also pins the public key against the registry entry to prevent
    /// a plugin from supplying its own untrusted key.
    /// This is the preferred install path — do not bypass.
    pub fn install_signed(
        &self,
        plugin: &SignedPlugin,
        entry: &RegistryEntry,
    ) -> Result<PathBuf, ManagerError> {
        // Pin: registry entry's public_key_hex must match the plugin's public key.
        let entry_key_bytes = hex::decode(&entry.public_key_hex)
            .map_err(|e| SigningError::HexDecode(e.to_string()))?;
        if entry_key_bytes != plugin.public_key {
            return Err(ManagerError::SignatureInvalid(SigningError::InvalidSignature));
        }
        verify_plugin(plugin)?;
        Ok(self.installer.install(entry, &plugin.wasm_bytes)?)
    }

    /// Install without signature verification (for testing / unsigned plugins).
    /// WARNING: Only use when signature is not available (e.g. local dev).
    /// Restricted to crate-internal use to prevent bypassing signature checks in production.
    pub(crate) fn install_from_bytes(
        &self,
        entry: &RegistryEntry,
        wasm_bytes: &[u8],
    ) -> Result<PathBuf, ManagerError> {
        Ok(self.installer.install(entry, wasm_bytes)?)
    }

    pub fn uninstall(&self, plugin_id: &str) -> Result<(), ManagerError> {
        if !self.installer.is_installed(plugin_id) {
            return Err(ManagerError::NotFound(plugin_id.to_string()));
        }
        Ok(self.installer.uninstall(plugin_id)?)
    }

    pub fn is_installed(&self, plugin_id: &str) -> bool {
        self.installer.is_installed(plugin_id)
    }

    /// Returns a boxed `DocSource` for the given `plugin_id`.
    /// Currently supports only the built-in Obsidian plugin.
    /// Returns `None` for unknown or WASM-only plugins.
    pub fn get_source(&self, plugin_id: &str) -> Option<Box<dyn DocSource + Send + Sync>> {
        match plugin_id {
            "com.doxus.obsidian" => {
                Some(Box::new(doxus_plugin_obsidian::ObsidianPlugin::new()))
            }
            _ => None,
        }
    }

    pub fn list_installed(&self) -> Result<Vec<String>, ManagerError> {
        if !self.plugins_dir.exists() {
            return Ok(vec![]);
        }
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.plugins_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::{
        registry::RegistryEntry,
        signing::{sha256_hex, SignedPlugin},
    };
    use doxus_plugin_sdk::{PluginKind, PluginMetadata};
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use tempfile::TempDir;

    fn make_entry(plugin_id: &str, wasm: &[u8]) -> RegistryEntry {
        RegistryEntry {
            plugin_id: plugin_id.to_string(),
            version: "1.0.0".into(),
            display_name: "Test Plugin".into(),
            download_url: "https://example.com/plugin.wasm".into(),
            checksum_sha256: sha256_hex(wasm),
            public_key_hex: "deadbeef".into(),
        }
    }

    /// Build an entry whose public_key_hex matches the given SignedPlugin's public key.
    fn make_pinned_entry(plugin_id: &str, wasm: &[u8], signed: &SignedPlugin) -> RegistryEntry {
        RegistryEntry {
            plugin_id: plugin_id.to_string(),
            version: "1.0.0".into(),
            display_name: "Test Plugin".into(),
            download_url: "https://example.com/plugin.wasm".into(),
            checksum_sha256: sha256_hex(wasm),
            public_key_hex: hex::encode(signed.public_key),
        }
    }

    fn make_signed_plugin(plugin_id: &str, wasm: &[u8]) -> (SignedPlugin, SigningKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let sig = signing_key.sign(wasm);
        let plugin = SignedPlugin {
            manifest: PluginMetadata {
                id: plugin_id.into(),
                name: "Test Plugin".into(),
                version: "1.0.0".into(),
                kind: PluginKind::External,
            },
            wasm_bytes: wasm.to_vec(),
            signature: sig.to_bytes(),
            public_key: signing_key.verifying_key().to_bytes(),
        };
        (plugin, signing_key)
    }

    #[test]
    fn install_signed_verifies_and_installs() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let wasm = b"valid wasm bytes";
        let (signed, _key) = make_signed_plugin("com.test.plugin", wasm);
        let entry = make_pinned_entry("com.test.plugin", wasm, &signed);

        mgr.install_signed(&signed, &entry).unwrap();
        assert!(mgr.is_installed("com.test.plugin"));
    }

    #[test]
    fn install_signed_rejects_tampered_wasm() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let wasm = b"original wasm";
        let (mut signed, _key) = make_signed_plugin("com.test.plugin", wasm);
        // Build entry with the correct key before tampering
        let entry = make_pinned_entry("com.test.plugin", b"tampered", &signed);
        signed.wasm_bytes = b"tampered".to_vec();

        let err = mgr.install_signed(&signed, &entry).unwrap_err();
        assert!(matches!(err, ManagerError::SignatureInvalid(_)));
    }

    #[test]
    fn install_signed_rejects_key_mismatch() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let wasm = b"valid wasm bytes";
        let (signed, _key) = make_signed_plugin("com.test.plugin", wasm);
        // Entry has a different (wrong) public key
        let entry = make_entry("com.test.plugin", wasm); // public_key_hex = "deadbeef"

        let err = mgr.install_signed(&signed, &entry).unwrap_err();
        assert!(matches!(err, ManagerError::SignatureInvalid(_)));
    }

    #[test]
    fn install_and_list() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm bytes";
        let entry = make_entry("com.test.plugin", wasm);

        mgr.install_from_bytes(&entry, wasm).unwrap();
        let list = mgr.list_installed().unwrap();
        assert!(list.contains(&"com.test.plugin".to_string()));
    }

    #[test]
    fn uninstall_removes_from_list() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm bytes";
        let entry = make_entry("com.test.plugin", wasm);

        mgr.install_from_bytes(&entry, wasm).unwrap();
        mgr.uninstall("com.test.plugin").unwrap();
        let list = mgr.list_installed().unwrap();
        assert!(!list.contains(&"com.test.plugin".to_string()));
    }

    #[test]
    fn is_installed_false_before_install() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        assert!(!mgr.is_installed("not-there"));
    }
}
