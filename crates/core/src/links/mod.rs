pub use doxus_plugin_sdk::links::{LinkExtractor, dx_uri_regex};

pub struct LinkResolver;

impl LinkResolver {
    /// Resolves a raw link string to a document ID.
    /// Looks first in the current project, then globally.
    /// Resolves a raw link string to a document ID.
    /// Looks first in the current project, then globally.
    pub fn resolve_link(
        conn: &rusqlite::Connection,
        current_project_id: i64,
        raw_link: &str,
    ) -> Option<i64> {
        // 1. Handle Doxus Virtual URIs
        if let Some(caps) = dx_uri_regex().captures(raw_link) {
            let source_project_id = &caps[1];
            let source_doc_id = &caps[2];

            let res: Option<i64> = conn.query_row(
                "SELECT d.id FROM documents d 
                 JOIN projects p ON d.project_id = p.id 
                 WHERE p.source_project_id = ?1 AND d.source_doc_id = ?2",
                rusqlite::params![source_project_id, source_doc_id],
                |r| r.get(0)
            ).ok();
            
            if res.is_some() {
                return res;
            }
        }

        // 2. Handle Wiki-link aliases
        let target = if raw_link.contains('|') {
            raw_link.split('|').next().unwrap_or(raw_link).trim()
        } else {
            raw_link.trim()
        };

        let mut normalized = target.trim_start_matches("./");
        normalized = normalized.trim_start_matches("../");
        if let Some(stripped) = normalized.strip_suffix(".md") {
            normalized = stripped;
        }

        // Optimized SQL using NOCASE indexes (Removing LOWER() wrapper)
        let sql = "SELECT id FROM documents 
                   WHERE project_id = ?1 
                   AND (source_doc_id = ?2 OR title = ?2 OR file_path = ?2 
                        OR source_doc_id = ?3 OR title = ?3 OR file_path = ?3
                        OR file_path LIKE '%' || ?2
                        OR file_path LIKE '%' || ?3
                        OR file_path LIKE '%' || ?3 || '.md')
                   LIMIT 1";
        
        let res: Option<i64> = conn.query_row(
            sql,
            rusqlite::params![current_project_id, target, normalized],
            |r| r.get(0)
        ).ok();

        if res.is_some() {
            return res;
        }

        // Global search using NOCASE indexes
        let sql_global = "SELECT id FROM documents 
                          WHERE source_doc_id = ?1 OR title = ?1 OR file_path = ?1
                             OR source_doc_id = ?2 OR title = ?2 OR file_path = ?2
                             OR file_path LIKE '%' || ?1
                             OR file_path LIKE '%' || ?2
                             OR file_path LIKE '%' || ?2 || '.md'
                          LIMIT 1";
        
        let res: Option<i64> = conn.query_row(
            sql_global,
            rusqlite::params![target, normalized],
            |r| r.get(0)
        ).ok();

        if res.is_some() {
            return res;
        }

        // Alias search
        let sql_alias = "SELECT document_id FROM document_aliases WHERE alias = ?1 OR alias = ?2 LIMIT 1";
        let res: Option<i64> = conn.query_row(
            sql_alias,
            rusqlite::params![target, normalized],
            |r| r.get(0)
        ).ok();

        res
    }

