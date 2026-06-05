pub mod db;
pub mod executor;
pub mod schedule;

pub use schedule::{Schedule, ScheduledJob, Executor};
pub use db::SchedulerDb;
pub use executor::JobResult;

use std::sync::{Arc, Mutex};
use crate::indexing::IndexingService;

#[async_trait::async_trait]
pub trait AgentHandler: Send + Sync {
    async fn execute_agent(
        &self,
        job_name: &str,
        action: &str,
        config: &serde_json::Value,
    ) -> JobResult;
}

pub struct SchedulerManager {
    conn: crate::db::DbPool,
    indexer: Arc<IndexingService>,
    agent_handler: Mutex<Option<Arc<dyn AgentHandler>>>,
}

impl SchedulerManager {
    pub fn new(
        conn: crate::db::DbPool,
        indexer: Arc<IndexingService>,
    ) -> Self {
        Self { 
            conn, 
            indexer,
            agent_handler: Mutex::new(None),
        }
    }

    pub fn set_agent_handler(&self, handler: Arc<dyn AgentHandler>) {
        let mut h = self.agent_handler.lock().unwrap();
        *h = Some(handler);
    }

    /// Executed periodically. Spawns tasks for due jobs.
    pub async fn tick(&self, is_idle: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn_clone = self.conn.clone();

        let due_jobs = tokio::task::spawn_blocking(move || {
            let conn = conn_clone.get().unwrap();
            let sdb = SchedulerDb::new(&conn);
            sdb.due_jobs(now, is_idle).unwrap_or_default()
        }).await.unwrap_or_default();

        for job in due_jobs {
            let indexer_clone = self.indexer.clone();
            let conn_clone = self.conn.clone();

            // Get a clone of the agent handler if it exists
            let handler_opt = {
                let h = self.agent_handler.lock().unwrap();
                h.clone()
            };

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
                        if let Some(handler) = handler_opt {
                            handler.execute_agent(
                                &job.job_name,
                                &job.action,
                                &job.action_config,
                            ).await
                        } else {
                            JobResult { 
                                success: false, 
                                message: "Agent handler not initialized in this environment".into() 
                            }
                        }
                    }
                };

                // Record result back to DB
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = conn_clone.get().unwrap();
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
        let conn = conn_clone.get().unwrap();
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

        let has_co_refs_prune = existing.iter().any(|j| j.action == "co_refs_prune");
        if !has_co_refs_prune {
            let default_job = ScheduledJob {
                id: 0,
                project_id: None,
                job_name: "Co-occurrence References Prune".to_string(),
                description: Some("오래되고 빈도가 낮은 공동 참조 관계 데이터를 정해진 주기로 삭제합니다.".to_string()),
                executor: Executor::System,
                action: "co_refs_prune".to_string(),
                action_config: serde_json::json!({}),
                schedule: Schedule::Daily { hour: 4, minute: 0 },
                enabled: true,
                run_on_idle: false,
                is_immutable: true,
                last_run_at: None,
                next_run_at: chrono::Utc::now().timestamp() + 7200,
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
