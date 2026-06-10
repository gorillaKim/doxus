use std::path::PathBuf;

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
    let mut path = PathBuf::new();
    if !root_name.is_empty() {
        path.push(sanitize_name(root_name));
    }
    for ancestor in ancestors {
        path.push(sanitize_name(ancestor));
    }

    let sanitized_title = sanitize_name(title);
    if is_parent {
        // 부모 페이지인 경우 제목으로 폴더를 하나 더 만들고 그 안에 파일을 둠
        path.push(&sanitized_title);
    }

    path.push(format!("{}.md", sanitized_title));
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Not a parent
        let path = build_relative_path(space, &ancestors, title, false);
        assert_eq!(path, "ENG/Project A/Design/Architecture.md");
    }

    #[test]
    fn test_build_relative_path_for_parent() {
        let root = "테크스펙";
        let ancestors = vec![];
        let title = "v3.1-MAD 최종 스펙";

        // Is a parent
        let path = build_relative_path(root, &ancestors, title, true);
        assert_eq!(path, "테크스펙/v3.1-MAD 최종 스펙/v3.1-MAD 최종 스펙.md");
    }

    #[test]
    fn test_build_relative_path_nested_hierarchy() {
        let root = "테크스펙";
        let ancestors = vec!["규칙 생성_수정 모달".to_string()];
        let title = "상세 로직";

        let path = build_relative_path(root, &ancestors, title, false);
        assert_eq!(path, "테크스펙/규칙 생성_수정 모달/상세 로직.md");
    }

    #[test]
    fn test_build_relative_path_empty_root_omits_redundant_folder() {
        let root = "";
        let ancestors = vec!["Folder A".to_string(), "Folder B".to_string()];
        let title = "Page";

        let path = build_relative_path(root, &ancestors, title, false);
        assert_eq!(path, "Folder A/Folder B/Page.md");
    }
}
