use std::path::{Path, PathBuf};
use std::sync::Mutex;

use reqwest::redirect::Policy;

use crate::marketplace::registry::RegistryEntry;
use crate::marketplace::signing::sha256_hex;

pub struct PluginInstaller {
    plugins_dir: PathBuf,
    allow_file_scheme: bool,
    http_client: Mutex<Option<reqwest::blocking::Client>>,
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
fn is_private_ip(addr: std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // 100.64.0.0/10 (CGNAT)
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // unique local fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // IPv4-mapped ::ffff:0:0/96 — check mapped v4 part
                || v6.to_ipv4_mapped().map(|v4| {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                }).unwrap_or(false)
        }
    }
}

fn validate_redirect_destination(url: &url::Url) -> Result<(), InstallerError> {
    let host = url.host_str().unwrap_or("");
    if let Ok(addr) = host.parse::<std::net::IpAddr>() {
        if is_private_ip(addr) {
            return Err(InstallerError::Download(
                format!("redirect to private IP rejected: {}", host),
            ));
        }
    }
    Ok(())
}

pub fn validate_plugin_id(plugin_id: &str) -> Result<(), InstallerError> {
    if plugin_id.is_empty()
        || !plugin_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(InstallerError::InvalidPluginId(plugin_id.to_string()));
    }
    Ok(())
}

fn atomic_write(dest: &std::path::Path, bytes: &[u8]) -> Result<(), InstallerError> {
    let tmp = dest.with_extension("wasm.tmp");
    std::fs::write(&tmp, bytes).map_err(InstallerError::Io)?;
    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp); // best-effort cleanup
        return Err(InstallerError::Io(e));
    }
    Ok(())
}

