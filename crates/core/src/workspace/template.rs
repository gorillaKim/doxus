use handlebars::Handlebars;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template not found: {0}")]
    NotFound(String),
    #[error("render error: {0}")]
    Render(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct TemplateEngine {
    hb: Handlebars<'static>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self {
            hb: Handlebars::new(),
        }
    }

    /// Register a named template from a string
    pub fn register(&mut self, name: &str, template: &str) -> Result<(), TemplateError> {
        self.hb
            .register_template_string(name, template)
            .map_err(|e| TemplateError::Render(e.to_string()))
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

    /// Register all built-in templates (note, meeting, decision, journal, retrospective)
    pub fn with_builtins() -> Self {
        let mut engine = Self::new();

        engine
            .register(
                "note",
                r#"---
title: {{title}}
date: {{date}}
tags: [{{tags}}]
---

# {{title}}

{{content}}

---
*Created: {{date}}*"#,
            )
            .expect("builtin note template is valid");

        engine
            .register(
                "meeting",
                r#"---
title: {{title}}
date: {{date}}
attendees: [{{attendees}}]
---

# {{title}}

## 참석자
{{attendees}}

## 안건
{{agenda}}

## 결정 사항
{{decisions}}

## 액션 아이템
{{action_items}}"#,
            )
            .expect("builtin meeting template is valid");

        engine
            .register(
                "decision",
                r#"---
title: {{title}}
date: {{date}}
status: proposed
---

# {{title}}

## 컨텍스트
{{context}}

## 결정
{{decision}}

## 결과
{{consequences}}"#,
            )
            .expect("builtin decision template is valid");

        engine
            .register(
                "journal",
                r#"---
date: {{date}}
mood: {{mood}}
---

# {{date}} 일지

{{content}}

## 회고
{{reflection}}"#,
            )
            .expect("builtin journal template is valid");

        engine
            .register(
                "retrospective",
                r#"---
date: {{date}}
sprint: {{sprint}}
---

# {{sprint}} 회고

## 잘된 점
{{went_well}}

## 개선할 점
{{to_improve}}

## 액션
{{action_items}}"#,
            )
            .expect("builtin retrospective template is valid");

        engine
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn with_builtins_has_five_templates() {
        let engine = TemplateEngine::with_builtins();
        for name in ["note", "meeting", "decision", "journal", "retrospective"] {
            assert!(engine.hb.has_template(name), "missing builtin: {name}");
        }
    }

    #[test]
    fn render_note_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "Test Note",
            "date": "2026-04-04",
            "tags": "rust, test",
            "content": "Some content here."
        });
        let out = engine.render("note", &ctx).unwrap();
        assert!(out.contains("title: Test Note"));
        assert!(out.contains("date: 2026-04-04"));
        assert!(out.contains("# Test Note"));
        assert!(out.contains("Some content here."));
        assert!(out.contains("*Created: 2026-04-04*"));
    }

    #[test]
    fn render_meeting_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "Sprint Planning",
            "date": "2026-04-04",
            "attendees": "Alice, Bob",
            "agenda": "Review backlog",
            "decisions": "Start feature X",
            "action_items": "- Alice: write spec"
        });
        let out = engine.render("meeting", &ctx).unwrap();
        assert!(out.contains("title: Sprint Planning"));
        assert!(out.contains("attendees: [Alice, Bob]"));
        assert!(out.contains("## 참석자"));
        assert!(out.contains("Alice, Bob"));
        assert!(out.contains("Start feature X"));
    }

    #[test]
    fn render_decision_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "title": "Use SQLite",
            "date": "2026-04-04",
            "context": "Need embedded DB",
            "decision": "SQLite with WAL",
            "consequences": "Simpler deployment"
        });
        let out = engine.render("decision", &ctx).unwrap();
        assert!(out.contains("title: Use SQLite"));
        assert!(out.contains("status: proposed"));
        assert!(out.contains("## 컨텍스트"));
        assert!(out.contains("Need embedded DB"));
        assert!(out.contains("SQLite with WAL"));
    }

    #[test]
    fn render_journal_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "date": "2026-04-04",
            "mood": "focused",
            "content": "Worked on templates.",
            "reflection": "Good progress."
        });
        let out = engine.render("journal", &ctx).unwrap();
        assert!(out.contains("date: 2026-04-04"));
        assert!(out.contains("mood: focused"));
        assert!(out.contains("# 2026-04-04 일지"));
        assert!(out.contains("Worked on templates."));
        assert!(out.contains("## 회고"));
        assert!(out.contains("Good progress."));
    }

    #[test]
    fn render_retrospective_template() {
        let engine = TemplateEngine::with_builtins();
        let ctx = json!({
            "date": "2026-04-04",
            "sprint": "Sprint 5",
            "went_well": "Team collaboration",
            "to_improve": "PR review speed",
            "action_items": "- Set review SLA"
        });
        let out = engine.render("retrospective", &ctx).unwrap();
        assert!(out.contains("sprint: Sprint 5"));
        assert!(out.contains("# Sprint 5 회고"));
        assert!(out.contains("## 잘된 점"));
        assert!(out.contains("Team collaboration"));
        assert!(out.contains("## 개선할 점"));
        assert!(out.contains("PR review speed"));
    }
}
