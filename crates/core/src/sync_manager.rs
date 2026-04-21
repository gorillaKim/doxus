use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, Instant};
use crate::indexing::IndexingService;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncTrigger {
    /// 앱이 포커스를 얻었을 때 (Incremental)
    Focus,
    /// 주기적인 자동 스캔 (Full/Deep)
    Periodic,
    /// 시스템 유휴 상태 감지 (Deep)
    Idle,
    /// 특정 프로젝트 수동 동기화
    Manual(String),
}

pub struct SyncManager {
    indexing_service: Arc<IndexingService>,
    tx: mpsc::Sender<SyncTrigger>,
    last_sync_times: Arc<Mutex<HashMap<String, Instant>>>,
    min_interval: Duration,
}

impl SyncManager {
    pub fn new(indexing_service: Arc<IndexingService>) -> (Self, mpsc::Receiver<SyncTrigger>) {
        let (tx, rx) = mpsc::channel(32);
        (
            Self {
                indexing_service,
                tx,
                last_sync_times: Arc::new(Mutex::new(HashMap::new())),
                min_interval: Duration::from_secs(300), // 5분 기본 Throttling
            },
            rx,
        )
    }

    pub async fn trigger(&self, trigger: SyncTrigger) {
        let _ = self.tx.send(trigger).await;
    }

    pub fn indexer(&self) -> Arc<IndexingService> {
        Arc::clone(&self.indexing_service)
    }

    pub async fn start_loop(self: Arc<Self>, mut rx: mpsc::Receiver<SyncTrigger>) {
        crate::log_d!("sync", "[SyncManager] Background loop started");
        
        while let Some(trigger) = rx.recv().await {
            crate::log_d!("sync", "[SyncManager] Received trigger: {:?}", trigger);
            
            match trigger {
                SyncTrigger::Manual(project_name) => {
                    let _ = self.indexing_service.index_project(&project_name).await;
                }
                SyncTrigger::Focus | SyncTrigger::Periodic | SyncTrigger::Idle => {
                    self.run_global_sync().await;
                }
            }
        }
    }

    async fn run_global_sync(&self) {
        let active_projects = match self.get_active_projects() {
            Ok(p) => p,
            Err(e) => {
                crate::log_d!("sync", "[SyncManager] Failed to get active projects: {}", e);
                return;
            }
        };

        for project_name in active_projects {
            if self.should_sync(&project_name).await {
                crate::log_d!("sync", "[SyncManager] Syncing project: {}", project_name);
                match self.indexing_service.index_project(&project_name).await {
                    Ok(n) => {
                        crate::log_d!("sync", "[SyncManager] Indexed {} documents for {}", n, project_name);
                        self.update_last_sync(&project_name).await;
                    }
                    Err(e) => {
                        crate::log_d!("sync", "[SyncManager] Sync failed for {}: {}", project_name, e);
                    }
                }
            }
        }
    }

    fn get_active_projects(&self) -> Result<Vec<String>, String> {
        self.indexing_service.list_active_projects()
    }

    async fn should_sync(&self, project_name: &str) -> bool {
        let last_syncs = self.last_sync_times.lock().await;
        if let Some(last) = last_syncs.get(project_name) {
            last.elapsed() >= self.min_interval
        } else {
            true
        }
    }

    async fn update_last_sync(&self, project_name: &str) {
        let mut last_syncs = self.last_sync_times.lock().await;
        last_syncs.insert(project_name.to_string(), Instant::now());
    }
}
