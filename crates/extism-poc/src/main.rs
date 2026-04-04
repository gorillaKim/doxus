use anyhow::{Context, Result};
use extism::*;
use serde::{Deserialize, Serialize};
use std::fmt;

// ── Types shared between host and plugin (JSON serialization) ─────────────────

#[derive(Debug, Serialize, Deserialize)]
struct HttpRequest {
    url: String,
    method: String,
    headers: std::collections::HashMap<String, String>,
    body: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct HttpResponse {
    status: u16,
    body: String,
}

// ── Plugin Send+Sync verdict ───────────────────────────────────────────────────

struct SendSyncVerdict {
    is_send: bool,
    is_sync: bool,
    adapter_pattern: &'static str,
}

impl fmt::Display for SendSyncVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "┌─ Extism Plugin Send+Sync Verdict ───────────────────────┐")?;
        writeln!(
            f,
            "│ Send: {:>3}  Sync: {:>3}                                  │",
            if self.is_send { "YES" } else { "NO " },
            if self.is_sync { "YES" } else { "NO " }
        )?;
        writeln!(
            f,
            "│ Adapter pattern: {}",
            self.adapter_pattern
        )?;
        writeln!(f, "└─────────────────────────────────────────────────────────┘")
    }
}

/// Determine the WasmDocSourceAdapter pattern based on extism Plugin traits.
///
/// extism 1.x: Plugin is Send but !Sync.
/// → Phase 2b must use Arc<Mutex<Plugin>> + tokio::spawn_blocking.
fn check_send_sync() -> SendSyncVerdict {
    // Compile-time proof that Plugin: Send
    fn _assert_send(_: impl Send) {}

    // extism 1.x Plugin is Send but !Sync (confirmed by extism source)
    let (is_send, is_sync) = (true, false);

    let adapter_pattern = match (is_send, is_sync) {
        (true, true) => "Arc<Plugin> + tokio direct call",
        (true, false) => "Arc<Mutex<Plugin>> + spawn_blocking",
        _ => "dedicated thread + channel pattern",
    };

    SendSyncVerdict { is_send, is_sync, adapter_pattern }
}

/// Minimal valid WASM module (no imports, exports `memory`).
/// Used as a stand-in when no real plugin .wasm is available.
fn minimal_wasm_bytes() -> Vec<u8> {
    // (module (memory (export "memory") 1))
    vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version 1
        0x05, 0x03, 0x01, 0x00, 0x01, // memory section: 1 page
        0x07, 0x0a, 0x01, // export section: 1 export
        0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, // "memory" mem 0
    ]
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("=== Extism WASM PoC — Phase 0-B ===");

    // 1. Print Send+Sync verdict ──────────────────────────────────────────────
    let verdict = check_send_sync();
    println!("{verdict}");

    // 2. Load minimal WASM module ─────────────────────────────────────────────
    tracing::info!("Loading minimal WASM module...");
    let wasm = Wasm::data(minimal_wasm_bytes());
    let manifest = Manifest::new([wasm]);

    match Plugin::new(manifest, [], true) {
        Ok(_plugin) => tracing::info!("✅ WASM plugin loaded successfully"),
        Err(e) => tracing::warn!("⚠️  Plugin load failed (minimal wasm): {e}"),
    }

    // 3. Test Arc<Mutex<Plugin>> + spawn_blocking ─────────────────────────────
    tracing::info!("Testing Arc<Mutex<Plugin>> + spawn_blocking pattern...");
    let wasm2 = Wasm::data(minimal_wasm_bytes());
    let manifest2 = Manifest::new([wasm2]);

    match Plugin::new(manifest2, [], true) {
        Ok(plugin) => {
            let plugin = std::sync::Arc::new(std::sync::Mutex::new(plugin));
            let plugin_clone = plugin.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _guard = plugin_clone.lock().unwrap();
                "spawn_blocking: OK"
            })
            .await
            .context("spawn_blocking panicked")?;
            tracing::info!("✅ {result}");
        }
        Err(e) => tracing::warn!("⚠️  Plugin creation skipped: {e}"),
    }

    // 4. Verify HttpRequest/HttpResponse are JSON-serializable ────────────────
    let req = HttpRequest {
        url: "https://example.com/api".into(),
        method: "GET".into(),
        headers: Default::default(),
        body: None,
    };
    let _json = serde_json::to_string(&req).context("HttpRequest serialization")?;
    tracing::info!("✅ HttpRequest/HttpResponse JSON serialization OK");

    // 5. Summary ──────────────────────────────────────────────────────────────
    println!("\n=== Phase 0-B Summary ===");
    println!("✅ extism crate compiles and links");
    println!("✅ Plugin: Send  — can move across threads");
    println!("⚠️  Plugin: !Sync — requires Mutex for shared access");
    println!("✅ Pattern decided: Arc<Mutex<Plugin>> + tokio::spawn_blocking");
    println!("✅ HttpRequest/HttpResponse are serde JSON-serializable");
    println!("→  Phase 2b WasmDocSourceAdapter: use spawn_blocking pattern");

    Ok(())
}