impl PluginInstaller {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir, allow_file_scheme: false, http_client: Mutex::new(None) }
    }

    pub fn new_with_file_scheme(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir, allow_file_scheme: true, http_client: Mutex::new(None) }
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    fn with_http_client<F, T>(&self, f: F) -> Result<T, InstallerError>
    where
        F: FnOnce(&reqwest::blocking::Client) -> Result<T, InstallerError>,
    {
        let mut guard = self
            .http_client
            .lock()
            .map_err(|e| InstallerError::Download(format!("http client lock poisoned: {e}")))?;
        if guard.is_none() {
            let client = reqwest::blocking::Client::builder()
                .redirect(Policy::limited(1))
                .build()
                .map_err(|e| InstallerError::Download(format!("failed to build HTTP client: {e}")))?;
            *guard = Some(client);
        }
        // SAFETY: we just set `Some` above if it was `None`
        f(guard.as_ref().expect("client was just initialized"))
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
        atomic_write(&dest, wasm_bytes)?;
        Ok(dest)
    }

    /// Download a WASM plugin from `url` and save it as `{plugin_id}.wasm` in `plugins_dir`.
    ///
    /// Allowed schemes: `https://`, `http://`. `file://` is permitted only when
    /// `allow_file_scheme` is set to `true` on the `PluginInstaller` instance
    /// (use [`PluginInstaller::new_with_file_scheme`] — intended for tests and local development only).
    /// Any other scheme (ftp://, data://, etc.) is rejected.
    /// Downloads are capped at 50 MB.
    /// HTTP redirects are not followed (SSRF prevention).
    pub fn install_from_url(
        &self,
        plugin_id: &str,
        url: &str,
        expected_sha256: Option<&str>,
    ) -> Result<PathBuf, InstallerError> {
        validate_plugin_id(plugin_id)?;

        const MAX_PLUGIN_SIZE: u64 = 50 * 1024 * 1024;

        let parsed = url::Url::parse(url)
            .map_err(|e| InstallerError::InvalidUrl(e.to_string()))?;

        let wasm_bytes: Vec<u8> = match parsed.scheme() {
            "http" | "https" => {
                self.with_http_client(|client| {
                    let resp = client
                        .get(url)
                        .send()
                        .map_err(|e| InstallerError::Download(e.to_string()))?;

                    // Validate final URL after any redirect to prevent SSRF.
                    // Only check when the host changed (i.e., a cross-host redirect occurred).
                    {
                        let original_host = parsed.host_str().unwrap_or("");
                        let final_host = resp.url().host_str().unwrap_or("");
                        if final_host != original_host {
                            validate_redirect_destination(resp.url())?;
                        }
                    }

                    if !resp.status().is_success() {
                        return Err(InstallerError::Download(
                            format!("HTTP {}: download failed", resp.status())
                        ));
                    }

                    if let Some(len) = resp.content_length() {
                        if len > MAX_PLUGIN_SIZE {
                            return Err(InstallerError::Download(format!(
                                "plugin file too large: {} bytes (max 50MB)", len
                            )));
                        }
                    }

                    let bytes = resp.bytes()
                        .map_err(|e| InstallerError::Download(e.to_string()))?;

                    if bytes.len() as u64 > MAX_PLUGIN_SIZE {
                        return Err(InstallerError::Download("plugin file exceeds 50MB limit".into()));
                    }

                    Ok(bytes.to_vec())
                })?
            }
            "file" => {
                // file:// is only allowed when allow_file_scheme is set on the installer
                // (used in tests and local development; rejected in production deployments)
                if !self.allow_file_scheme {
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

        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(&wasm_bytes);
            if actual != expected {
                return Err(InstallerError::ChecksumMismatch {
                    expected: expected.to_string(),
                    actual,
                });
            }
        }

        std::fs::create_dir_all(&self.plugins_dir)?;
        let dest = self.plugins_dir.join(format!("{plugin_id}.wasm"));
        atomic_write(&dest, &wasm_bytes)?;
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
            auth_type: "none".into(),
            guide_url: String::new(),
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
    fn install_from_url_with_allow_file_flag_true_installs() {
        let tmp = TempDir::new().unwrap();
        let wasm_src = tmp.path().join("test.wasm");
        std::fs::write(&wasm_src, b"\x00asm\x01\x00\x00\x00").unwrap();
        let url = url::Url::from_file_path(&wasm_src).unwrap();

        let installer = PluginInstaller::new_with_file_scheme(tmp.path().to_path_buf());
        let result = installer.install_from_url("com.test.plugin", url.as_str(), None);
        assert!(result.is_ok(), "allow_file_scheme=true should install from file://");
    }

    #[test]
    fn install_from_url_with_allow_file_flag_false_rejects_file_scheme() {
        let tmp = TempDir::new().unwrap();
        let wasm_src = tmp.path().join("test.wasm");
        std::fs::write(&wasm_src, b"\x00asm\x01\x00\x00\x00").unwrap();
        let url = url::Url::from_file_path(&wasm_src).unwrap();

        let installer = PluginInstaller::new(tmp.path().to_path_buf()); // default: allow_file_scheme=false
        let err = installer.install_from_url("com.test.plugin", url.as_str(), None).unwrap_err();
        assert!(matches!(err, InstallerError::InvalidUrl(_)), "allow_file_scheme=false should reject file://");
    }

    #[test]
    fn install_uses_atomic_write_no_tmp_file_remains() {
        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let wasm = b"fake wasm";
        let checksum = sha256_hex(wasm);
        let entry = make_entry(&checksum);

        installer.install(&entry, wasm).unwrap();

        // .wasm file must exist
        assert!(tmp.path().join("com.test.plugin.wasm").exists());
        // .wasm.tmp file must NOT remain after successful install
        assert!(
            !tmp.path().join("com.test.plugin.wasm.tmp").exists(),
            ".tmp file should not remain after install"
        );
    }

    #[test]
    fn install_from_url_rejects_redirect() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/plugin.wasm")
            .with_status(302)
            .with_header("location", "http://169.254.169.254/latest/meta-data/")
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server.url());
        let result = installer.install_from_url("com.test.plugin", &url, None);

        assert!(result.is_err(), "redirect to private IP should be rejected");
    }

    #[test]
    fn install_from_url_succeeds_on_200_with_wasm_bytes() {
        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/plugin.wasm")
            .with_status(200)
            .with_header("content-type", "application/wasm")
            .with_body(wasm_bytes)
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server.url());
        let result = installer.install_from_url("com.test.plugin", &url, None);

        assert!(result.is_ok(), "200 response should succeed: {:?}", result.err());
        assert!(tmp.path().join("com.test.plugin.wasm").exists());
    }

    #[test]
    fn install_from_url_rejects_oversized_file() {
        let tmp_dir = TempDir::new().unwrap();
        let installer = PluginInstaller::new_with_file_scheme(tmp_dir.path().to_path_buf());

        // Create a file larger than 50MB
        let oversized_path = tmp_dir.path().join("oversized.wasm");
        let oversized_bytes = vec![0u8; 51 * 1024 * 1024];
        std::fs::write(&oversized_path, &oversized_bytes).unwrap();

        let url = url::Url::from_file_path(&oversized_path).unwrap();
        let result = installer.install_from_url("com.test.plugin", url.as_str(), None);

        match result {
            Err(InstallerError::Download(msg)) => {
                assert!(msg.contains("50MB"), "expected 50MB error, got: {msg}");
            }
            other => panic!("expected Download error, got: {other:?}"),
        }
    }

    #[test]
    fn install_from_url_rejects_bad_checksum() {
        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/plugin.wasm")
            .with_status(200)
            .with_body(wasm_bytes)
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server.url());
        let err = installer.install_from_url("com.test.plugin", &url, Some("0000000000000000000000000000000000000000000000000000000000000000")).unwrap_err();
        assert!(matches!(err, InstallerError::ChecksumMismatch { .. }), "expected ChecksumMismatch, got: {err:?}");
    }

    #[test]
    fn install_from_url_accepts_correct_checksum() {
        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let checksum = sha256_hex(wasm_bytes);
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/plugin.wasm")
            .with_status(200)
            .with_body(wasm_bytes)
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server.url());
        let result = installer.install_from_url("com.test.plugin", &url, Some(&checksum));
        assert!(result.is_ok(), "correct checksum should succeed: {:?}", result.err());
    }

    #[test]
    fn install_from_url_none_checksum_skips_verification() {
        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/plugin.wasm")
            .with_status(200)
            .with_body(wasm_bytes)
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server.url());
        let result = installer.install_from_url("com.test.plugin", &url, None);
        assert!(result.is_ok(), "None checksum should skip verification: {:?}", result.err());
    }

    #[test]
    fn install_from_url_follows_single_redirect_to_cdn() {
        // Server B: serves the actual WASM bytes
        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let mut server_b = mockito::Server::new();
        let _mock_b = server_b
            .mock("GET", "/plugin.wasm")
            .with_status(200)
            .with_header("content-type", "application/wasm")
            .with_body(wasm_bytes)
            .create();

        // Server A: 302 → Server B (same host = localhost, so SSRF check does not block)
        let mut server_a = mockito::Server::new();
        let redirect_url = format!("{}/plugin.wasm", server_b.url());
        let _mock_a = server_a
            .mock("GET", "/plugin.wasm")
            .with_status(302)
            .with_header("location", &redirect_url)
            .create();

        let tmp = TempDir::new().unwrap();
        let installer = PluginInstaller::new(tmp.path().to_path_buf());
        let url = format!("{}/plugin.wasm", server_a.url());
        let result = installer.install_from_url("com.test.plugin", &url, None);

        assert!(result.is_ok(), "single redirect to CDN (localhost) should succeed: {:?}", result.err());
        assert!(tmp.path().join("com.test.plugin.wasm").exists());
    }

    #[test]
    fn validate_redirect_destination_rejects_private_ip() {
        let private_addrs = [
            "http://192.168.1.1/evil.wasm",
            "http://10.0.0.1/evil.wasm",
            "http://172.16.0.1/evil.wasm",
            "http://127.0.0.1/evil.wasm",
        ];
        for addr in &private_addrs {
            let url = url::Url::parse(addr).unwrap();
            let result = validate_redirect_destination(&url);
            assert!(result.is_err(), "expected SSRF rejection for {addr}: got Ok");
            match result.unwrap_err() {
                InstallerError::Download(msg) => {
                    assert!(msg.contains("redirect"), "expected 'redirect' in error for {addr}, got: {msg}");
                }
                other => panic!("expected Download error for {addr}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn is_private_ip_identifies_rfc1918_and_loopback() {
        use std::net::IpAddr;
        assert!(is_private_ip("10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip("127.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip("0.0.0.0".parse::<IpAddr>().unwrap()));
        assert!(!is_private_ip("8.8.8.8".parse::<IpAddr>().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse::<IpAddr>().unwrap()));
    }
}
