use crate::PluginError;

/// Validates a path segment or full relative path to ensure it doesn't try to escape the root.
/// Returns Ok(()) if safe, Err(PluginError::PermissionDenied) if dangerous.
pub fn validate_path(path: &str) -> Result<(), PluginError> {
    // 1. ".." check (Path Traversal)
    if path.contains("..") {
        return Err(PluginError::PermissionDenied(format!(
            "Path traversal attempt detected: {}",
            path
        )));
    }

    // 2. Absolute path check (should be relative to storage root)
    if path.starts_with('/')
        || path.starts_with('\\')
        || (path.len() >= 2
            && path.chars().next().unwrap().is_ascii_alphabetic()
            && path.chars().nth(1).unwrap() == ':')
    {
        return Err(PluginError::PermissionDenied(format!(
            "Absolute paths are not allowed: {}",
            path
        )));
    }

    // 3. UNC/Windows specific suspicious paths
    if path.contains("\\\\") {
        return Err(PluginError::PermissionDenied(format!(
            "Invalid path format (UNC/suspicious): {}",
            path
        )));
    }

    Ok(())
}

/// Parses a folder-title combination into a list of safe segments.
/// Handles leading/trailing slashes and ensures no traversal.
pub fn parse_hierarchical_path(
    folder: Option<&str>,
    title: &str,
) -> Result<Vec<String>, PluginError> {
    let mut segments = Vec::new();

    if let Some(f) = folder {
        validate_path(f)?;
        for part in f.split(&['/', '\\']).filter(|s| !s.is_empty()) {
            segments.push(part.to_string());
        }
    }

    validate_path(title)?;
    // Title itself might contain slashes if used as a path
    for part in title.split(&['/', '\\']).filter(|s| !s.is_empty()) {
        segments.push(part.to_string());
    }

    if segments.is_empty() {
        return Err(PluginError::Internal("Empty path or title".into()));
    }

    Ok(segments)
}

/// Sanitize a string to be used as a filename or directory name.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Generate a relative path from a root name and ancestors.
/// If `is_parent` is true, the title itself becomes a directory and the file is placed inside it.
/// e.g. "Space/Parent/Parent.md" instead of "Space/Parent.md"
pub fn build_relative_path(
    root_name: &str,
    ancestors: &[String],
    title: &str,
    is_parent: bool,
) -> String {
    use std::path::PathBuf;
    let mut path = PathBuf::new();
    if !root_name.is_empty() {
        path.push(sanitize_name(root_name));
    }
    for ancestor in ancestors {
        path.push(sanitize_name(ancestor));
    }

    let sanitized_title = sanitize_name(title);
    if is_parent {
        path.push(&sanitized_title);
    }

    path.push(format!("{}.md", sanitized_title));
    path.to_string_lossy().to_string()
}

/// Resolves a unique title based on attempt number (Option B suffixing).
/// 10 attempts limit is enforced.
pub fn resolve_unique_title(base_title: &str, attempt: usize) -> Result<String, PluginError> {
    if attempt == 0 {
        Ok(base_title.to_string())
    } else if attempt > 10 {
        Err(PluginError::Internal(format!(
            "Failed to find unique path for '{}' after 10 attempts",
            base_title
        )))
    } else {
        Ok(format!("{} ({})", base_title, attempt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path() {
        assert!(validate_path("notes/daily").is_ok());
        assert!(validate_path("meeting.md").is_ok());

        assert!(validate_path("../etc/passwd").is_err());
        assert!(validate_path("notes/../../secret").is_err());
        assert!(validate_path("/absolute/path").is_err());
        assert!(validate_path(r"C:\windows").is_err());
        assert!(validate_path(r"\\share\file").is_err());
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("Normal Title"), "Normal Title");
        assert_eq!(sanitize_name("Title with / slash"), "Title with _ slash");
        assert_eq!(sanitize_name("In:Valid*Char?"), "In_Valid_Char_");
    }

    #[test]
    fn test_build_relative_path() {
        let space = "ENG";
        let ancestors = vec!["Project A".to_string(), "Design".to_string()];
        let title = "Architecture";
        let path = build_relative_path(space, &ancestors, title, false);
        assert_eq!(path, "ENG/Project A/Design/Architecture.md");
    }

    #[test]
    fn test_resolve_unique_title() {
        assert_eq!(resolve_unique_title("Doc", 0).unwrap(), "Doc");
        assert_eq!(resolve_unique_title("Doc", 1).unwrap(), "Doc (1)");
        assert_eq!(resolve_unique_title("Doc", 2).unwrap(), "Doc (2)");
        assert!(resolve_unique_title("Doc", 11).is_err());
    }
}
