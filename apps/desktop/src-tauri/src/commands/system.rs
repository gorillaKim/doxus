use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use sysinfo::{Pid, System, Disks};
use tauri::Emitter;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct ResourceUsage {
    pub cpu_usage: f32,           // %
    pub memory_usage: u64,        // Bytes (RSS)
    pub total_memory: u64,        // Bytes
    pub disk_usage: u64,          // Bytes (~/.doxus size)
    pub total_disk: u64,          // Bytes
    pub available_disk: u64,      // Bytes
}

fn get_dir_size(path: PathBuf) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

#[tauri::command]
pub async fn get_resource_usage() -> Result<ResourceUsage, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = Pid::from_u32(std::process::id());
    
    // Process-specific info
    let cpu_usage = if let Some(process) = sys.process(pid) {
        process.cpu_usage()
    } else {
        0.0
    };

    let memory_usage = if let Some(process) = sys.process(pid) {
        process.memory()
    } else {
        0
    };

    // System-wide info
    let total_memory = sys.total_memory();
    
    // Disk info: ~/.doxus/db (where the Knowledge Index grows)
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let doxus_db_dir = PathBuf::from(&home).join(".doxus/db");
    let disk_usage = if doxus_db_dir.exists() {
        get_dir_size(doxus_db_dir)
    } else {
        0
    };

    // Overall disk capacity where home is located
    let mut total_disk = 0;
    let mut available_disk = 0;
    
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount_point = disk.mount_point().to_str().unwrap_or("");
        if !mount_point.is_empty() && home.starts_with(mount_point) {
            total_disk = disk.total_space();
            available_disk = disk.available_space();
            // Don't break immediately, we might find a more specific match (shorter path vs longer path)
            // But usually the first match is fine for home dir.
        }
    }

    Ok(ResourceUsage {
        cpu_usage,
        memory_usage,
        total_memory,
        disk_usage,
        total_disk,
        available_disk,
    })
}

// ── Model download commands ─────────────────────────────────────────────────

fn default_model_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".doxus/models")
}

/// Check whether the ONNX model + tokenizer are already present on disk.
///
/// Returns `{ "exists": bool, "path": Option<String> }`.
#[tauri::command]
pub async fn check_model_status() -> Result<serde_json::Value, String> {
    let path = doxus_core::embedding::resolve_model_path();
    let exists = path.is_some();
    Ok(serde_json::json!({
        "exists": exists,
        "path": path.map(|p| p.to_string_lossy().to_string()),
    }))
}

/// Download the default ONNX model and tokenizer into `~/.doxus/models/`,
/// emitting progress events and atomically swapping the running embedder on success.
///
/// Events:
/// - `model:download-progress` — `{ file, percent, bytes_downloaded, total_bytes, status: "downloading" }`
/// - `model:download-complete` — `{}` after the new embedder has been loaded
///
/// Errors are returned as `Err(String)`; partial files are removed by the
/// downloader's cleanup contract.
#[tauri::command]
pub async fn download_onnx_model(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<serde_json::Value, String> {
    let target_dir = default_model_dir();
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("failed to create model directory: {e}"))?;

    let handle = app_handle.clone();
    let result = doxus_core::model_downloader::download_model(&target_dir, move |p| {
        let _ = handle.emit(
            "model:download-progress",
            serde_json::json!({
                "file": p.file,
                "percent": p.percent,
                "bytes_downloaded": p.bytes_downloaded,
                "total_bytes": p.total_bytes,
                "status": "downloading",
            }),
        );
    })
    .await;

    if let Err(e) = result {
        return Err(format!("download failed: {e}"));
    }

    // Load the new embedder and swap it in-place.
    let new_embedder = tokio::task::spawn_blocking(doxus_core::embedding::OnnxEmbedder::from_default_path)
        .await
        .map_err(|e| format!("embedder load task failed: {e}"))?
        .map_err(|e| format!("failed to load ONNX model after download: {e}"))?;

    {
        let mut guard = state.embedder.write().await;
        *guard = Arc::new(new_embedder);
    }

    app_handle
        .emit("model:download-complete", serde_json::json!({}))
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({ "status": "ok" }))
}
