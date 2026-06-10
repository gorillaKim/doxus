use once_cell::sync::Lazy;
use regex::Regex;

static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());

/// Markdown 본문에서 frontmatter, 멀티라인 코드 블록, HTML 태그를 제거한 후
/// 목차 아웃라인(헤더 목록)과 첫번째 '#' 기호 이후의 첫 3문장을 결합하여 하이브리드 요약을 생성합니다.
/// 만약 frontmatter에 description 필드가 존재하면 이를 최우선 요약으로 사용합니다.
/// 최종 요약은 500자를 초과할 경우 잘라내고 "..."을 붙입니다.
pub fn lead3_extract(content: &str) -> String {
    if content.trim().is_empty() {
        return String::new();
    }

    // 1. frontmatter에서 description 최우선 추출 시도
    if let Some(desc) = extract_frontmatter_description(content) {
        let mut summary = desc.trim().to_string();
        if summary.chars().count() > 500 {
            let truncated: String = summary.chars().take(500).collect();
            summary = format!("{}...", truncated);
        }
        return summary;
    }

    // 2. frontmatter 영역 제거
    let mut lines = content.lines().peekable();
    let mut body_lines = Vec::new();
    if let Some(first_line) = lines.peek() {
        if first_line.trim() == "---" {
            lines.next(); // 첫 --- 건너뜀
            let mut found_end = false;
            while let Some(line) = lines.next() {
                if line.trim() == "---" {
                    found_end = true;
                    break;
                }
            }
            if !found_end {
                lines = content.lines().peekable();
            }
        }
    }

    // 3. 멀티라인 코드 블록 제거 및 라인 수집
    let mut in_code_block = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block {
            body_lines.push(line);
        }
    }
    let body = body_lines.join("\n");

    // 4. HTML 태그 제거
    let clean_body = HTML_TAG_RE.replace_all(&body, "");

    // 5. 목차 아웃라인 추출 (HTML 제거 본문 기준)
    let outline = extract_outline(&clean_body);

    let mut summary = if !outline.is_empty() {
        // 목차가 있는 경우: 목차 개요만 사용
        outline.trim().to_string()
    } else {
        // 목차가 없는 경우: 본문 전체(clean_body)에서 상위 3문장 추출
        let sentences = split_sentences(&clean_body);
        let lead3_sentences: Vec<&str> = sentences.iter().take(3).map(|s| s.as_str()).collect();
        lead3_sentences.join(" ")
    };

    // 10. 500자 초과 시 자르고 "..." 추가
    if summary.chars().count() > 500 {
        let truncated: String = summary.chars().take(500).collect();
        summary = format!("{}...", truncated);
    }

    summary.trim().to_string()
}

/// frontmatter 영역에서 YAML 형식의 description 값을 추출합니다.
fn extract_frontmatter_description(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if let Some(first) = lines.next() {
        if first.trim() == "---" {
            for line in lines {
                let trimmed = line.trim();
                if trimmed == "---" {
                    break;
                }
                if trimmed.starts_with("description:") {
                    let val = trimmed["description:".len()..].trim();
                    let val = val.trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Markdown 텍스트에서 주요 헤더(#, ##, ###) 목록을 추출하여 목차 개요 문자열을 생성합니다.
fn extract_outline(content: &str) -> String {
    let mut headers = Vec::new();
    let mut current_len = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#") {
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if hash_count > 0 && hash_count <= 3 {
                let header_title = trimmed[hash_count..].trim();
                if !header_title.is_empty() {
                    let formatted = format!("{} {}", "#".repeat(hash_count), header_title);
                    current_len += formatted.len() + 2;
                    if current_len > 150 {
                        break;
                    }
                    headers.push(formatted);
                }
            }
        }
    }

    if headers.is_empty() {
        String::new()
    } else {
        format!("[목차: {}] ", headers.join(", "))
    }
}

/// 온점(.), 물음표(?), 느낌표(!) 기준으로 한국어 및 영어 문장을 분리합니다.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        current.push(c);
        if c == '.' || c == '?' || c == '!' {
            let is_sentence_end = match chars.peek() {
                Some(&next_c) => next_c.is_whitespace(),
                None => true,
            };
            if is_sentence_end {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    let normalized = normalize_whitespace(&trimmed);
                    sentences.push(normalized);
                }
                current.clear();
            }
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        let normalized = normalize_whitespace(&trimmed);
        sentences.push(normalized);
    }

    sentences
}

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<&str>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_removal() {
        let content = "\
---
title: Test Page
tags: [a, b]
---
# First Header
This is the first sentence. This is the second. This is the third.";
        let summary = lead3_extract(content);
        assert_eq!(summary, "[목차: # First Header]");
    }

    #[test]
    fn test_frontmatter_description_prioritized() {
        let content = "\
---
title: Test Page
description: \"이 문서는 요약의 우선순위를 테스트합니다.\"
tags: [a, b]
---
# First Header
This is the first sentence. This is the second. This is the third.";
        let summary = lead3_extract(content);
        assert_eq!(summary, "이 문서는 요약의 우선순위를 테스트합니다.");
    }

    #[test]
    fn test_no_headers_fallback() {
        let content = "This is the first sentence. This is the second. This is the third.";
        let summary = lead3_extract(content);
        assert_eq!(
            summary,
            "This is the first sentence. This is the second. This is the third."
        );
    }

    #[test]
    fn test_code_block_removal() {
        let content = "\
# Header
This is the first sentence.
```rust
fn test() {
    println!(\"not in summary\");
}
```
This is the second. This is the third.";
        let summary = lead3_extract(content);
        assert_eq!(summary, "[목차: # Header]");
    }

    #[test]
    fn test_html_removal() {
        let content = "\
# Header
This is <span class=\"bold\">first</span> sentence. This is <b>second</b>. This is third.";
        let summary = lead3_extract(content);
        assert_eq!(summary, "[목차: # Header]");
    }

    #[test]
    fn test_korean_english_mixed_sentence_splitting() {
        let content = "\
# 문서 요약 테스트
이것은 첫 번째 문장입니다. This is 3.5 version of the documentation, which is second sentence! 그리고 이것이 세 번째 문장입니다.";
        let summary = lead3_extract(content);
        assert_eq!(summary, "[목차: # 문서 요약 테스트]");
    }

    #[test]
    fn test_body_processing_without_headers() {
        let content = "\
This is <span class=\"bold\">first</span> sentence.
```rust
fn test() {}
```
This is the second. This is the third.";
        let summary = lead3_extract(content);
        assert_eq!(
            summary,
            "This is first sentence. This is the second. This is the third."
        );
    }

    #[test]
    fn test_max_length_truncation() {
        let long_sentence_1 = "가".repeat(200);
        let long_sentence_2 = "나".repeat(200);
        let long_sentence_3 = "다".repeat(200);
        let content = format!(
            "{}. {}. {}.",
            long_sentence_1, long_sentence_2, long_sentence_3
        );
        let summary = lead3_extract(&content);
        assert_eq!(summary.chars().count(), 503); // 500자 + "..." (3자)
        assert!(summary.ends_with("..."));
    }
}
