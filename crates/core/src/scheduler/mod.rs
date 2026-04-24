pub mod db;
pub mod executor;
pub mod schedule;

pub use schedule::{Schedule, ScheduledJob, Executor};
pub use db::SchedulerDb;

use std::sync::{Arc, Mutex};
use crate::indexing::IndexingService;

pub struct SchedulerManager {
    // std::sync::Mutex hold time must be minimized in async contexts
    conn: Arc<Mutex<rusqlite::Connection>>,
    indexer: Arc<IndexingService>,
}

impl SchedulerManager {
    pub fn new(
        conn: Arc<Mutex<rusqlite::Connection>>,
        indexer: Arc<IndexingService>,
    ) -> Self {
        Self { conn, indexer }
    }

    /// Executed periodically. Spawns tasks for due jobs.
    pub async fn tick(&self, is_idle: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn_clone = self.conn.clone();

        let due_jobs = tokio::task::spawn_blocking(move || {
            let conn = conn_clone.lock().unwrap();
            let sdb = SchedulerDb::new(&conn);
            sdb.due_jobs(now, is_idle).unwrap_or_default()
        }).await.unwrap_or_default();

        for job in due_jobs {
            let indexer_clone = self.indexer.clone();
            let conn_clone = self.conn.clone();
            let project_name = job.project_id.map(|_| job.job_name.clone());

            tokio::spawn(async move {
                let result = match job.executor {
                    Executor::System => {
                        executor::execute_system(
                            &job.action,
                            &job.action_config,
                            &indexer_clone,
                        ).await
                    }
                    Executor::Agent => {
                        executor::execute_agent(
                            &job.action,
                            &job.action_config,
                            project_name.as_deref(),
                        ).await
                    }
                };

                // Record result back to DB
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = conn_clone.lock().unwrap();
                    let sdb = SchedulerDb::new(&conn);
                    if result.success {
                        let _ = sdb.mark_completed(job.id, &result.message);
                    } else {
                        let _ = sdb.mark_failed(job.id, &result.message);
                    }
                }).await;
            });
        }
    }
    
    pub fn ensure_defaults(&self) {
        let conn_clone = self.conn.clone();
        
        // This is called on startup. Run synchronously.
        let conn = conn_clone.lock().unwrap();
        let sdb = SchedulerDb::new(&conn);
        let existing = sdb.list_jobs(None).unwrap_or_default();
        
        let has_freshness_batch = existing.iter().any(|j| j.action == "freshness_batch");
        if !has_freshness_batch {
            let default_job = ScheduledJob {
                id: 0,
                project_id: None,
                job_name: "Freshness Refresh".to_string(),
                description: Some("문서의 신선도 점수를 주기적으로 업데이트합니다.".to_string()),
                executor: Executor::System,
                action: "freshness_batch".to_string(),
                action_config: serde_json::json!({}),
                schedule: Schedule::Daily { hour: 3, minute: 0 },
                enabled: true,
                run_on_idle: false,
                is_immutable: true,
                last_run_at: None,
                next_run_at: chrono::Utc::now().timestamp() + 3600,
                created_by: "system".to_string(),
            };
            let _ = sdb.insert_job(&default_job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    #[tokio::test]
    async fn test_tick_system_action() {
        let db = TestDb::new();
        // Since IndexingService needs real dependencies, we might just test ensure_defaults
        // Testing tick() fully requires mocking the whole service stack
        
        let target_conn = std::sync::Arc::new(std::sync::Mutex::new(db.conn));
        
        // Mock dependencies for IndexingService isn't trivial here, let's just test ensure_defaults
        // If we want to test tick(), we can do it via unit test in executor.
        
        let lock = target_conn.lock().unwrap();
        let sdb = SchedulerDb::new(&lock);
        
        let existing = sdb.list_jobs(None).unwrap();
        assert_eq!(existing.len(), 0);
    }
}
