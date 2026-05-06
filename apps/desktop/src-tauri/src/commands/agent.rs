use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use doxus_agent::cli_detector::{detect_cli, CliKind};

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

// ── 가이드 템플릿 ───────────────────────────────────────────────────────────

pub const CLAUDE_MD_INSTRUCTIONS: &str = r#"이 프로젝트의 지식과 문서는 Doxus 지식 그래프에 의해 관리되고 있습니다. 에이전트는 다음 도구를 사용하여 지식의 파편들을 연결하고 깊이 있는 리서치를 수행할 수 있습니다.

### 🔴 필수 규칙 (MUST)

**문서 검색, 컨텍스트 파악, 아키텍처 이해 등 탐색 작업에서 반드시 `doxus_*` 도구를 먼저 사용하십시오.**
Read / Grep / Glob 등 직접 파일 접근은 코드 파일·설정 파일 등 Doxus로 해결 불가능한 경우에만 허용됩니다.

### 🛠️ 탐색 도구 및 사용 순서

| 작업 | 사용 도구 |
|------|-----------|
| 프로젝트 파악 | `doxus_list_projects` → `doxus_agent_summary` |
| 문서 검색 | `doxus_search(query, mode="hybrid")` — 항상 첫 번째 |
| 섹션만 읽기 | `doxus_get_section(path, heading)` — 토큰 90% 절약 |
| 전체 문서 | `doxus_get_document` — 섹션으로 해결 안 될 때만 |
| 연관 탐색 | `doxus_get_cluster(id, depth=2)` |
| 역방향 링크 | `doxus_get_backlinks(id)` |
| 정방향 링크 | `doxus_get_links(id)` |
| 경로 탐색 | `doxus_find_path(from, to)` |

### 💡 탐색 시나리오 (Scenarios)
- **리서치 심화 (Deep Dive)**: `doxus_search` → 상위 결과에 `doxus_get_cluster` → 섹션 읽기
- **영향도 평가 (Impact Analysis)**: 코드 수정 시 `doxus_get_backlinks`로 영향받는 설계/기획 문서 점검
- **프로젝트 횡단 탐색 (Cross-Project)**: `doxus://ProjectName/DocID` 링크 발견 시 해당 프로젝트로 `doxus_get_document` 호출

### ⚡ 효율적인 탐색 팁
- `doxus_get_toc`로 목차 먼저 → `doxus_get_section`으로 필요한 부분만 읽기
- 검색 결과 `snippet`으로 관련성 판단 후 전체 읽기 여부 결정
- 발견된 문서들 사이의 **연결(Links)**을 무시하지 마십시오"#;

// ── 배경 리더 ────────────────────────────────────────────────────────────────

pub fn spawn_background_reader(
    sidecar: std::sync::Arc<doxus_agent::sync_sidecar::SyncSidecarManager>,
    app: tauri::AppHandle,
    pending: crate::state::PendingMessages,
    collected: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
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

                    // Collect content if this session is marked for collection
                    if let Some(content) = resp.content.as_deref() {
                        if let Ok(mut coll) = collected.lock() {
                            if let Some(buf) = coll.get_mut(&session_id) {
                                buf.push_str(content);
                            }
                        }
                    }

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
            for (_sid, tx) in pending.drain() {
                let _ = tx.send(false);
            }
        }
    });
}

// ── Tauri 커맨드 ─────────────────────────────────────────────────────────────

pub async fn chat_start_session_impl(
    app: tauri::AppHandle,
    state: &crate::AppState,
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
            state.collected_messages.clone(),
        );
    }

    let system_prompt = state.prompt_loader.build_system_prompt();

    // MCP 서버 설정: HTTP 엔드포인트 우선, 없으면 stdio fallback
    let mcp_servers = {
        let endpoint = state.mcp_endpoint.lock().ok().and_then(|g| g.clone());
        let token = state.mcp_token.lock().ok().map(|g| g.clone()).unwrap_or_default();
        if let Some(url) = endpoint {
            serde_json::json!({
                "doxus": {
                    "type": "http",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {}", token) }
                }
            })
        } else {
            find_doxus_mcp()
                .map(|p| serde_json::json!({ "doxus": { "type": "stdio", "command": p.to_string_lossy(), "args": [] } }))
                .unwrap_or(serde_json::json!({}))
        }
    };

    let bridge_token = std::env::var("DOXUS_BRIDGE_TOKEN").unwrap_or_default();

    let start_req = serde_json::json!({
        "type": "start",
        "sessionId": session_id,
        "cliType": cli_type,
        "cliPath": cli_path,
        "model": model,
        "systemPrompt": system_prompt,
        "mcpServers": mcp_servers,
        "bridgeToken": bridge_token
    });

    state.sidecar.send_request(&start_req)
}

