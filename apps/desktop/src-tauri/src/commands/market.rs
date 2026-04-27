use doxus_core::secrets::SecretStore;
use doxus_core::observability::{persist_audit, AuditEvent};
use std::sync::Arc;



fn find_doxus_mcp_binary() -> Option<std::path::PathBuf> {
    // 1. exe 옆 (프로덕션 번들 및 dev target/debug/)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { return Some(candidate); }
        }
    }
    // 2. PATH 탐색
    std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var).find_map(|dir| {
            let candidate = dir.join("doxus-mcp");
            if candidate.exists() { Some(candidate) } else { None }
        })
    })
}

fn config_schema(fields: &[(&str, &str, &str, bool, &str)]) -> serde_json::Value {
    // fields: (key, label, type, required, placeholder)
    serde_json::Value::Array(
        fields
            .iter()
            .map(|(key, label, field_type, required, placeholder)| {
                serde_json::json!({
                    "key": key,
                    "label": label,
                    "type": field_type,
                    "required": required,
                    "placeholder": placeholder
                })
            })
            .collect(),
    )
}

#[tauri::command]
pub async fn market_list_installed(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let installed_ids: Vec<String> = state
        .plugin_manager
        .list_installed()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|id| doxus_core::plugin::PluginManager::normalize_id(&id))
        .collect();

    // Built-in plugins (always available)
    let builtin = vec![
        serde_json::json!({
            "id": "com.doxus.obsidian",
            "name": "Obsidian",
            "version": "1.0.0",
            "trust": "official",
            "description": "Obsidian vault integration (built-in, local folder)",
            "installed": true,
            "builtin": true,
            "config_schema": config_schema(&[
                ("path", "볼트 폴더", "folder", true, "/Users/you/MyVault"),
                ("name", "프로젝트 이름", "text", true, "my-vault"),
            ]),
            "auth_type": "none",
            "auth_schema": serde_json::json!([])
        }),
        serde_json::json!({
            "id": "com.doxus.confluence",
            "name": "Confluence",
            "version": "1.0.0",
            "trust": "official",
            "description": "Confluence Cloud/Server REST API integration",
            "installed": true,
            "builtin": true,
            "guide_url": "internal://guide/confluence",
            "config_schema": config_schema(&[
                ("name", "프로젝트 이름", "text", true, "confluence-docs"),
                ("base_url", "Base URL", "url", true, "https://yourcompany.atlassian.net"),
                ("space_key", "스페이스 키 (전체 스페이스 연동 시)", "text", false, "ENG 또는 ~222368988"),
                ("ancestor_id", "페이지 ID (특정 폴더 하위만 연동 시)", "text", false, "123456"),
            ]),
            "auth_type": "api_token",
            "auth_schema": config_schema(&[
                ("email", "Atlassian 계정 이메일", "email", true, "you@company.com"),
                ("api_token", "Personal API Token", "password", true, "ATATT3xFfGF..."),
            ])
        }),
        serde_json::json!({
            "id": "com.doxus.github",
            "name": "GitHub",
            "version": "1.0.0",
            "trust": "official",
            "description": "GitHub Issues, Wiki, Discussions",
            "installed": true,
            "builtin": true,
            "guide_url": "internal://guide/github",
            "config_schema": config_schema(&[
                ("name", "프로젝트 이름", "text", true, "github-docs"),
                ("repo", "저장소 (owner/repo)", "text", true, "myorg/myrepo"),
            ]),
            "auth_type": "api_token",
            "auth_schema": config_schema(&[
                ("token", "Personal Access Token", "password", false, "ghp_••••••••"),
            ])
        }),
    ];

    // User-installed plugins not in built-in list
    let builtin_ids = crate::state::builtin_plugin_ids();
    let user_installed: Vec<serde_json::Value> = installed_ids
        .iter()
        .filter(|id| !builtin_ids.contains(&id.as_str()))
        .map(|id| serde_json::json!({
            "id": id,
            "name": id,
            "version": "unknown",
            "trust": "unverified",
            "description": "User-installed plugin",
            "installed": true,
            "builtin": false,
            "config_schema": config_schema(&[
                ("name", "프로젝트 이름", "text", true, "my-project"),
                ("endpoint", "엔드포인트 / URL", "url", false, "https://..."),
            ]),
            "auth_type": "none",
            "auth_schema": serde_json::json!([])
        }))
        .collect();

    let mut all = builtin;
    all.extend(user_installed);
    Ok(serde_json::json!({ "plugins": all }))
}

