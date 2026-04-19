/// Settings persistence tests (TDD - RED phase)
/// These tests verify save_settings/load_settings roundtrip via config.toml
use doxus_desktop_lib::commands::settings::{AppSettings, load_settings_from_path, save_settings_to_path};
use tempfile::TempDir;

#[test]
fn save_and_load_settings_roundtrip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");

    let original = AppSettings {
        embedding_model: "ollama".to_string(),
        language: "en".to_string(),
        theme: "dark".to_string(),
        debug_tags: vec![],
        keychain_migrated: false,
    };

    save_settings_to_path(&original, &path).unwrap();
    let loaded = load_settings_from_path(&path).unwrap();

    assert_eq!(loaded.embedding_model, "ollama");
    assert_eq!(loaded.language, "en");
    assert_eq!(loaded.theme, "dark");
}

#[test]
fn load_returns_defaults_when_no_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent_config.toml");

    let defaults = load_settings_from_path(&path).unwrap();

    assert_eq!(defaults.embedding_model, "onnx");
    assert_eq!(defaults.language, "ko");
    assert_eq!(defaults.theme, "system");
}

#[test]
fn save_creates_config_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");

    assert!(!path.exists());

    let settings = AppSettings::default();
    save_settings_to_path(&settings, &path).unwrap();

    assert!(path.exists());
}

#[test]
fn save_rejects_invalid_embedding_model() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");

    let bad = AppSettings {
        embedding_model: "openai-gpt4".to_string(),
        language: "ko".to_string(),
        theme: "system".to_string(),
        debug_tags: vec![],
        keychain_migrated: false,
    };

    let result = save_settings_to_path(&bad, &path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("embedding_model"), "error should mention embedding_model, got: {err}");
}
