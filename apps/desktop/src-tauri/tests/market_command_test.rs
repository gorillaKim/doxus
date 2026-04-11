/// Integration tests for market commands — Task 8 (TDD Red→Green→Refactor)
///
/// Tests cover:
/// 1. market_fetch_registry returns registry entries via mocked HTTP
/// 2. Trust anchor verification: SignedPlugin.public_key must match RegistryEntry.public_key_hex
/// 3. Untrusted key (not in registry) is rejected
/// 4. Tampered WASM (bad signature) is rejected

use doxus_core::marketplace::registry::{RegistryClient, RegistryEntry};
use doxus_core::marketplace::signing::{
    generate_keypair, sha256_hex, sign_plugin, verify_plugin, SignedPlugin, SigningError,
};
use doxus_plugin_sdk::{PluginKind, PluginMetadata};
use ed25519_dalek::VerifyingKey;

// ── helpers ──────────────────────────────────────────────────────────────────

fn dummy_metadata(id: &str) -> PluginMetadata {
    PluginMetadata {
        id: id.to_string(),
        name: "Test Plugin".to_string(),
        version: "1.0.0".to_string(),
        kind: PluginKind::External,
    }
}

fn make_registry_entry(plugin_id: &str, public_key_hex: &str) -> RegistryEntry {
    RegistryEntry {
        plugin_id: plugin_id.to_string(),
        version: "1.0.0".to_string(),
        display_name: "Test Plugin".to_string(),
        download_url: format!("https://registry.doxus.io/{}.wasm", plugin_id),
        checksum_sha256: "abc123".to_string(),
        public_key_hex: public_key_hex.to_string(),
    }
}

/// Verify that the public key embedded in a `SignedPlugin` matches the
/// `public_key_hex` field of its corresponding `RegistryEntry`.
/// Returns `Ok(())` when the trust anchor check passes.
fn verify_trust_anchor(
    signed: &SignedPlugin,
    entry: &RegistryEntry,
) -> Result<(), String> {
    let expected_bytes =
        hex::decode(&entry.public_key_hex).map_err(|e| format!("hex decode: {e}"))?;
    if expected_bytes.len() != 32 {
        return Err(format!(
            "public_key_hex has wrong length: {} bytes (expected 32)",
            expected_bytes.len()
        ));
    }
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&expected_bytes);
    let registry_vk = VerifyingKey::from_bytes(&key_arr)
        .map_err(|e| format!("invalid registry public key: {e}"))?;

    if registry_vk.to_bytes() != signed.public_key {
        return Err("trust anchor mismatch: plugin key not in registry".to_string());
    }
    Ok(())
}

// ── test 1: fetch_registry returns entries ───────────────────────────────────

#[tokio::test]
async fn market_fetch_registry_returns_entries() {
    let mut server = mockito::Server::new_async().await;
    let body = serde_json::json!([{
        "plugin_id": "com.doxus.confluence",
        "version": "1.2.0",
        "display_name": "Confluence",
        "download_url": "https://registry.doxus.io/confluence-1.2.0.wasm",
        "checksum_sha256": "deadbeef0000000000000000000000000000000000000000000000000000dead",
        "public_key_hex": "0000000000000000000000000000000000000000000000000000000000000001"
    }]);
    let _mock = server
        .mock("GET", "/plugins.json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body.to_string())
        .create_async()
        .await;

    let client = RegistryClient::new(server.url()).unwrap();
    let entries = client.fetch_entries().await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].plugin_id, "com.doxus.confluence");
    assert_eq!(entries[0].version, "1.2.0");
}

// ── test 2: install verifies signature against registry key ──────────────────

#[test]
fn market_install_verifies_signature_against_registry_key() {
    let wasm = b"valid wasm bytes for test";
    let (signing_key, verifying_key) = generate_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let metadata = dummy_metadata("com.test.plugin");
    let signed = sign_plugin(wasm, &metadata, &signing_key);

    let entry = make_registry_entry("com.test.plugin", &public_key_hex);

    // Trust anchor check must pass
    assert!(
        verify_trust_anchor(&signed, &entry).is_ok(),
        "trust anchor should pass when keys match"
    );
    // Signature must also be valid
    assert!(
        verify_plugin(&signed).is_ok(),
        "plugin signature should be valid"
    );
}

// ── test 3: untrusted key is rejected ────────────────────────────────────────

#[test]
fn market_install_rejects_untrusted_key() {
    let wasm = b"valid wasm bytes for test";
    // Attacker generates their own key pair
    let (attacker_signing_key, _attacker_vk) = generate_keypair();
    // Registry contains a *different* trusted key
    let (_trusted_signing_key, trusted_vk) = generate_keypair();
    let trusted_key_hex = hex::encode(trusted_vk.to_bytes());

    let metadata = dummy_metadata("com.test.plugin");
    // Plugin is signed with attacker's key, not the trusted registry key
    let signed = sign_plugin(wasm, &metadata, &attacker_signing_key);

    let entry = make_registry_entry("com.test.plugin", &trusted_key_hex);

    // Trust anchor check must fail
    let result = verify_trust_anchor(&signed, &entry);
    assert!(
        result.is_err(),
        "plugin signed with untrusted key should be rejected"
    );
    assert!(
        result.unwrap_err().contains("trust anchor mismatch"),
        "error should mention trust anchor"
    );
}

// ── test 4: tampered WASM is rejected ────────────────────────────────────────

#[test]
fn market_install_rejects_tampered_wasm() {
    let original_wasm = b"original wasm content";
    let (signing_key, verifying_key) = generate_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let metadata = dummy_metadata("com.test.plugin");
    let mut signed = sign_plugin(original_wasm, &metadata, &signing_key);

    // Tamper with WASM after signing
    signed.wasm_bytes = b"tampered wasm content".to_vec();

    let entry = make_registry_entry("com.test.plugin", &public_key_hex);

    // Trust anchor check passes (key is correct)
    assert!(
        verify_trust_anchor(&signed, &entry).is_ok(),
        "trust anchor check should still pass (key is correct)"
    );
    // But signature verification must fail
    let result = verify_plugin(&signed);
    assert!(
        result.is_err(),
        "tampered WASM should fail signature verification"
    );
    assert!(
        matches!(result.unwrap_err(), SigningError::InvalidSignature),
        "error should be InvalidSignature"
    );
}
