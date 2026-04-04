pub mod template;
pub use template::{TemplateEngine, TemplateError};

use rusqlite::{Connection, ErrorCode};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub project_ids: Vec<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceTemplate {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub config_json: String,
    pub created_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("name already exists: {0}")]
    Duplicate(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::ConstraintViolation
    )
}

pub struct WorkspaceRepo<'a> {
    conn: &'a Connection,
}

impl<'a> WorkspaceRepo<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, name: &str, description: Option<&str>) -> Result<Workspace, WorkspaceError> {
        let now = now_secs();
        let project_ids_json = "[]";
        self.conn
            .execute(
                "INSERT INTO workspaces(name, description, project_ids, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![name, description, project_ids_json, now, now],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    WorkspaceError::Duplicate(name.to_string())
                } else {
                    WorkspaceError::Db(e)
                }
            })?;
        let id = self.conn.last_insert_rowid();
        self.get(id)
    }

    pub fn get(&self, id: i64) -> Result<Workspace, WorkspaceError> {
        self.conn
            .query_row(
                "SELECT id, name, description, project_ids, created_at, updated_at
                 FROM workspaces WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => WorkspaceError::NotFound(id.to_string()),
                other => WorkspaceError::Db(other),
            })
            .and_then(|(id, name, description, project_ids_json, created_at, updated_at)| {
                let project_ids: Vec<i64> = serde_json::from_str(&project_ids_json)?;
                Ok(Workspace { id, name, description, project_ids, created_at, updated_at })
            })
    }

    pub fn list(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, project_ids, created_at, updated_at
             FROM workspaces ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, name, description, project_ids_json, created_at, updated_at) = row?;
            let project_ids: Vec<i64> = serde_json::from_str(&project_ids_json)
                .map_err(WorkspaceError::Json)?;
            result.push(Workspace { id, name, description, project_ids, created_at, updated_at });
        }
        Ok(result)
    }

    pub fn delete(&self, id: i64) -> Result<(), WorkspaceError> {
        let affected = self.conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
        if affected == 0 {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn add_project(&self, workspace_id: i64, project_id: i64) -> Result<(), WorkspaceError> {
        let mut ws = self.get(workspace_id)?;
        if !ws.project_ids.contains(&project_id) {
            ws.project_ids.push(project_id);
            let json = serde_json::to_string(&ws.project_ids)?;
            self.conn.execute(
                "UPDATE workspaces SET project_ids = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![json, now_secs(), workspace_id],
            )?;
        }
        Ok(())
    }

    pub fn remove_project(&self, workspace_id: i64, project_id: i64) -> Result<(), WorkspaceError> {
        let mut ws = self.get(workspace_id)?;
        ws.project_ids.retain(|&id| id != project_id);
        let json = serde_json::to_string(&ws.project_ids)?;
        self.conn.execute(
            "UPDATE workspaces SET project_ids = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![json, now_secs(), workspace_id],
        )?;
        Ok(())
    }
}

pub struct TemplateRepo<'a> {
    conn: &'a Connection,
}

impl<'a> TemplateRepo<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(
        &self,
        name: &str,
        description: Option<&str>,
        config_json: &str,
    ) -> Result<WorkspaceTemplate, WorkspaceError> {
        let now = now_secs();
        self.conn
            .execute(
                "INSERT INTO workspace_templates(name, description, config_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![name, description, config_json, now],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    WorkspaceError::Duplicate(name.to_string())
                } else {
                    WorkspaceError::Db(e)
                }
            })?;
        let id = self.conn.last_insert_rowid();
        self.conn
            .query_row(
                "SELECT id, name, description, config_json, created_at
                 FROM workspace_templates WHERE id = ?1",
                [id],
                |row| {
                    Ok(WorkspaceTemplate {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        config_json: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .map_err(WorkspaceError::Db)
    }

    pub fn list(&self) -> Result<Vec<WorkspaceTemplate>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, config_json, created_at
             FROM workspace_templates ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WorkspaceTemplate {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                config_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(WorkspaceError::Db)
    }

    pub fn delete(&self, id: i64) -> Result<(), WorkspaceError> {
        let affected = self.conn.execute(
            "DELETE FROM workspace_templates WHERE id = ?1",
            [id],
        )?;
        if affected == 0 {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    #[test]
    fn create_workspace_and_get() {
        let db = TestDb::new();
        let repo = WorkspaceRepo::new(&db.conn);
        let ws = repo.create("my-workspace", Some("desc")).unwrap();
        assert_eq!(ws.name, "my-workspace");
        let fetched = repo.get(ws.id).unwrap();
        assert_eq!(fetched.description, Some("desc".to_string()));
    }

    #[test]
    fn list_workspaces() {
        let db = TestDb::new();
        let repo = WorkspaceRepo::new(&db.conn);
        repo.create("ws1", None).unwrap();
        repo.create("ws2", None).unwrap();
        let list = repo.list().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn delete_workspace() {
        let db = TestDb::new();
        let repo = WorkspaceRepo::new(&db.conn);
        let ws = repo.create("to-delete", None).unwrap();
        repo.delete(ws.id).unwrap();
        let result = repo.get(ws.id);
        assert!(matches!(result, Err(WorkspaceError::NotFound(_))));
    }

    #[test]
    fn add_and_remove_project_from_workspace() {
        let db = TestDb::new();
        let repo = WorkspaceRepo::new(&db.conn);
        let ws = repo.create("ws", None).unwrap();
        repo.add_project(ws.id, 101).unwrap();
        let updated = repo.get(ws.id).unwrap();
        assert!(updated.project_ids.contains(&101));
        repo.remove_project(ws.id, 101).unwrap();
        let updated2 = repo.get(ws.id).unwrap();
        assert!(!updated2.project_ids.contains(&101));
    }

    #[test]
    fn duplicate_workspace_name_errors() {
        let db = TestDb::new();
        let repo = WorkspaceRepo::new(&db.conn);
        repo.create("unique", None).unwrap();
        let result = repo.create("unique", None);
        assert!(matches!(result, Err(WorkspaceError::Duplicate(_))));
    }

    #[test]
    fn create_and_list_templates() {
        let db = TestDb::new();
        let repo = TemplateRepo::new(&db.conn);
        repo.create("starter", Some("basic template"), "{}").unwrap();
        let list = repo.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "starter");
    }
}
