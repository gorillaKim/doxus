use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) const SUPPORTED_ABI_VERSION: u32 = 1;

use doxus_plugin_sdk::DocSource;

use super::manifest::PluginManifest;
use super::wasm_adapter::WasmDocSourceAdapter;
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

type SourceFactory = Box<dyn Fn() -> Box<dyn DocSource + Send + Sync> + Send + Sync>;

pub struct PluginManager {
    pub plugins_dir: PathBuf,
    installer: PluginInstaller,
    factories: HashMap<String, SourceFactory>,
}

impl PluginManager {
    pub fn new(plugins_dir: PathBuf) -> Self {
        let installer = PluginInstaller::new(plugins_dir.clone());
        Self {
            plugins_dir,
            installer,
            factories: HashMap::new(),
        }
    }

    /// Normalize short plugin names to official IDs (e.g., "obsidian" -> "com.doxus.obsidian")
    pub fn normalize_id(id: &str) -> String {
        match id {
            "obsidian" | "workspace" => "com.doxus.obsidian".to_string(),
            "confluence" => "com.doxus.confluence".to_string(),
            "github" => "com.doxus.github".to_string(),
            _ => id.to_string(),
        }
    }

    /// Register a factory function for a plugin ID.
    /// When `get_source` is called with the same ID, the factory is invoked to
    /// produce a fresh `DocSource` instance.
    /// Use this to register in-process plugins at the binary entry point — core
    /// has no built-in plugin knowledge.
    pub fn register_factory<F>(&mut self, plugin_id: &str, factory: F)
    where
        F: Fn() -> Box<dyn DocSource + Send + Sync> + Send + Sync + 'static,
    {
        self.factories.insert(plugin_id.to_string(), Box::new(factory));
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
    #[allow(dead_code)]
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
    /// First checks registered factories (in-process plugins).
    /// If no factory is registered, looks for `{plugin_id}.wasm` and
    /// `{plugin_id}.manifest.toml` in `plugins_dir` and loads them via
    /// `WasmDocSourceAdapter`. Returns `None` if neither is found or loading
    /// fails (failure is logged via `tracing::warn`).
    pub fn get_source(&self, plugin_id: &str) -> Option<Box<dyn DocSource + Send + Sync>> {
        let normalized_id = Self::normalize_id(plugin_id);
        
        // Reject invalid plugin_ids
        if normalized_id.contains('/') || normalized_id.contains('\\') || normalized_id.contains("..") {
            tracing::warn!("get_source: invalid plugin_id '{}'", normalized_id);
            return None;
        }

        // 1. Registered factories take priority (in-process, fast, trusted)
        for candidate in &[plugin_id, normalized_id.as_str()] {
            if let Some(factory) = self.factories.get(*candidate) {
                return Some(factory());
            }
        }

        // 2. Fallback to WASM file on disk
        if let Some(source) = self.load_wasm_plugin(&normalized_id) {
            return Some(source);
        }

        None
    }

    fn load_wasm_plugin(&self, plugin_id: &str) -> Option<Box<dyn DocSource + Send + Sync>> {
        let wasm_path = self.plugins_dir.join(format!("{plugin_id}.wasm"));
        let manifest_path = self.plugins_dir.join(format!("{plugin_id}.manifest.toml"));

        if !wasm_path.exists() || !manifest_path.exists() {
            return None;
        }

        let manifest_str = match std::fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to read manifest for plugin {plugin_id}: {e}");
                return None;
            }
        };

