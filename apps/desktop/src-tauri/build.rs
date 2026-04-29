fn main() {
    // Guard: prevent accidental release with an unsigned updater.
    // Run `cargo tauri signer generate -w ~/.tauri/doxus.key` and replace the
    // placeholder in tauri.conf.json before cutting a release tag.
    let conf = std::fs::read_to_string("tauri.conf.json").unwrap_or_default();
    if conf.contains("PLACEHOLDER_ED25519_PUBLIC_KEY") {
        // Allow dev builds but fail release builds.
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!(
                "Release build detected with placeholder updater pubkey. \
                 Replace PLACEHOLDER_ED25519_PUBLIC_KEY in tauri.conf.json \
                 with the real ED25519 public key before shipping."
            );
        }
    }

    tauri_build::build()
}