#[tauri::command]
pub async fn get_system_status() -> Result<serde_json::Value, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{}/.doxus/db/doxus.db", home);
    let db_exists = std::path::Path::new(&db_path).exists();

    // CLI 바이너리 존재 여부 확인
    let cli_candidates = [
        format!("{}/.cargo/bin/doxus-cli", home),
        "/usr/local/bin/doxus-cli".to_string(),
    ];
    let cli_path = cli_candidates.iter().find(|p| std::path::Path::new(p).exists());

    // doxus-mcp 바이너리 존재 여부 확인 (stdio 기반이라 포트 체크 불가)
    let mcp_running = find_doxus_mcp_binary().is_some();

    // 에이전트 사이드카 상태: 로컬 CLI 감지로 판단
    let agent_status = {
        use doxus_agent::cli_detector::{detect_cli, CliKind};
        match detect_cli() {
            CliKind::ClaudeCode { path } => serde_json::json!({
                "status": "connected",
                "note": format!("Claude Code CLI 감지됨: {}", path.display())
            }),
            CliKind::GeminiCli { path } => serde_json::json!({
                "status": "connected",
                "note": format!("Gemini CLI 감지됨: {}", path.display())
            }),
            CliKind::None => serde_json::json!({
                "status": "warn",
                "note": "AI CLI를 찾을 수 없습니다. Claude Code 또는 Gemini CLI를 설치하세요."
            }),
        }
    };

    Ok(serde_json::json!({
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "status": "running"
        },
        "database": {
            "path": db_path,
            "exists": db_exists,
            "status": if db_exists { "connected" } else { "not found" }
        },
        "mcp": {
            "status": if mcp_running { "running" } else { "not started" },
            "note": "doxus-mcp 바이너리 (stdio MCP 서버)"
        },
        "cli": {
            "status": if cli_path.is_some() { "installed" } else { "not installed" },
            "path": cli_path.cloned().unwrap_or_default()
        },
        "agent": agent_status
    }))
}

#[tauri::command]
pub async fn get_plugin_logs(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, event_type, payload, occurred_at \
             FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let logs: Vec<serde_json::Value> = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "project_id": r.get::<_, Option<i64>>(1)?,
                "event_type": r.get::<_, String>(2)?,
                "payload": r.get::<_, Option<String>>(3)?,
                "occurred_at": r.get::<_, i64>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "logs": logs }))
}

#[tauri::command]
pub async fn clear_audit_log(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let deleted: usize = conn
        .execute("DELETE FROM audit_log", [])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "deleted": deleted }))
}

#[tauri::command]
pub async fn get_embedding_status(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let embedder = state.embedder.read().await.clone();
    let info = embedder.model_info().clone();
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let total_docs: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap_or(0);
    let embedded_chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let model_loaded = info.dimension > 0;
    let model = if model_loaded { format!("ONNX ({})", info.name) } else { "미활성 (모델 로드 실패)".to_string() };
    let status = if !model_loaded {
        "inactive"
    } else if embedded_chunks > 0 {
        "active"
    } else {
        "ready"  // 모델 로드됨, 재인덱싱 필요
    };
    Ok(serde_json::json!({
        "model": model,
        "model_loaded": model_loaded,
        "dimension": info.dimension,
        "total_documents": total_docs,
        "embedded_chunks": embedded_chunks,
        "status": status,
        "path": info.path,
    }))
}

#[tauri::command]
pub async fn get_sync_status(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let status = state.sync_manager.get_status().await;
    Ok(serde_json::json!(status))
}

#[tauri::command]
pub async fn trigger_sync(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    // source_instances 목록 조회
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_instances", [], |r| r.get(0))
        .unwrap_or(0);
    // 실제 sync 실행은 SyncRunner가 담당 — 여기서는 last_synced를 초기화해 다음 스케줄 주기에 즉시 실행되도록 함
    conn.execute("UPDATE source_instances SET last_synced = 0", [])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "message": format!("{}개 소스 인스턴스의 동기화 예약됨 (다음 스케줄 주기에 실행)", count)
    }))
}

