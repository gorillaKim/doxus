pub mod template;
pub use template::{TemplateEngine, TemplateError, TemplateInfo, extract_variables, extract_frontmatter_variables, extract_body_variables};

use rusqlite::{Connection, ErrorCode};

// ── 디폴트 워크스페이스 seed ─────────────────────────────────────────────────

/// 앱 시작 시 호출. 디폴트 워크스페이스 프로젝트가 없으면 자동 생성.
/// `data_dir`: ~/.doxus 경로 (예: dirs::home_dir()/.doxus)
pub fn ensure_default_workspace(
    conn: &Connection,
    data_dir: &std::path::Path,
) -> Result<i64, WorkspaceError> {
    // 이미 존재하면 해당 id 반환
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE source_type='workspace' AND is_default=1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = existing {
        return Ok(id);
    }

    // ~/.doxus/workspaces/default/ 폴더 생성
    let ws_path = data_dir.join("workspaces").join("default");
    std::fs::create_dir_all(&ws_path).map_err(|e| {
        WorkspaceError::Db(rusqlite::Error::InvalidParameterName(format!(
            "폴더 생성 실패: {e}"
        )))
    })?;

    let path_str = ws_path.to_string_lossy().to_string();
    let now = now_secs();

    conn.execute(
        "INSERT INTO projects(name, display_name, description, path, status, source_type, config_json, is_default, created_at, updated_at)
         VALUES ('default-workspace', '기본 워크스페이스', '기본 문서 저장소', ?1, 'active', 'workspace', '{}', 1, ?2, ?2)",
        rusqlite::params![path_str, now],
    )?;

    Ok(conn.last_insert_rowid())
}

// ── 새 프로젝트 기반 워크스페이스 타입 ──────────────────────────────────────

/// projects 테이블의 워크스페이스 row
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceProject {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub path: String,
    pub is_default: bool,
    pub created_at: i64,
}

/// 새 워크스페이스 프로젝트 생성 (추가 워크스페이스, is_default=0)
pub fn create_workspace_project(
    conn: &Connection,
    data_dir: &std::path::Path,
    display_name: &str,
    description: Option<&str>,
) -> Result<WorkspaceProject, WorkspaceError> {
    let slug = display_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .to_lowercase();
    let name = format!("ws-{slug}");

    let ws_path = data_dir.join("workspaces").join(&name);
    std::fs::create_dir_all(&ws_path).map_err(|e| {
        WorkspaceError::Db(rusqlite::Error::InvalidParameterName(format!(
            "폴더 생성 실패: {e}"
        )))
    })?;

    let path_str = ws_path.to_string_lossy().to_string();
    let now = now_secs();

    conn.execute(
        "INSERT INTO projects(name, display_name, description, path, status, source_type, config_json, is_default, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'active', 'workspace', '{}', 0, ?5, ?5)",
        rusqlite::params![name, display_name, description, path_str, now],
    )
    .map_err(|e| {
        if is_unique_violation(&e) {
            WorkspaceError::Duplicate(name.clone())
        } else {
            WorkspaceError::Db(e)
        }
    })?;

    let id = conn.last_insert_rowid();
    get_workspace_project(conn, id)
}

