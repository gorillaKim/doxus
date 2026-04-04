use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

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
