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
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("download failed: {0}")]
    Download(String),
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

    /// Download a WASM plugin from `url` and save it as `{plugin_id}.wasm` in `plugins_dir`.
    ///
    /// Allowed schemes: `https://`, `http://`. `file://` is permitted only in test builds.
    /// Any other scheme (ftp://, data://, etc.) is rejected.
    /// Downloads are capped at 50 MB.
    pub fn install_from_url(
        &self,
        plugin_id: &str,
        url: &str,
    ) -> Result<PathBuf, InstallerError> {
        validate_plugin_id(plugin_id)?;

        const MAX_PLUGIN_SIZE: u64 = 50 * 1024 * 1024;

        let parsed = url::Url::parse(url)
            .map_err(|e| InstallerError::InvalidUrl(e.to_string()))?;

        let wasm_bytes: Vec<u8> = match parsed.scheme() {
            "https" | "http" => {
                let url_owned = url.to_string();
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| InstallerError::Download(e.to_string()))?
                    .block_on(async move {
                        let resp = reqwest::get(&url_owned)
                            .await
                            .map_err(|e: reqwest::Error| InstallerError::Download(e.to_string()))?;
                        if let Some(len) = resp.content_length() {
                            if len > MAX_PLUGIN_SIZE {
                                return Err(InstallerError::Download(format!(
                                    "plugin file too large: {} bytes (max 50MB)",
                                    len
                                )));
                            }
                        }
                        let bytes = resp
                            .bytes()
                            .await
                            .map_err(|e: reqwest::Error| InstallerError::Download(e.to_string()))?;
                        if bytes.len() as u64 > MAX_PLUGIN_SIZE {
                            return Err(InstallerError::Download(
                                "plugin file exceeds 50MB limit".into(),
                            ));
                        }
                        Ok::<Vec<u8>, InstallerError>(bytes.to_vec())
                    })?
            }
            "file" => {
                // file:// is only allowed when DOXUS_ALLOW_FILE_INSTALL env var is set
                // (used in tests and local development; rejected in production deployments)
                if std::env::var("DOXUS_ALLOW_FILE_INSTALL").is_err() {
                    return Err(InstallerError::InvalidUrl(
                        "file:// scheme is not allowed in production".into(),
                    ));
                }
                {
                    let path = parsed
                        .to_file_path()
                        .map_err(|_| InstallerError::InvalidUrl("cannot convert file:// to path".into()))?;
                    let meta = std::fs::metadata(&path)?;
                    if meta.len() > MAX_PLUGIN_SIZE {
                        return Err(InstallerError::Download(format!(
                            "plugin file too large: {} bytes (max 50MB)",
                            meta.len()
                        )));
                    }
                    std::fs::read(path)?
                }
            }
            other => {
                return Err(InstallerError::InvalidUrl(format!(
                    "unsupported scheme '{}': only http, https, file are allowed",
                    other
                )));
            }
        };

        std::fs::create_dir_all(&self.plugins_dir)?;
        let dest = self.plugins_dir.join(format!("{plugin_id}.wasm"));
        let tmp = dest.with_extension("wasm.tmp");
        std::fs::write(&tmp, &wasm_bytes).map_err(InstallerError::Io)?;
        std::fs::rename(&tmp, &dest).map_err(InstallerError::Io)?;
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

    #[test]
    fn install_from_url_rejects_oversized_file() {
        std::env::set_var("DOXUS_ALLOW_FILE_INSTALL", "1");
        let tmp_dir = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp_dir.path().to_path_buf());

        // Create a file larger than 50MB
        let oversized_path = tmp_dir.path().join("oversized.wasm");
        let oversized_bytes = vec![0u8; 51 * 1024 * 1024];
        std::fs::write(&oversized_path, &oversized_bytes).unwrap();

        let url = url::Url::from_file_path(&oversized_path).unwrap();
        let result = installer.install_from_url("com.test.plugin", url.as_str());

        match result {
            Err(InstallerError::Download(msg)) => {
                assert!(msg.contains("50MB"), "expected 50MB error, got: {msg}");
            }
            other => panic!("expected Download error, got: {other:?}"),
        }
    }
}
