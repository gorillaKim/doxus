use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use crate::secrets::SecretStore;

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("secret not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("token endpoint returned error: {error} — {error_description}")]
    TokenEndpoint {
        error: String,
        error_description: String,
    },
    #[error("state mismatch: expected {expected}, got {got}")]
    StateMismatch { expected: String, got: String },
    #[error("secret store error: {0}")]
    SecretStore(String),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
}

// ── Secret store implementations ──────────────────────────────────────────────

/// Keyring-backed implementation that uses UnifiedKeychainStore.
pub struct KeyringSecretStore {
    inner: crate::secrets::UnifiedKeychainStore,
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        let store = crate::secrets::UnifiedKeychainStore::new("doxus", "com.doxus.secrets.v1");
        let _ = store.load_from_keychain();
        Self { inner: store }
    }
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, service: &str, account: &str) -> Result<String, crate::secrets::SecretsError> {
        self.inner
            .get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), crate::secrets::SecretsError> {
        self.inner
            .set(service, account, secret)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), crate::secrets::SecretsError> {
        self.inner
            .delete(service, account)
    }
}

// ── OAuth 2.0 Types ───────────────────────────────────────────────────────────

/// Configuration for an OAuth 2.0 Authorization Code flow client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Full URL of the authorization endpoint (e.g. `https://auth.example.com/oauth/authorize`)
    pub auth_url: String,
    /// Full URL of the token endpoint (e.g. `https://auth.example.com/oauth/token`)
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

/// Returned by `OAuthFlow::authorization_url` — caller must redirect user here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    /// Full URL the user should open in their browser.
    pub url: String,
    /// CSRF state value — must be passed back to `exchange_code`.
    pub state: String,
}

/// Tokens returned after a successful exchange or refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) when the access token expires, if known.
    pub expires_at: Option<u64>,
}

impl OAuthToken {
    /// Returns `true` when the token is known to be expired (with a 30-second buffer).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            None => false,
            Some(exp) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                now + 30 >= exp
            }
        }
    }
}

// ── Wire types for the token endpoint response ────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

// ── OAuthFlow ─────────────────────────────────────────────────────────────────

/// Stateless helper that drives the OAuth 2.0 Authorization Code flow.
pub struct OAuthFlow {
    config: OAuthConfig,
    http_client: reqwest::Client,
}

