//! PromptLoader — loads and builds the system prompt for the doxus librarian agent.

use std::path::PathBuf;

mod defaults {
    pub const SYSTEM: &str = include_str!("../resources/librarian/system.md");
}

/// Strip YAML frontmatter from a markdown string.
fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            return rest[end + 3..].trim();
        }
    }
    trimmed
}

pub struct PromptLoader {
    agents_dir: PathBuf,
}

impl PromptLoader {
    pub fn new() -> Result<Self, String> {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        Ok(Self {
            agents_dir: PathBuf::from(home).join(".doxus").join("agents"),
        })
    }

    /// Write default prompt files if they don't exist.
    pub fn ensure_defaults(&self) -> Result<(), String> {
        let dir = self.agents_dir.join("librarian");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let system_path = dir.join("system.md");
        if !system_path.exists() {
            std::fs::write(&system_path, defaults::SYSTEM).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Return the combined system prompt for the librarian agent.
    pub fn build_system_prompt(&self) -> String {
        let custom_path = self.agents_dir.join("librarian").join("system.md");
        let content = if custom_path.exists() {
            std::fs::read_to_string(&custom_path).unwrap_or_else(|_| defaults::SYSTEM.to_string())
        } else {
            defaults::SYSTEM.to_string()
        };
        strip_frontmatter(&content).to_string()
    }
}