#[tauri::command]
pub async fn chat_start_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
    session_id: String,
    cli_type: String,
    cli_path: String,
    model: String,
) -> Result<(), String> {
    chat_start_session_impl(app, state.inner(), session_id, cli_type, cli_path, model).await
}

pub async fn chat_send_message_impl(
    state: &crate::AppState,
    session_id: String,
    message: String,
) -> Result<(), String> {
    state.sidecar.ensure_running(&state.sidecar_script)?;

    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    {
        let mut pending = state.pending_messages.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        pending.insert(session_id.clone(), tx);
    }

    let req = serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "content": message
    });

    if let Err(e) = state.sidecar.send_request(&req) {
        if let Ok(mut p) = state.pending_messages.lock() {
            p.remove(&session_id);
        }
        return Err(e);
    }

    let _ = rx.await;
    Ok(())
}

#[tauri::command]
pub async fn chat_send_message(
    state: tauri::State<'_, Arc<crate::AppState>>,
    session_id: String,
    message: String,
) -> Result<(), String> {
    chat_send_message_impl(state.inner(), session_id, message).await
}

/// 진행 중인 메시지 취소.
#[tauri::command]
pub fn chat_cancel(
    state: tauri::State<'_, Arc<crate::AppState>>,
    session_id: String,
) -> Result<(), String> {
    let req = serde_json::json!({ "type": "cancel", "sessionId": session_id });
    state.sidecar.send_request(&req)
}

/// 세션 종료 및 sidecar 상태 정리.
#[tauri::command]
pub fn chat_close_session(
    state: tauri::State<'_, Arc<crate::AppState>>,
    session_id: String,
) -> Result<(), String> {
    let req = serde_json::json!({
        "type": "close_session",
        "sessionId": session_id
    });
    state.sidecar.send_request(&req)
}

// ── 에이전트 상태 및 CLI 탐색 ───────────────────────────────────────────────

#[tauri::command]
pub async fn detect_cli_path(provider: String) -> Result<serde_json::Value, String> {
    let kind = detect_cli();
    match kind {
        CliKind::ClaudeCode { path } if provider == "claude" => {
            Ok(serde_json::json!({
                "found": true,
                "cliType": "claude",
                "cliPath": path.to_string_lossy(),
            }))
        }
        CliKind::GeminiCli { path } if provider == "gemini" => {
            Ok(serde_json::json!({
                "found": true,
                "cliType": "gemini",
                "cliPath": path.to_string_lossy(),
            }))
        }
        _ => {
            // 특정 프로바이더가 감지되지 않았거나 다른 프로바이더가 감지됨
            Ok(serde_json::json!({
                "found": false,
                "cliType": provider,
                "cliPath": provider,
            }))
        }
    }
}

#[tauri::command]
pub async fn agent_status(
    state: tauri::State<'_, Arc<crate::AppState>>,
    _provider: String,
) -> Result<serde_json::Value, String> {
    let is_running = state.sidecar.is_running();
    Ok(serde_json::json!({
        "status": if is_running { "ok" } else { "idle" },
        "running": is_running,
    }))
}

// ── Claude MCP Config Management ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeMcpConfig {
    #[serde(default)]
    pub mcp_servers: serde_json::Value,
}

fn get_claude_config_file_path() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    
    // Candidates for Claude Desktop config
    let paths = vec![
        std::path::PathBuf::from(&home).join("Library/Application Support/Claude/claude_desktop_config.json"),
        std::path::PathBuf::from(&home).join(".claude/claude_desktop_config.json"),
    ];

    for path in paths {
        if path.exists() {
            return Ok(path);
        }
    }

    // Default to the official macOS location if neither exists
    Ok(std::path::PathBuf::from(&home).join("Library/Application Support/Claude/claude_desktop_config.json"))
}