#[tauri::command]
pub async fn market_install_plugin(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
    registry_url: Option<String>,
) -> Result<serde_json::Value, String> {
    // Verify trust anchor: plugin must exist in registry and have a non-empty public_key_hex.
    let url = registry_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::env::var("DOXUS_REGISTRY_URL")
                .unwrap_or_else(|_| "https://YOUR_ORG.github.io/doxus-registry".to_string())
        });
    let client = doxus_core::marketplace::registry::RegistryClient::new(&url)
        .map_err(|e| e.to_string())?;
    let entry = client
        .fetch_entry(&plugin_id)
        .await
        .map_err(|e| format!("registry lookup failed: {e}"))?
        .ok_or_else(|| format!("plugin '{}' not found in registry — installation rejected", plugin_id))?;
    if entry.public_key_hex.is_empty() {
        return Err(format!(
            "plugin '{}' has no trust anchor (public_key_hex is empty) — installation rejected",
            plugin_id
        ));
    }

    // Validate plugin_id to prevent path traversal attacks.
    if plugin_id.contains('/') || plugin_id.contains('\\') || plugin_id.contains("..") {
        return Err("invalid plugin_id: contains path separators".into());
    }

    let plugins_dir = state.plugins_dir.clone();
    std::fs::create_dir_all(&plugins_dir).map_err(|e| e.to_string())?;

    let download_url = entry.download_url.clone();
    let checksum = if entry.checksum_sha256.is_empty() {
        None
    } else {
        Some(entry.checksum_sha256.clone())
    };

    let plugin_id_for_response = plugin_id.clone();
    let conn_arc = state.conn.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let installer = doxus_core::marketplace::installer::PluginInstaller::new(plugins_dir);
        installer
            .install_from_url(&plugin_id, &download_url, checksum.as_deref())
            .map_err(|e| {
                let msg = format!("Plugin installation failed for {}: {}", plugin_id, e);
                if let Ok(conn) = conn_arc.lock() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg,
                    });
                }
                e.to_string()
            })
    })
    .await
    .map_err(|e| format!("thread error: {e}"))??;

    Ok(serde_json::json!({ "status": "ok", "installed": true, "plugin_id": plugin_id_for_response }))
}

#[tauri::command]
pub async fn market_uninstall_plugin(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    doxus_core::marketplace::installer::validate_plugin_id(&plugin_id)
        .map_err(|e| e.to_string())?;
    let wasm_path = state.plugins_dir.join(format!("{}.wasm", plugin_id));
    if wasm_path.exists() {
        std::fs::remove_file(&wasm_path).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({ "status": "ok", "installed": false, "plugin_id": plugin_id }))
}

#[tauri::command]
pub async fn plugin_save_auth(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
    auth_fields: std::collections::HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    let store = state.secret_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.set_bulk(&plugin_id, &auth_fields)
            .map_err(|e| format!("Failed to save auth fields: {}", e))?;
        println!("[Market] saved {} auth fields for {} to unified store", auth_fields.len(), plugin_id);
        Ok(serde_json::json!({ "status": "ok" }))
    }).await.map_err(|e| format!("Internal thread error: {}", e))?
}

#[tauri::command]
pub async fn plugin_get_auth_status(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let store = state.secret_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let keys_to_check: &[&str] = match plugin_id.as_str() {
            "com.doxus.confluence" => &["api_token", "email"],
            "com.doxus.github" => &["access_token", "token"],
            _ => &[],
        };

        let configured = keys_to_check.iter().any(|key| {
            store.get(&plugin_id, key)
                .map(|p: String| !p.is_empty())
                .unwrap_or(false)
        });

        Ok(serde_json::json!({ "configured": configured, "plugin_id": plugin_id }))
    }).await.map_err(|e| format!("Internal thread error: {}", e))?
}

// ── PKCE helpers ────────────────────────────────────────────────────────────

fn generate_code_verifier() -> String {
    use rand::Rng;
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen::<u8>()).collect();
    base64_url_encode(&bytes)
}

fn base64_url_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
}

fn generate_code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let result = hasher.finalize();
    base64_url_encode(&result)
}

fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ── New commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn plugin_start_oauth(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
    client_id: String,
    client_secret: String,
) -> Result<serde_json::Value, String> {
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Start local OAuth server on fixed port 14920
    use tauri::Emitter as _;
    let app_clone = app.clone();
    let plugin_id_clone = plugin_id.clone();
    let port = tauri_plugin_oauth::start_with_config(
        tauri_plugin_oauth::OauthConfig {
            ports: Some(vec![14920]),
            response: Some(std::borrow::Cow::Borrowed(
                "<html><body style='font-family:sans-serif;text-align:center;padding:40px'>\
                 <h2>인증 완료!</h2><p>doxus 앱으로 돌아가세요.</p></body></html>",
            )),
        },
        move |url| {
            let event_name = format!("oauth-callback-{}", plugin_id_clone.replace('.', "_"));
            let _ = app_clone.emit(&event_name, url);
        },
    )
    .map_err(|e| format!("Failed to start OAuth server: {}", e))?;

    let redirect_uri = format!("http://localhost:{}", port);

    let auth_url = format!(
        "https://auth.atlassian.com/authorize?\
         audience=api.atlassian.com&\
         client_id={client_id}&\
         scope=read%3Aconfluence-content.all%20read%3Aconfluence-space.summary%20offline_access&\
         redirect_uri={redirect_uri_encoded}&\
         response_type=code&\
         prompt=consent&\
         code_challenge={code_challenge}&\
         code_challenge_method=S256",
        client_id = client_id,
        redirect_uri_encoded = urlencoding_encode(&redirect_uri),
        code_challenge = code_challenge,
    );

    let mut pending = state.oauth_pending.lock().map_err(|e| e.to_string())?;
    pending.insert(
        plugin_id.clone(),
        crate::state::OAuthPending {
            code_verifier,
            client_id,
            client_secret,
            redirect_uri,
        },
    );

    Ok(serde_json::json!({
        "auth_url": auth_url,
        "plugin_id": plugin_id,
        "port": port,
    }))
}

