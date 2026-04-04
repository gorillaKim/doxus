use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Allowed Claude model IDs.
const CLAUDE_MODELS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "claude-haiku-4-5-20251001",
];

/// Allowed Gemini model IDs.
const GEMINI_MODELS: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.0-flash",
];

fn validate_model(provider: &str, model: &str) -> Result<(), String> {
    let allowed = match provider {
        "claude" => CLAUDE_MODELS,
        "gemini" => GEMINI_MODELS,
        _ => return Err(format!("unknown provider: {provider}")),
    };
    if allowed.contains(&model) {
        Ok(())
    } else {
        Err(format!("model '{model}' is not allowed for provider '{provider}'"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentRequest {
    pub session_id: String,
    pub message: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub text: String,
    pub session_id: String,
    pub done: bool,
}

/// Returns "ok" if API key present, "warn" otherwise.
pub fn compute_agent_status(provider: &str) -> String {
    let has_key = match provider {
        "claude" => std::env::var("ANTHROPIC_API_KEY").is_ok(),
        "gemini" => {
            std::env::var("GEMINI_API_KEY").is_ok()
                || std::env::var("GOOGLE_API_KEY").is_ok()
        }
        _ => false,
    };
    if has_key { "ok" } else { "warn" }.to_string()
}

/// Send a message to Claude or Gemini API and return the response.
/// Uses ANTHROPIC_API_KEY / GEMINI_API_KEY from env.
#[tauri::command]
pub async fn agent_send_message(
    session_id: String,
    message: String,
    provider: String,
    model: String,
) -> Result<AgentResponse, String> {
    validate_model(&provider, &model)?;
    match provider.as_str() {
        "claude" => send_claude(session_id, message, model).await,
        "gemini" => send_gemini(session_id, message, model).await,
        other => Err(format!("unknown provider: {other}")),
    }
}

/// Get agent connection status for a provider.
#[tauri::command]
pub async fn agent_status(provider: String) -> Result<serde_json::Value, String> {
    let status = compute_agent_status(&provider);
    let message = match status.as_str() {
        "ok" => format!("{provider} API key detected"),
        _ => format!("{provider} API key not found. Set ANTHROPIC_API_KEY or GEMINI_API_KEY"),
    };
    Ok(serde_json::json!({ "status": status, "message": message }))
}

async fn send_claude(
    session_id: String,
    message: String,
    model: String,
) -> Result<AgentResponse, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "messages": [{ "role": "user", "content": message }]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Claude API error {status}: {text}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = json["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(AgentResponse { text, session_id, done: true })
}

async fn send_gemini(
    session_id: String,
    message: String,
    model: String,
) -> Result<AgentResponse, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    // model is pre-validated against allowlist — safe to use in URL path
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    );
    let body = serde_json::json!({
        "contents": [{ "role": "user", "parts": [{ "text": message }] }]
    });

    let resp = client
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {status}: {text}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(AgentResponse { text, session_id, done: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize all env-mutating tests to avoid races between set_var/remove_var.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn validate_model_rejects_unknown() {
        assert!(validate_model("claude", "gpt-4").is_err());
        assert!(validate_model("gemini", "claude-sonnet-4-6").is_err());
        assert!(validate_model("unknown", "anything").is_err());
    }

    #[test]
    fn validate_model_accepts_known() {
        assert!(validate_model("claude", "claude-sonnet-4-6").is_ok());
        assert!(validate_model("gemini", "gemini-2.5-pro").is_ok());
    }

    #[test]
    fn agent_send_message_payload_serializes() {
        let payload = AgentRequest {
            session_id: "sess-1".into(),
            message: "hello".into(),
            provider: "claude".into(),
            model: "claude-sonnet-4-6".into(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session_id"));
        assert!(json.contains("claude-sonnet-4-6"));
    }

    #[test]
    fn agent_response_deserializes() {
        let json = r#"{"text":"hi there","session_id":"sess-1","done":true}"#;
        let resp: AgentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.text, "hi there");
        assert!(resp.done);
    }

    #[test]
    fn agent_status_no_key_returns_warn() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        let status = compute_agent_status("claude");
        assert_eq!(status, "warn");
    }

    #[test]
    fn agent_status_with_claude_key_returns_ok() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GEMINI_API_KEY");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-test");
        let status = compute_agent_status("claude");
        std::env::remove_var("ANTHROPIC_API_KEY");
        assert_eq!(status, "ok");
    }

    #[test]
    fn agent_status_with_gemini_key_returns_ok() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("GEMINI_API_KEY", "gk-test");
        let status = compute_agent_status("gemini");
        std::env::remove_var("GEMINI_API_KEY");
        assert_eq!(status, "ok");
    }
}
