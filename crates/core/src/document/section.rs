/// 마크다운 섹션 파싱 및 교체
///
/// 규칙:
/// - 헤딩 기준: ATX 스타일 (`#`, `##`, `###` 등)만 지원. setext 미지원.
/// - frontmatter(`---`로 시작/끝나는 블록)는 섹션으로 취급하지 않음.
/// - 코드 펜스(`` ``` `` 또는 `~~~`) 내부의 `#`는 헤딩으로 인식하지 않음.
/// - 같은 제목의 섹션이 여러 개일 경우 순서(0-based index)로 구분.

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// 헤딩 텍스트 (예: "## 배경")
    pub heading: String,
    /// 헤딩 레벨 (1~6)
    pub level: u8,
    /// 섹션 전체 내용 (헤딩 라인 포함)
    pub content: String,
    /// 시작 라인 인덱스 (0-based)
    pub start_line: usize,
    /// 끝 라인 인덱스 (exclusive, 0-based)
    pub end_line: usize,
}

/// 문서에서 섹션 목록을 파싱한다.
pub fn parse_sections(content: &str) -> Vec<Section> {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    // 코드 펜스 상태 추적
    let mut in_code_fence = false;
    let mut fence_char = ' ';

    // frontmatter 범위 (라인 인덱스)
    let frontmatter_end = detect_frontmatter_end(&lines);

    // 헤딩 위치 수집
    let mut headings: Vec<(usize, u8, String)> = Vec::new(); // (line_idx, level, text)

    for (i, line) in lines.iter().enumerate() {
        // frontmatter 건너뜀
        if i < frontmatter_end {
            continue;
        }

        // 코드 펜스 진입/탈출
        if is_fence_delimiter(line) {
            let ch = line.chars().next().unwrap_or(' ');
            if in_code_fence {
                if ch == fence_char {
                    in_code_fence = false;
                }
            } else {
                in_code_fence = true;
                fence_char = ch;
            }
            continue;
        }

        if in_code_fence {
            continue;
        }

        // ATX 헤딩 파싱
        if let Some((level, title)) = parse_atx_heading(line) {
            headings.push((i, level, title));
        }
    }

    // 섹션 범위 계산
    let mut sections = Vec::new();
    for (j, &(start, level, ref title)) in headings.iter().enumerate() {
        let end = headings.get(j + 1).map(|&(s, _, _)| s).unwrap_or(total);
        let section_lines = &lines[start..end];
        sections.push(Section {
            heading: format!("{} {}", "#".repeat(level as usize), title),
            level,
            content: section_lines.join("\n"),
            start_line: start,
            end_line: end,
        });
    }

    sections
}

/// 특정 헤딩의 섹션 내용을 교체한다. 헤딩이 중복일 경우 `occurrence`(0-based)로 선택.
/// 반환값: 교체된 전체 문서 content
pub fn replace_section(
    content: &str,
    heading_text: &str,
    occurrence: usize,
    new_section_content: &str,
) -> Result<String, SectionError> {
    let sections = parse_sections(content);
    let matches: Vec<&Section> = sections
        .iter()
        .filter(|s| section_heading_matches(&s.heading, heading_text))
        .collect();

    let target = matches.get(occurrence).ok_or_else(|| SectionError::NotFound {
        heading: heading_text.to_string(),
        occurrence,
    })?;

    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<&str> = lines[..target.start_line].to_vec();
    result_lines.extend(new_section_content.lines());
    result_lines.extend_from_slice(&lines[target.end_line..]);

    Ok(result_lines.join("\n"))
}

/// 특정 헤딩 뒤에 새 섹션을 삽입한다.
/// `after_heading` = None이면 문서 맨 끝에 추가.
pub fn insert_section_after(
    content: &str,
    after_heading: Option<&str>,
    new_section_content: &str,
) -> Result<String, SectionError> {
    match after_heading {
        None => {
            // 문서 끝에 추가
            let sep = if content.ends_with('\n') { "" } else { "\n" };
            Ok(format!("{}{}{}", content, sep, new_section_content))
        }
        Some(heading_text) => {
            let sections = parse_sections(content);
            let target = sections
                .iter()
                .find(|s| section_heading_matches(&s.heading, heading_text))
                .ok_or_else(|| SectionError::NotFound {
                    heading: heading_text.to_string(),
                    occurrence: 0,
                })?;

            let lines: Vec<&str> = content.lines().collect();
            let mut result_lines: Vec<&str> = lines[..target.end_line].to_vec();
            result_lines.extend(new_section_content.lines());
            result_lines.extend_from_slice(&lines[target.end_line..]);
            Ok(result_lines.join("\n"))
        }
    }
}

/// 특정 섹션을 삭제한다.
pub fn delete_section(
    content: &str,
    heading_text: &str,
    occurrence: usize,
) -> Result<String, SectionError> {
    let sections = parse_sections(content);
    let matches: Vec<&Section> = sections
        .iter()
        .filter(|s| section_heading_matches(&s.heading, heading_text))
        .collect();

    let target = matches.get(occurrence).ok_or_else(|| SectionError::NotFound {
        heading: heading_text.to_string(),
        occurrence,
    })?;

    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<&str> = lines[..target.start_line].to_vec();
    result_lines.extend_from_slice(&lines[target.end_line..]);
    Ok(result_lines.join("\n"))
}