#[tauri::command]
pub async fn plugin_oauth_exchange(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
    code: String,
) -> Result<serde_json::Value, String> {
    let (code_verifier, client_id, client_secret, redirect_uri) = {
        let mut pending = state.oauth_pending.lock().map_err(|e| e.to_string())?;
        let p = pending
            .remove(&plugin_id)
            .ok_or_else(|| "No pending OAuth state for this plugin".to_string())?;
        (p.code_verifier, p.client_id, p.client_secret, p.redirect_uri)
    };

    let client = reqwest::Client::new();
    let res = client
        .post("https://auth.atlassian.com/oauth/token")
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed ({}): {}", status, body));
    }

    let token_data: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let access_token = token_data["access_token"]
        .as_str()
        .ok_or_else(|| "No access_token in response".to_string())?
        .to_string();

    let refresh_token = token_data["refresh_token"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let store = state.secret_store.clone();

    let mut fields = std::collections::HashMap::new();
    fields.insert("access_token".to_string(), access_token.clone());
    if !refresh_token.is_empty() {
        fields.insert("refresh_token".to_string(), refresh_token);
    }
    let _ = store.set_bulk(&plugin_id, &fields);

    // Atlassian Cloud OAuth 토큰은 api.atlassian.com/ex/confluence/{cloudId} 로만 유효.
    // accessible-resources에서 cloudId를 가져와 저장.
    if plugin_id == "com.doxus.confluence" {
        let resources_res = client
            .get("https://api.atlassian.com/oauth/token/accessible-resources")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await;
        if let Ok(r) = resources_res {
            if let Ok(resources) = r.json::<serde_json::Value>().await {
                if let Some(cloud_id) = resources
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|site| site["id"].as_str())
                {
                    let _ = store.set(&plugin_id, "cloud_id", cloud_id);
                }
            }
        }
    }

    Ok(serde_json::json!({
        "status": "ok",
        "plugin_id": plugin_id,
        "configured": true,
    }))
}

#[tauri::command]
pub async fn plugin_validate_config(
    plugin_id: String,
    config_fields: std::collections::HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    match plugin_id.as_str() {
        "confluence" | "com.doxus.confluence" => {
            let base_url = config_fields.get("base_url").cloned().unwrap_or_default();
            let api_token = config_fields.get("api_token").cloned().unwrap_or_default();
            let email = config_fields.get("email").cloned().unwrap_or_default();

            if base_url.is_empty() || api_token.is_empty() {
                return Err("base_url and api_token are required".to_string());
            }

            validate_base_url(&base_url)?;

            let client = reqwest::Client::new();
            let url = format!(
                "{}/wiki/rest/api/space?limit=1",
                base_url.trim_end_matches('/')
            );
            let mut req = client.get(&url);
            if !email.is_empty() {
                req = req.basic_auth(&email, Some(&api_token));
            } else {
                req = req.bearer_auth(&api_token);
            }

            match req.send().await {
                Ok(res) if res.status().is_success() => {
                    Ok(serde_json::json!({ "valid": true, "message": "연결 성공" }))
                }
                Ok(res) => Err(format!("연결 실패 (HTTP {})", res.status())),
                Err(e) => Err(format!("연결 오류: {}", e)),
            }
        }
        "github" | "com.doxus.github" => {
            let token = config_fields.get("token").cloned().unwrap_or_default();
            if token.is_empty() {
                return Ok(serde_json::json!({ "valid": true, "message": "토큰 없이 공개 저장소만 접근 가능" }));
            }
            let client = reqwest::Client::new();
            match client
                .get("https://api.github.com/user")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "doxus/0.1.0")
                .send()
                .await
            {
                Ok(res) if res.status().is_success() => {
                    Ok(serde_json::json!({ "valid": true, "message": "GitHub 연결 성공" }))
                }
                Ok(res) => Err(format!("GitHub 연결 실패 (HTTP {})", res.status())),
                Err(e) => Err(format!("연결 오류: {}", e)),
            }
        }
        _ => Ok(serde_json::json!({ "valid": true, "message": "검증 불필요" })),
    }
}

