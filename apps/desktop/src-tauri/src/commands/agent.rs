use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use doxus_agent::cli_detector::{detect_cli, verify_claude_version, CliKind};

// ── 브리지 응답 (사이드카 → Rust) ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(rename = "sessionId", default)]
    pub session_id: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: Option<Vec<String>>,
    #[serde(rename = "toolName", default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub retryable: Option<bool>,
}

// ── 배경 리더 ────────────────────────────────────────────────────────────────

pub fn spawn_background_reader(
    sidecar: std::sync::Arc<doxus_agent::sync_sidecar::SyncSidecarManager>,
    app: tauri::AppHandle,
    pending: crate::state::PendingMessages,
) {
    use std::io::BufRead;
    use tauri::Emitter;

    let Some(mut reader) = sidecar.take_reader() else {
        eprintln!("[reader] no reader available");
        return;
    };

    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => { eprintln!("[reader] sidecar EOF"); break; }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    let resp: BridgeResponse = match serde_json::from_str(trimmed) {
                        Ok(r) => r,
                        Err(e) => { eprintln!("[reader] parse error: {e}: {trimmed}"); continue; }
                    };

                    if resp.msg_type == "init" { continue; }

                    let session_id = resp.session_id.clone();
                    let is_terminal = matches!(resp.msg_type.as_str(), "result" | "error" | "cancelled");

                    let _ = app.emit(&format!("chat-stream:{session_id}"), &resp);

                    if is_terminal {
                        let success = resp.msg_type == "result";
                        if let Ok(mut pending) = pending.lock() {
                            if let Some(tx) = pending.remove(&session_id) {
                                let _ = tx.send(success);
                            }
                        }
                    }
                }
                Err(e) => { eprintln!("[reader] read error: {e}"); break; }
            }
        }

        // 사이드카 종료 시 대기 중인 모든 세션 해제
        if let Ok(mut pending) = pending.lock() {
            for (sid, tx) in pending.drain() {
                let _ = tx.send(false);
            }
        }
    });
}

// ── Tauri 커맨드 ─────────────────────────────────────────────────────────────

/// 세션 시작: 사이드카를 구동하고 Claude/Gemini 세션을 등록한다.
#[tauri::command]
pub async fn chat_start_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: String,
    cli_type: String,  // "claude" | "gemini"
    cli_path: String,
    model: String,
) -> Result<(), String> {
    // 사이드카 시작
    state.sidecar.ensure_running(&state.sidecar_script)?;

    // 배경 리더 한 번만 생성
    if !state.reader_started.swap(true, Ordering::SeqCst) {
        spawn_background_reader(
            state.sidecar.clone(),
            app,
            state.pending_messages.clone(),
        );
    }

    let system_prompt = state.prompt_loader.build_system_prompt();

    // doxus-mcp 경로 탐색 (선택적)
    let mcp_servers = find_doxus_mcp()
        .map(|p| serde_json::json!({ "doxus": { "type": "stdio", "command": p.to_string_lossy(), "args": [] } }))
        .unwrap_or(serde_json::json!({}));

    let start_req = serde_json::json!({
        "type": "start",
        "sessionId": session_id,
        "cliType": cli_type,
        "cliPath": cli_path,
        "model": model,
        "systemPrompt": system_prompt,
        "mcpServers": mcp_servers
    });

    state.sidecar.send_request(&start_req)
}

/// 메시지 전송: 결과가 올 때까지 블로킹.
#[tauri::command]
pub async fn chat_send_message(
    state: tauri::State<'_, crate::AppState>,
    session_id: String,
    message: String,
) -> Result<(), String> {
    state.sidecar.ensure_running(&state.sidecar_script)?;

    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    {
        let mut pending = state.pending_messages.lock().map_err(|e| e.to_string())?;
        pending.insert(session_id.clone(), tx);
    }

    let req = serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "content": message
    });

    if let Err(e) = state.sidecar.send_request(&req) {
        state.pending_messages.lock().ok().map(|mut p| p.remove(&session_id));
        return Err(e);
    }

    let _ = rx.await;
    Ok(())
}

/// 진행 중인 메시지 취소.
#[tauri::command]
pub fn chat_cancel(
    state: tauri::State<'_, crate::AppState>,
    session_id: String,
) -> Result<(), String> {
    let req = serde_json::json!({ "type": "cancel", "sessionId": session_id });
    state.sidecar.send_request(&req)
}

