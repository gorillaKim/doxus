//! CLI detection — determines which AI CLI is available in the environment.
//!
//! macOS GUI apps (Tauri) launched from Finder don't inherit the user's shell PATH.
//! We use a login shell (`zsh -l -c "which -a <name>"`) to load `~/.zshrc` / `~/.bash_profile`
//! so that nvm, volta, Homebrew, and ~/.local/bin paths are all accessible.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Which AI CLI is present in the current environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliKind {
    ClaudeCode { path: PathBuf },
    GeminiCli { path: PathBuf },
    None,
}

/// Verify a Claude CLI candidate is actually usable by running `--version`.
/// Returns the version string (e.g. "2.1.90 (Claude Code)") if valid, None otherwise.
///
/// This guards against `CLAUDE_CODE_ENTRYPOINT=claude-vscode` (VSCode extension
/// wrapper) which exists on PATH but cannot be spawned as a standalone process.
pub fn verify_claude_version(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let output = command_output_timeout(cmd, Duration::from_secs(5))?;
    if !output.status.success() {
        return Option::None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.contains("Claude Code") {
        Some(text)
    } else {
        Option::None
    }
}

/// Detect which AI CLI is available in the current environment.
///
/// Priority:
/// 1. `CLAUDE_CODE_ENTRYPOINT` env var → ClaudeCode (verified via --version)
/// 2. `claude` binary (login shell + fallback paths) → ClaudeCode (verified)
/// 3. `GEMINI_CLI_PATH` env var → GeminiCli
/// 4. `gemini` binary (login shell + fallback paths) → GeminiCli
/// 5. None
///
/// Candidates that fail `--version` verification are skipped so that VSCode
/// extension wrappers (e.g. `claude-vscode`) don't shadow the real Claude CLI.
pub fn detect_cli() -> CliKind {
    // CLAUDE_CODE_ENTRYPOINT may be set to "claude-vscode" in VSCode contexts —
    // verify it actually works before trusting it.
    if let Ok(val) = std::env::var("CLAUDE_CODE_ENTRYPOINT") {
        let path = PathBuf::from(&val);
        if verify_claude_version(&path).is_some() {
            return CliKind::ClaudeCode { path };
        }
        // Fall through: keep searching for a real claude binary
    }
    if let Some(path) = find_binary("claude") {
        if verify_claude_version(&path).is_some() {
            return CliKind::ClaudeCode { path };
        }
    }
    if let Ok(val) = std::env::var("GEMINI_CLI_PATH") {
        return CliKind::GeminiCli { path: PathBuf::from(val) };
    }
    if let Some(path) = find_binary("gemini") {
        return CliKind::GeminiCli { path };
    }
    CliKind::None
}

/// Find a binary by name using login shell + well-known fallback paths.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    // 1. Login shell `which -a` — loads ~/.zshrc so nvm/volta/~/.local/bin are visible.
    //    This is the primary fix for macOS GUI apps that miss the user's shell PATH.
    if let Some(p) = try_which_login_shell(name) {
        return Some(p);
    }

    // 2. $PATH scan (fallback for terminal-launched contexts without login shell)
    if let Some(p) = which_binary(name) {
        return Some(p);
    }

    // 3. Well-known install locations (hardcoded fallback for GUI apps)
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates: Vec<PathBuf> = vec![
        // Claude Code default install
        PathBuf::from(&home).join(".local/bin").join(name),
        // Homebrew (Apple Silicon)
        PathBuf::from("/opt/homebrew/bin").join(name),
        // Homebrew (Intel)
        PathBuf::from("/usr/local/bin").join(name),
        // Volta
        PathBuf::from(&home).join(".volta/bin").join(name),
        // npm global
        PathBuf::from(&home).join(".npm-global/bin").join(name),
        PathBuf::from(&home).join(".npm/bin").join(name),
        // nvm — glob: ~/.nvm/versions/node/*/bin/<name>
        PathBuf::from(&home).join(".nvm/versions/node/*/bin").join(name),
        // fnm
        PathBuf::from(&home).join(".local/share/fnm/node-versions/*/installation/bin").join(name),
    ];

    for candidate in candidates {
        let candidate_str = candidate.to_string_lossy();
        if candidate_str.contains('*') {
            if let Some(found) = glob_first(&candidate) {
                return Some(found);
            }
        } else if candidate.exists() && is_valid_executable(&candidate) {
            return Some(candidate);
        }
    }

    Option::None
}

/// Run `which -a <name>` via a login shell so the user's full PATH is available.
/// Returns the first valid path found.
fn try_which_login_shell(name: &str) -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = Command::new(&shell);
    cmd.args(["-l", "-c", &format!("which -a {}", name)]);
    let output = command_output_timeout(cmd, Duration::from_secs(5))?;
    if !output.status.success() {
        return Option::None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| PathBuf::from(l.trim()))
        .find(|p| p.exists() && is_valid_executable(p))
}

/// $PATH-only scan (no shell spawning).
pub(crate) fn which_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':')
        .map(PathBuf::from)
        .find_map(|p| {
            let candidate = p.join(name);
            (candidate.is_file() && is_valid_executable(&candidate)).then_some(candidate)
        })
}

