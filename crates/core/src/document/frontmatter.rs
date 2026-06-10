/// YAML frontmatter 파싱 및 생성
///
/// 템플릿 content에서 frontmatter를 추출하고,
/// 구조화된 필드 + 본문으로부터 마크다운 문서를 생성한다.
use std::collections::BTreeMap;

/// frontmatter 키-값 쌍 (순서 유지를 위해 BTreeMap 대신 Vec 사용)
pub type FrontmatterFields = Vec<(String, String)>;

/// 파싱된 frontmatter + body
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTemplate {
    /// frontmatter 필드 (순서 유지)
    pub fields: FrontmatterFields,
    /// frontmatter 이후 본문
    pub body: String,
}

/// 마크다운 content에서 YAML frontmatter를 파싱한다.
/// frontmatter가 없으면 빈 fields + 전체 content를 body로 반환.
pub fn parse_frontmatter(content: &str) -> ParsedTemplate {
    let lines: Vec<&str> = content.lines().collect();

    // frontmatter 시작 확인
    if lines.first().map(|l| l.trim()) != Some("---") {
        return ParsedTemplate {
            fields: vec![],
            body: content.to_string(),
        };
    }

    // 닫는 --- 찾기
    let mut end_idx = 0;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end_idx = i;
            break;
        }
    }

    if end_idx == 0 {
        return ParsedTemplate {
            fields: vec![],
            body: content.to_string(),
        };
    }

    // frontmatter 라인 파싱 (간단한 key: value 파싱)
    let mut fields: FrontmatterFields = Vec::new();
    for line in &lines[1..end_idx] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let value = trimmed[colon_pos + 1..].trim().to_string();
            if !key.is_empty() {
                fields.push((key, value));
            }
        }
    }

    // body: frontmatter 이후 (선행 빈 줄 제거)
    let body_lines = &lines[end_idx + 1..];
    let body_start = body_lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(0);
    let body = body_lines[body_start..].join("\n");

    ParsedTemplate { fields, body }
}

/// frontmatter 필드 + body를 마크다운 문서로 합친다.
pub fn build_document(fields: &[(String, String)], body: &str) -> String {
    if fields.is_empty() {
        return body.to_string();
    }

    let mut out = String::from("---\n");
    for (key, value) in fields {
        out.push_str(&format!("{}: {}\n", key, value));
    }
    out.push_str("---\n\n");
    out.push_str(body);
    out
}

/// 템플릿 content의 플레이스홀더({{field}})를 실제 값으로 치환한다.
pub fn fill_placeholders(content: &str, values: &BTreeMap<String, String>) -> String {
    let mut result = content.to_string();
    for (key, value) in values {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}

// ── 테스트 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_frontmatter_basic() {
        let content =
            "---\ntitle: 회의록\ndate: 2026-04-13\ntags: meeting, weekly\n---\n\n# 회의록\n\n내용";
        let parsed = parse_frontmatter(content);
        assert_eq!(parsed.fields.len(), 3);
        assert_eq!(parsed.fields[0], ("title".into(), "회의록".into()));
        assert_eq!(parsed.fields[1], ("date".into(), "2026-04-13".into()));
        assert_eq!(parsed.fields[2], ("tags".into(), "meeting, weekly".into()));
        assert!(parsed.body.starts_with("# 회의록"));
    }

    #[test]
    fn parse_frontmatter_empty() {
        let content = "# 그냥 문서\n\n내용입니다.";
        let parsed = parse_frontmatter(content);
        assert!(parsed.fields.is_empty());
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn parse_frontmatter_unclosed() {
        let content = "---\ntitle: 미완성\ndate: 2026-01-01\n본문 시작";
        let parsed = parse_frontmatter(content);
        // 닫는 ---가 없으면 frontmatter 없음 처리
        assert!(parsed.fields.is_empty());
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn parse_frontmatter_empty_value() {
        let content = "---\ntitle:\nstatus: draft\n---\n\n본문";
        let parsed = parse_frontmatter(content);
        assert_eq!(parsed.fields.len(), 2);
        assert_eq!(parsed.fields[0], ("title".into(), "".into()));
        assert_eq!(parsed.fields[1], ("status".into(), "draft".into()));
    }

    #[test]
    fn build_document_with_fields() {
        let fields = vec![
            ("title".into(), "회의록".into()),
            ("date".into(), "2026-04-13".into()),
        ];
        let result = build_document(&fields, "# 회의록\n\n내용");
        assert!(result.starts_with("---\n"));
        assert!(result.contains("title: 회의록\n"));
        assert!(result.contains("date: 2026-04-13\n"));
        assert!(result.ends_with("# 회의록\n\n내용"));
    }

    #[test]
    fn build_document_no_fields() {
        let result = build_document(&[], "# 그냥 문서");
        assert_eq!(result, "# 그냥 문서");
        assert!(!result.contains("---"));
    }

    #[test]
    fn roundtrip_parse_build() {
        let original = "---\ntitle: 테스트\nstatus: draft\n---\n\n# 제목\n\n본문입니다.";
        let parsed = parse_frontmatter(original);
        let rebuilt = build_document(&parsed.fields, &parsed.body);
        assert_eq!(rebuilt, original);
    }

    #[test]
    fn fill_placeholders_basic() {
        let template =
            "---\ntitle: {{title}}\ndate: {{date}}\n---\n\n# {{title}}\n\n내용을 입력하세요.";
        let mut values = BTreeMap::new();
        values.insert("title".into(), "주간 회의록".into());
        values.insert("date".into(), "2026-04-13".into());
        let result = fill_placeholders(template, &values);
        assert!(result.contains("title: 주간 회의록"));
        assert!(result.contains("date: 2026-04-13"));
        assert!(result.contains("# 주간 회의록"));
        assert!(!result.contains("{{"));
    }

    #[test]
    fn fill_placeholders_missing_key_preserved() {
        let template = "---\ntitle: {{title}}\nauthor: {{author}}\n---\n";
        let mut values = BTreeMap::new();
        values.insert("title".into(), "문서".into());
        let result = fill_placeholders(template, &values);
        assert!(result.contains("title: 문서"));
        assert!(
            result.contains("{{author}}"),
            "미입력 필드는 플레이스홀더 유지"
        );
    }
}
