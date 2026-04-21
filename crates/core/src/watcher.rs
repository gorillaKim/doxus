use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use notify::{RecursiveMode, RecommendedWatcher};
use notify_debouncer_mini::{new_debouncer, Debouncer, DebounceEventResult};
use tokio::sync::{mpsc, Mutex};
use crate::indexing::IndexingService;
use crate::sync_manager::SyncTrigger;
use doxus_plugin_sdk::SyncPolicy;

pub struct WatcherManager {
    indexing_service: Arc<IndexingService>,
    sync_tx: mpsc::Sender<SyncTrigger>,
    watchers: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
}

impl WatcherManager {
    pub fn new(indexing_service: Arc<IndexingService>, sync_tx: mpsc::Sender<SyncTrigger>) -> Self {
        Self {
            indexing_service,
            sync_tx,
            watchers: Mutex::new(HashMap::new()),
        }
    }

    pub async fn start_watching(&self, project_name: &str) -> Result<(), String> {
        let policy = self.indexing_service.get_project_policy(project_name).await
            .map_err(|e| format!("Failed to get policy: {}", e))?;

        if let SyncPolicy::Realtime(opts) = policy {
            let root_path = PathBuf::from(&opts.root);
            if !root_path.exists() {
                return Err(format!("Watchable root does not exist: {}", opts.root.display()));
            }

            let tx = self.sync_tx.clone();
            let p_name = project_name.to_string();

            let mut debouncer = new_debouncer(Duration::from_millis(500), move |res: DebounceEventResult| {
                match res {
                    Ok(events) => {
                        for event in events {
                            // 무시 필터링 적용
                            if should_ignore(&event.path) {
                                continue;
                            }

                            let trigger = SyncTrigger::FileEvent {
                                project_name: p_name.clone(),
                                path: event.path,
                            };
                            let _ = tx.blocking_send(trigger);
                        }
                    }
                    Err(e) => {
                        crate::log_d!("watcher", "Watcher error: {:?}", e);
                    }
                }
            }).map_err(|e| format!("Failed to create watcher: {}", e))?;

            debouncer.watcher().watch(&root_path, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to start watching: {}", e))?;

            let mut watchers = self.watchers.lock().await;
            watchers.insert(project_name.to_string(), debouncer);
            
            crate::log_d!("watcher", "Started watching project: {} at {}", project_name, opts.root.display());
        }

        Ok(())
    }
    pub async fn stop_watching(&self, project_name: &str) {
        let mut watchers = self.watchers.lock().await;
        if watchers.remove(project_name).is_some() {
            crate::log_d!("watcher", "Stopped watching project: {}", project_name);
        }
    }

    pub async fn restart_all(&self) -> Result<(), String> {
        let projects = self.indexing_service.list_active_projects()
            .map_err(|e| format!("Failed to list active projects: {}", e))?;

        for project_name in projects {
            let _ = self.start_watching(&project_name).await;
        }
        Ok(())
    }
}

/// 특정 파일이나 디렉토리가 와쳐에서 무시되어야 하는지 배정
fn should_ignore(path: &std::path::Path) -> bool {
    let components = path.components();
    for component in components {
        let name = component.as_os_str().to_string_lossy();
        
        // 1. 숨김 디렉토리/파일 무시 (.git, .obsidian, .doxus, .claude 등)
        if name.starts_with('.') && name != "." && name != ".." {
            return true;
        }

        // 2. 가비지 디렉토리 무시
        if name == "node_modules" || name == "target" || name == "dist" {
            return true;
        }
    }

    // 3. DB 파일 무시 (프로젝트 루트에 db가 있는 경우 대비)
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        if file_name.starts_with("doxus.db") {
            return true;
        }
    }

    // 4. 확장자 필터링 (필요시 추가 가능하나 현재는 기본적으로 md 위주로만 수집하도록 IndexingService에서 걸러짐)
    // 여기서는 최소한의 노이즈만 제거

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use crate::plugin::PluginManager;
    use crate::search::SearchEngine;
    use tempfile::TempDir;

    async fn setup_watcher() -> (Arc<WatcherManager>, mpsc::Receiver<SyncTrigger>, TempDir, Arc<std::sync::Mutex<rusqlite::Connection>>) {
        let db = TestDb::new();
        let conn = Arc::new(std::sync::Mutex::new(db.conn));
        let tmp = TempDir::new().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), Arc::new(crate::embedding::NoOpEmbedder) as Arc<dyn crate::embedding::EmbeddingProvider + Send + Sync>));
        let indexer = Arc::new(IndexingService::new(conn.clone(), pm, engine));
        let (tx, rx) = mpsc::channel(32);
        let wm = Arc::new(WatcherManager::new(indexer, tx));
        (wm, rx, tmp, conn)
    }

    #[tokio::test]
    async fn test_watcher_ignores_hidden_files() {
        let (wm, mut rx, tmp, conn) = setup_watcher().await;
        let root = tmp.path().to_string_lossy().to_string();

        conn.lock().unwrap().execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at)
             VALUES ('watch-ignore-test', 'WatchIgnore', '', ?1, 0, 0)",
            [format!("{{\"type\":\"realtime\",\"root\":\"{}\",\"ignore_patterns\":[],\"extensions\":[]}}", root)]
        ).unwrap();

        wm.start_watching("watch-ignore-test").await.unwrap();

        // 1. 숨김 파일 생성 (.obsidian/workspace.json)
        let obsidian_dir = tmp.path().join(".obsidian");
        std::fs::create_dir(&obsidian_dir).unwrap();
        let workspace_json = obsidian_dir.join("workspace.json");
        std::fs::write(&workspace_json, "{}").unwrap();

        // 2. DB 파일 생성 (doxus.db)
        let db_file = tmp.path().join("doxus.db");
        std::fs::write(&db_file, "pure-evil-db-content").unwrap();

        // 3. 정상 파일 생성 (valid.md)
        let valid_md = tmp.path().join("valid.md");
        std::fs::write(&valid_md, "hello").unwrap();

        // 트리거 확인: 숨김/DB 파일에 대해서는 트리거가 발생하지 않고, valid.md에 대해서만 한 번 발생해야 함
        let trigger = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap();
        assert!(trigger.is_some());
        if let Some(SyncTrigger::FileEvent { path, .. }) = trigger {
            assert!(path.to_string_lossy().contains("valid.md"));
            assert!(!path.to_string_lossy().contains(".obsidian"));
            assert!(!path.to_string_lossy().contains("doxus.db"));
        }
    }
}