/// 워크스페이스 프로젝트 단건 조회
pub fn get_workspace_project(
    conn: &Connection,
    id: i64,
) -> Result<WorkspaceProject, WorkspaceError> {
    conn.query_row(
        "SELECT id, name, display_name, description, path, is_default, created_at
         FROM projects WHERE id=?1 AND source_type='workspace'",
        [id],
        |r| {
            Ok(WorkspaceProject {
                id: r.get(0)?,
                name: r.get(1)?,
                display_name: r.get(2)?,
                description: r.get(3)?,
                path: r.get(4)?,
                is_default: r.get::<_, i64>(5)? == 1,
                created_at: r.get(6)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => WorkspaceError::NotFound(id.to_string()),
        other => WorkspaceError::Db(other),
    })
}

/// 워크스페이스 프로젝트 목록 조회
pub fn list_workspace_projects(conn: &Connection) -> Result<Vec<WorkspaceProject>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, display_name, description, path, is_default, created_at
         FROM projects WHERE source_type='workspace' ORDER BY is_default DESC, created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(WorkspaceProject {
            id: r.get(0)?,
            name: r.get(1)?,
            display_name: r.get(2)?,
            description: r.get(3)?,
            path: r.get(4)?,
            is_default: r.get::<_, i64>(5)? == 1,
            created_at: r.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(WorkspaceError::Db)
}

/// 워크스페이스 프로젝트 삭제 (is_default=1이면 거부)
pub fn delete_workspace_project(conn: &Connection, id: i64) -> Result<(), WorkspaceError> {
    // 디폴트 워크스페이스 삭제 방지
    let is_default: i64 = conn
        .query_row(
            "SELECT is_default FROM projects WHERE id=?1 AND source_type='workspace'",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| WorkspaceError::NotFound(id.to_string()))?;

    if is_default == 1 {
        return Err(WorkspaceError::Db(rusqlite::Error::InvalidParameterName(
            "디폴트 워크스페이스는 삭제할 수 없습니다".to_string(),
        )));
    }

    let affected = conn.execute(
        "DELETE FROM projects WHERE id=?1 AND source_type='workspace'",
        [id],
    )?;
    if affected == 0 {
        return Err(WorkspaceError::NotFound(id.to_string()));
    }
    Ok(())
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    // ── 프로젝트 기반 워크스페이스 테스트 ──────────────────────────────────────

    #[test]
    fn ensure_default_workspace_creates_project() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        let id = ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        assert!(id > 0);

        let ws = get_workspace_project(&db.conn, id).unwrap();
        assert!(ws.is_default);
        assert_eq!(ws.name, "default-workspace");

        // 폴더가 실제로 생성되었는지 확인
        let ws_path = tmp.path().join("workspaces").join("default");
        assert!(ws_path.exists(), "워크스페이스 폴더가 생성되어야 함");
    }

    #[test]
    fn ensure_default_workspace_idempotent() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        let id1 = ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        let id2 = ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        assert_eq!(id1, id2, "두 번 호출해도 같은 id 반환");

        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE source_type='workspace' AND is_default=1",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1, "디폴트 워크스페이스는 하나만 존재해야 함");
    }

    #[test]
    fn create_workspace_project_creates_folder() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        // 디폴트 워크스페이스 먼저 seed
        ensure_default_workspace(&db.conn, tmp.path()).unwrap();

        let ws = create_workspace_project(&db.conn, tmp.path(), "업무 워크스페이스", Some("업무용")).unwrap();
        assert!(!ws.is_default);
        assert_eq!(ws.display_name, "업무 워크스페이스");

        let ws_path = std::path::Path::new(&ws.path);
        assert!(ws_path.exists(), "워크스페이스 폴더가 생성되어야 함");
    }

    #[test]
    fn delete_default_workspace_is_rejected() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        let id = ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        let result = delete_workspace_project(&db.conn, id);
        assert!(result.is_err(), "디폴트 워크스페이스 삭제는 거부되어야 함");
    }

    #[test]
    fn delete_non_default_workspace_succeeds() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        let ws = create_workspace_project(&db.conn, tmp.path(), "삭제될 워크스페이스", None).unwrap();
        delete_workspace_project(&db.conn, ws.id).unwrap();

        let list = list_workspace_projects(&db.conn).unwrap();
        assert_eq!(list.len(), 1, "디폴트 워크스페이스만 남아야 함");
    }

    #[test]
    fn list_workspace_projects_default_first() {
        let db = TestDb::new();
        let tmp = tempfile::tempdir().unwrap();
        ensure_default_workspace(&db.conn, tmp.path()).unwrap();
        create_workspace_project(&db.conn, tmp.path(), "추가 워크스페이스", None).unwrap();

        let list = list_workspace_projects(&db.conn).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_default, "디폴트 워크스페이스가 첫 번째여야 함");
    }
}
