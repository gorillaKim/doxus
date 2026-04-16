use doxus_plugin_sdk::{PluginConfig, PluginSecrets, SecretValue};

/// 시스템 키체인(keyring)에서 인증 정보를 로드하여 설정과 시크릿에 주입한다.
pub fn inject_keychain_auth(plugin_id: &str, config: &mut PluginConfig, secrets: &mut PluginSecrets) {
    match plugin_id {
        "com.doxus.confluence" => {
            // 1. API Token 로드
            if let Ok(entry) = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:api_token") {
                if let Ok(token) = entry.get_password() {
                    secrets.fields.insert("api_token".to_string(), SecretValue::Text(token.clone()));
                    config.fields.insert("api_token".to_string(), serde_json::json!(token));
                }
            }
            // 2. Email 로드 (컨플루언스 Basic Auth 필수 항목)
            if let Ok(entry) = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:email") {
                if let Ok(email) = entry.get_password() {
                    config.fields.insert("email".to_string(), serde_json::json!(email));
                    tracing::info!("Loaded email and token from keychain for Confluence");
                }
            }
        },
        "com.doxus.github" => {
            if let Ok(entry) = keyring::Entry::new("doxus", "doxus:com.doxus.github:token") {
                if let Ok(token) = entry.get_password() {
                    secrets.fields.insert("token".to_string(), SecretValue::Text(token));
                }
            }
        },
        _ => {}
    }
}
