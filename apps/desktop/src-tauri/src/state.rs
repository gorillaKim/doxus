use std::path::PathBuf;
use doxus_core::plugin::PluginManager;

pub struct AppState {
    pub plugin_manager: PluginManager,
}

impl AppState {
    pub fn new() -> Self {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let plugins_dir = home.join(".doxus").join("plugins");
        Self {
            plugin_manager: PluginManager::new(plugins_dir),
        }
    }
}
