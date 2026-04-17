/// Domain types mapped from DB rows.
/// rusqlite Row is never exposed outside this module.

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub path: String,
    pub status: ProjectStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectStatus {
    Active,
    Disabled,
}

impl ProjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

impl std::str::FromStr for ProjectStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            other => Err(format!("unknown status: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub id: i64,
    pub project_id: i64,
    pub source_doc_id: String,
    pub file_path: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub content_hash: String,
    pub plugin_id: Option<String>,
    pub last_indexed: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: i64,
    pub document_id: i64,
    pub heading_path: Option<String>,
    pub content: String,
    pub chunk_index: i32,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub document_id: i64,
    pub chunk_id: i64,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub heading_path: Option<String>,
    pub snippet: String,
    pub context_content: Option<String>,
    pub metadata_json: Option<String>,
    pub score: f64,
}