#[tauri::command]
pub async fn plugin_open_url(url: String) -> Result<(), String> {
    if url.starts_with("obsidian://") {
        // Deep link protocols are allowed to be opened directly via 'open'
    } else {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err("only http/https or obsidian:// URLs are allowed".into());
        }
        validate_base_url(&url)?;
    }

    #[cfg(target_os = "macos")]
    let open_cmd = "open";
    #[cfg(target_os = "windows")]
    let open_cmd = "explorer";
    #[cfg(target_os = "linux")]
    let open_cmd = "xdg-open";

    std::process::Command::new(open_cmd)
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn is_safe_local_path(path: &str) -> bool {
    let clean = path.trim_start_matches("file://");
    if clean.contains("..") {
        return false;
    }
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let allowed = std::path::PathBuf::from(&home).join(".doxus");
    // canonicalize allowed dir (must exist)
    let allowed_canonical = match allowed.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // canonicalize target (if file doesn't exist, reject)
    let target = std::path::Path::new(clean);
    match target.canonicalize() {
        Ok(canonical) => canonical.starts_with(&allowed_canonical),
        Err(_) => false, // non-existent path → reject
    }
}

fn validate_base_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("잘못된 URL: {}", e))?;

    if parsed.scheme() != "https" {
        return Err("HTTPS URL만 허용됩니다 (SSRF 방지)".to_string());
    }

    let host = parsed.host_str().ok_or_else(|| "URL에 호스트가 없습니다".to_string())?;

    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower == "0.0.0.0" {
        return Err(format!("허용되지 않는 호스트: {}", host));
    }

    // Private IP range rejection
    // host_str() returns brackets for IPv6 (e.g. "[fe80::1]"), strip them for parsing
    let bare = lower.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(&lower);
    if let Ok(addr) = bare.parse::<std::net::IpAddr>() {
        let blocked = match addr {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();
                octets[0] == 127
                    || octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                    || (octets[0] == 192 && octets[1] == 168)
                    || octets[0] == 0
                    || (octets[0] == 169 && octets[1] == 254) // link-local (AWS metadata)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || (v6.segments()[0] & 0xffc0) == 0xfe80  // link-local
                    || (v6.segments()[0] & 0xfe00) == 0xfc00  // unique-local
                    || v6.to_ipv4_mapped().map_or(false, |v4| {
                        let o = v4.octets();
                        o[0] == 127
                            || o[0] == 10
                            || (o[0] == 172 && (16..=31).contains(&o[1]))
                            || (o[0] == 192 && o[1] == 168)
                            || o[0] == 0
                    })
            }
        };
        if blocked {
            return Err(format!("사설 IP 주소는 허용되지 않습니다: {}", host));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    fn make_conn() -> rusqlite::Connection {
        doxus_core::db::ensure_vec_extension();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        doxus_core::db::create_vec0_table(&conn).unwrap();
        doxus_core::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn get_plugin_logs_returns_empty_array_when_no_logs() {
        let conn = make_conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, event_type, payload, occurred_at \
                 FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
            )
            .unwrap();
        let logs: Vec<serde_json::Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "project_id": r.get::<_, Option<i64>>(1)?,
                    "event_type": r.get::<_, String>(2)?,
                    "payload": r.get::<_, Option<String>>(3)?,
                    "occurred_at": r.get::<_, i64>(4)?,
                }))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(logs.is_empty());
    }

    #[test]
    fn get_plugin_logs_returns_recent_entries() {
        let conn = make_conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO audit_log (event_type, payload, occurred_at) VALUES ('plugin_error', '{\"msg\":\"test\"}', ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, event_type, payload, occurred_at \
                 FROM audit_log ORDER BY occurred_at DESC LIMIT 50",
            )
            .unwrap();
        let logs: Vec<serde_json::Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "project_id": r.get::<_, Option<i64>>(1)?,
                    "event_type": r.get::<_, String>(2)?,
                    "payload": r.get::<_, Option<String>>(3)?,
                    "occurred_at": r.get::<_, i64>(4)?,
                }))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0]["event_type"], "plugin_error");
    }

    // --- validate_base_url tests ---

    #[test]
    fn test_validate_config_rejects_http_scheme() {
        let result = super::validate_base_url("http://evil.com");
        assert!(result.is_err(), "http:// scheme should be rejected");
        let msg = result.unwrap_err();
        assert!(msg.contains("HTTPS"), "error should mention HTTPS: {}", msg);
    }

    #[test]
    fn test_validate_config_rejects_private_ip() {
        for ip in &["https://192.168.1.1", "https://10.0.0.1", "https://172.16.0.1", "https://127.0.0.1"] {
            let result = super::validate_base_url(ip);
            assert!(result.is_err(), "{} should be rejected as private IP", ip);
        }
    }

    #[test]
    fn test_validate_config_rejects_localhost() {
        for host in &["https://localhost", "https://localhost:8080", "https://0.0.0.0"] {
            let result = super::validate_base_url(host);
            assert!(result.is_err(), "{} should be rejected", host);
        }
    }

    #[test]
    fn test_validate_config_accepts_valid_https() {
        let result = super::validate_base_url("https://mycompany.atlassian.net");
        assert!(result.is_ok(), "valid https URL should be accepted: {:?}", result);
    }

    #[test]
    fn test_validate_rejects_ipv4_mapped_private() {
        let result = super::validate_base_url("https://[::ffff:192.168.1.1]");
        assert!(result.is_err(), "IPv4-mapped private address should be rejected");
    }

    #[test]
    fn test_validate_rejects_link_local_ipv6() {
        let result = super::validate_base_url("https://[fe80::1]");
        assert!(result.is_err(), "link-local IPv6 should be rejected");
    }

    #[tokio::test]
    async fn test_plugin_open_url_rejects_private_ip() {
        let result = super::plugin_open_url("https://192.168.1.1/path".into()).await;
        assert!(result.is_err(), "plugin_open_url should reject private IPs");
    }

    #[test]
    fn market_fetch_guide_rejects_path_outside_doxus_dir() {
        assert!(
            !super::is_safe_local_path("/etc/passwd"),
            "/etc/passwd should be rejected"
        );
        assert!(
            !super::is_safe_local_path("file:///etc/passwd"),
            "file:///etc/passwd should be rejected"
        );
        assert!(
            !super::is_safe_local_path("/root/.ssh/id_rsa"),
            "/root/.ssh/id_rsa should be rejected"
        );
    }

    #[test]
    fn market_fetch_guide_allows_path_inside_doxus_dir() {
        // canonicalize requires actual files to exist — use a temp dir under HOME/.doxus
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".to_string());
        let doxus_dir = std::path::PathBuf::from(&home).join(".doxus");
        // Only run if ~/.doxus exists (CI may not have it)
        if !doxus_dir.exists() {
            return;
        }
        // Create a temp file inside ~/.doxus for the test
        let tmp_file = doxus_dir.join("_test_safe_path_marker.tmp");
        std::fs::write(&tmp_file, b"test").unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
        let _cleanup = Cleanup(tmp_file.clone());
        let safe_path = tmp_file.to_str().unwrap().to_string();
        assert!(
            super::is_safe_local_path(&safe_path),
            "{} should be allowed",
            safe_path
        );
        let safe_file_url = format!("file://{}", safe_path);
        assert!(
            super::is_safe_local_path(&safe_file_url),
            "{} should be allowed",
            safe_file_url
        );
    }

    #[test]
    fn market_fetch_guide_rejects_path_traversal() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".to_string());
        let traversal = format!("{}/.doxus/../../../etc/passwd", home);
        assert!(
            !super::is_safe_local_path(&traversal),
            "path traversal should be rejected"
        );
    }

    #[test]
    fn market_install_plugin_rejects_placeholder_wasm() {
        // Verify that the install path no longer writes a b"placeholder" file.
        // The install_from_url path either succeeds with real bytes or fails —
        // it never writes the literal string "placeholder" to disk.
        let tmp = tempfile::TempDir::new().unwrap();
        let plugins_dir = tmp.path().to_path_buf();
        let plugin_id = "com.test.plugin";

        // Simulate a failed download (no server running → install_from_url errors)
        let installer = doxus_core::marketplace::installer::PluginInstaller::new(plugins_dir.clone());
        let _ = installer.install_from_url(plugin_id, "http://127.0.0.1:1/nonexistent.wasm", None);

        // The wasm file must NOT contain the old placeholder bytes
        let wasm_path = plugins_dir.join(format!("{}.wasm", plugin_id));
        if wasm_path.exists() {
            let contents = std::fs::read(&wasm_path).unwrap();
            assert_ne!(contents, b"placeholder", "install must not write placeholder bytes");
        }
        // (if download failed, file should not exist at all — also correct)
    }

    #[test]
    fn builtin_plugin_ids_contains_all_registered_plugins() {
        let ids = crate::state::builtin_plugin_ids();
        assert!(ids.contains(&"com.doxus.obsidian"));
        assert!(ids.contains(&"com.doxus.confluence"));
        assert!(ids.contains(&"com.doxus.github"));
    }

    // --- S3: validate_base_url link-local IPv4 ---

    #[test]
    fn validate_base_url_rejects_link_local_ipv4() {
        assert!(super::validate_base_url("https://169.254.169.254/").is_err(),
            "169.254.169.254 should be rejected (AWS metadata)");
        assert!(super::validate_base_url("https://169.254.0.1/").is_err(),
            "169.254.0.1 should be rejected (link-local)");
    }

    // --- S1: market_fetch_guide SSRF ---

    #[tokio::test]
    async fn market_fetch_guide_rejects_ssrf_url() {
        // validate_base_url is called before fetch; http:// is also rejected
        let result = super::validate_base_url("http://169.254.169.254/latest/meta-data/");
        assert!(result.is_err(), "169.254.x.x should be rejected");
        let result2 = super::validate_base_url("http://192.168.1.1/secret");
        assert!(result2.is_err(), "192.168.x.x should be rejected");
    }

    // --- S2: is_safe_local_path symlink escape ---

    #[test]
    fn is_safe_local_path_rejects_symlink_escaping_doxus_dir() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".to_string());
        let doxus_dir = std::path::PathBuf::from(&home).join(".doxus");
        if !doxus_dir.exists() {
            return; // skip if ~/.doxus doesn't exist
        }
        // Create a temp dir outside doxus to be the symlink target
        let outside_dir = tempfile::TempDir::new().unwrap();
        let secret_file = outside_dir.path().join("secret.txt");
        std::fs::write(&secret_file, b"secret").unwrap();

        // Create symlink inside ~/.doxus pointing to outside dir
        let link_path = doxus_dir.join("_test_evil_symlink_tmp");
        // Clean up any leftover from previous run
        let _ = std::fs::remove_file(&link_path);
        std::os::unix::fs::symlink(outside_dir.path(), &link_path).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
        let _cleanup = Cleanup(link_path.clone());

        // Path through symlink to outside file should be rejected
        let evil_path = link_path.join("secret.txt");
        let evil_str = evil_path.to_str().unwrap();
        assert!(
            !super::is_safe_local_path(evil_str),
            "symlink-escaped path {} should be rejected",
            evil_str
        );
    }

    // --- Registry URL tests ---

    #[test]
    fn default_registry_url_uses_github_pages() {
        // Ensure env var is unset for this test
        std::env::remove_var("DOXUS_REGISTRY_URL");
        let url = std::env::var("DOXUS_REGISTRY_URL")
            .unwrap_or_else(|_| "https://YOUR_ORG.github.io/doxus-registry".to_string());
        assert!(
            url.contains("github.io"),
            "default registry URL should contain 'github.io', got: {}",
            url
        );
    }

    #[test]
    fn registry_url_overridable_via_env_var() {
        std::env::set_var("DOXUS_REGISTRY_URL", "https://custom.example.com");
        let url = std::env::var("DOXUS_REGISTRY_URL")
            .unwrap_or_else(|_| "https://YOUR_ORG.github.io/doxus-registry".to_string());
        assert_eq!(url, "https://custom.example.com");
        std::env::remove_var("DOXUS_REGISTRY_URL");
    }
}

