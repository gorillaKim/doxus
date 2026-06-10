use doxus_core::marketplace::registry::{
    find_best_match, matches_version, RegistryEntry, RegistryError,
};

fn make_entry(version: &str) -> RegistryEntry {
    RegistryEntry {
        plugin_id: "com.test.plugin".to_string(),
        version: version.to_string(),
        display_name: "Test".to_string(),
        download_url: "https://example.com/plugin.wasm".to_string(),
        checksum_sha256: "abc".to_string(),
        public_key_hex: "def".to_string(),
        auth_type: "none".to_string(),
        guide_url: String::new(),
    }
}

#[test]
fn exact_version_match() {
    assert_eq!(matches_version("1.0.0", "1.0.0").unwrap(), true);
    assert_eq!(matches_version("1.0.0", "1.0.1").unwrap(), false);
}

#[test]
fn caret_range_matches() {
    assert_eq!(matches_version("^1.2.0", "1.3.5").unwrap(), true);
    assert_eq!(matches_version("^1.2.0", "2.0.0").unwrap(), false);
}

#[test]
fn tilde_range_matches() {
    assert_eq!(matches_version("~1.2.0", "1.2.5").unwrap(), true);
    assert_eq!(matches_version("~1.2.0", "1.3.0").unwrap(), false);
}

#[test]
fn find_best_match_selects_highest_compatible() {
    let entries = vec![
        make_entry("1.0.0"),
        make_entry("1.2.0"),
        make_entry("1.5.0"),
        make_entry("2.0.0"),
    ];
    let best = find_best_match(&entries, "^1.0.0").unwrap().unwrap();
    assert_eq!(best.version, "1.5.0");
}

#[test]
fn invalid_version_string_returns_error() {
    let result = matches_version("not-a-version", "1.0.0");
    assert!(matches!(result, Err(RegistryError::Parse(_))));
}
