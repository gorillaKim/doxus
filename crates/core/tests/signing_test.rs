use doxus_core::marketplace::signing::{
    generate_keypair, load_keypair, save_keypair, sign_plugin, verify_plugin,
};
use doxus_plugin_sdk::{PluginKind, PluginMetadata};
use tempfile::tempdir;

fn dummy_manifest() -> PluginMetadata {
    PluginMetadata {
        id: "com.test.signing".into(),
        name: "Signing Test Plugin".into(),
        version: "1.0.0".into(),
        kind: PluginKind::External,
    }
}

#[test]
fn sign_and_verify_roundtrip() {
    let (signing_key, _verifying_key) = generate_keypair();
    let wasm_bytes = b"fake wasm bytes for signing test".to_vec();
    let manifest = dummy_manifest();

    let signed = sign_plugin(&wasm_bytes, &manifest, &signing_key);
    assert!(verify_plugin(&signed).is_ok());
}

#[test]
fn generate_keypair_produces_valid_pair() {
    let (signing_key, verifying_key) = generate_keypair();
    let wasm_bytes = b"another wasm payload".to_vec();
    let manifest = dummy_manifest();

    let signed = sign_plugin(&wasm_bytes, &manifest, &signing_key);
    // verifying_key returned from generate_keypair must match the one embedded in signed
    assert_eq!(signed.public_key, verifying_key.to_bytes() as [u8; 32]);
    assert!(verify_plugin(&signed).is_ok());
}

#[test]
fn tampered_wasm_fails_verification() {
    let (signing_key, _) = generate_keypair();
    let wasm_bytes = b"original wasm content".to_vec();
    let manifest = dummy_manifest();

    let mut signed = sign_plugin(&wasm_bytes, &manifest, &signing_key);
    // Tamper with wasm bytes after signing
    signed.wasm_bytes = b"tampered wasm content".to_vec();

    assert!(verify_plugin(&signed).is_err());
}

#[test]
fn keypair_save_load_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let key_path = dir.path().join("signing.key");

    let (signing_key, _) = generate_keypair();
    save_keypair(&key_path, &signing_key).expect("save_keypair");

    let loaded_key = load_keypair(&key_path).expect("load_keypair");

    // Signing with loaded key must produce verifiable output
    let wasm_bytes = b"wasm bytes after key reload".to_vec();
    let manifest = dummy_manifest();
    let signed = sign_plugin(&wasm_bytes, &manifest, &loaded_key);
    assert!(verify_plugin(&signed).is_ok());
}