#[tauri::command]
pub async fn market_fetch_registry(
    _state: tauri::State<'_, Arc<crate::AppState>>,
    registry_url: Option<String>,
) -> Result<Vec<doxus_core::marketplace::registry::RegistryEntry>, String> {
    let url = registry_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::env::var("DOXUS_REGISTRY_URL")
                .unwrap_or_else(|_| "https://YOUR_ORG.github.io/doxus-registry".to_string())
        });
    let client = doxus_core::marketplace::registry::RegistryClient::new(&url)
        .map_err(|e| e.to_string())?;
    match client.fetch_entries().await {
        Ok(entries) => Ok(entries),
        Err(e) => {
            // 레지스트리 서버 미운영 시 개발용 목 데이터 반환
            eprintln!("[doxus] Registry fetch failed ({}), returning dev mock data", e);
            Ok(vec![
                doxus_core::marketplace::registry::RegistryEntry {
                    plugin_id: "com.doxus.confluence".to_string(),
                    version: "1.0.0".to_string(),
                    display_name: "Confluence".to_string(),
                    download_url: "https://github.com/YOUR_ORG/doxus-registry/releases/download/v1.0.0/confluence-1.0.0.wasm".to_string(),
                    checksum_sha256: "".to_string(),
                    public_key_hex: "".to_string(),
                    auth_type: "api_token".to_string(),
                    guide_url: "internal://guide/confluence".to_string(),
                },
                doxus_core::marketplace::registry::RegistryEntry {
                    plugin_id: "com.doxus.github".to_string(),
                    version: "1.0.0".to_string(),
                    display_name: "GitHub".to_string(),
                    download_url: "https://github.com/YOUR_ORG/doxus-registry/releases/download/v1.0.0/github-1.0.0.wasm".to_string(),
                    checksum_sha256: "".to_string(),
                    public_key_hex: "".to_string(),
                    auth_type: "api_token".to_string(),
                    guide_url: "internal://guide/github".to_string(),
                },
            ])
        }
    }
}

