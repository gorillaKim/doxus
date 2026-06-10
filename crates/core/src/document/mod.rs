pub mod frontmatter;
pub mod section;
pub mod service;

pub use frontmatter::{
    build_document, fill_placeholders, parse_frontmatter, FrontmatterFields, ParsedTemplate,
};
pub use section::{
    delete_section, insert_section_after, parse_sections, replace_section, Section, SectionError,
};
pub use service::DocumentService;
