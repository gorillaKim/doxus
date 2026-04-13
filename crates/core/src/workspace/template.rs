use handlebars::Handlebars;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template not found: {0}")]
    NotFound(String),
    #[error("render error: {0}")]
    Render(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// 서버가 자동 주입하는 변수 — 사용자/에이전트가 제공할 필요 없음
const AUTO_INJECT: &[&str] = &["created", "updated"];

#[derive(Debug, Clone)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    /// 전체 변수 목록 (frontmatter + body)
    pub variables: Vec<String>,
    /// frontmatter 섹션(--- ... ---) 에서만 등장하는 변수 (auto-inject 제외)
    pub frontmatter_fields: Vec<String>,
    /// body 섹션에서만 등장하는 변수 (frontmatter 변수 및 auto-inject 제외)
    pub body_variables: Vec<String>,
}

pub struct TemplateEngine {
    hb: Handlebars<'static>,
    sources: HashMap<String, String>,
    descriptions: HashMap<String, String>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self {
            hb: Handlebars::new(),
            sources: HashMap::new(),
            descriptions: HashMap::new(),
        }
    }

    /// Register a named template from a string
    pub fn register(&mut self, name: &str, template: &str) -> Result<(), TemplateError> {
        self.register_with_description(name, template, "")
    }

    pub fn register_with_description(&mut self, name: &str, template: &str, description: &str) -> Result<(), TemplateError> {
        self.hb
            .register_template_string(name, template)
            .map_err(|e| TemplateError::Render(e.to_string()))?;
        self.sources.insert(name.to_string(), template.to_string());
        self.descriptions.insert(name.to_string(), description.to_string());
        Ok(())
    }

    /// Render a registered template with given context
    pub fn render(&self, name: &str, context: &Value) -> Result<String, TemplateError> {
        if !self.hb.has_template(name) {
            return Err(TemplateError::NotFound(name.to_string()));
        }
        self.hb
            .render(name, context)
            .map_err(|e| TemplateError::Render(e.to_string()))
    }

    /// List all registered templates with metadata
    pub fn list_templates(&self) -> Vec<TemplateInfo> {
        let mut result: Vec<TemplateInfo> = self.sources.iter().map(|(name, src)| {
            let fm = extract_frontmatter_variables(src);
            let body = extract_body_variables(src);
            TemplateInfo {
                name: name.clone(),
                description: self.descriptions.get(name).cloned().unwrap_or_default(),
                variables: extract_variables(src),
                frontmatter_fields: fm,
                body_variables: body,
            }
        }).collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Get the raw source of a registered template
    pub fn get_template_source(&self, name: &str) -> Option<String> {
        self.sources.get(name).cloned()
    }

    /// Register all built-in templates (10 types)
    pub fn with_builtins() -> Self {
        let mut engine = Self::new();

        engine.register_with_description("note", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: []
status: draft
---

# {{title}}

{{content}}

## 참고 문서

{{ref_docs}}
"#, "일반 메모").expect("builtin note");

        engine.register_with_description("meeting", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [meeting]
date: {{date}}
attendees: [{{attendees}}]
---

# {{title}}

## 참석자

{{attendees}}

## 안건

{{agenda}}

## 논의 내용

## 결정 사항

{{decisions}}

## 액션 아이템

{{action_items}}

## 참고 문서

{{ref_docs}}
"#, "회의록").expect("builtin meeting");

        engine.register_with_description("decision", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [decision]
date: {{date}}
status: proposed
---

# {{title}}

## 배경

{{context}}

## 선택지

## 결정

{{decision}}

## 근거

{{consequences}}

## 참고 문서

{{ref_docs}}
"#, "아키텍처 결정 기록 (ADR)").expect("builtin decision");

        engine.register_with_description("journal", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [journal]
date: {{date}}
mood: {{mood}}
---

# {{title}}

{{content}}

## 회고

{{reflection}}

## 참고 문서

{{ref_docs}}
"#, "일기/개인 기록").expect("builtin journal");

        engine.register_with_description("retrospective", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [retrospective]
date: {{date}}
sprint: {{sprint}}
---

# {{sprint}} 회고

## 잘된 점

{{went_well}}

## 개선할 점

{{to_improve}}

## 액션 아이템

{{action_items}}

## 참고 문서

{{ref_docs}}
"#, "회고").expect("builtin retrospective");

        engine.register_with_description("devlog", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [devlog]
date: {{date}}
---

# {{title}}

## 작업 내용

{{work_summary}}

## 배운 점

{{learnings}}

## 다음 할 일

{{next_steps}}

## 참고 문서

{{ref_docs}}
"#, "개발 일지").expect("builtin devlog");

        engine.register_with_description("weekly", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [weekly]
period: {{period}}
---

# {{title}}

## 이번 주 목표

{{goals}}

## 진행 상황

{{progress}}

## 다음 주 계획

{{next_week}}

## 참고 문서

{{ref_docs}}
"#, "주간 보고").expect("builtin weekly");

        engine.register_with_description("study", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [study]
source: {{source}}
---

# {{title}}

## 핵심 개념

{{key_concepts}}

## 정리

{{summary}}

## 참고 자료

{{references}}

## 참고 문서

{{ref_docs}}
"#, "학습 노트").expect("builtin study");

        engine.register_with_description("library", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [library]
author: {{author}}
source: {{source}}
---

# {{title}}

## 요약

{{summary}}

## 핵심 인용

{{quotes}}

## 활용 방안

{{applications}}

## 참고 문서

{{ref_docs}}
"#, "참고 자료 정리").expect("builtin library");

        engine.register_with_description("article", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [article]
source: {{source}}
author: {{author}}
date: {{date}}
status: published
---

# {{title}}

{{content}}

## 참고 문서

{{ref_docs}}
"#, "아티클").expect("builtin article");

        engine.register_with_description("todo", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [todo]
date: {{date}}
---

# {{title}}

## 할 일

{{tasks}}

## 메모

{{notes}}

## 참고 문서

{{ref_docs}}
"#, "할 일 목록").expect("builtin todo");

        engine.register_with_description("techspec", r#"---
title: "{{title}}"
aliases: []
created: {{created}}
updated: {{updated}}
tags: [spec]
author: {{author}}
status: {{status}}
---

# {{title}}

## 개요

{{overview}}

## 설계

{{design}}

## API

{{api}}

## 대안 검토

{{alternatives}}

## 참고 문서

{{ref_docs}}
"#, "기술 명세서").expect("builtin techspec");

        engine
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract {{variable}} names from a Handlebars template source.
/// Uses simple parsing: finds all {{name}} patterns, deduplicates, preserves order.
pub fn extract_variables(template_src: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    let mut chars = template_src.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second {
            // skip whitespace
            let mut name = String::new();
            let mut closed = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    // consume closing }}
                    closed = true;
                    break;
                }
                name.push(ch);
            }
            if closed {
                // consume second }
                chars.next();
                let name = name.trim().to_string();
                // skip block helpers (#if, #each, /if, /each, else, this, .)
                if !name.is_empty()
                    && !name.starts_with('#')
                    && !name.starts_with('/')
                    && name != "else"
                    && name != "this"
                    && name != "."
                    && !name.contains(' ')
                {
                    if seen.insert(name.clone()) {
                        result.push(name);
                    }
                }
            }
        }
    }
    result
}

/// Split template source into (frontmatter_src, body_src).
/// Returns ("", full_src) if no valid --- delimiters found.
fn split_template_sections(src: &str) -> (&str, &str) {
    let src = src.trim_start();
    if !src.starts_with("---") {
        return ("", src);
    }
    // find second ---
    let after_first = &src[3..];
    if let Some(pos) = after_first.find("\n---") {
        let fm = &after_first[..pos];
        let body = &after_first[pos + 4..]; // skip \n---
        (fm, body)
    } else {
        ("", src)
    }
}

/// Extract {{variable}} names that appear in the frontmatter section only.
/// Auto-injected variables (created, updated) are excluded.
pub fn extract_frontmatter_variables(src: &str) -> Vec<String> {
    let (fm, _) = split_template_sections(src);
    extract_variables(fm)
        .into_iter()
        .filter(|v| !AUTO_INJECT.contains(&v.as_str()))
        .collect()
}

/// Extract {{variable}} names that appear only in the body (after frontmatter).
/// Variables already in frontmatter and auto-injected variables are excluded.
pub fn extract_body_variables(src: &str) -> Vec<String> {
    let (_, body) = split_template_sections(src);
    let fm_vars = extract_frontmatter_variables(src);
    extract_variables(body)
        .into_iter()
        .filter(|v| !AUTO_INJECT.contains(&v.as_str()) && !fm_vars.contains(v))
        .collect()
}

// ── 테스트 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── 기존 테스트 ──────────────────────────────────────────────────────────

    #[test]
    fn register_and_render_simple_template() {
        let mut engine = TemplateEngine::new();
        engine.register("hello", "Hello, {{name}}!").unwrap();
        let result = engine.render("hello", &json!({"name": "World"})).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn render_unknown_template_returns_not_found() {
        let engine = TemplateEngine::new();
        let err = engine.render("nonexistent", &json!({})).unwrap_err();
        assert!(matches!(err, TemplateError::NotFound(n) if n == "nonexistent"));
    }

    #[test]
    fn with_builtins_has_twelve_templates() {
        let engine = TemplateEngine::with_builtins();
        let names = ["note", "meeting", "decision", "journal", "retrospective",
                      "devlog", "weekly", "study", "library", "article", "todo", "techspec"];
        for name in names {
            assert!(engine.hb.has_template(name), "missing builtin: {name}");
        }
    }

    // ── 새 테스트 ────────────────────────────────────────────────────────────

    #[test]
    fn list_templates_returns_twelve_builtins() {
        let engine = TemplateEngine::with_builtins();
        assert_eq!(engine.list_templates().len(), 12);
    }

    #[test]
    fn todo_template_has_body_variables() {
        let engine = TemplateEngine::with_builtins();
        let src = engine.get_template_source("todo").unwrap();
        let body_vars = extract_body_variables(&src);
        assert!(body_vars.contains(&"tasks".to_string()), "tasks should be in body_variables");
        assert!(body_vars.contains(&"notes".to_string()), "notes should be in body_variables");
    }

    #[test]
    fn techspec_template_has_body_variables() {
        let engine = TemplateEngine::with_builtins();
        let src = engine.get_template_source("techspec").unwrap();
        let body_vars = extract_body_variables(&src);
        assert!(body_vars.contains(&"overview".to_string()), "overview should be in body_variables");
        assert!(body_vars.contains(&"design".to_string()), "design should be in body_variables");
        assert!(body_vars.contains(&"api".to_string()), "api should be in body_variables");
        assert!(body_vars.contains(&"alternatives".to_string()), "alternatives should be in body_variables");
    }

    #[test]
    fn todo_frontmatter_has_date() {
        let engine = TemplateEngine::with_builtins();
        let src = engine.get_template_source("todo").unwrap();
        let fm_fields = extract_frontmatter_variables(&src);
        assert!(fm_fields.contains(&"date".to_string()), "date should be in frontmatter_fields");
    }

    #[test]
    fn techspec_frontmatter_has_author_status() {
        let engine = TemplateEngine::with_builtins();
        let src = engine.get_template_source("techspec").unwrap();
        let fm_fields = extract_frontmatter_variables(&src);
        assert!(fm_fields.contains(&"author".to_string()), "author should be in frontmatter_fields");
        assert!(fm_fields.contains(&"status".to_string()), "status should be in frontmatter_fields");
    }

    #[test]
    fn list_templates_has_descriptions() {
        let engine = TemplateEngine::with_builtins();
        let meeting = engine.list_templates().into_iter().find(|t| t.name == "meeting").unwrap();
        assert!(!meeting.description.is_empty());
        assert_eq!(meeting.description, "회의록");
    }

    #[test]
    fn get_template_source_returns_some_for_note() {
        let engine = TemplateEngine::with_builtins();
        let src = engine.get_template_source("note");
        assert!(src.is_some());
        assert!(src.unwrap().contains("{{title}}"));
    }

    #[test]
    fn get_template_source_returns_none_for_unknown() {
        let engine = TemplateEngine::with_builtins();
        assert!(engine.get_template_source("nonexistent").is_none());
    }

    #[test]
    fn extract_variables_from_simple_template() {
        let src = "---\ntitle: {{title}}\ndate: {{date}}\n---\n# {{title}}\n{{content}}";
        let vars = extract_variables(src);
        assert_eq!(vars, vec!["title", "date", "content"]);
    }

    #[test]
    fn extract_variables_deduplicates() {
        let src = "{{title}} {{title}} {{body}}";
        let vars = extract_variables(src);
        assert_eq!(vars, vec!["title", "body"]);
    }

    #[test]
    fn extract_variables_skips_block_helpers() {
        let src = "{{#if show}}{{item}}{{/if}}{{name}}";
        let vars = extract_variables(src);
        assert!(vars.contains(&"item".to_string()));
        assert!(vars.contains(&"name".to_string()));
        assert!(!vars.contains(&"#if".to_string()));
        assert!(!vars.contains(&"/if".to_string()));
    }

    #[test]
    fn list_templates_includes_variables() {
        let engine = TemplateEngine::with_builtins();
        let meeting = engine.list_templates().into_iter().find(|t| t.name == "meeting").unwrap();
        assert!(meeting.variables.contains(&"title".to_string()));
        assert!(meeting.variables.contains(&"attendees".to_string()));
        assert!(meeting.variables.contains(&"agenda".to_string()));
    }

    #[test]
    fn render_note_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "Test Note",
            "created": "2026-04-13",
            "updated": "2026-04-13",
            "content": "Some content here."
        });
        let out = engine.render("note", &ctx).unwrap();
        assert!(out.contains("title: \"Test Note\""));
        assert!(out.contains("created: 2026-04-13"));
        assert!(out.contains("# Test Note"));
        assert!(out.contains("Some content here."));
    }

    #[test]
    fn render_meeting_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "Sprint Planning",
            "created": "2026-04-13",
            "updated": "2026-04-13",
            "date": "2026-04-13",
            "attendees": "Alice, Bob",
            "agenda": "Review backlog",
            "decisions": "Start feature X",
            "action_items": "- Alice: write spec"
        });
        let out = engine.render("meeting", &ctx).unwrap();
        assert!(out.contains("title: \"Sprint Planning\""));
        assert!(out.contains("attendees: [Alice, Bob]"));
        assert!(out.contains("## 참석자"));
        assert!(out.contains("Start feature X"));
    }

    // ── frontmatter/body 분리 테스트 ────────────────────────────────────────

    #[test]
    fn extract_frontmatter_variables_only_from_fm_section() {
        let src = "---\ntitle: {{title}}\ndate: {{date}}\n---\n# {{title}}\n{{content}}";
        let fm = extract_frontmatter_variables(src);
        assert!(fm.contains(&"title".to_string()), "title should be in frontmatter");
        assert!(fm.contains(&"date".to_string()), "date should be in frontmatter");
        assert!(!fm.contains(&"content".to_string()), "content should NOT be in frontmatter");
    }

    #[test]
    fn extract_body_variables_only_from_body_section() {
        let src = "---\ntitle: {{title}}\ndate: {{date}}\n---\n# {{title}}\n{{content}}\n{{summary}}";
        let body = extract_body_variables(src);
        assert!(body.contains(&"content".to_string()), "content should be in body");
        assert!(body.contains(&"summary".to_string()), "summary should be in body");
        // title appears in body too (# {{title}}) but is NOT a pure body variable
        // however it will be included since it appears in body section
    }

    #[test]
    fn template_info_has_frontmatter_and_body_fields() {
        let engine = TemplateEngine::with_builtins();
        let devlog = engine.list_templates().into_iter().find(|t| t.name == "devlog").unwrap();
        // frontmatter fields: title, date (template-specific), created/updated excluded (auto)
        assert!(devlog.frontmatter_fields.contains(&"title".to_string()));
        assert!(devlog.frontmatter_fields.contains(&"date".to_string()));
        // body variables: work_summary, learnings, next_steps
        assert!(devlog.body_variables.contains(&"work_summary".to_string()));
        assert!(devlog.body_variables.contains(&"learnings".to_string()));
        assert!(devlog.body_variables.contains(&"next_steps".to_string()));
        // body variables should NOT include auto-injected
        assert!(!devlog.body_variables.contains(&"created".to_string()));
        assert!(!devlog.body_variables.contains(&"updated".to_string()));
    }

    #[test]
    fn template_info_meeting_frontmatter_includes_attendees() {
        let engine = TemplateEngine::with_builtins();
        let meeting = engine.list_templates().into_iter().find(|t| t.name == "meeting").unwrap();
        assert!(meeting.frontmatter_fields.contains(&"attendees".to_string()));
        assert!(meeting.frontmatter_fields.contains(&"date".to_string()));
        assert!(meeting.body_variables.contains(&"agenda".to_string()));
        assert!(meeting.body_variables.contains(&"decisions".to_string()));
        assert!(meeting.body_variables.contains(&"action_items".to_string()));
    }

    #[test]
    fn render_devlog_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "2026-04-13 개발일지",
            "created": "2026-04-13",
            "updated": "2026-04-13",
            "date": "2026-04-13",
            "work_summary": "TemplateEngine 확장",
            "learnings": "Handlebars AST",
            "next_steps": "MCP 도구 추가"
        });
        let out = engine.render("devlog", &ctx).unwrap();
        assert!(out.contains("tags: [devlog]"));
        assert!(out.contains("## 작업 내용"));
        assert!(out.contains("TemplateEngine 확장"));
    }
}
