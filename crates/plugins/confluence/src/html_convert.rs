//! Confluence storage format → Markdown 변환.
//!
//! Confluence storage format은 `ac:`, `ri:` 네임스페이스 XML 방언을 사용한다.
//! html5ever 기반 파서들이 namespace-qualified 태그를 올바르게 처리하지 못할 수 있으므로,
//! quick-xml로 XML을 전처리한 후 htmd로 Markdown 변환한다.

/// Confluence storage format HTML을 Markdown으로 변환한다.
/// 변환에 실패하면 원본 HTML 태그를 제거한 텍스트를 반환한다.
pub fn confluence_html_to_markdown(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }

    // Stage 1: ac:*/ri:* XML 태그를 표준 HTML로 정규화
    let normalized = normalize_confluence_xml(html);

    // Stage 2: htmd로 Markdown 변환
    match htmd::convert(&normalized) {
        Ok(md) => md.trim().to_string(),
        Err(_) => {
            // 폴백: HTML 태그 제거
            strip_html_tags(&normalized)
        }
    }
}

/// Confluence XML 전용 태그를 표준 HTML로 변환한다.
fn normalize_confluence_xml(html: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut output = String::with_capacity(html.len());
    let mut reader = Reader::from_str(html);
    reader.config_mut().check_end_names = false;

    let mut buf = Vec::new();
    let mut in_plain_text_body = false;
    let mut plain_text_content = String::new();
    let mut code_language = String::new();
    let mut in_code_macro = false;
    let mut depth_in_macro = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                match name.as_str() {
                    "ac:structured-macro" => {
                        let macro_name = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| {
                                std::str::from_utf8(a.key.as_ref())
                                    .map(|k| k == "ac:name")
                                    .unwrap_or(false)
                            })
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                            .unwrap_or_default();
                        if macro_name == "code" || macro_name == "noformat" {
                            in_code_macro = true;
                            code_language.clear();
                        }
                        depth_in_macro += 1;
                    }
                    "ac:plain-text-body" if in_code_macro => {
                        in_plain_text_body = true;
                        plain_text_content.clear();
                    }
                    "ac:parameter" if in_code_macro => {
                        // language parameter — content captured as text node
                    }
                    "ri:page" => {
                        let title = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| {
                                std::str::from_utf8(a.key.as_ref())
                                    .map(|k| k == "ri:content-title")
                                    .unwrap_or(false)
                            })
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                            .unwrap_or_default();
                        if !title.is_empty() {
                            output.push_str(&format!("<a>{}</a>", html_escape(&title)));
                        }
                    }
                    "ac:link" | "ac:image" | "ri:attachment" | "ri:user" => {
                        // drop wrapper tags
                    }
                    _ if name.starts_with("ac:") || name.starts_with("ri:") => {
                        // drop other ac:/ri: tags
                    }
                    _ => {
                        output.push('<');
                        output.push_str(&name);
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if let (Ok(k), Ok(v)) = (
                                std::str::from_utf8(attr.key.as_ref()),
                                std::str::from_utf8(&attr.value),
                            ) {
                                output.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
                            }
                        }
                        output.push('>');
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                match name.as_str() {
                    "ac:structured-macro" => {
                        if in_code_macro {
                            depth_in_macro = depth_in_macro.saturating_sub(1);
                            if depth_in_macro == 0 {
                                output.push_str(&format!(
                                    "<pre><code class=\"language-{code_language}\">{}</code></pre>",
                                    html_escape(&plain_text_content)
                                ));
                                in_code_macro = false;
                                plain_text_content.clear();
                                code_language.clear();
                            }
                        } else {
                            depth_in_macro = depth_in_macro.saturating_sub(1);
                        }
                    }
                    "ac:plain-text-body" if in_plain_text_body => {
                        in_plain_text_body = false;
                    }
                    _ if name.starts_with("ac:") || name.starts_with("ri:") => {
                        // drop
                    }
                    _ => {
                        output.push_str(&format!("</{name}>"));
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.unescape() {
                    if in_plain_text_body {
                        plain_text_content.push_str(&text);
                    } else if !in_code_macro {
                        output.push_str(&text);
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                if let Ok(text) = std::str::from_utf8(e) {
                    if in_plain_text_body {
                        plain_text_content.push_str(text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                // XML 파싱 실패 시 원본 반환 후 htmd에 맡김
                return html.to_string();
            }
            _ => {}
        }
        buf.clear();
    }

    output
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// HTML 속성값 이스케이프 — 따옴표 포함.
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// HTML 태그를 모두 제거하고 텍스트만 반환 (최후 폴백).
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_html() {
        let result = confluence_html_to_markdown("<p>Hello <strong>world</strong></p>");
        assert!(result.contains("**world**"), "got: {result}");
    }

    #[test]
    fn empty_input() {
        assert_eq!(confluence_html_to_markdown(""), "");
    }

    #[test]
    fn strips_ac_tags_from_output() {
        let html = r#"<ac:structured-macro ac:name="code"><ac:plain-text-body><![CDATA[fn main() {}]]></ac:plain-text-body></ac:structured-macro>"#;
        let result = confluence_html_to_markdown(html);
        assert!(!result.contains("<ac:"), "raw tags remain: {result}");
        assert!(result.contains("fn main"), "code content lost: {result}");
    }

    #[test]
    fn handles_ri_page_link() {
        let html = r#"<ac:link><ri:page ri:content-title="My Page"/></ac:link>"#;
        let result = confluence_html_to_markdown(html);
        assert!(!result.contains("ri:"), "raw ri: tags remain: {result}");
    }
}
