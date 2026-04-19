pub use doxus_plugin_sdk::links::{LinkExtractor, dx_uri_regex};

pub struct LinkResolver;

impl LinkResolver {
    /// Resolves a raw link string to a document ID.
    /// Looks first in the current project, then globally.
    pub fn resolve_link(
        conn: &rusqlite::Connection,
        current_project_id: i64,
        raw_link: &str,
    ) -> Option<i64> {
        // 1. Handle Doxus Virtual URIs: doxus://{source_project_id}/{source_doc_id}
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

        // Base normalization: remove extensions and common path prefixes
        let mut normalized = raw_link.trim().trim_start_matches("./");
        normalized = normalized.trim_start_matches("../");
        if let Some(stripped) = normalized.strip_suffix(".md") {
            normalized = stripped;
        }

        // a. Match by source_doc_id, title, or file_path (with suffix matching for paths)
        let sql = "SELECT id FROM documents 
                   WHERE project_id = ?1 
                   AND (source_doc_id = ?2 OR title = ?2 OR file_path = ?2 
                        OR source_doc_id = ?3 OR title = ?3 OR file_path = ?3
                        OR file_path LIKE '%' || ?2
                        OR file_path LIKE '%' || ?3
                        OR file_path LIKE '%' || ?3 || '.md')";
        
        let res: Option<i64> = conn.query_row(
            sql,
            rusqlite::params![current_project_id, raw_link, normalized],
            |r| r.get(0)
        ).ok();

        if res.is_some() {
            return res;
        }

        // 3. Global search (Fallthrough)
        let sql_global = "SELECT id FROM documents 
                          WHERE source_doc_id = ?1 OR title = ?1 OR file_path = ?1
                             OR source_doc_id = ?2 OR title = ?2 OR file_path = ?2
                             OR file_path LIKE '%' || ?1
                             OR file_path LIKE '%' || ?2
                             OR file_path LIKE '%' || ?2 || '.md'
                          LIMIT 1";
        
        let res: Option<i64> = conn.query_row(
            sql_global,
            rusqlite::params![raw_link, normalized],
            |r| r.get(0)
        ).ok();

        if res.is_some() {
            return res;
        }

        // 4. TODO: Alias search
        
        None
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

        let total_unresolved = rows.len();
        if total_unresolved > 0 {
            crate::log_d!("links", "[LinkResolver] Found {} unresolved links. Starting resolution...", total_unresolved);
        }

        let mut resolved_count = 0;
        for (link_id, source_doc_id, target_raw) in rows {
            // Get the project_id of the source document
            let project_id: i64 = conn.query_row(
                "SELECT project_id FROM documents WHERE id = ?1",
                [source_doc_id],
                |r| r.get(0)
            ).map_err(|e| e.to_string())?;

            if let Some(target_id) = Self::resolve_link(conn, project_id, &target_raw) {
                conn.execute(
                    "UPDATE document_links SET target_id = ?1 WHERE id = ?2",
                    rusqlite::params![target_id, link_id]
                ).map_err(|e| e.to_string())?;
                resolved_count += 1;
                crate::log_d!("links", "[LinkResolver] Resolved link '{}' -> document ID: {}", target_raw, target_id);
            }
        }

        if resolved_count > 0 {
            crate::log_d!("links", "[LinkResolver] Successfully resolved {}/{} links", resolved_count, total_unresolved);
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
        assert!(links.contains(&"https://example.com".to_string()));
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
