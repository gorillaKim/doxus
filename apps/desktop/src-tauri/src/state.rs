use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use rusqlite::Connection;
use doxus_core::plugin::PluginManager;

pub struct OAuthPending {
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub struct AppState {
    pub conn: Mutex<Connection>,
    pub plugin_manager: PluginManager,
    pub plugins_dir: PathBuf,
    pub oauth_pending: Mutex<HashMap<String, OAuthPending>>, // key = plugin_id
}

impl AppState {
    pub fn new(conn: Connection, plugins_dir: PathBuf) -> Self {
        Self {
            conn: Mutex::new(conn),
            plugin_manager: PluginManager::new(plugins_dir.clone()),
            plugins_dir,
            oauth_pending: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_creates_with_in_memory_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        let state = AppState::new(conn, PathBuf::from("/tmp"));
        // lock works
        let _guard = state.conn.lock().unwrap();
        drop(_guard);
        // oauth_pending is empty initially
        let pending = state.oauth_pending.lock().unwrap();
        assert!(pending.is_empty());
    }
}
