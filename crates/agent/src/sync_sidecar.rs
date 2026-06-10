//! Synchronous Node.js sidecar manager with JSONL protocol.
//!
//! Uses separate Arc<Mutex<>> for stdin and reader so that concurrent
//! send_request() calls never block the background reader thread.

use std::io::{BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Find the Node.js binary. macOS GUI apps don't inherit the user's shell PATH.
fn find_node_binary() -> std::path::PathBuf {
    let system_candidates = [
        "/usr/local/bin/node",
        "/opt/homebrew/bin/node",
        "/usr/bin/node",
    ];
    for path in &system_candidates {
        if Path::new(path).exists() {
            return std::path::PathBuf::from(path);
        }
    }
    // nvm fallback
    if let Ok(home) = std::env::var("HOME") {
        let nvm_dir = std::path::PathBuf::from(home).join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm_dir) {
            let mut versions: Vec<_> = entries.flatten().collect();
            versions.sort_by_key(|e| e.file_name());
            for entry in versions.iter().rev() {
                let candidate = entry.path().join("bin/node");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    std::path::PathBuf::from("node")
}

/// Manages a Node.js sidecar process with JSONL protocol.
///
/// stdin and reader use SEPARATE mutexes so send_request() is never blocked
/// by a concurrent blocking read_line in the background reader thread.
pub struct SyncSidecarManager {
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// Taken by the background reader thread after ensure_running().
    reader: Mutex<Option<BufReader<ChildStdout>>>,
    child: Mutex<Option<Child>>,
    debug_enabled: AtomicBool,
}

impl SyncSidecarManager {
    pub fn new() -> Self {
        Self {
            stdin: Arc::new(Mutex::new(None)),
            reader: Mutex::new(None),
            child: Mutex::new(None),
            debug_enabled: AtomicBool::new(false),
        }
    }

    pub fn set_debug(&self, enabled: bool) {
        self.debug_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Acquire a mutex, recovering from poisoning (safe — poison just means
    /// a previous holder panicked; the data is still usable).
    fn lock_child(&self) -> std::sync::MutexGuard<'_, Option<Child>> {
        self.child.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn lock_stdin(&self) -> std::sync::MutexGuard<'_, Option<ChildStdin>> {
        self.stdin.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn lock_reader(&self) -> std::sync::MutexGuard<'_, Option<BufReader<ChildStdout>>> {
        self.reader.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start the sidecar if not already running (or if it has crashed).
    pub fn ensure_running(&self, script: &Path) -> Result<(), String> {
        let mut child_guard = self.lock_child();

        // Check if existing child is still alive
        if let Some(child) = child_guard.as_mut() {
            match child.try_wait() {
                Ok(None) => return Ok(()), // still running
                _ => {
                    child_guard.take();
                    self.lock_stdin().take();
                    self.lock_reader().take();
                }
            }
        }

        let node_bin = find_node_binary();
        if self.debug_enabled.load(Ordering::Relaxed) {
            eprintln!(
                "[sidecar] Starting: {} {}",
                node_bin.display(),
                script.display()
            );
        }

        let mut child = Command::new(&node_bin)
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("failed to start sidecar: {e}"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;

        *self.lock_stdin() = Some(stdin);
        *self.lock_reader() = Some(BufReader::new(stdout));
        *child_guard = Some(child);

        if self.debug_enabled.load(Ordering::Relaxed) {
            eprintln!("[sidecar] Started");
        }
        Ok(())
    }

    /// Take ownership of the reader for the background reader thread.
    pub fn take_reader(&self) -> Option<BufReader<ChildStdout>> {
        self.lock_reader().take()
    }

    /// Send a JSONL request to the sidecar stdin.
    pub fn send_request(&self, request: &serde_json::Value) -> Result<(), String> {
        let mut guard = self.lock_stdin();
        let stdin = guard.as_mut().ok_or("sidecar not running")?;
        let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.lock_child()
            .as_mut()
            .map(|c| c.try_wait().ok().and_then(|s| s).is_none())
            .unwrap_or(false)
    }

    pub fn shutdown(&self) {
        self.lock_stdin().take();
        if let Some(mut child) = self.lock_child().take() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    if self.debug_enabled.load(Ordering::Relaxed) {
                        eprintln!("[sidecar] already exited");
                    }
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    if self.debug_enabled.load(Ordering::Relaxed) {
                        eprintln!("[sidecar] killed");
                    }
                }
            }
        }
    }
}

impl Default for SyncSidecarManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SyncSidecarManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
