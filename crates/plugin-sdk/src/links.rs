use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref WIKI_LINK_RE: Regex = Regex::new(r"\[\[(.*?)\]\]").unwrap();
    static ref MD_LINK_RE: Regex = Regex::new(r"\[.*?\]\((.*?)\)").unwrap();
    // Support Unicode and spaces: doxus://[project_name]/[doc_id]
    // Exclude trailing punctuation like . , ) ] from the ID
    static ref DX_URI_RE: Regex = Regex::new(r"doxus://([^/]+)/([^\s\]\)\.,]+)").unwrap();
}

pub struct LinkExtractor;

impl LinkExtractor {
    /// Extracts all types of links from the given content.
    /// Supports WikiLinks, Markdown Links, and Doxus Virtual URIs.
    pub fn extract_links(content: &str) -> Vec<String> {
        let mut links = Vec::new();
        let mut seen = std::collections::HashSet::new();
        
        // 1. WikiLinks [[Target]] or [[Target|Alias]]
        for cap in WIKI_LINK_RE.captures_iter(content) {
            let full_link = &cap[1];
            let link = if let Some(pipe_pos) = full_link.find('|') {
                &full_link[..pipe_pos]
            } else {
                full_link
            };
            let link = link.to_string();
            if seen.insert(link.clone()) {
                links.push(link);
            }
        }

        // 2. Markdown Links [Text](Target)
        for cap in MD_LINK_RE.captures_iter(content) {
            let link = cap[1].to_string();
            
            // Filter out external web links (http, https, obsidian, etc.)
            // We only care about root-relative paths or doxus:// URIs for graph extraction.
            if link.contains("://") && !link.starts_with("doxus://") {
                continue;
            }

            if seen.insert(link.clone()) {
                links.push(link);
            }
        }

        // 3. Doxus Virtual URIs doxus://project/id
        for cap in DX_URI_RE.captures_iter(content) {
            let link = cap[0].to_string();
            if seen.insert(link.clone()) {
                links.push(link);
            }
        }
        
        links
    }
}

pub fn dx_uri_regex() -> &'static Regex {
    &DX_URI_RE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mixed_links() {
        let content = r#"
Check [[Internal Doc]], [[Aliased Doc|Some Alias]], [External](https://google.com), 
and a cross-project link: doxus://AI 리포트 V3/4756242498.
Also a [nested link](doxus://Proj/ID).
"#;
        let links = LinkExtractor::extract_links(content);
        assert!(links.contains(&"Internal Doc".to_string()));
        assert!(links.contains(&"Aliased Doc".to_string()));
        assert!(!links.contains(&"https://google.com".to_string()));
        // Note: The trailing dot in the content should NOT be part of the ID
        assert!(links.contains(&"doxus://AI 리포트 V3/4756242498".to_string()));
        assert!(links.contains(&"doxus://Proj/ID".to_string()));
    }
}
