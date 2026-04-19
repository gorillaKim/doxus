use serde::Serialize;
use std::path::PathBuf;
use sysinfo::{Pid, System, Disks};
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
