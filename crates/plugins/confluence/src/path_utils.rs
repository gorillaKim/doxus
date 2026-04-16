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
        let space = "ENG";
        let ancestors = vec!["Project A".to_string(), "Design".to_string()];
        let title = "Architecture";
        
        let path = build_relative_path(space, &ancestors, title);
        assert_eq!(path, "ENG/Project A/Design/Architecture.md");
    }

    #[test]
    fn test_build_relative_path_nested_hierarchy() {
        // 이미지 예시 재현: 테크스펙(Space) -> 규칙 생성/수정 모달(Parent Page/Folder) -> 규칙 생성... (Child Page)
        let root = "테크스펙"; 
        let ancestors = vec!["규칙 생성_수정 모달".to_string()];
        let title = "규칙 생성_수정 모달 리팩토링 테크";
        
        let path = build_relative_path(root, &ancestors, title);
        assert_eq!(path, "테크스펙/규칙 생성_수정 모달/규칙 생성_수정 모달 리팩토링 테크.md");
    }

    #[test]
    fn test_build_relative_path_with_folder_feature() {
        // 컨플루언스 '폴더' 기능을 사용하는 경우
        let space_name = "테크스펙";
        let ancestors = vec!["FloatingBottomPanel".to_string()]; // 이 요소가 폴더인 경우
        let title = "FloatingBottomPanel 구현 가이드";
        
        let path = build_relative_path(space_name, &ancestors, title);
        assert_eq!(path, "테크스펙/FloatingBottomPanel/FloatingBottomPanel 구현 가이드.md");
    }
}
