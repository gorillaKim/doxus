use crate::indexing::IndexingService;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, Instant};

type ProgressCallback = Arc<dyn Fn(String, usize, usize) + Send + Sync>;
type EventSender = mpsc::Sender<(String, usize)>;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncTrigger {
    /// 앱이 포커스를 얻었을 때 (Incremental)
    Focus,
    /// 주기적인 자동 스캔 (Full/Deep)
    Periodic,
    /// 시스템 유휴 상태 감지 (Deep)
    Idle,
    /// 특정 프로젝트 수동 동기화 (Incremental)
    Manual(String),
    /// 특정 프로젝트 강제 전체 재인덱싱 (Full)
    FullReindex { project_name: String },
    /// 파일 시스템 감시자에 의한 이벤트 (Push)
    FileEvent {
        project_name: String,
        path: std::path::PathBuf,
    },
}

use std::collections::VecDeque;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncStatus {
    pub active_tasks: Vec<ActiveTaskSummary>,
    pub recent_triggers: Vec<SyncTriggerSummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveTaskSummary {
    pub project_name: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncTriggerSummary {
    pub trigger_type: String,
    pub project_name: Option<String>,
    pub details: Option<String>,
    pub timestamp: i64,
}

/// # SyncManager 내부 상태 구조체 (Thread-safe Shared State)
pub struct SyncManagerState {
    pub last_sync_times: HashMap<String, Instant>,
    pub jitter_map: HashMap<String, f64>,
    pub watcher_manager: Option<Arc<crate::watcher::WatcherManager>>,
    pub active_tasks: std::collections::HashMap<String, i64>,
    pub recent_triggers: VecDeque<SyncTriggerSummary>,
    pub event_tx: Option<EventSender>,
    pub progress_callback: Option<ProgressCallback>,
}

/// # SyncManager 동시성 및 락 획득 설계 (Concurrency & Locking Strategy)
///
/// SyncManager는 다양한 비동기 트리거(Focus, Periodic, FileEvent 등)에 반응하여
/// 여러 프로젝트의 인덱싱 및 증분 동기화 작업을 스케줄링하고 조율합니다.
///
/// ## 락 교착 상태(Deadlock) 원천 배제
/// 기존에 개별적으로 분리되어 존재하던 8개의 Mutex 필드들을 하나의 단일 상태 구조체인 `SyncManagerState`로 통합하여,
/// `state: Arc<Mutex<SyncManagerState>>` 단 1개의 Mutex만을 사용하도록 설계했습니다.
/// - 단일 락(state lock) 구조이므로 다중 Mutex 획득으로 인한 교착 상태 발생 가능성이 **근본적으로 차단**됩니다.
///
/// ## 비동기 대기(Await) 중 락 소유 금지 원칙
/// 락을 획득한 상태에서 비동기 I/O나 대기(`.await`)를 수행하면 성능 병목이 발생할 수 있습니다.
/// - `EventSender` (mpsc::Sender) 및 `ProgressCallback` (Arc) 등의 설정 필드들은 락 내부에서 클론(Clone)하여
///   바깥으로 빼낸 뒤, 락을 해제한 상태에서 비동기 전송 및 스폰을 처리합니다.
pub struct SyncManager {
    indexing_service: Arc<IndexingService>,
    tx: mpsc::Sender<SyncTrigger>,
    state: Arc<Mutex<SyncManagerState>>,
}

impl SyncManager {
    pub fn new(indexing_service: Arc<IndexingService>) -> (Self, mpsc::Receiver<SyncTrigger>) {
        let (tx, rx) = mpsc::channel(32);
        let state = SyncManagerState {
            last_sync_times: HashMap::new(),
            jitter_map: HashMap::new(),
            watcher_manager: None,
            active_tasks: std::collections::HashMap::new(),
            recent_triggers: VecDeque::with_capacity(10),
            event_tx: None,
            progress_callback: None,
        };
        (
            Self {
                indexing_service,
                tx,
                state: Arc::new(Mutex::new(state)),
            },
            rx,
        )
    }

    pub fn indexing_service(&self) -> Arc<IndexingService> {
        self.indexing_service.clone()
    }

    pub async fn trigger(&self, trigger: SyncTrigger) {
        let _ = self.tx.send(trigger).await;
    }

    pub async fn record_external_trigger(
        &self,
        t_type: &str,
        project_name: Option<String>,
        details: Option<String>,
    ) {
        let mut state = self.state.lock().await;
        let recent = &mut state.recent_triggers;
        if recent.len() >= 10 {
            recent.pop_back();
        }
        recent.push_front(SyncTriggerSummary {
            trigger_type: t_type.to_string(),
            project_name,
            details,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .unwrap_or_default()
                .as_secs() as i64,
        });
    }

    pub async fn trigger_full_indexing_by_name(
        &self,
        name: &str,
        full: bool,
    ) -> Result<(), String> {
        if full {
            let _ = self
                .tx
                .send(SyncTrigger::FullReindex {
                    project_name: name.to_string(),
                })
                .await;
        } else {
            let _ = self.tx.send(SyncTrigger::Manual(name.to_string())).await;
        }
        Ok(())
    }

    pub async fn mark_task_started(&self, project_name: &str) -> bool {
        let mut state = self.state.lock().await;
        let active = &mut state.active_tasks;
        if active.contains_key(project_name) {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        active.insert(project_name.to_string(), now);
        true
    }

    /// 중복 여부와 관계없이 태스크를 강제 등록. 수동 인덱싱 커맨드에서 사용.
    pub async fn force_mark_task_started(&self, project_name: &str) {
        let mut state = self.state.lock().await;
        let active = &mut state.active_tasks;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        active.insert(project_name.to_string(), now);
    }

    pub async fn mark_task_done(&self, project_name: &str) {
        let mut state = self.state.lock().await;
        state.active_tasks.remove(project_name);
    }

    pub fn indexer(&self) -> Arc<IndexingService> {
        Arc::clone(&self.indexing_service)
    }

    pub async fn set_event_sender(&self, tx: EventSender) {
        let mut state = self.state.lock().await;
        state.event_tx = Some(tx);
    }

    pub async fn set_progress_callback(
        &self,
        cb: impl Fn(String, usize, usize) + Send + Sync + 'static,
    ) {
        let mut state = self.state.lock().await;
        state.progress_callback = Some(Arc::new(cb));
    }

    pub async fn init_watchers(&self) {
        let active_projects = self.get_active_projects().unwrap_or_default();
        for project_name in active_projects {
            crate::log_d!(
                "sync",
                "[SyncManager] Running initial Catch-up scan for {}",
                project_name
            );
            self.run_task(&project_name, false).await;
        }

        let wm = Arc::new(crate::watcher::WatcherManager::new(
            Arc::clone(&self.indexing_service),
            self.tx.clone(),
        ));
        let _ = wm.restart_all().await;
        let mut state = self.state.lock().await;
        state.watcher_manager = Some(wm);
    }

    pub async fn start_loop(self: Arc<Self>, mut rx: mpsc::Receiver<SyncTrigger>) {
        crate::log_d!("sync", "[SyncManager] Background loop started");

        while let Some(trigger) = rx.recv().await {
            crate::log_d!("sync", "[SyncManager] Received trigger: {:?}", trigger);

            // Record trigger to recent list
            {
                let mut state = self.state.lock().await;
                let recent = &mut state.recent_triggers;
                if recent.len() >= 10 {
                    recent.pop_back();
                }

                let (t_type, p_name, details) = match &trigger {
                    SyncTrigger::Focus => (
                        "Focus",
                        None,
                        Some("Window focused - checking projects".to_string()),
                    ),
                    SyncTrigger::Periodic => (
                        "Periodic",
                        None,
                        Some("Scheduled periodic check".to_string()),
                    ),
                    SyncTrigger::Idle => (
                        "Idle",
                        None,
                        Some("System idle - background maintenance".to_string()),
                    ),
                    SyncTrigger::Manual(name) => (
                        "Manual",
                        Some(name.clone()),
                        Some(format!("User requested sync for {}", name)),
                    ),
                    SyncTrigger::FullReindex { project_name } => (
                        "FullReindex",
                        Some(project_name.clone()),
                        Some(format!(
                            "User requested FORCE FULL re-index for {}",
                            project_name
                        )),
                    ),
                    SyncTrigger::FileEvent { project_name, path } => {
                        let filename = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown file");
                        (
                            "FileEvent",
                            Some(project_name.clone()),
                            Some(format!("Changed: {}", filename)),
                        )
                    }
                };

                recent.push_front(SyncTriggerSummary {
                    trigger_type: t_type.to_string(),
                    project_name: p_name,
                    details,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .unwrap_or_default()
                        .as_secs() as i64,
                });
            }

            match trigger {
                SyncTrigger::Manual(project_name) => {
                    self.run_task(&project_name, false).await;
                }
                SyncTrigger::FullReindex { project_name } => {
                    self.run_task(&project_name, true).await;
                }
                SyncTrigger::FileEvent { project_name, .. } => {
                    // 쿨다운 적용: 마지막 동기화 이후 너무 짧은 시간(예: 2초) 내에는 스킵
                    let should_skip = {
                        let state = self.state.lock().await;
                        if let Some(last) = state.last_sync_times.get(&project_name) {
                            last.elapsed() < Duration::from_secs(2)
                        } else {
                            false
                        }
                    };

                    if should_skip {
                        crate::log_d!(
                            "sync",
                            "[SyncManager] Skipping FileEvent for {} (cooldown)",
                            project_name
                        );
                        continue;
                    }

                    self.run_task(&project_name, false).await;
                }
                SyncTrigger::Focus | SyncTrigger::Periodic | SyncTrigger::Idle => {
                    self.run_global_sync(trigger).await;
                }
            }
        }
    }

    async fn run_task(&self, project_name: &str, full: bool) {
        tracing::info!(
            "[SyncManager][run_task] 시작: project={} full={}",
            project_name,
            full
        );
        self.force_mark_task_started(project_name).await;

        let cb = {
            let state = self.state.lock().await;
            state.progress_callback.clone()
        };

        let progress_cb = cb.map(|cb_func| {
            let name = project_name.to_string();
            move |done: usize, total: usize| {
                let cb_clone = cb_func.clone();
                let n = name.clone();
                tokio::spawn(async move {
                    cb_clone(n, done, total);
                });
            }
        });

        let result = if full {
            if let Some(on_progress) = progress_cb {
                self.indexing_service
                    .index_project_with_progress(project_name, true, on_progress)
                    .await
            } else {
                self.indexing_service
                    .index_project(project_name, true)
                    .await
            }
        } else if let Some(on_progress) = progress_cb {
            self.indexing_service
                .index_project_changes(project_name, on_progress)
                .await
        } else {
            self.indexing_service
                .index_project_changes(project_name, |_, _| {})
                .await
        };
        self.update_last_sync(project_name).await;

        match &result {
            Ok(count) => tracing::info!(
                "[SyncManager][run_task] 완료: project={} indexed={}",
                project_name,
                count
            ),
            Err(e) => tracing::info!(
                "[SyncManager][run_task] 실패: project={} err={}",
                project_name,
                e
            ),
        }

        if let Ok(count) = result {
            let tx = {
                let state = self.state.lock().await;
                state.event_tx.clone()
            };
            if let Some(sender) = tx {
                let _ = sender.send((project_name.to_string(), count)).await;
            }
        }

        self.mark_task_done(project_name).await;
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
                crate::log_d!(
                    "sync",
                    "[SyncManager] Syncing project: {} (Trigger: {:?})",
                    project_name,
                    trigger
                );
                self.run_task(&project_name, false).await;
            }
        }
    }

    pub async fn get_status(&self) -> SyncStatus {
        let state = self.state.lock().await;
        SyncStatus {
            active_tasks: state.active_tasks
                .iter()
                .map(|(name, started_at)| ActiveTaskSummary {
                    project_name: name.clone(),
                    started_at: *started_at,
                })
                .collect(),
            recent_triggers: state.recent_triggers.iter().cloned().collect(),
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
                false
            }
            (SyncPolicy::OnFocus, SyncTrigger::Focus) => {
                let last = {
                    let state = self.state.lock().await;
                    state.last_sync_times.get(project_name).cloned()
                };
                if let Some(last_time) = last {
                    let plugin_id = match self.get_project_plugin_id(project_name).await {
                        Ok(id) => id,
                        Err(_) => "com.doxus.obsidian".to_string(),
                    };

                    let is_external = plugin_id.contains("confluence")
                        || plugin_id.contains("github")
                        || !plugin_id.starts_with("com.doxus");
                    let cooldown_secs = if is_external { 15 * 60 } else { 60 };

                    if last_time.elapsed() < Duration::from_secs(cooldown_secs) {
                        crate::log_d!(
                            "sync",
                            "[SyncManager] Skipping Focus trigger for {} (cooldown {}s)",
                            project_name,
                            cooldown_secs
                        );
                        return false;
                    }
                    true
                } else {
                    true
                }
            }
            (SyncPolicy::Interval { seconds }, SyncTrigger::Periodic) => {
                let mut state = self.state.lock().await;
                if let Some(last_time) = state.last_sync_times.get(project_name).cloned() {
                    let jitter = *state.jitter_map
                        .entry(project_name.to_string())
                        .or_insert_with(|| {
                            use rand::Rng;
                            let mut rng = rand::thread_rng();
                            rng.gen_range(-0.1..0.1)
                        });
                    let interval = Duration::from_secs_f64(seconds as f64 * (1.0 + jitter));
                    last_time.elapsed() >= interval
                } else {
                    true
                }
            }
            (SyncPolicy::Manual, _) => false,
            _ => false,
        }
    }

    async fn get_jitter(&self, project_name: &str) -> f64 {
        let mut state = self.state.lock().await;
        *state.jitter_map
            .entry(project_name.to_string())
            .or_insert_with(|| {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                rng.gen_range(-0.1..0.1)
            })
    }

    async fn update_last_sync(&self, project_name: &str) {
        let mut state = self.state.lock().await;
        state.last_sync_times.insert(project_name.to_string(), Instant::now());
        state.jitter_map.remove(project_name);
    }

    async fn get_project_plugin_id(&self, project_name: &str) -> Result<String, String> {
        let conn = self.indexing_service.conn();
        let conn = conn.get().map_err(|e| e.to_string())?;

        let plugin_id: String = conn
            .query_row(
                "SELECT COALESCE(si.plugin_id, p.source_type) 
             FROM projects p 
             LEFT JOIN source_instances si ON p.id = si.project_id 
             WHERE p.name = ?1",
                rusqlite::params![project_name],
                |row: &rusqlite::Row<'_>| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        Ok(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbPool;
    use crate::plugin::PluginManager;
    use crate::search::SearchEngine;

    async fn setup_manager() -> (Arc<SyncManager>, DbPool, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = crate::db::create_pool(&db_path).unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(
            pool.clone(),
            Arc::new(crate::embedding::NoOpEmbedder)
                as Arc<dyn crate::embedding::EmbeddingProvider + Send + Sync>,
        ));
        let indexer = Arc::new(IndexingService::new(pool.clone(), pm, engine));
        let (mgr, _) = SyncManager::new(indexer);
        (Arc::new(mgr), pool, temp_dir)
    }

    #[tokio::test]
    async fn test_should_sync_on_focus() {
        let (mgr, pool, _temp) = setup_manager().await;
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('proj1', 'P1', '', '{\"type\":\"on_focus\"}', 0, 0)",
            []
        ).unwrap();
        drop(conn);

        // 1. Focus trigger -> true
        assert!(mgr.should_sync("proj1", &SyncTrigger::Focus).await);

        // 2. Periodic trigger -> false
        assert!(!mgr.should_sync("proj1", &SyncTrigger::Periodic).await);
    }

    #[tokio::test]
    async fn test_should_sync_interval() {
        let (mgr, pool, _temp) = setup_manager().await;
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('proj1', 'P1', '', '{\"type\":\"interval\",\"seconds\":60}', 0, 0)",
            []
        ).unwrap();
        drop(conn);

        // No record of last sync -> true
        assert!(mgr.should_sync("proj1", &SyncTrigger::Periodic).await);

        mgr.update_last_sync("proj1").await;

        // Just synced -> false
        assert!(!mgr.should_sync("proj1", &SyncTrigger::Periodic).await);
    }

    #[tokio::test]
    async fn test_manual_task_marking() {
        let (mgr, _, _temp) = setup_manager().await;
        let project = "manual_proj";

        // 1. Initial state
        let status = mgr.get_status().await;
        assert!(status.active_tasks.is_empty());

        // 2. Mark started
        assert!(mgr.mark_task_started(project).await);
        let status = mgr.get_status().await;
        assert_eq!(status.active_tasks.len(), 1);
        assert_eq!(status.active_tasks[0].project_name, project);

        // 2-1. Duplicate start should return false
        assert!(!mgr.mark_task_started(project).await);

        // 3. Mark done
        mgr.mark_task_done(project).await;
        let status = mgr.get_status().await;
        assert!(status.active_tasks.is_empty());
    }
}