/// Claude/Gemini 연결 상태 확인 (설정 화면용).
#[tauri::command]
pub async fn agent_status(provider: String) -> Result<serde_json::Value, String> {
    let cli = detect_cli();

    let (status, message) = match (provider.as_str(), &cli) {
        ("claude", CliKind::ClaudeCode { path }) => {
            let version = verify_claude_version(path)
                .unwrap_or_else(|| path.display().to_string());
            ("ok", format!("Claude Code CLI 감지됨: {version}"))
        }
        ("gemini", CliKind::GeminiCli { path }) => (
            "ok",
            format!("Gemini CLI 감지됨: {}", path.display()),
        ),
        ("claude", CliKind::GeminiCli { path }) => (
            "warn",
            format!("Claude CLI를 찾을 수 없습니다. Gemini CLI: {}", path.display()),
        ),
        ("gemini", CliKind::ClaudeCode { path }) => (
            "warn",
            format!("Gemini CLI를 찾을 수 없습니다. Claude CLI: {}", path.display()),
        ),
        (_, CliKind::None) => (
            "warn",
            "AI CLI를 찾을 수 없습니다. Claude Code 또는 Gemini CLI를 설치하세요.".into(),
        ),
        (_, CliKind::ClaudeCode { path }) => {
            let version = verify_claude_version(path)
                .unwrap_or_else(|| path.display().to_string());
            ("warn", format!("알 수 없는 provider '{provider}'. Claude Code: {version}"))
        }
        (_, CliKind::GeminiCli { path }) => (
            "warn",
            format!("알 수 없는 provider '{provider}'. Gemini CLI: {}", path.display()),
        ),
    };

    Ok(serde_json::json!({ "status": status, "message": message }))
}

/// CLI 경로 반환 (프론트엔드에서 chat_start_session에 전달).
#[tauri::command]
pub fn detect_cli_path(provider: String) -> Result<serde_json::Value, String> {
    let cli = detect_cli();
    match (provider.as_str(), cli) {
        ("claude", CliKind::ClaudeCode { path }) | ("", CliKind::ClaudeCode { path }) => {
            Ok(serde_json::json!({ "found": true, "cliType": "claude", "cliPath": path.to_string_lossy() }))
        }
        ("gemini", CliKind::GeminiCli { path }) | ("", CliKind::GeminiCli { path }) => {
            Ok(serde_json::json!({ "found": true, "cliType": "gemini", "cliPath": path.to_string_lossy() }))
        }
        (_, CliKind::ClaudeCode { path }) => {
            Ok(serde_json::json!({ "found": true, "cliType": "claude", "cliPath": path.to_string_lossy() }))
        }
        (_, CliKind::GeminiCli { path }) => {
            Ok(serde_json::json!({ "found": true, "cliType": "gemini", "cliPath": path.to_string_lossy() }))
        }
        _ => Ok(serde_json::json!({ "found": false, "cliType": "", "cliPath": "" })),
    }
}

// ── 헬퍼 ─────────────────────────────────────────────────────────────────────

fn find_doxus_mcp() -> Option<std::path::PathBuf> {
    // 1. exe 옆 (릴리즈 번들)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { return Some(candidate); }
        }
    }

    // 2. PATH 탐색
    if let Some(found) = std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var).find_map(|dir| {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { Some(candidate) } else { None }
        })
    }) {
        return Some(found);
    }

    // 3. dev 모드 폴백: current_exe에서 workspace root(target 부모) 탐색
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            // target/ 디렉토리를 찾으면 그 부모가 workspace root
            if parent.file_name().map(|n| n == "target").unwrap_or(false) {
                // workspace_root/target/debug 또는 release
                let debug = parent.join("debug").join("doxus-mcp");
                if debug.exists() { return Some(debug); }
                let release = parent.join("release").join("doxus-mcp");
                if release.exists() { return Some(release); }
                break;
            }
            // target/debug/build/... 구조 처리
            if dir.file_name().map(|n| n == "target").unwrap_or(false) {
                let debug = dir.join("debug").join("doxus-mcp");
                if debug.exists() { return Some(debug); }
                let release = dir.join("release").join("doxus-mcp");
                if release.exists() { return Some(release); }
                break;
            }
            dir = parent;
        }
    }

    None
}

// ── 테스트 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_response_deserializes_text() {
        let json = r#"{"type":"text","sessionId":"s1","content":"안녕","done":false}"#;
        let r: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.msg_type, "text");
        assert_eq!(r.content.unwrap(), "안녕");
    }

    #[test]
    fn bridge_response_deserializes_result() {
        let json = r#"{"type":"result","sessionId":"s1","content":"done","cost":0.01}"#;
        let r: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.msg_type, "result");
        assert_eq!(r.cost.unwrap(), 0.01);
    }

    #[test]
    fn bridge_response_deserializes_tool_use() {
        let json = r#"{"type":"tool_use","sessionId":"s1","toolName":"doxus_search","status":"running"}"#;
        let r: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.tool_name.unwrap(), "doxus_search");
    }
}