impl OAuthFlow {
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }

    /// Builds the authorization URL the user must visit.
    ///
    /// `state` should be a securely-generated random string used to prevent CSRF.
    pub fn authorization_url(&self, state: &str) -> Result<String, OAuthError> {
        let scopes = self.config.scopes.join(" ");
        let mut url = url::Url::parse(&self.config.auth_url)
            .map_err(|e| OAuthError::InvalidUrl(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", &scopes)
            .append_pair("state", state);
        Ok(url.to_string())
    }

    /// Exchanges an authorization `code` for tokens.
    ///
    /// `expected_state` must match the `state` returned by the authorization server.
    pub async fn exchange_code(
        &self,
        code: &str,
        received_state: &str,
        expected_state: &str,
    ) -> Result<OAuthToken, OAuthError> {
        if received_state != expected_state {
            return Err(OAuthError::StateMismatch {
                expected: expected_state.to_string(),
                got: received_state.to_string(),
            });
        }

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.config.redirect_uri),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
        ];

        let resp: TokenResponse = self
            .http_client
            .post(&self.config.token_url)
            .form(&params)
            .send()
            .await?
            .json()
            .await?;

        Self::token_from_response(resp)
    }

    /// Uses a refresh token to obtain a new access token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthToken, OAuthError> {
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
        ];

        let resp: TokenResponse = self
            .http_client
            .post(&self.config.token_url)
            .form(&params)
            .send()
            .await?
            .json()
            .await?;

        Self::token_from_response(resp)
    }

    /// Returns `true` when the token is known to be expired (delegates to `OAuthToken::is_expired`).
    pub fn is_expired(token: &OAuthToken) -> bool {
        token.is_expired()
    }

    fn token_from_response(resp: TokenResponse) -> Result<OAuthToken, OAuthError> {
        if let Some(error) = resp.error {
            return Err(OAuthError::TokenEndpoint {
                error,
                error_description: resp.error_description.unwrap_or_default(),
            });
        }

        let access_token = resp
            .access_token
            .ok_or_else(|| OAuthError::MissingField("access_token".into()))?;

        let expires_at = resp.expires_in.map(|secs| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                + secs
        });

        Ok(OAuthToken {
            access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;

    fn make_flow(token_url: &str) -> OAuthFlow {
        OAuthFlow::new(OAuthConfig {
            client_id: "client123".into(),
            client_secret: "secret456".into(),
            auth_url: "https://auth.example.com/oauth/authorize".into(),
            token_url: token_url.to_string(),
            redirect_uri: "https://app.example.com/callback".into(),
            scopes: vec!["read:content".into(), "write:content".into()],
        })
    }

    // ── MemorySecretStore ─────────────────────────────────────────────────────

    #[test]
    fn memory_store_set_get() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "s3cr3t").unwrap();
        let val = store.get("svc", "acct").unwrap();
        assert_eq!(val, "s3cr3t");
    }

    #[test]
    fn memory_store_not_found() {
        let store = MemorySecretStore::new();
        let result = store.get("svc", "missing");
        assert!(matches!(result, Err(crate::secrets::SecretsError::NotFound(_))));
    }

    #[test]
    fn memory_store_delete() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "val").unwrap();
        store.delete("svc", "acct").unwrap();
        let result = store.get("svc", "acct");
        assert!(matches!(result, Err(crate::secrets::SecretsError::NotFound(_))));
    }

    #[test]
    fn memory_store_overwrite() {
        let store = MemorySecretStore::new();
        store.set("svc", "acct", "first").unwrap();
        store.set("svc", "acct", "second").unwrap();
        let val = store.get("svc", "acct").unwrap();
        assert_eq!(val, "second");
    }

    // ── authorization_url ────────────────────────────────────────────────────

    #[test]
    fn authorization_url_contains_required_params() {
        let flow = make_flow("https://auth.example.com/oauth/token");
        let url = flow.authorization_url("my-state-42").unwrap();

        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("state=my-state-42"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope="));
    }

    #[test]
    fn authorization_url_encodes_scopes() {
        let flow = make_flow("https://auth.example.com/oauth/token");
        let url = flow.authorization_url("s").unwrap();
        // space between scopes should be percent-encoded or present in URL
        assert!(url.contains("read%3Acontent") || url.contains("read:content"));
    }

    #[test]
    fn authorization_url_invalid_auth_url_returns_error() {
        let flow = OAuthFlow::new(OAuthConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: "not a valid url !!!".into(),
            token_url: "https://auth.example.com/oauth/token".into(),
            redirect_uri: "https://app.example.com/callback".into(),
            scopes: vec![],
        });
        let err = flow.authorization_url("state").unwrap_err();
        assert!(matches!(err, OAuthError::InvalidUrl(_)));
    }

    // ── state mismatch ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn exchange_code_rejects_state_mismatch() {
        let flow = make_flow("https://auth.example.com/oauth/token");
        let err = flow
            .exchange_code("code123", "bad-state", "expected-state")
            .await
            .unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch { .. }));
    }

    // ── is_expired ───────────────────────────────────────────────────────────

    #[test]
    fn is_expired_returns_false_when_no_expiry() {
        let token = OAuthToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: None,
        };
        assert!(!OAuthFlow::is_expired(&token));
    }

    #[test]
    fn is_expired_returns_true_for_past_timestamp() {
        let token = OAuthToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: Some(1), // 1970-01-01 — definitely expired
        };
        assert!(OAuthFlow::is_expired(&token));
    }

    #[test]
    fn is_expired_returns_false_for_future_timestamp() {
        let far_future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let token = OAuthToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: Some(far_future),
        };
        assert!(!OAuthFlow::is_expired(&token));
    }
}

// ── Keychain Auth Injection ──────────────────────────────────────────────────

/// 시스템 키체인(keyring)에서 인증 정보를 로드하여 플러그인 설정과 시크릿에 주입합니다.
/// MCP 서버와 데스크톱 앱에서 공통으로 사용됩니다.
pub fn inject_keychain_auth(
    plugin_id: &str,
    config: &mut doxus_plugin_sdk::PluginConfig,
    secrets: &mut doxus_plugin_sdk::PluginSecrets,
) {
    let store = crate::secrets::UnifiedKeychainStore::new("doxus", "com.doxus.secrets.v1");
    let _ = store.load_from_keychain();

    match plugin_id {
        "com.doxus.confluence" => {
            // 1. API Token 로드
            if let Ok(token) = store.get(plugin_id, "api_token") {
                tracing::info!("[Auth] Loaded api_token for {} from unified store", plugin_id);
                secrets
                    .fields
                    .insert("api_token".to_string(), doxus_plugin_sdk::SecretValue::Text(token.clone()));
                config
                    .fields
                    .insert("api_token".to_string(), serde_json::json!(token));
            }
            // 2. Email 로드 (컨플루언스 Basic Auth 필수 항목)
            if let Ok(email) = store.get(plugin_id, "email") {
                tracing::info!("[Auth] Loaded email for {} from unified store", plugin_id);
                config
                    .fields
                    .insert("email".to_string(), serde_json::json!(email));
                tracing::info!("Loaded email and token from keychain for Confluence");
            }
        }
        "com.doxus.github" => {
            if let Ok(token) = store.get(plugin_id, "token") {
                tracing::info!("[Auth] Loaded token for {} from unified store", plugin_id);
                secrets
                    .fields
                    .insert("token".to_string(), doxus_plugin_sdk::SecretValue::Text(token));
            }
        }
        _ => {}
    }
}
