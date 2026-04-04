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
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
        let path = self.plugins_dir.join(format!("{plugin_id}.wasm"));
        std::fs::remove_file(path)?;
        Ok(())
    }

    pub fn is_installed(&self, plugin_id: &str) -> bool {
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
}
