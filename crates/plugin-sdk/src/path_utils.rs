use crate::PluginError;

/// Validates a path segment or full relative path to ensure it doesn't try to escape the root.
/// Returns Ok(()) if safe, Err(PluginError::PermissionDenied) if dangerous.
pub fn validate_path(path: &str) -> Result<(), PluginError> {
    // 1. ".." check (Path Traversal)
    if path.contains("..") {
        return Err(PluginError::PermissionDenied(format!("Path traversal attempt detected: {}", path)));
    }

    // 2. Absolute path check (should be relative to storage root)
    if path.starts_with('/') || path.starts_with('\\') 
        || (path.len() >= 2 && path.chars().next().unwrap().is_ascii_alphabetic() && path.chars().nth(1).unwrap() == ':')
    {
        return Err(PluginError::PermissionDenied(format!("Absolute paths are not allowed: {}", path)));
    }

    // 3. UNC/Windows specific suspicious paths
    if path.contains("\\\\") {
        return Err(PluginError::PermissionDenied(format!("Invalid path format (UNC/suspicious): {}", path)));
    }

    Ok(())
}

/// Parses a folder-title combination into a list of safe segments.
/// Handles leading/trailing slashes and ensures no traversal.
pub fn parse_hierarchical_path(folder: Option<&str>, title: &str) -> Result<Vec<String>, PluginError> {
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
}