    /// Resolves links only for a specific project to save time during indexing.
    pub fn resolve_project_links(conn: &rusqlite::Connection, project_id: i64) -> Result<usize, String> {
        let mut stmt = conn
            .prepare("SELECT dl.id, dl.target_raw FROM document_links dl 
                      JOIN documents d ON dl.source_id = d.id 
                      WHERE d.project_id = ?1 AND dl.target_id IS NULL")
            .map_err(|e| e.to_string())?;

        let rows: Vec<(i64, String)> = stmt
            .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        let mut resolved_count = 0;
        for (link_id, target_raw) in rows {
            if let Some(target_id) = Self::resolve_link(conn, project_id, &target_raw) {
                let _ = conn.execute(
                    "UPDATE document_links SET target_id = ?1 WHERE id = ?2",
                    rusqlite::params![target_id, link_id]
                );
                resolved_count += 1;
            }
        }
        Ok(resolved_count)
    }

    /// Finds all links with NULL target_id and tries to resolve them.
    pub fn resolve_all_unresolved_links(conn: &rusqlite::Connection) -> Result<usize, String> {
        let mut stmt = conn
            .prepare("SELECT id, source_id, target_raw FROM document_links WHERE target_id IS NULL")
            .map_err(|e| e.to_string())?;

        let rows: Vec<(i64, i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        let mut resolved_count = 0;
        for (link_id, source_doc_id, target_raw) in rows {
            let project_id: Result<i64, _> = conn.query_row(
                "SELECT project_id FROM documents WHERE id = ?1",
                [source_doc_id],
                |r| r.get(0)
            );

            if let Ok(pid) = project_id {
                if let Some(target_id) = Self::resolve_link(conn, pid, &target_raw) {
                    let _ = conn.execute(
                        "UPDATE document_links SET target_id = ?1 WHERE id = ?2",
                        rusqlite::params![target_id, link_id]
                    );
                    resolved_count += 1;
                }
            }
        }
        Ok(resolved_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    #[test]
    fn test_extract_wiki_links() {
        let content = "Check [[Important Document]] and [[Another-Doc]].";
        let links = LinkExtractor::extract_links(content);
        assert!(links.contains(&"Important Document".to_string()));
        assert!(links.contains(&"Another-Doc".to_string()));
    }

    #[test]
    fn test_extract_markdown_links() {
        let content = "Read [this](https://example.com) or [that](internal-doc.md).";
        let links = LinkExtractor::extract_links(content);
        // External links (http/https) are intentionally filtered out
        assert!(!links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"internal-doc.md".to_string()));
    }

    #[test]
    fn test_extract_doxus_uris() {
        let content = "Reference: doxus://my-project/source-123 and doxus://AI 리포트 V3/4756242498";
        let links = LinkExtractor::extract_links(content);
        assert!(links.contains(&"doxus://my-project/source-123".to_string()));
        assert!(links.contains(&"doxus://AI 리포트 V3/4756242498".to_string()));
    }

    #[test]
    fn test_extract_mixed_links() {
        let content = "Mixed [[Wiki]], [MD](md-link), and doxus://p/d.";
        let links = LinkExtractor::extract_links(content);
        assert_eq!(links.len(), 3);
        assert!(links.contains(&"Wiki".to_string()));
        assert!(links.contains(&"md-link".to_string()));
        assert!(links.contains(&"doxus://p/d".to_string()));
    }

    #[test]
    fn test_resolve_internal_link() {
        let db = TestDb::new();
        // Setup projects and documents
        db.conn.execute("INSERT INTO projects(name, source_project_id, display_name, path, created_at, updated_at) VALUES ('p1', 'proj-1', 'P1', '/p1', 0, 0)", []).unwrap();
        let p1_id = db.conn.last_insert_rowid();
        db.conn.execute("INSERT INTO documents(project_id, source_doc_id, title, content_hash) VALUES (?1, 'doc-a', 'Doc A', 'h1')", [p1_id]).unwrap();
        let doc_a_id = db.conn.last_insert_rowid();

        // 1. Match by source_doc_id in same project
        assert_eq!(LinkResolver::resolve_link(&db.conn, p1_id, "doc-a"), Some(doc_a_id));
        // 2. Match by title in same project
        assert_eq!(LinkResolver::resolve_link(&db.conn, p1_id, "Doc A"), Some(doc_a_id));
    }

    #[test]
    fn test_resolve_doxus_uri() {
        let db = TestDb::new();
        db.conn.execute("INSERT INTO projects(name, source_project_id, display_name, path, created_at, updated_at) VALUES ('p1', 'proj-1', 'P1', '/p1', 0, 0)", []).unwrap();
        let p1_id = db.conn.last_insert_rowid();
        db.conn.execute("INSERT INTO documents(project_id, source_doc_id, title, content_hash) VALUES (?1, 'doc-a', 'Doc A', 'h1')", [p1_id]).unwrap();
        let doc_a_id = db.conn.last_insert_rowid();

        // Cross-project resolution via doxus://
        assert_eq!(LinkResolver::resolve_link(&db.conn, 999, "doxus://proj-1/doc-a"), Some(doc_a_id));
    }

    #[test]
    fn test_resolve_global_fallthrough() {
        let db = TestDb::new();
        // Project 1
        db.conn.execute("INSERT INTO projects(name, source_project_id, display_name, path, created_at, updated_at) VALUES ('p1', 'proj-1', 'P1', '/p1', 0, 0)", []).unwrap();
        let p1_id = db.conn.last_insert_rowid();
        db.conn.execute("INSERT INTO documents(project_id, source_doc_id, title, content_hash) VALUES (?1, 'unique-doc', 'Unique', 'h1')", [p1_id]).unwrap();
        let doc_u_id = db.conn.last_insert_rowid();

        // Resolve globally even if in different project (p2)
        assert_eq!(LinkResolver::resolve_link(&db.conn, 888, "unique-doc"), Some(doc_u_id));
    }
}
