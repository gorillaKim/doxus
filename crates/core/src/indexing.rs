use std::sync::{Arc, Mutex};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

use crate::plugin::PluginManager;
use crate::search::{SearchEngine, DocMeta};
use crate::auth::inject_keychain_auth;
use crate::links::{LinkExtractor, LinkResolver};
use doxus_plugin_sdk::{FetchAllOpts, PluginConfig, PluginSecrets, SyncPolicy};
use crate::observability::{persist_audit, AuditEvent};

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
    pub async fn index_project(&self, name: &str, full: bool) -> Result<usize, String> {
        let (project_id, plugin_id, config_json, project_path, strategy, _policy) = self.get_project_config(name).await?;
        
        // 1. 플러그인 초기화
        let mut plugin = self.plugin_manager.get_source(&plugin_id)
            .ok_or_else(|| {
                let msg = format!("플러그인을 찾을 수 없습니다: {plugin_id}");
                if let Ok(conn) = self.conn.lock() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg.clone(),
                    });
                }
                msg
            })?;

        let mut config_fields = self.parse_config(&config_json);
        let mut secrets = PluginSecrets::default();

        if !project_path.is_empty() {
            config_fields.insert("path".to_string(), serde_json::Value::String(project_path.clone()));
        }

        inject_keychain_auth(&plugin_id, &mut PluginConfig { fields: config_fields.clone() }, &mut secrets).await;
        
        let mut final_config = PluginConfig { fields: config_fields };
        inject_keychain_auth(&plugin_id, &mut final_config, &mut secrets).await;

        plugin.initialize(final_config, secrets).await
            .map_err(|e| {
                let msg = format!("플러그인 초기화 실패: {e}");
                if let Ok(conn) = self.conn.lock() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg.clone(),
                    });
                }
                msg
            })?;

        // 1. 인덱싱 시작 로그
        {
            if let Ok(conn) = self.conn.lock() {
                persist_audit(&conn, &AuditEvent::IndexStart { project_id });
            }
        }

        // 2. 인덱싱 루프 및 오류 처리
        let mut total = 0;
        let mut cursor = None;
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        let result = async {
            loop {
                let stream = plugin.fetch_all(FetchAllOpts { cursor, page_size: 50 }).await.map_err(|e| e.to_string())?;
                let docs = stream.documents;
                if docs.is_empty() { break; }

                for doc in docs {
                    let source_doc_id = doc.id.0.clone();
                    seen_ids.insert(source_doc_id.clone());

                    let title = doc.title.as_deref()
                        .map(|s| s.to_string())
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| {
                            doc.relative_path.as_deref()
                                .or(Some(&doc.id.0))
                                .and_then(|p| {
                                    p.split('/').last()
                                        .map(|s| s.strip_suffix(".md").unwrap_or(s).to_string())
                                })
                        })
                        .unwrap_or_else(|| "Untitled".to_string());

                    // [최적화] 대규모 프로젝트에서 메모리 보호를 위해 HashMap 대신 
                    // DB에서 개별적으로 재인덱싱 필요성을 확인합니다.
                    if !full && !self.needs_reindexing(project_id, &source_doc_id, doc.updated_at).await {
                        continue;
                    }

                    // Content is empty (optimization from local plugins like Obsidian)
                    // Fetch full document content only when we actually decide to index it
                    let mut final_doc = doc;
                    if final_doc.content.is_empty() {
                        match plugin.fetch_document(&final_doc.id).await {
                            Ok(full_doc) => { final_doc = full_doc; }
                            Err(e) => {
                                crate::log_d!("indexer", "[Core-Indexer] Failed to fetch full content for {}: {}", source_doc_id, e);
                                continue;
                            }
                        }
                    }

                    match self.index_single_document(project_id, final_doc, &strategy).await {
                        Ok(_) => { total += 1; }
                        Err(e) => {
                            crate::log_d!("indexer", "[Core-Indexer] Error indexing '{}' ({}): {}", title, source_doc_id, e);
                            if let Ok(conn) = self.conn.lock() {
                                persist_audit(&conn, &AuditEvent::PluginError {
                                    plugin_id: plugin_id.clone(),
                                    message: format!("문서 '{}' ({}) 인덱싱 실패: {}", title, source_doc_id, e),
                                });
                            }
                        }
                    }
                }

                cursor = stream.next_cursor;
                if cursor.is_none() { break; }
            }
            Ok::<(), String>(())
        }.await;

        // 3. 뒷정리 및 링크 해결
        let _ = self.remove_deleted_documents(project_id, &seen_ids).await;
        {
            if let Ok(conn) = self.conn.lock() {
                let _ = LinkResolver::resolve_project_links(&conn, project_id);
                // 인덱싱 종료 로그 기록 (에러가 있었더라도 지금까지 처리된 개수 기록)
                persist_audit(&conn, &AuditEvent::IndexComplete { project_id, docs_indexed: total });
            }
        }

        match result {
            Ok(_) => Ok(total),
            Err(e) => Err(e),
        }
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

        // 테스트용 문서 삽입 (updated_at = 100, last_indexed = 100)
        conn.lock().unwrap().execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, updated_at, last_indexed) \
             VALUES (1, 'doc1', 'Doc1', 'hash', 100, 100)",
            [],
        ).unwrap();

        // chunk_count > 0 이어야 needs_reindexing이 타임스탬프 비교로 진행됨
        conn.lock().unwrap().execute(
            "INSERT INTO chunks (document_id, content, chunk_index) \
             SELECT id, 'content', 0 FROM documents WHERE source_doc_id = 'doc1'",
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

    #[tokio::test]
    async fn test_index_project_audits_on_plugin_not_found() {
        let db = TestDb::new();
        let conn = Arc::new(std::sync::Mutex::new(db.conn));
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(conn.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(conn.clone(), pm, engine);

        // 프로젝트 삽입 (source_type = 'non_existent')
        conn.lock().unwrap().execute(
            "INSERT INTO projects (name, display_name, path, source_type, created_at, updated_at) \
             VALUES ('my-proj', 'My Proj', '/tmp', 'non_existent', 0, 0)",
            [],
        ).unwrap();

        // 실행 (플러그인을 찾을 수 없으므로 에러 발생 예상)
        let result = indexer.index_project("my-proj", false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("플러그인을 찾을 수 없습니다"));

        // audit_log 확인
        let (event_type, message): (String, String) = conn.lock().unwrap().query_row(
            "SELECT event_type, payload FROM audit_log",
            [],
            |r| {
                let event_type: String = r.get(0)?;
                let payload: String = r.get(1)?;
                let event: AuditEvent = serde_json::from_str(&payload).unwrap();
                let msg = match event {
                    AuditEvent::PluginError { message, .. } => message,
                    _ => "wrong event".to_string(),
                };
                Ok((event_type, msg))
            }
        ).unwrap();

        assert_eq!(event_type, "plugin_error");
        assert!(message.contains("플러그인을 찾을 수 없습니다"));
    }
}
