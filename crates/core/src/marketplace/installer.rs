use std::path::PathBuf;

use crate::marketplace::registry::RegistryEntry;
use crate::marketplace::signing::sha256_hex;

pub struct PluginInstaller {
    pub plugins_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallerError {
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("invalid plugin id: {0}")]
    InvalidPluginId(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Validates that a plugin_id contains only safe characters (alphanumeric, dots, hyphens,
/// underscores). Prevents path traversal attacks when plugin_id is used in file paths.
fn validate_plugin_id(plugin_id: &str) -> Result<(), InstallerError> {
    if plugin_id.is_empty()
        || !plugin_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(InstallerError::InvalidPluginId(plugin_id.to_string()));
    }
    Ok(())
}

impl PluginInstaller {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    pub fn install(
        &self,
        entry: &RegistryEntry,
        wasm_bytes: &[u8],
    ) -> Result<PathBuf, InstallerError> {
        validate_plugin_id(&entry.plugin_id)?;
        let actual = sha256_hex(wasm_bytes);
        if actual != entry.checksum_sha256 {
            return Err(InstallerError::ChecksumMismatch {
                expected: entry.checksum_sha256.clone(),
                actual,
            });
        }
        std::fs::create_dir_all(&self.plugins_dir)?;
        let dest = self.plugins_dir.join(format!("{}.wasm", entry.plugin_id));
        std::fs::write(&dest, wasm_bytes)?;
        Ok(dest)
    }

    pub fn uninstall(&self, plugin_id: &str) -> Result<(), InstallerError> {
        validate_plugin_id(plugin_id)?;
        let path = self.plugins_dir.join(format!("{plugin_id}.wasm"));
        std::fs::remove_file(path)?;
        Ok(())
    }

    pub fn is_installed(&self, plugin_id: &str) -> bool {
        if validate_plugin_id(plugin_id).is_err() {
            return false;
        }
        self.plugins_dir.join(format!("{plugin_id}.wasm")).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::signing::sha256_hex;
    use tempfile::TempDir;

    fn make_entry(checksum: &str) -> RegistryEntry {
        RegistryEntry {
            plugin_id: "com.test.plugin".into(),
            version: "1.0.0".into(),
            display_name: "Test Plugin".into(),
            download_url: "https://example.com/plugin.wasm".into(),
            checksum_sha256: checksum.into(),
            public_key_hex: "deadbeef".into(),
        }
    }

    #[test]
    fn install_writes_wasm_file_to_plugins_dir() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm content";
        let checksum = sha256_hex(wasm);
        let entry = make_entry(&checksum);

        let path = installer.install(&entry, wasm).unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), wasm);
    }

    #[test]
    fn install_rejects_bad_checksum() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm content";
        let entry = make_entry("0000000000000000000000000000000000000000000000000000000000000000");

        let err = installer.install(&entry, wasm).unwrap_err();
        assert!(matches!(err, InstallerError::ChecksumMismatch { .. }));
    }

    #[test]
    fn install_rejects_path_traversal_plugin_id() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm";
        let checksum = sha256_hex(wasm);
        let mut entry = make_entry(&checksum);
        entry.plugin_id = "../../evil".to_string();
        let err = installer.install(&entry, wasm).unwrap_err();
        assert!(matches!(err, InstallerError::InvalidPluginId(_)));
    }

    #[test]
    fn uninstall_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let err = installer.uninstall("../../etc/passwd").unwrap_err();
        assert!(matches!(err, InstallerError::InvalidPluginId(_)));
    }

    #[test]
    fn uninstall_removes_file() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm";
        let checksum = sha256_hex(wasm);
        let entry = make_entry(&checksum);

        installer.install(&entry, wasm).unwrap();
        assert!(installer.is_installed("com.test.plugin"));

        installer.uninstall("com.test.plugin").unwrap();
        assert!(!installer.is_installed("com.test.plugin"));
    }

    #[test]
    fn is_installed_returns_false_for_missing() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        assert!(!installer.is_installed("com.nonexistent.plugin"));
    }

    #[test]
    fn is_installed_returns_false_for_traversal_attempt() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        assert!(!installer.is_installed("../../evil"));
    }
}
