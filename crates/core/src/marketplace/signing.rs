use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use std::path::Path;

use doxus_plugin_sdk::PluginMetadata;

#[derive(Debug, Clone)]
pub struct SignedPlugin {
    pub manifest: PluginMetadata,
    pub wasm_bytes: Vec<u8>,
    pub signature: [u8; 64],
    pub public_key: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("invalid signature")]
    InvalidSignature,
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("hex decode error: {0}")]
    HexDecode(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid key bytes")]
    InvalidKey,
}

/// Generate a new ed25519 keypair using OS randomness.
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// Sign a WASM plugin, returning a `SignedPlugin` ready for verification.
pub fn sign_plugin(
    wasm_bytes: &[u8],
    manifest: &PluginMetadata,
    signing_key: &SigningKey,
) -> SignedPlugin {
    let signature = signing_key.sign(wasm_bytes);
    SignedPlugin {
        manifest: manifest.clone(),
        wasm_bytes: wasm_bytes.to_vec(),
        signature: signature.to_bytes(),
        public_key: signing_key.verifying_key().to_bytes(),
    }
}

/// Persist a signing key as raw 32-byte seed to `path`.
///
/// On Unix the file is created with mode 0o600 (owner read/write only)
/// to prevent private key exposure.
pub fn save_keypair(path: &Path, signing_key: &SigningKey) -> Result<(), SigningError> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(&signing_key.to_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, signing_key.to_bytes())?;
    }
    Ok(())
}

/// Load a signing key from a raw 32-byte seed file at `path`.
pub fn load_keypair(path: &Path) -> Result<SigningKey, SigningError> {
    let bytes = std::fs::read(path)?;
    let seed: [u8; 32] = bytes.try_into().map_err(|_| SigningError::InvalidKey)?;
    Ok(SigningKey::from_bytes(&seed))
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn verify_plugin(plugin: &SignedPlugin) -> Result<(), SigningError> {
    let verifying_key = VerifyingKey::from_bytes(&plugin.public_key)
        .map_err(|_| SigningError::InvalidSignature)?;
    let signature = Signature::from_bytes(&plugin.signature);
    verifying_key
        .verify(&plugin.wasm_bytes, &signature)
        .map_err(|_| SigningError::InvalidSignature)
}

/// Verify a plugin signature against a **trusted** registry public key.
///
/// Unlike [`verify_plugin`], this function rejects the plugin if its embedded
/// `public_key` does not match `trusted_public_key` — preventing a malicious
/// plugin from substituting its own key.
pub fn verify_plugin_with_anchor(
    plugin: &SignedPlugin,
    trusted_public_key: &[u8; 32],
) -> Result<(), SigningError> {
    if &plugin.public_key != trusted_public_key {
        return Err(SigningError::InvalidSignature);
    }
    verify_plugin(plugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use doxus_plugin_sdk::PluginKind;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn dummy_manifest() -> PluginMetadata {
        PluginMetadata {
            id: "com.test.plugin".into(),
            name: "Test Plugin".into(),
            version: "1.0.0".into(),
            kind: PluginKind::External,
        }
    }

    fn make_signed_plugin(wasm_bytes: Vec<u8>) -> (SignedPlugin, SigningKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let signature = signing_key.sign(&wasm_bytes);
        let plugin = SignedPlugin {
            manifest: dummy_manifest(),
            wasm_bytes,
            signature: signature.to_bytes(),
            public_key: signing_key.verifying_key().to_bytes(),
        };
        (plugin, signing_key)
    }

    #[test]
    fn verify_plugin_accepts_valid_signature() {
        let (plugin, _) = make_signed_plugin(b"valid wasm bytes".to_vec());
        assert!(verify_plugin(&plugin).is_ok());
    }

    #[test]
    fn verify_plugin_rejects_bad_signature() {
        let (mut plugin, _) = make_signed_plugin(b"original bytes".to_vec());
        // Tamper with wasm_bytes after signing
        plugin.wasm_bytes = b"tampered bytes".to_vec();
        assert!(matches!(verify_plugin(&plugin), Err(SigningError::InvalidSignature)));
    }

    #[test]
    fn sha256_hex_is_deterministic() {
        let data = b"hello doxus";
        let h1 = sha256_hex(data);
        let h2 = sha256_hex(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // hex of 32 bytes
    }
}
