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
    /// 파일 시스템 감시자에 의한 이벤트 (Push)
    FileEvent { project_name: String, path: std::path::PathBuf },
}

pub struct SyncManager {
    indexing_service: Arc<IndexingService>,
    tx: mpsc::Sender<SyncTrigger>,
    last_sync_times: Arc<Mutex<HashMap<String, Instant>>>,
    jitter_map: Arc<Mutex<HashMap<String, f64>>>,
    watcher_manager: Arc<Mutex<Option<Arc<crate::watcher::WatcherManager>>>>,
}

impl SyncManager {
    pub fn new(indexing_service: Arc<IndexingService>) -> (Self, mpsc::Receiver<SyncTrigger>) {
        let (tx, rx) = mpsc::channel(32);
        (
            Self {
                indexing_service,
                tx,
                last_sync_times: Arc::new(Mutex::new(HashMap::new())),
                jitter_map: Arc::new(Mutex::new(HashMap::new())),
                watcher_manager: Arc::new(Mutex::new(None)),
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

    pub async fn init_watchers(&self) {
        let active_projects = self.get_active_projects().unwrap_or_default();
        for project_name in active_projects {
            crate::log_d!("sync", "[SyncManager] Running initial Catch-up scan for {}", project_name);
            let _ = self.indexing_service.index_project(&project_name).await;
            self.update_last_sync(&project_name).await;
        }

        let wm = Arc::new(crate::watcher::WatcherManager::new(
            Arc::clone(&self.indexing_service),
            self.tx.clone(),
        ));
        let _ = wm.restart_all().await;
        let mut guard = self.watcher_manager.lock().await;
        *guard = Some(wm);
    }

    pub async fn start_loop(self: Arc<Self>, mut rx: mpsc::Receiver<SyncTrigger>) {
        crate::log_d!("sync", "[SyncManager] Background loop started");
        
        while let Some(trigger) = rx.recv().await {
            crate::log_d!("sync", "[SyncManager] Received trigger: {:?}", trigger);
            
            match trigger {
                SyncTrigger::Manual(project_name) => {
                    let _ = self.indexing_service.index_project(&project_name).await;
                    self.update_last_sync(&project_name).await;
                }
                SyncTrigger::FileEvent { project_name, .. } => {
                    // 쿨다운 적용: 마지막 동기화 이후 너무 짧은 시간(예: 2초) 내에는 스킵
                    let should_skip = {
                        let last_syncs = self.last_sync_times.lock().await;
                        if let Some(last) = last_syncs.get(&project_name) {
                            last.elapsed() < Duration::from_secs(2)
                        } else {
                            false
                        }
                    };

                    if should_skip {
                        crate::log_d!("sync", "[SyncManager] Skipping FileEvent for {} (cooldown)", project_name);
                        continue;
                    }

                    let _ = self.indexing_service.index_project(&project_name).await;
                    self.update_last_sync(&project_name).await;
                }
                SyncTrigger::Focus | SyncTrigger::Periodic | SyncTrigger::Idle => {
                    self.run_global_sync(trigger).await;
                }
            }
        }
    }

    async fn run_global_sync(&self, trigger: SyncTrigger) {
        let active_projects = match self.get_active_projects() {
            Ok(p) => p,
            Err(e) => {
                crate::log_d!("sync", "[SyncManager] Failed to get active projects: {}", e);
                return;
            }
        };

        for project_name in active_projects {
            if self.should_sync(&project_name, &trigger).await {
                crate::log_d!("sync", "[SyncManager] Syncing project: {} (Trigger: {:?})", project_name, trigger);
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

    async fn should_sync(&self, project_name: &str, trigger: &SyncTrigger) -> bool {
        let policy = match self.indexing_service.get_project_policy(project_name).await {
            Ok(p) => p,
            Err(_) => return false,
        };

        use doxus_plugin_sdk::SyncPolicy;

        match (policy, trigger) {
            (SyncPolicy::Realtime(_), _) => {
                // Realtime은 FileEvent가 직접 index_project를 호출하므로 global_sync에서는 스킵
                false
            }
            (SyncPolicy::OnFocus, SyncTrigger::Focus) => {
                let last_syncs = self.last_sync_times.lock().await;
                if let Some(last) = last_syncs.get(project_name) {
                    // 플러그인 유형에 따라 쿨다운 차등 적용
                    // TODO: 인덱싱 서비스에서 플러그인 정보를 미리 가져오도록 최적화 가능
                    let plugin_id = match self.get_project_plugin_id(project_name).await {
                        Ok(id) => id,
                        Err(_) => "com.doxus.obsidian".to_string(), // 기본값
                    };

                    let is_external = plugin_id.contains("confluence") || plugin_id.contains("github") || !plugin_id.starts_with("com.doxus");
                    let cooldown_secs = if is_external { 15 * 60 } else { 60 };

                    if last.elapsed() < Duration::from_secs(cooldown_secs) {
                        crate::log_d!("sync", "[SyncManager] Skipping Focus trigger for {} (cooldown {}s)", project_name, cooldown_secs);
                        return false;
                    }
                    true
                } else {
                    true
                }
            }
            (SyncPolicy::Interval { seconds }, SyncTrigger::Periodic) => {
                let last_syncs = self.last_sync_times.lock().await;
                if let Some(last) = last_syncs.get(project_name) {
                    let jitter = self.get_jitter(project_name).await;
                    let interval = Duration::from_secs_f64(seconds as f64 * (1.0 + jitter));
                    last.elapsed() >= interval
                } else {
                    true
                }
            }
            (SyncPolicy::Manual, _) => false,
            _ => false,
        }
    }

    async fn get_jitter(&self, project_name: &str) -> f64 {
        let mut jitter_map = self.jitter_map.lock().await;
        *jitter_map.entry(project_name.to_string()).or_insert_with(|| {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            rng.gen_range(-0.1..0.1) // ±10% jitter
        })
    }

    async fn update_last_sync(&self, project_name: &str) {
        let mut last_syncs = self.last_sync_times.lock().await;
        last_syncs.insert(project_name.to_string(), Instant::now());
        // Reset jitter
        let mut jitter_map = self.jitter_map.lock().await;
        jitter_map.remove(project_name);
    }

    async fn get_project_plugin_id(&self, project_name: &str) -> Result<String, String> {
        let conn = self.indexing_service.conn();
        let conn = conn.lock().map_err(|_| "db lock poisoned")?;
        
        let plugin_id: String = conn.query_row(
            "SELECT COALESCE(si.plugin_id, p.source_type) 
             FROM projects p 
             LEFT JOIN source_instances si ON p.id = si.project_id 
             WHERE p.name = ?1",
            rusqlite::params![project_name],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;

        Ok(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use crate::plugin::PluginManager;
    use crate::search::SearchEngine;
    use doxus_plugin_sdk::SyncPolicy;

    async fn setup_manager() -> (Arc<SyncManager>, Arc<std::sync::Mutex<rusqlite::Connection>>) {
        let db = TestDb::new();
        let conn = Arc::new(std::sync::Mutex::new(db.conn));
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), Arc::new(crate::embedding::NoOpEmbedder) as Arc<dyn crate::embedding::EmbeddingProvider + Send + Sync>));
        let indexer = Arc::new(IndexingService::new(conn.clone(), pm, engine));
        let (mgr, _) = SyncManager::new(indexer);
        (Arc::new(mgr), conn)
    }

    #[tokio::test]
    async fn test_should_sync_on_focus() {
        let (mgr, conn) = setup_manager().await;
        
        conn.lock().unwrap().execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('proj1', 'P1', '', '{\"type\":\"on_focus\"}', 0, 0)",
            []
        ).unwrap();

        // 1. Focus trigger -> true
        assert!(mgr.should_sync("proj1", &SyncTrigger::Focus).await);

        // 2. Periodic trigger -> false
        assert!(!mgr.should_sync("proj1", &SyncTrigger::Periodic).await);
    }

    #[tokio::test]
    async fn test_should_sync_interval() {
        let (mgr, conn) = setup_manager().await;
        
        conn.lock().unwrap().execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('proj1', 'P1', '', '{\"type\":\"interval\",\"seconds\":60}', 0, 0)",
            []
        ).unwrap();

        // No record of last sync -> true
        assert!(mgr.should_sync("proj1", &SyncTrigger::Periodic).await);

        mgr.update_last_sync("proj1").await;

        // Just synced -> false
        assert!(!mgr.should_sync("proj1", &SyncTrigger::Periodic).await);
    }
}
