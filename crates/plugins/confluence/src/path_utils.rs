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
pub fn build_relative_path(root_name: &str, ancestors: &[String], title: &str) -> String {
    let mut path = PathBuf::new();
    if !root_name.is_empty() {
        path.push(sanitize_name(root_name));
    }
    for ancestor in ancestors {
        path.push(sanitize_name(ancestor));
    }
    path.push(format!("{}.md", sanitize_name(title)));
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
        // Space root case (not used in our fix anymore, but good for coverage)
        let space = "ENG";
        let ancestors = vec!["Project A".to_string(), "Design".to_string()];
        let title = "Architecture";
        
        let path = build_relative_path(space, &ancestors, title);
        assert_eq!(path, "ENG/Project A/Design/Architecture.md");
    }

    #[test]
    fn test_build_relative_path_nested_hierarchy() {
        let root = "테크스펙"; 
        let ancestors = vec!["규칙 생성_수정 모달".to_string()];
        let title = "규칙 생성_수정 모달 리팩토링 테크";
        
        let path = build_relative_path(root, &ancestors, title);
        assert_eq!(path, "테크스펙/규칙 생성_수정 모달/규칙 생성_수정 모달 리팩토링 테크.md");
    }

    #[test]
    fn test_build_relative_path_with_folder_feature() {
        let space_name = "테크스펙";
        let ancestors = vec!["FloatingBottomPanel".to_string()]; 
        let title = "FloatingBottomPanel 구현 가이드";
        
        let path = build_relative_path(space_name, &ancestors, title);
        assert_eq!(path, "테크스펙/FloatingBottomPanel/FloatingBottomPanel 구현 가이드.md");
    }

    #[test]
    fn test_build_relative_path_empty_root_omits_redundant_folder() {
        // [Doxus Fix] ancestor_id 혹은 스페이스 최상위 루트인 경우 
        // root_name을 비워서 최상위 폴더가 중복 출력되는 것을 방지합니다.
        let root = "";
        let ancestors = vec!["Folder A".to_string(), "Folder B".to_string()];
        let title = "Page";
        
        let path = build_relative_path(root, &ancestors, title);
        assert_eq!(path, "Folder A/Folder B/Page.md");
    }
}