// ── 내부 헬퍼 ────────────────────────────────────────────────────────────────

fn detect_frontmatter_end(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim()) != Some("---") {
        return 0;
    }
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            return i + 1;
        }
    }
    0 // 닫는 --- 없으면 frontmatter 없음으로 처리
}

fn is_fence_delimiter(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// ATX 헤딩 파싱. 성공 시 (level, title_text) 반환.
fn parse_atx_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    // 헤딩 다음에 공백이 있어야 ATX 헤딩
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    let title = rest.trim().to_string();
    Some((hashes as u8, title))
}

/// 헤딩 텍스트 비교: `## 배경` vs `배경` 또는 `## 배경` 모두 매칭
fn section_heading_matches(section_heading: &str, query: &str) -> bool {
    let query_trimmed = query.trim();
    // 완전 일치
    if section_heading == query_trimmed {
        return true;
    }
    // 헤딩 기호 제거 후 비교
    let section_title = section_heading
        .trim_start_matches('#')
        .trim();
    section_title == query_trimmed
}

// ── 에러 타입 ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SectionError {
    #[error("섹션을 찾을 수 없음: '{heading}' (occurrence {occurrence})")]
    NotFound { heading: String, occurrence: usize },
}

// ── 테스트 ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# 제목

소개 텍스트.

## 배경

배경 내용입니다.

## 구현

구현 내용입니다.

### 세부 구현

세부 내용.

## 결론

결론 내용.
"#;

    #[test]
    fn parse_sections_basic() {
        let sections = parse_sections(SAMPLE);
        assert_eq!(sections.len(), 5);
        assert_eq!(sections[0].heading, "# 제목");
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[1].heading, "## 배경");
        assert_eq!(sections[2].heading, "## 구현");
        assert_eq!(sections[3].heading, "### 세부 구현");
        assert_eq!(sections[4].heading, "## 결론");
    }

    #[test]
    fn parse_sections_ignores_code_fence_hashes() {
        let content = "## 실제 섹션\n\n```python\n# 이건 헤딩이 아님\n```\n\n## 다음 섹션\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "## 실제 섹션");
        assert_eq!(sections[1].heading, "## 다음 섹션");
    }

    #[test]
    fn parse_sections_ignores_frontmatter() {
        let content = "---\ntitle: 문서\ndate: 2026-01-01\n---\n\n## 본문\n\n내용\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "## 본문");
    }

    #[test]
    fn replace_section_replaces_content() {
        let new_content = "## 배경\n\n완전히 교체된 내용입니다.\n";
        let result = replace_section(SAMPLE, "## 배경", 0, new_content).unwrap();
        assert!(result.contains("완전히 교체된 내용입니다."));
        assert!(!result.contains("배경 내용입니다."));
        // 다른 섹션은 유지
        assert!(result.contains("구현 내용입니다."));
    }

    #[test]
    fn replace_section_by_title_only() {
        // 헤딩 기호 없이 제목만으로도 매칭
        let result = replace_section(SAMPLE, "배경", 0, "## 배경\n\n교체됨.\n").unwrap();
        assert!(result.contains("교체됨."));
    }

    #[test]
    fn replace_section_not_found_returns_error() {
        let result = replace_section(SAMPLE, "없는섹션", 0, "## 없는섹션\n내용\n");
        assert!(result.is_err());
    }

    #[test]
    fn insert_section_after_heading() {
        let result = insert_section_after(SAMPLE, Some("## 배경"), "## 새 섹션\n\n새 내용.\n").unwrap();
        // 새 섹션이 배경 바로 뒤에 삽입됨
        let bg_pos = result.find("배경 내용입니다.").unwrap();
        let new_pos = result.find("새 내용.").unwrap();
        assert!(new_pos > bg_pos, "새 섹션이 배경 뒤에 있어야 함");
    }

    #[test]
    fn insert_section_at_end_when_no_heading() {
        let result = insert_section_after(SAMPLE, None, "## 추가 섹션\n\n추가 내용.\n").unwrap();
        assert!(result.ends_with("추가 내용.\n"));
    }

    #[test]
    fn delete_section_removes_content() {
        let result = delete_section(SAMPLE, "## 배경", 0).unwrap();
        assert!(!result.contains("배경 내용입니다."));
        assert!(result.contains("구현 내용입니다."));
    }

    #[test]
    fn duplicate_heading_occurrence_select() {
        let content = "## 섹션\n\n첫 번째.\n\n## 섹션\n\n두 번째.\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);

        let result0 = replace_section(content, "## 섹션", 0, "## 섹션\n\n교체0.\n").unwrap();
        assert!(result0.contains("교체0."));
        assert!(result0.contains("두 번째."));

        let result1 = replace_section(content, "## 섹션", 1, "## 섹션\n\n교체1.\n").unwrap();
        assert!(result1.contains("첫 번째."));
        assert!(result1.contains("교체1."));
    }
}
