use std::sync::{Arc, Mutex};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

use crate::plugin::PluginManager;
use crate::search::{SearchEngine, DocMeta};
use crate::auth::inject_keychain_auth;
use crate::links::{LinkExtractor, LinkResolver};
use doxus_plugin_sdk::{FetchAllOpts, PluginConfig, PluginSecrets, SyncPolicy};

pub struct IndexingService {
    conn: Arc<Mutex<rusqlite::Connection>>,
    plugin_manager: Arc<PluginManager>,
    engine: Arc<SearchEngine>,
}

impl IndexingService {
    pub fn new(
        conn: Arc<Mutex<rusqlite::Connection>>,
        plugin_manager: Arc<PluginManager>,
        engine: Arc<SearchEngine>,
    ) -> Self {
        Self { conn, plugin_manager, engine }
    }

    pub fn conn(&self) -> Arc<Mutex<rusqlite::Connection>> {
        Arc::clone(&self.conn)
    }

    /// 프로젝트의 소스 타입 및 설정을 조회하여 인덱싱을 수행합니다.
    pub async fn index_project(&self, name: &str) -> Result<usize, String> {
        let (project_id, plugin_id, config_json, project_path, strategy, _policy) = self.get_project_config(name).await?;
        
        // 1. 플러그인 초기화
        let mut plugin = self.plugin_manager.get_source(&plugin_id)
            .ok_or_else(|| format!("플러그인을 찾을 수 없습니다: {plugin_id}"))?;

        let mut config_fields = self.parse_config(&config_json);
        let mut secrets = PluginSecrets::default();

        if !project_path.is_empty() {
            config_fields.insert("path".to_string(), serde_json::Value::String(project_path.clone()));
        }

        inject_keychain_auth(&plugin_id, &mut PluginConfig { fields: config_fields.clone() }, &mut secrets).await;
        
        let mut final_config = PluginConfig { fields: config_fields };
        inject_keychain_auth(&plugin_id, &mut final_config, &mut secrets).await;

        plugin.initialize(final_config, secrets).await
            .map_err(|e| format!("플러그인 초기화 실패: {e}"))?;

        // 2. 인덱싱 루프
        let mut total = 0;
        let mut cursor = None;
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            let stream = plugin.fetch_all(FetchAllOpts { cursor, page_size: 50 }).await
                .map_err(|e| format!("문서 수집 실패: {e}"))?;

            let docs = stream.documents;
            crate::log_d!("indexer", "[Core-Indexer] Received {} documents from plugin", docs.len());
            if docs.is_empty() { break; }

            // 기존 문서 메타데이터 조회 (업데이트 시간 비교용)
            let existing_meta = self.get_existing_metadata(project_id).await?;

            for doc in docs {
                let source_doc_id = doc.id.0.clone();
                seen_ids.insert(source_doc_id.clone());

                let title = doc.title.as_deref()
                    .map(|s| s.to_string())
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        // Attempt to extract title from source_doc_id or relative path
                        doc.relative_path.as_deref()
                            .or(Some(&doc.id.0))
                            .and_then(|p| {
                                p.split('/').last()
                                    .map(|s| s.strip_suffix(".md").unwrap_or(s).to_string())
                            })
                    })
                    .unwrap_or_else(|| "Untitled".to_string());

                // 업데이트 시간 및 제목 상태 비교를 통한 스킵 로직
                if let Some((old_ts, old_title)) = existing_meta.get(&source_doc_id) {
                    let needs_repair = old_title == "Untitled" || old_title.is_empty();

                    if doc.updated_at == Some(*old_ts) && !needs_repair {
                        crate::log_d!("indexer", "[Core-Indexer] Skipping unchanged document: {} (ID: {})", title, source_doc_id);
                        continue;
                    }

                    if needs_repair {
                        crate::log_d!("indexer", "[Core-Indexer] Healing 'Untitled' document: {} (ID: {})", title, source_doc_id);
                    }
                }

