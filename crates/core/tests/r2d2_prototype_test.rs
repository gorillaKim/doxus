use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

#[derive(Debug)]
pub struct SqliteConnectionManager {
    path: PathBuf,
}

impl SqliteConnectionManager {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let conn = Connection::open(&self.path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(conn)
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        conn.execute_batch("SELECT 1;")
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

#[tokio::test]
async fn test_r2d2_sqlite_pool_concurrency() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("r2d2_test.db");

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);",
            [],
        )
        .unwrap();
    }

    let manager = SqliteConnectionManager::new(db_path);
    let pool = r2d2::Pool::builder().max_size(5).build(manager).unwrap();

    let pool = Arc::new(pool);
    let mut handles = vec![];

    for i in 0..10 {
        let pool_clone = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let conn = pool_clone.get().unwrap();
            conn.execute(
                "INSERT INTO users (name) VALUES (?1)",
                [format!("user-{}", i)],
            )
            .unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
                .unwrap();
            count
        });
        handles.push(handle);
    }

    for h in handles {
        let count = h.await.unwrap();
        assert!(count > 0);
    }

    let conn = pool.get().unwrap();
    let final_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap();
    assert_eq!(final_count, 10);
}
