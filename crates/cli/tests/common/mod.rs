use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnv {
    pub db_path: PathBuf,
    pub _dir: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        TestEnv { db_path, _dir: dir }
    }

    pub fn doxus_cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("doxus").unwrap();
        cmd.env("DOXUS_DB_PATH", &self.db_path);
        cmd
    }
}
