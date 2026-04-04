//! CLI detection — determines which AI CLI is available in the environment.

/// Which AI CLI is present in the current environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliKind {
    ClaudeCode,
    GeminiCli,
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
    if std::env::var("CLAUDE_CODE_ENTRYPOINT").is_ok() {
        return CliKind::ClaudeCode;
    }
    if which_binary("claude").is_some() {
        return CliKind::ClaudeCode;
    }
    if std::env::var("GEMINI_CLI_PATH").is_ok() || which_binary("gemini").is_some() {
        return CliKind::GeminiCli;
    }
    CliKind::None
}

pub(crate) fn which_binary(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':')
        .map(std::path::PathBuf::from)
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
        std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "1");
        let kind = detect_cli();
        match orig {
            Some(v) => std::env::set_var("CLAUDE_CODE_ENTRYPOINT", v),
            None => std::env::remove_var("CLAUDE_CODE_ENTRYPOINT"),
        }
        assert_eq!(kind, CliKind::ClaudeCode);
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
        assert_eq!(kind, CliKind::GeminiCli);
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
        assert_eq!(kind, CliKind::ClaudeCode);
    }
}