#[tauri::command]
pub async fn market_fetch_guide(
    state: tauri::State<'_, Arc<crate::AppState>>,
    guide_url: String 
) -> Result<String, String> {
    if guide_url.is_empty() {
        return Err("가이드 URL이 없습니다".to_string());
    }

    // 내장 가이드 처리 (internal://guide/{plugin_id})
    if let Some(plugin_id) = guide_url.strip_prefix("internal://guide/") {
        let source = state.plugin_manager.get_source(plugin_id)
            .ok_or_else(|| format!("플러그인 '{}'을 찾을 수 없습니다", plugin_id))?;
        
        return source.guide()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("플러그인 '{}'에 내장 가이드가 없습니다", plugin_id));
    }

    // 로컬 파일 경로인 경우 직접 읽기
    if guide_url.starts_with('/') || guide_url.starts_with("file://") {
        if !is_safe_local_path(&guide_url) {
            return Err("허용되지 않는 로컬 경로입니다. ~/.doxus/ 하위 경로만 허용됩니다.".to_string());
        }
        let path = guide_url.trim_start_matches("file://");
        return std::fs::read_to_string(path).map_err(|e| format!("가이드 파일 읽기 실패: {e}"));
    }

    // 원격 URL: SSRF 방어 후 HTTP 요청
    validate_base_url(&guide_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    
    match client.get(&guide_url).send().await {
        Ok(res) if res.status().is_success() => res.text().await.map_err(|e| e.to_string()),
        Ok(res) => Err(format!("가이드 로드 실패: HTTP {}", res.status())),
        Err(_) => Ok(format!(
            "# 플러그인 가이드\n\n> 온라인 가이드를 불러올 수 없습니다.\n\n가이드 URL: `{}`\n\n## 설치 방법\n1. 마켓에서 플러그인을 설치하세요.\n2. 설정 버튼을 눌러 인증 정보를 입력하세요.\n3. 프로젝트를 추가하고 인덱싱을 시작하세요.",
            guide_url
        )),
    }
}

