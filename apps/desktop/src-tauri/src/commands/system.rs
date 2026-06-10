use once_cell::sync::Lazy;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use sysinfo::{Disks, Pid, ProcessesToUpdate, System};
use tauri::Emitter;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct ResourceUsage {
    pub cpu_usage: f32,      // Normalized % (0-100)
    pub memory_usage: u64,   // Bytes (RSS)
    pub total_memory: u64,   // Bytes
    pub disk_usage: u64,     // Bytes (~/.doxus/db + models size)
    pub total_disk: u64,     // Bytes
    pub available_disk: u64, // Bytes
}

fn get_dir_size(path: PathBuf) -> u64 {
    if !path.exists() {
        return 0;
    }
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

static SYSTEM: Lazy<Arc<Mutex<System>>> = Lazy::new(|| Arc::new(Mutex::new(System::new())));

type DiskInfo = (std::time::Instant, u64, u64, u64);
static DISK_INFO_CACHE: Lazy<Arc<Mutex<DiskInfo>>> = Lazy::new(|| {
    Arc::new(Mutex::new((
        std::time::Instant::now() - std::time::Duration::from_secs(3600),
        0,
        0,
        0,
    )))
});

#[tauri::command]
pub async fn get_resource_usage() -> Result<ResourceUsage, String> {
    let mut sys = SYSTEM.lock().map_err(|_| "system lock poisoned")?;

    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.refresh_memory();

    // sysinfo cpu_usage() returns 0..100*N_CORES scale; normalize with actual core count.
    // sys.cpus() requires a separate refresh_cpu_list call, so use std instead.
    let core_count = std::thread::available_parallelism()
        .map(|n| n.get() as f32)
        .unwrap_or(1.0);
    let (cpu_usage, memory_usage): (f32, u64) = if let Some(process) = sys.process(pid) {
        let normalized = (process.cpu_usage() / core_count).min(100.0);
        (normalized, process.memory())
    } else {
        (0.0, 0)
    };

    let total_memory = sys.total_memory();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let doxus_root = PathBuf::from(&home).join(".doxus");
    let doxus_db_dir = doxus_root.join("db");
    let doxus_models_dir = doxus_root.join("models");

    let (disk_usage, total_disk, available_disk) = {
        let mut cache = DISK_INFO_CACHE
            .lock()
            .map_err(|_| "disk cache lock poisoned")?;
        if cache.0.elapsed() > std::time::Duration::from_secs(60) {
            let usage = get_dir_size(doxus_db_dir) + get_dir_size(doxus_models_dir);

            let mut total = 0;
            let mut available = 0;
            let disks = Disks::new_with_refreshed_list();
            for disk in &disks {
                let mount_point = disk.mount_point().to_str().unwrap_or("");
                if !mount_point.is_empty() && home.starts_with(mount_point) {
                    total = disk.total_space();
                    available = disk.available_space();
                    break;
                }
            }

            cache.0 = std::time::Instant::now();
            cache.1 = usage;
            cache.2 = total;
            cache.3 = available;
            (usage, total, available)
        } else {
            (cache.1, cache.2, cache.3)
        }
    };

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
    let new_embedder =
        tokio::task::spawn_blocking(doxus_core::embedding::OnnxEmbedder::from_default_path)
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