                if let Err(e) = self.index_single_document(project_id, doc, &strategy).await {
                    crate::log_d!("indexer", "[Core-Indexer] Error indexing {}: {}", source_doc_id, e);
                } else {
                    total += 1;
                }
            }

            cursor = stream.next_cursor;
            if cursor.is_none() { break; }
        }

        // 3. 소스에서 사라진 문서 제거
        let removed = self.remove_deleted_documents(project_id, &seen_ids).await?;
        if removed > 0 {
            crate::log_d!("indexer", "[Core-Indexer] Removed {} deleted documents from index", removed);
        }

        // 4. 링크 해결 수행 (target_raw -> target_id)
        {
            let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
            if let Err(e) = LinkResolver::resolve_all_unresolved_links(&conn) {
                crate::log_d!("indexer", "[Core-Indexer] Link resolution error: {}", e);
            }
        }

        crate::log_d!("indexer", "[Core-Indexer] Cycle finished. Total indexed this run: {}", total);
        Ok(total)
    }

    /// 단일 문서를 인덱싱합니다. (청킹, 임베딩, DB 저장 포함)
    pub async fn index_single_document(&self, project_id: i64, doc: doxus_plugin_sdk::RawDocument, strategy: &str) -> Result<(), String> {
        let title = doc.title.as_deref().unwrap_or("Untitled");
        crate::log_d!("indexer", "[Core-Indexer] Processing document: {} (ID: {})", title, doc.id.0);

        // 내용에서 링크 추출 및 플러그인 제공 링크와 병합
        let mut all_links = LinkExtractor::extract_links(&doc.content);
        all_links.extend(doc.links.clone());
        all_links.sort();
        all_links.dedup();

        let meta = DocMeta {
            url: doc.url.clone(),
            tags: doc.tags.clone(),
            metadata: doc.metadata.clone(),
            created_at: doc.created_at,
            updated_at: doc.updated_at,
            relative_path: doc.relative_path.clone(),
            links: all_links,
            ..Default::default()
        };

        self.engine.index_document_async_with_meta(
            project_id,
            &doc.id.0,
            title,
            &doc.content,
            meta,
            strategy
        ).await.map_err(|e| format!("Indexing error: {e}"))
    }

    /// 특정 문서가 재인덱싱이 필요한지 확인합니다.
    pub async fn needs_reindexing(&self, project_id: i64, source_doc_id: &str, new_updated_at: Option<i64>) -> bool {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return true,
        };

        let (old_updated_at, last_indexed, chunk_count, current_title): (Option<i64>, Option<i64>, i64, String) = conn.query_row(
            "SELECT d.updated_at, d.last_indexed, COUNT(c.id), d.title
             FROM documents d 
             LEFT JOIN chunks c ON d.id = c.document_id
             WHERE d.project_id = ?1 AND d.source_doc_id = ?2
             GROUP BY d.id",
            params![project_id, source_doc_id],
            |r| Ok((
                r.get(0).ok().flatten(), 
                r.get(1).ok().flatten(), 
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3).unwrap_or_default()
            ))
        ).unwrap_or((None, None, 0, String::new()));

        // 1. 인덱싱된 기록이 아예 없거나 청크가 0개인 경우
        if last_indexed.is_none() || chunk_count == 0 {
            return true;
        }

        // 2. 제목이 'Untitled'인 경우 강제 재인덱싱 (데이터 복구용)
        if current_title == "Untitled" {
            return true;
        }

        // 3. 타임스탬프 비교
        match (new_updated_at, old_updated_at) {
            (Some(new), Some(old)) => new != old,
            (None, _) => true, // 새로운 타임스탬프 정보가 없다면 안전하게 재인덱싱
            (_, None) => true,
        }
    }

    pub async fn get_project_policy(&self, name: &str) -> Result<SyncPolicy, String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        let policy_json: Option<String> = conn.query_row(
            "SELECT sync_policy_json FROM projects WHERE name = ?1",
            params![name],
            |r| r.get(0)
        ).map_err(|e| format!("정책 조회 실패: {e}"))?;

        match policy_json {
            Some(json) => serde_json::from_str(&json).map_err(|e| format!("정책 파싱 실패: {e}")),
            None => Ok(SyncPolicy::Interval { seconds: 7200 }), // Default to 2h
        }
    }

    async fn get_project_config(&self, name: &str) -> Result<(i64, String, String, String, String, SyncPolicy), String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        let row = conn.query_row(
            "SELECT p.id, si.plugin_id, si.config_json, p.path, p.storage_strategy, p.sync_policy_json
             FROM projects p
             JOIN source_instances si ON p.id = si.project_id
             WHERE p.name = ?1
             LIMIT 1",
            params![name],
            |r| {
                let pid: i64 = r.get(0)?;
                let plugin_id: String = r.get(1)?;
                let config_json: String = r.get(2)?;
                let path: String = r.get(3)?;
                let strategy: String = r.get(4)?;
                let policy_json: Option<String> = r.get(5)?;
                let policy = policy_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(SyncPolicy::Interval { seconds: 7200 });
                Ok((pid, plugin_id, config_json, path, strategy, policy))
            }
        );

        if let Ok(r) = row {
            return Ok(r);
        }

        conn.query_row(
            "SELECT id, COALESCE(source_type, 'obsidian'), COALESCE(config_json, '{}'), path, storage_strategy, sync_policy_json
             FROM projects WHERE name = ?1",
            params![name],
            |r| {
                let pid: i64 = r.get(0)?;
                let stype: String = r.get(1)?;
                let cjson: String = r.get(2)?;
                let ppath: String = r.get(3)?;
                let strategy: String = r.get(4)?;
                let policy_json: Option<String> = r.get(5)?;
                let plugin_id = if stype == "obsidian" || stype == "confluence" || stype == "github" {
                    format!("com.doxus.{stype}")
                } else {
                    stype
                };
                let policy = policy_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(SyncPolicy::Interval { seconds: 7200 });
                Ok((pid, plugin_id, cjson, ppath, strategy, policy))
            }
        ).map_err(|e| format!("프로젝트 설정을 찾을 수 없습니다: {e}"))
    }

    fn parse_config(&self, json_str: &str) -> HashMap<String, Value> {
        let fields: HashMap<String, Value> = serde_json::from_str(json_str).unwrap_or_default();
        if let Some(inner) = fields.get("fields").and_then(|v| v.as_object()) {
            return inner.clone().into_iter().collect();
        }
        fields
    }

    pub fn list_active_projects(&self) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        let mut stmt = conn.prepare("SELECT name FROM projects WHERE status = 'active'")
            .map_err(|e| e.to_string())?;
        
        let projects = stmt.query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        
        Ok(projects)
    }

    /// 소스에서 사라진 문서를 DB에서 제거합니다. chunks/FTS/벡터는 CASCADE 트리거로 자동 정리됩니다.
    async fn remove_deleted_documents(
        &self,
        project_id: i64,
        seen_ids: &std::collections::HashSet<String>,
    ) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        let mut stmt = conn.prepare(
            "SELECT source_doc_id FROM documents WHERE project_id = ?1"
        ).map_err(|e| e.to_string())?;

        let db_ids: Vec<String> = stmt
            .query_map(params![project_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        let mut removed = 0;
        for id in db_ids {
            if !seen_ids.contains(&id) {
                crate::log_d!("indexer", "[Core-Indexer] Removing deleted document from index: {}", id);
                conn.execute(
                    "DELETE FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
                    params![project_id, id],
                ).map_err(|e| e.to_string())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    async fn get_existing_metadata(&self, project_id: i64) -> Result<HashMap<String, (i64, String)>, String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        let mut stmt = conn.prepare("SELECT source_doc_id, updated_at, title FROM documents WHERE project_id = ?1")
            .map_err(|e| e.to_string())?;
        
        let meta_iter = stmt.query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?.unwrap_or_default()))
        }).map_err(|e| e.to_string())?;

        let mut meta_map = HashMap::new();
        for item in meta_iter {
            if let Ok((id, updated_at, title)) = item {
                meta_map.insert(id, (updated_at, title));
            }
        }
        Ok(meta_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_needs_reindexing_logic() {
        let db = TestDb::new();
        let conn = Arc::new(std::sync::Mutex::new(db.conn));
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp/plugins")));
        let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(conn.clone(), pm, engine);

        // 테스트용 프로젝트 삽입
        conn.lock().unwrap().execute(
            "INSERT INTO projects (id, name, display_name, path, created_at, updated_at) \
             VALUES (1, 'test', 'Test', '/tmp', 0, 0)",
            [],
        ).unwrap();

        // 테스트용 문서 삽입 (updated_at = 100)
        conn.lock().unwrap().execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content, content_hash, updated_at) \
             VALUES (1, 'doc1', 'Doc1', 'content', 'hash', 100)",
            [],
        ).unwrap();

        // 1. 타임스탬프 동일한 경우 -> false
        assert!(!indexer.needs_reindexing(1, "doc1", Some(100)).await);

        // 2. 타임스탬프 다른 경우 -> true
        assert!(indexer.needs_reindexing(1, "doc1", Some(200)).await);

        // 3. 타임스탬프 없는 경우 -> true (안전하게 재인덱싱)
        assert!(indexer.needs_reindexing(1, "doc1", None).await);

        // 4. 새로운 문서인 경우 -> true
        assert!(indexer.needs_reindexing(1, "new_doc", Some(100)).await);
    }

    #[tokio::test]
    async fn test_get_project_policy() {
        let db = TestDb::new();
        let conn = Arc::new(std::sync::Mutex::new(db.conn));
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(conn.clone(), pm, engine);

        conn.lock().unwrap().execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at) \
             VALUES ('test-policy', 'Test', '/tmp', '{\"type\":\"on_focus\"}', 0, 0)",
            [],
        ).unwrap();

        let policy = indexer.get_project_policy("test-policy").await.unwrap();
        assert!(matches!(policy, SyncPolicy::OnFocus));

        let default_policy = indexer.get_project_policy("non-existent").await;
        assert!(default_policy.is_err());
    }
}