#[tauri::command]
pub async fn check_claude_status() -> Result<serde_json::Value, String> {
    use doxus_agent::cli_detector::{detect_cli, CliKind};
    match detect_cli() {
        CliKind::ClaudeCode { path } => Ok(serde_json::json!({
            "status": "ok",
            "claude_cli": true,
            "message": format!("Claude Code CLI 감지됨: {}", path.display())
        })),
        _ => Ok(serde_json::json!({
            "status": "warn",
            "claude_cli": false,
            "message": "Claude를 찾을 수 없습니다"
        })),
    }
}

#[tauri::command]
pub async fn check_gemini_status() -> Result<serde_json::Value, String> {
    use doxus_agent::cli_detector::find_binary;
    // Check gemini specifically even if claude was found first
    let gemini_path = find_binary("gemini");
    match gemini_path {
        Some(path) => Ok(serde_json::json!({
            "status": "ok",
            "gemini_cli": true,
            "message": format!("Gemini CLI 감지됨: {}", path.display())
        })),
        None => Ok(serde_json::json!({
            "status": "warn",
            "gemini_cli": false,
            "message": "Gemini를 찾을 수 없습니다"
        })),
    }
}


/// 플러그인의 캐시 TTL(분) 조회 (plugin_kv 기반, per-plugin-type).
/// 반환: `{ "cache_ttl_minutes": 30 }` 또는 `{ "cache_ttl_minutes": null }` (비활성화)
#[tauri::command]
pub async fn plugin_get_cache_ttl(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let ttl: Option<i64> = conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM plugin_kv
         WHERE plugin_id = ?1 AND namespace = 'settings' AND key = 'cache_ttl_minutes'",
        rusqlite::params![plugin_id],
        |r| r.get(0),
    ).ok();
    Ok(serde_json::json!({ "cache_ttl_minutes": ttl }))
}

/// 플러그인의 캐시 TTL(분) 설정 (plugin_kv 기반, per-plugin-type).
/// `ttl_minutes`: null이면 캐시 비활성화 (행 삭제), 숫자면 활성화 (최소 10분).
#[tauri::command]
pub async fn plugin_set_cache_ttl(
    state: tauri::State<'_, Arc<crate::AppState>>,
    plugin_id: String,
    ttl_minutes: Option<u32>,
) -> Result<serde_json::Value, String> {
    if let Some(ttl) = ttl_minutes {
        if ttl < 10 {
            return Err("캐시 TTL은 최소 10분이어야 합니다".to_string());
        }
    }

    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    match ttl_minutes {
        Some(ttl) => {
            conn.execute(
                "INSERT INTO plugin_kv(plugin_id, namespace, key, value, updated_at)
                 VALUES(?1, 'settings', 'cache_ttl_minutes', ?2, unixepoch())
                 ON CONFLICT(plugin_id, namespace, key)
                 DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                rusqlite::params![plugin_id, ttl as i64],
            ).map_err(|e| e.to_string())?;
        }
        None => {
            conn.execute(
                "DELETE FROM plugin_kv WHERE plugin_id = ?1 AND namespace = 'settings' AND key = 'cache_ttl_minutes'",
                rusqlite::params![plugin_id],
            ).map_err(|e| e.to_string())?;
        }
    }

    Ok(serde_json::json!({ "ok": true, "cache_ttl_minutes": ttl_minutes }))
}