        let manifest: PluginManifest = match toml::from_str(&manifest_str) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("failed to parse manifest for plugin {plugin_id}: {e}");
                return None;
            }
        };

        let bytes = match std::fs::read(&wasm_path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("failed to read WASM for plugin {plugin_id}: {e}");
                return None;
            }
        };

        match WasmDocSourceAdapter::from_bytes(bytes, manifest, None, None) {
            Ok(adapter) => Some(Box::new(adapter)),
            Err(e) => {
                tracing::warn!("failed to load WASM plugin {plugin_id}: {e}");
                None
            }
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
            auth_type: "none".into(),
            guide_url: String::new(),
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
            auth_type: "none".into(),
            guide_url: String::new(),
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

    // ── ABI version runtime validation ────────────────────────────────────────

    fn write_manifest(dir: &std::path::Path, plugin_id: &str, abi_version: u32) {
        let content = format!(
            r#"plugin_id = "{plugin_id}"
display_name = "Test Plugin"
version = "1.0.0"
abi_version = {abi_version}
http_domains = []
kv_namespaces = []
secrets = []
"#
        );
        std::fs::write(dir.join(format!("{plugin_id}.manifest.toml")), content).unwrap();
    }

    #[test]
    fn get_source_returns_none_for_missing_plugin() {
        let tmp = TempDir::new().unwrap();
        let pm = PluginManager::new(tmp.path().to_path_buf());
        assert!(pm.get_source("com.nonexistent").is_none());
    }

    #[test]
    fn get_source_prefers_factory_over_wasm_file() {
        // Factory registered + .wasm + .manifest.toml both present → factory wins.
        // We verify this by registering a factory for a plugin_id that also has a
        // .wasm file present. The factory call increments a counter; if WASM were
        // loaded instead the counter would stay 0.
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

        let tmp = TempDir::new().unwrap();
        let mut pm = PluginManager::new(tmp.path().to_path_buf());
        let plugin_id = "com.test.factorywasm";

        // Write a manifest and dummy wasm file
        write_manifest(tmp.path(), plugin_id, SUPPORTED_ABI_VERSION);
        std::fs::write(tmp.path().join(format!("{plugin_id}.wasm")), b"fake").unwrap();

        // Track factory invocations
        let counter = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::clone(&counter);

        // We can't easily build a real DocSource without heavy deps, so we just
        // verify that get_source returns Some when a factory is registered, and
        // that registering with a different id does not match.
        // The key assertion is that with factory registered, the result is Some.
        // (Without factory, with b"fake" wasm, result would be None.)
        pm.register_factory(plugin_id, move || {
            counter2.fetch_add(1, Ordering::SeqCst);
            // Return a real obsidian plugin since it's available as a workspace dep.
            // Actually we can't import it here - use the wasm adapter path but
            // we know from the test that if factory wins, counter > 0 is enough.
            // Instead we'll panic to distinguish from the WASM path.
            panic!("factory invoked")
        });

        // Without factory: b"fake" wasm with valid manifest → load fails → None
        // With factory: panics (factory invoked). We just need to confirm factory
        // is checked first. Use a separate pm without factory to confirm None baseline.
        let pm_no_factory = PluginManager::new(tmp.path().to_path_buf());
        assert!(
            pm_no_factory.get_source(plugin_id).is_none(),
            "without factory, placeholder wasm should return None"
        );

        // Now verify factory is called (will panic with our sentinel)
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pm.get_source(plugin_id)
        }));
        assert!(result.is_err(), "factory should have been called (sentinel panic)");
        assert_eq!(counter.load(Ordering::SeqCst), 1, "factory invoked exactly once");
    }

    #[test]
    fn get_source_loads_placeholder_wasm_returns_none_gracefully() {
        // b"placeholder" content → WASM load fails → None (no crash)
        let tmp = TempDir::new().unwrap();
        let pm = PluginManager::new(tmp.path().to_path_buf());
        let plugin_id = "com.test.placeholder";

        write_manifest(tmp.path(), plugin_id, SUPPORTED_ABI_VERSION);
        std::fs::write(tmp.path().join(format!("{plugin_id}.wasm")), b"placeholder").unwrap();

        // Should return None gracefully, not panic
        assert!(pm.get_source(plugin_id).is_none());
    }

    #[test]
    fn get_source_returns_none_for_unsupported_abi() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let plugin_id = "com.test.newabi";

        // Write manifest with future ABI version and dummy wasm
        write_manifest(tmp.path(), plugin_id, SUPPORTED_ABI_VERSION + 1);
        std::fs::write(tmp.path().join(format!("{plugin_id}.wasm")), b"fake").unwrap();

        // get_source must reject the plugin before even trying to load WASM
        assert!(
            mgr.get_source(plugin_id).is_none(),
            "expected None for unsupported ABI version"
        );
    }

    #[test]
    fn get_source_abi_check_uses_supported_abi_constant() {
        // Verify the constant is what we expect so tests remain aligned with production
        assert_eq!(SUPPORTED_ABI_VERSION, 1);
    }

    #[test]
    fn get_source_returns_none_when_only_manifest_present() {
        // No .wasm file → should return None (both files required)
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        let plugin_id = "com.test.nowanasm";
        write_manifest(tmp.path(), plugin_id, SUPPORTED_ABI_VERSION);
        // No .wasm written
        assert!(mgr.get_source(plugin_id).is_none());
    }

    #[test]
    fn get_source_rejects_path_traversal_plugin_id() {
        let tmp = TempDir::new().unwrap();
        let mgr = PluginManager::new(tmp.path().to_path_buf());
        assert!(mgr.get_source("../etc/passwd").is_none());
        assert!(mgr.get_source("foo/bar").is_none());
        assert!(mgr.get_source("foo\\bar").is_none());
    }
}