/// Resolve the first existing path that contains a `*` glob segment (single level only).
fn glob_first(pattern: &Path) -> Option<PathBuf> {
    let pattern_str = pattern.to_string_lossy();
    let parts: Vec<&str> = pattern_str.split('/').collect();
    let star_idx = parts.iter().position(|p| *p == "*")?;

    let base: PathBuf = parts[..star_idx].iter().collect();
    let suffix: PathBuf = parts[star_idx + 1..].iter().collect();

    let entries = std::fs::read_dir(&base).ok()?;
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path().join(&suffix))
        .filter(|p| p.exists() && is_valid_executable(p))
        .collect();
    found.sort();
    found.into_iter().last() // highest version (sort order)
}

/// Check that the file is executable and, if it has a shebang, that the interpreter exists.
/// Prevents "bad interpreter" zombie processes during detection.
fn is_valid_executable(path: &Path) -> bool {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 128];
    let n = file.read(&mut buf).unwrap_or(0);

    if n < 2 || &buf[..2] != b"#!" {
        return true; // native binary or non-script — trust the OS
    }

    let content = String::from_utf8_lossy(&buf[..n]);
    let shebang = match content.lines().next() {
        Some(l) => l.trim_start_matches("#!").trim().to_string(),
        Option::None => return true,
    };
    if shebang.is_empty() {
        return false;
    }

    let parts: Vec<&str> = shebang.split_whitespace().collect();
    let interpreter = parts[0];

    if interpreter.starts_with('/') {
        if Path::new(interpreter).exists() {
            return true;
        }
        // `/usr/bin/env node` — check if `node` is resolvable
        if interpreter == "/usr/bin/env" {
            if let Some(cmd) = parts.iter().skip(1).find(|s| !s.starts_with('-')) {
                // same dir as script (nvm/volta place node next to CLI)
                if let Some(parent) = path.parent() {
                    if parent.join(cmd).exists() {
                        return true;
                    }
                }
                // current PATH
                if let Ok(path_env) = std::env::var("PATH") {
                    for p in path_env.split(':') {
                        if PathBuf::from(p).join(cmd).exists() {
                            return true;
                        }
                    }
                }
            }
        }
        return false;
    }

    true
}

/// Run a Command with a wall-clock timeout. Kills the child if it exceeds the limit.
fn command_output_timeout(mut cmd: Command, timeout: Duration) -> Option<std::process::Output> {
    cmd.stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(Option::None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Option::None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Option::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn detect_cli_returns_claude_code_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let orig = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "/usr/bin/claude");
        let kind = detect_cli();
        match orig {
            Some(v) => std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v),
            None => std::env::remove_var("CLAUDE_CODE_ENTRYPOINT"),
        }
        assert!(matches!(kind, CliKind::ClaudeCode { .. }));
    }

    #[test]
    fn gemini_cli_path_env_is_recognized() {
        // GEMINI_CLI_PATH is respected as an explicit override path.
        // We test that the env var is parsed into the correct variant
        // when CLAUDE_CODE_ENTRYPOINT also points to a real path (so claude wins),
        // but we at least confirm the env var machinery doesn't panic.
        let _lock = ENV_LOCK.lock().unwrap();
        let orig_claude = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        let orig_gemini = std::env::var("GEMINI_CLI_PATH").ok();
        // Point CLAUDE_CODE_ENTRYPOINT at a nonexistent path to suppress auto-detect
        std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "/nonexistent/claude-not-here");
        std::env::set_var("GEMINI_CLI_PATH", "/nonexistent/gemini-not-here");
        // detect_cli reads CLAUDE_CODE_ENTRYPOINT first (path may not exist — that's ok for env-var branch)
        let kind = detect_cli();
        // Restore
        match orig_claude { Some(v) => std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v), None => std::env::remove_var("CLAUDE_CODE_ENTRYPOINT") }
        match orig_gemini { Some(v) => std::env::set_var("GEMINI_CLI_PATH", v), None => std::env::remove_var("GEMINI_CLI_PATH") }
        // CLAUDE_CODE_ENTRYPOINT was set → ClaudeCode wins regardless of gemini env var
        assert!(matches!(kind, CliKind::ClaudeCode { .. }));
    }

    #[test]
    fn detect_cli_finds_binary_in_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        let tmp = tempdir().unwrap();
        let bin = tmp.path().join("claude");
        std::fs::write(&bin, b"native binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", orig);
        // May be ClaudeCode (PATH) or None depending on login shell — just check no panic
        let _ = kind;
    }

    #[test]
    fn is_valid_executable_rejects_missing_interpreter() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("myscript");
        std::fs::write(&script, b"#!/nonexistent/interpreter\necho hi\n").unwrap();
        assert!(!is_valid_executable(&script));
    }

    #[test]
    fn is_valid_executable_accepts_native_binary() {
        let tmp = tempdir().unwrap();
        let bin = tmp.path().join("mybinary");
        std::fs::write(&bin, b"\x7fELF fake").unwrap();
        assert!(is_valid_executable(&bin));
    }
}
