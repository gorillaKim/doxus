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
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let installed_ids = state
        .plugin_manager
        .list_installed()
        .map_err(|e| e.to_string())?;

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
            "installed": installed_ids.contains(&"com.doxus.confluence".to_string()),
            "builtin": false,
            "config_schema": config_schema(&[
                ("name", "프로젝트 이름", "text", true, "confluence-docs"),
                ("base_url", "Base URL", "url", true, "https://yourcompany.atlassian.net"),
                ("space_key", "스페이스 키", "text", false, "ENG"),
            ]),
            "auth_type": "oauth",
            "auth_schema": config_schema(&[
                ("client_id", "Client ID", "text", true, "your-atlassian-app-client-id"),
                ("client_secret", "Client Secret", "password", true, "your-atlassian-app-client-secret"),
            ])
        }),
        serde_json::json!({
            "id": "com.doxus.github",
            "name": "GitHub",
            "version": "1.0.0",
            "trust": "official",
            "description": "GitHub Issues, Wiki, Discussions",
            "installed": installed_ids.contains(&"com.doxus.github".to_string()),
            "builtin": false,
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
    let builtin_ids = ["com.doxus.obsidian", "com.doxus.confluence", "com.doxus.github"];
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
    state: tauri::State<'_, crate::AppState>,
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
pub async fn market_install_plugin(
    state: tauri::State<'_, crate::AppState>,
    plugin_id: String,
    registry_url: Option<String>,
) -> Result<serde_json::Value, String> {
    // Verify trust anchor: plugin must exist in registry and have a non-empty public_key_hex.
    let url = registry_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://registry.doxus.io".to_string());
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

    // Write a placeholder .wasm file so list_installed() picks it up on next load.
    // Real WASM download + signature verification happens in Phase 4 registry implementation.
    let plugins_dir = &state.plugins_dir;
    std::fs::create_dir_all(plugins_dir).map_err(|e| e.to_string())?;
    let wasm_path = plugins_dir.join(format!("{}.wasm", plugin_id));
    std::fs::write(&wasm_path, b"placeholder").map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "status": "ok", "installed": true, "plugin_id": plugin_id }))
}

#[tauri::command]
pub async fn market_uninstall_plugin(
    state: tauri::State<'_, crate::AppState>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let wasm_path = state.plugins_dir.join(format!("{}.wasm", plugin_id));
    if wasm_path.exists() {
        std::fs::remove_file(&wasm_path).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({ "status": "ok", "installed": false, "plugin_id": plugin_id }))
}

#[tauri::command]
pub async fn plugin_save_auth(
    plugin_id: String,
    auth_fields: std::collections::HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    for (key, value) in &auth_fields {
        let service = "doxus";
        let username = format!("doxus:{}:{}", plugin_id, key);
        match keyring::Entry::new(service, &username) {
            Ok(entry) => {
                if let Err(e) = entry.set_password(value) {
                    eprintln!("keyring set_password failed for {}: {}", username, e);
                }
            }
            Err(e) => {
                eprintln!("keyring entry creation failed for {}: {}", username, e);
            }
        }
    }
    Ok(serde_json::json!({ "status": "ok" }))
}

#[tauri::command]
pub async fn plugin_get_auth_status(
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let keys_to_check: &[&str] = match plugin_id.as_str() {
        "com.doxus.confluence" => &["access_token", "api_token"],
        "com.doxus.github" => &["access_token", "token"],
        _ => &[],
    };

    let configured = keys_to_check.iter().any(|key| {
        keyring::Entry::new("doxus", &format!("doxus:{}:{}", plugin_id, key))
            .ok()
            .and_then(|e| e.get_password().ok())
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    });

    Ok(serde_json::json!({ "configured": configured, "plugin_id": plugin_id }))
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
    state: tauri::State<'_, crate::AppState>,
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
    state: tauri::State<'_, crate::AppState>,
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

    let kr_access =
        keyring::Entry::new("doxus", &format!("doxus:{}:access_token", plugin_id))
            .map_err(|e| e.to_string())?;
    kr_access.set_password(&access_token).map_err(|e| e.to_string())?;

    if !refresh_token.is_empty() {
        if let Ok(kr_refresh) =
            keyring::Entry::new("doxus", &format!("doxus:{}:refresh_token", plugin_id))
        {
            let _ = kr_refresh.set_password(&refresh_token);
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
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("only http/https URLs are allowed".into());
    }
    validate_base_url(&url)?;
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
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
        let conn = rusqlite::Connection::open_in_memory().unwrap();
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
}

#[tauri::command]
pub async fn market_fetch_registry(
    _state: tauri::State<'_, crate::AppState>,
    registry_url: Option<String>,
) -> Result<Vec<doxus_core::marketplace::registry::RegistryEntry>, String> {
    let url = registry_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://registry.doxus.io".to_string());
    let client = doxus_core::marketplace::registry::RegistryClient::new(&url)
        .map_err(|e| e.to_string())?;
    client.fetch_entries().await.map_err(|e| e.to_string())
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

#[tauri::command]
pub async fn get_workspaces(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let repo = doxus_core::workspace::WorkspaceRepo::new(&conn);
    let workspaces = repo.list().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "workspaces": workspaces }))
}