#[tauri::command]
pub async fn get_claude_mcp_config() -> Result<serde_json::Value, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let desktop_path = get_claude_config_file_path()?;
    
    // Check multiple CLI paths
    let cli_paths = vec![
        std::path::PathBuf::from(&home).join(".claude.json"),
        std::path::PathBuf::from(&home).join(".mcp.json"),
    ];

    let (desktop_connected, desktop_config) = if desktop_path.exists() {
        let content = std::fs::read_to_string(&desktop_path).unwrap_or_default();
        let config: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
        let connected = config.get("mcpServers").and_then(|m| m.get("doxus")).is_some();
        (connected, Some(config))
    } else {
        (false, None)
    };

    let mut cli_connected = false;
    let mut cli_config_consolidated = serde_json::json!({"mcpServers": {}});

    for cli_path in cli_paths {
        if cli_path.exists() {
            let content = std::fs::read_to_string(&cli_path).unwrap_or_default();
            let config: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
            
            let has_mcp = config.get("mcpServers").and_then(|m| m.get("doxus")).is_some();
            
            // For .claude.json, we also check is_enabled
            let is_enabled = if cli_path.to_string_lossy().contains(".claude.json") {
                config.get("enabledMcpjsonServers")
                    .and_then(|a| a.as_array())
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some("doxus")))
                    .unwrap_or(false)
            } else {
                true // .mcp.json is usually auto-enabled
            };

            if has_mcp && is_enabled {
                cli_connected = true;
                // Keep the first config found or merge? Let's just keep the latest for display.
                cli_config_consolidated = config;
            }
        }
    }

    Ok(serde_json::json!({
        "desktop": {
            "connected": desktop_connected,
            "path": desktop_path.to_string_lossy(),
            "config": desktop_config
        },
        "cli": {
            "connected": cli_connected,
            "path": "~/.claude.json, ~/.mcp.json",
            "config": cli_config_consolidated
        }
    }))
}

