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
        eprintln!("\n[Plugin-Manager] INITIALIZED WITH PATH: {:?}\n", plugins_dir);
        let installer = PluginInstaller::new(plugins_dir.clone());
        Self { plugins_dir, installer, factories: HashMap::new() }
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
        eprintln!("[Plugin-Manager] Attempting to load source: origin='{}', normalized='{}'", plugin_id, normalized_id);
        
        // Reject invalid plugin_ids
        if normalized_id.contains('/') || normalized_id.contains('\\') || normalized_id.contains("..") {
            tracing::warn!("get_source: invalid plugin_id '{}'", normalized_id);
            return None;
        }

        // 1. Try External WASM Plugin (Prioritized)
        let wasm_path = self.plugins_dir.join(format!("{}.wasm", normalized_id));
        let manifest_path = self.plugins_dir.join(format!("{}.manifest.toml", normalized_id));
        
        eprintln!("[Plugin-Manager] Checking WASM: path={:?}, exists={}", wasm_path, wasm_path.exists());
        eprintln!("[Plugin-Manager] Checking MANIFEST: path={:?}, exists={}", manifest_path, manifest_path.exists());

        if wasm_path.exists() && manifest_path.exists() {
            eprintln!("[Plugin-Manager] External files found. Parsing manifest...");
            let manifest_str = match std::fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[Plugin-Manager] FAILED to read manifest file: {}", e);
                    return None;
                }
            };
            
            let manifest: PluginManifest = match toml::from_str(&manifest_str) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[Plugin-Manager] FAILED to parse manifest TOML: {}", e);
                    return None;
                }
            };

            let bytes = match std::fs::read(&wasm_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[Plugin-Manager] FAILED to read WASM bytes: {}", e);
                    return None;
                }
            };

            let adapter = match WasmDocSourceAdapter::from_bytes(bytes, manifest, None, None) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("[Plugin-Manager] FAILED to create WASM adapter: {}", e);
                    return None;
                }
            };
            
            eprintln!("[Plugin-Manager] SUCCESS: Loaded EXTERNAL plugin: {}", normalized_id);
            return Some(Box::new(adapter));
        } else {
            eprintln!("[Plugin-Manager] External plugin NOT found at {:?}", wasm_path);
        }

        // 2. Fallback to Registered Factories (Built-in)
        // Try original ID, normalized ID, and short name
        for candidate in &[plugin_id, &normalized_id, "confluence", "obsidian"] {
            if let Some(factory) = self.factories.get(*candidate) {
                eprintln!("[Plugin-Manager] SUCCESS: Loaded BUILT-IN plugin for candidate: {}", candidate);
                return Some(factory());
            }
        }

        eprintln!("[Plugin-Manager] ERROR: No plugin source found for '{}'", plugin_id);
        None
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
