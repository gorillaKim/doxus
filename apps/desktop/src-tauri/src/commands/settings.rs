use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const ALLOWED_EMBEDDING_MODELS: &[&str] = &["onnx", "ollama"];
const ALLOWED_LANGUAGES: &[&str] = &["ko", "en"];
const ALLOWED_THEMES: &[&str] = &["light", "dark", "system"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub embedding_model: String,
    pub language: String,
    pub theme: String,
    pub debug_tags: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            embedding_model: "onnx".to_string(),
            language: "ko".to_string(),
            theme: "system".to_string(),
            debug_tags: vec![],
        }
    }
}

fn validate(settings: &AppSettings) -> Result<(), String> {
    if !ALLOWED_EMBEDDING_MODELS.contains(&settings.embedding_model.as_str()) {
        return Err(format!(
            "embedding_model must be one of {:?}, got '{}'",
            ALLOWED_EMBEDDING_MODELS, settings.embedding_model
        ));
    }
    if !ALLOWED_LANGUAGES.contains(&settings.language.as_str()) {
        return Err(format!(
            "language must be one of {:?}, got '{}'",
            ALLOWED_LANGUAGES, settings.language
        ));
    }
    if !ALLOWED_THEMES.contains(&settings.theme.as_str()) {
        return Err(format!(
            "theme must be one of {:?}, got '{}'",
            ALLOWED_THEMES, settings.theme
        ));
    }
    Ok(())
}

/// Save settings to an arbitrary path (used in tests and by the Tauri command).
pub fn save_settings_to_path(settings: &AppSettings, path: &Path) -> Result<(), String> {
    validate(settings)?;
    let toml_str = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, toml_str).map_err(|e| e.to_string())
}

/// Load settings from an arbitrary path. Returns defaults when the file does not exist.
pub fn load_settings_from_path(path: &Path) -> Result<AppSettings, String> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&content).map_err(|e| e.to_string())
}

fn config_path(_app_handle: &AppHandle) -> PathBuf {
    // Prefer DOXUS_CONFIG_PATH override (useful for tests that run in Tauri context).
    if let Ok(p) = std::env::var("DOXUS_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".doxus/config.toml")
}

#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    app_handle: AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let path = config_path(&app_handle);
    doxus_core::observability::set_debug_tags(settings.debug_tags.clone());
    state.sidecar.set_debug(doxus_core::observability::is_debug_enabled("agent"));
    save_settings_to_path(&settings, &path)
}

#[tauri::command]
pub async fn load_settings(app_handle: AppHandle) -> Result<AppSettings, String> {
    let path = config_path(&app_handle);
    load_settings_from_path(&path)
}
