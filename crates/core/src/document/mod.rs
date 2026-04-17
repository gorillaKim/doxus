pub mod section;
pub mod frontmatter;
pub mod service;

pub use section::{Section, SectionError, parse_sections, replace_section, insert_section_after, delete_section};
pub use frontmatter::{ParsedTemplate, FrontmatterFields, parse_frontmatter, build_document, fill_placeholders};
pub use service::DocumentService;