#[tauri::command]
pub async fn upsert_claude_mcp_config(
    state: tauri::State<'_, std::sync::Arc<crate::AppState>>,
    target: String,
) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let paths = match target.as_str() {
        "desktop" => vec![get_claude_config_file_path()?],
        "cli" => vec![
            std::path::PathBuf::from(&home).join(".claude.json"),
            std::path::PathBuf::from(&home).join(".mcp.json"),
        ],
        _ => return Err("Invalid target".to_string()),
    };

    // HTTP 엔드포인트 우선, 없으면 stdio fallback
    let endpoint = state.mcp_endpoint.lock().ok().and_then(|g| g.clone());
    let token = state.mcp_token.lock().ok().map(|g| g.clone()).unwrap_or_default();

    let mcp_entry = if let Some(url) = endpoint {
        serde_json::json!({
            "type": "http",
            "url": url,
            "headers": { "Authorization": format!("Bearer {}", token) }
        })
    } else {
        let mcp_path = find_doxus_mcp()
            .ok_or_else(|| "doxus-mcp not found and HTTP server not running.".to_string())?
            .to_string_lossy()
            .to_string();
        let bridge_token = std::fs::read_to_string(
            std::path::PathBuf::from(&home).join(".doxus/.bridge_token"),
        )
        .unwrap_or_default()
        .trim()
        .to_string();
        let db_path = std::env::var("DOXUS_DB_PATH")
            .unwrap_or_else(|_| format!("{}/.doxus/db/doxus.db", home));
        serde_json::json!({
            "command": mcp_path,
            "args": [],
            "env": { "DOXUS_BRIDGE_TOKEN": bridge_token, "DOXUS_DB_PATH": db_path },
            "type": "stdio"
        })
    };

    for path in paths {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let mut config: serde_json::Value = if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read config: {e}"))?;
            serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if config.get("mcpServers").is_none() {
            if let Some(obj) = config.as_object_mut() {
                obj.insert("mcpServers".to_string(), serde_json::json!({}));
            }
        }

        if let Some(mcp_servers) = config.get_mut("mcpServers").and_then(|m| m.as_object_mut()) {
            mcp_servers.insert("doxus".to_string(), mcp_entry.clone());
        }

        // Special for CLI-related files: ensure "doxus" is in enabledMcpjsonServers if it exists
        // (Claude Code uses this field in .claude.json)
        if target == "cli" && path.to_string_lossy().contains(".claude.json") {
            if config.get("enabledMcpjsonServers").is_none() {
                if let Some(obj) = config.as_object_mut() {
                    obj.insert("enabledMcpjsonServers".to_string(), serde_json::json!([]));
                }
            }
            if let Some(arr) = config.get_mut("enabledMcpjsonServers").and_then(|a| a.as_array_mut()) {
                if !arr.iter().any(|v| v.as_str() == Some("doxus")) {
                    arr.push(serde_json::json!("doxus"));
                }
            }
        }

        let content = serde_json::to_string_pretty(&config).map_err(|e| format!("Failed to serialize config: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn remove_claude_mcp_config(target: String) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let paths = match target.as_str() {
        "desktop" => vec![get_claude_config_file_path()?],
        "cli" => vec![
            std::path::PathBuf::from(&home).join(".claude.json"),
            std::path::PathBuf::from(&home).join(".mcp.json"),
        ],
        _ => return Err("Invalid target".to_string()),
    };

    for path in paths {
        if !path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read config: {e}"))?;
        let mut config: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {e}"))?;

        let mut modified = false;

        if let Some(mcp_servers) = config.get_mut("mcpServers").and_then(|m| m.as_object_mut()) {
            if mcp_servers.remove("doxus").is_some() {
                modified = true;
            }
        }

        if target == "cli" && path.to_string_lossy().contains(".claude.json") {
            if let Some(arr) = config.get_mut("enabledMcpjsonServers").and_then(|a| a.as_array_mut()) {
                let initial_len = arr.len();
                arr.retain(|v| v.as_str() != Some("doxus"));
                if arr.len() != initial_len {
                    modified = true;
                }
            }
        }

        if modified {
            let content = serde_json::to_string_pretty(&config).map_err(|e| format!("Failed to serialize config: {e}"))?;
            std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {e}"))?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn generate_global_claude_md() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let claude_dir = std::path::PathBuf::from(home).join(".claude");
    
    if !claude_dir.exists() {
        std::fs::create_dir_all(&claude_dir).map_err(|e| e.to_string())?;
    }
    
    let path = claude_dir.join("CLAUDE.md");
    let instr_header = "## AI 에이전트 도구 (Doxus)";
    let instr_body = CLAUDE_MD_INSTRUCTIONS;

    let mut content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read global CLAUDE.md: {e}"))?
    } else {
        "# Global Instructions for AI Agents\n\n".to_string()
    };

    if content.contains(instr_header) {
        return Ok(()); // Already configured
    }

    content.push_str("\n\n");
    content.push_str(instr_header);
    content.push('\n');
    content.push_str(instr_body);

    std::fs::write(&path, content).map_err(|e| format!("Failed to write global CLAUDE.md: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn get_claude_md_template() -> Result<String, String> {
    Ok(CLAUDE_MD_INSTRUCTIONS.to_string())
}

#[tauri::command]
pub async fn generate_project_claude_md(path: String) -> Result<(), String> {
    let project_path = std::path::PathBuf::from(path);
    if !project_path.exists() {
        return Err("Project path does not exist".to_string());
    }

    let claude_md_path = project_path.join("CLAUDE.md");
    let instr_header = "## AI 에이전트 도구 (Doxus)";
    let instr_body = CLAUDE_MD_INSTRUCTIONS;

    let mut content = if claude_md_path.exists() {
        std::fs::read_to_string(&claude_md_path).map_err(|e| format!("Failed to read CLAUDE.md: {e}"))?
    } else {
        "# Project Instructions\n\n".to_string()
    };

    if content.contains(instr_header) {
        return Ok(()); // Already configured
    }

    content.push_str("\n\n");
    content.push_str(instr_header);
    content.push('\n');
    content.push_str(instr_body);

    std::fs::write(&claude_md_path, content).map_err(|e| format!("Failed to write CLAUDE.md: {e}"))?;

    Ok(())
}

// ── 헬퍼 ─────────────────────────────────────────────────────────────────────

fn find_doxus_mcp() -> Option<std::path::PathBuf> {
    // 1. Next to the executable (Highest priority - covers both dev and bundled cases)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { 
                return Some(candidate); 
            }
        }
    }

    // 2. Search up from current executable for workspace root (dev mode coverage)
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            // Check if we are at workspace root (parent of target/)
            if parent.file_name().map(|n| n == "target").unwrap_or(false) {
                // Return corresponding build artifact if found
                let debug = parent.join("debug").join("doxus-mcp");
                if debug.exists() { return Some(debug); }
                let release = parent.join("release").join("doxus-mcp");
                if release.exists() { return Some(release); }
                break;
            }
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

    // 3. System PATH search
    if let Some(found) = std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var).find_map(|dir| {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { Some(candidate) } else { None }
        })
    }) {
        return Some(found);
    }

    // 4. macOS standard installation path (Last resort fallback)
    #[cfg(target_os = "macos")]
    {
        let installed = std::path::PathBuf::from("/Applications/doxus.app/Contents/MacOS/doxus-mcp");
        if installed.exists() { return Some(installed); }
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
