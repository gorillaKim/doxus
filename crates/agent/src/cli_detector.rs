//! CLI detection — determines which AI CLI is available in the environment.

use std::path::PathBuf;

/// Which AI CLI is present in the current environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliKind {
    ClaudeCode { path: PathBuf },
    GeminiCli { path: PathBuf },
    None,
}

/// Detect which AI CLI is available in the current environment.
///
/// Priority:
/// 1. `CLAUDE_CODE_ENTRYPOINT` env var → ClaudeCode
/// 2. `claude` binary in PATH → ClaudeCode
/// 3. `GEMINI_CLI_PATH` env var → GeminiCli
/// 4. `gemini` binary in PATH → GeminiCli
/// 5. None
pub fn detect_cli() -> CliKind {
    if let Ok(val) = std::env::var("CLAUDE_CODE_ENTRYPOINT") {
        return CliKind::ClaudeCode { path: PathBuf::from(val) };
    }
    if let Some(path) = which_binary("claude") {
        return CliKind::ClaudeCode { path };
    }
    if let Ok(val) = std::env::var("GEMINI_CLI_PATH") {
        return CliKind::GeminiCli { path: PathBuf::from(val) };
    }
    if let Some(path) = which_binary("gemini") {
        return CliKind::GeminiCli { path };
    }
    CliKind::None
}

pub(crate) fn which_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':')
        .map(PathBuf::from)
        .find_map(|p| {
            let candidate = p.join(name);
            candidate.is_file().then_some(candidate)
        })
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
    fn detect_cli_returns_gemini_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::set_var("GEMINI_CLI_PATH", "/usr/local/bin/gemini");
        let tmp = tempdir().unwrap();
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", orig);
        std::env::remove_var("GEMINI_CLI_PATH");
        assert!(matches!(kind, CliKind::GeminiCli { .. }));
    }

    #[test]
    fn detect_cli_returns_none_without_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let orig_claude = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        let orig_gemini = std::env::var("GEMINI_CLI_PATH").ok();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", &orig_path);
        if let Some(v) = orig_claude { std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v); }
        if let Some(v) = orig_gemini { std::env::set_var("GEMINI_CLI_PATH", v); }
        assert_eq!(kind, CliKind::None);
    }

    #[test]
    fn detect_none_when_no_cli_in_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let orig_claude = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
        let orig_gemini = std::env::var("GEMINI_CLI_PATH").ok();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", &orig_path);
        if let Some(v) = orig_claude { std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v); }
        if let Some(v) = orig_gemini { std::env::set_var("GEMINI_CLI_PATH", v); }
        assert_eq!(kind, CliKind::None);
    }

    #[test]
    fn detect_claude_from_env() {
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
    fn detect_cli_finds_binary_in_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        std::env::remove_var("GEMINI_CLI_PATH");
        let tmp = tempdir().unwrap();
        let bin = tmp.path().join("claude");
        std::fs::write(&bin, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", tmp.path());
        let kind = detect_cli();
        std::env::set_var("PATH", orig);
        assert!(matches!(kind, CliKind::ClaudeCode { .. }));
    }
}
