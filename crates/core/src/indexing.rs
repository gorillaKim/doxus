use std::sync::Arc;
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

use crate::plugin::PluginManager;
use crate::search::{SearchEngine, DocMeta};
use crate::auth::inject_keychain_auth;
use crate::links::{LinkExtractor, LinkResolver};
use doxus_plugin_sdk::{FetchAllOpts, FetchChangesOpts, PluginConfig, PluginSecrets, SyncPolicy};
use crate::observability::{persist_audit, AuditEvent};
use crate::db::DbPool;

pub struct IndexingService {
    conn: DbPool,
    plugin_manager: Arc<PluginManager>,
    engine: Arc<SearchEngine>,
}

impl IndexingService {
    pub fn new(
        conn: DbPool,
        plugin_manager: Arc<PluginManager>,
        engine: Arc<SearchEngine>,
    ) -> Self {
        Self { conn, plugin_manager, engine }
    }

    pub fn conn(&self) -> DbPool {
        self.conn.clone()
    }

    /// 프로젝트의 소스 타입 및 설정을 조회하여 인덱싱을 수행합니다.
    pub async fn index_project(&self, name: &str, full: bool) -> Result<usize, String> {
        self.index_project_with_progress(name, full, |_, _| {}).await
    }

    /// Like `index_project` but calls `on_progress(docs_done, total_docs)` after each batch.
    pub async fn index_project_with_progress(
        &self,
        name: &str,
        full: bool,
        on_progress: impl Fn(usize, usize) + Send,
    ) -> Result<usize, String> {
        let (project_id, plugin_id, config_json, project_path, _strategy, _policy) = self.get_project_config(name).await?;
        
        // 1. 플러그인 초기화
        let mut plugin = self.plugin_manager.get_source(&plugin_id)
            .ok_or_else(|| {
                let msg = format!("플러그인을 찾을 수 없습니다: {plugin_id}");
                if let Ok(conn) = self.conn.write_conn() {
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

        let mut final_config = PluginConfig { fields: config_fields };
        inject_keychain_auth(&plugin_id, &mut final_config, &mut secrets).await;

        plugin.initialize(final_config, secrets).await
            .map_err(|e| {
                let msg = format!("플러그인 초기화 실패: {e}");
                if let Ok(conn) = self.conn.write_conn() {
                    persist_audit(&conn, &AuditEvent::PluginError {
                        plugin_id: plugin_id.clone(),
                        message: msg.clone(),
                    });
                }
                msg
            })?;

        // 1. 인덱싱 시작 로그
        {
            if let Ok(conn) = self.conn.write_conn() {
                persist_audit(&conn, &AuditEvent::IndexStart { project_id });
            }
        }

        // 2. 인덱싱 루프 및 오류 처리
        let sync_start_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let mut total = 0;
        let mut estimated_total: usize = 0;
        let mut cursor = None;
        let mut consecutive_empty = 0;

        let result = async {
            loop {
                crate::log_d!("indexer", "[Core-Indexer][fetch_all] 페이지 요청: cursor={:?}", cursor);
                let stream = plugin.fetch_all(FetchAllOpts { cursor, page_size: 50 }).await.map_err(|e| e.to_string())?;
                crate::log_d!("indexer", "[Core-Indexer][fetch_all] 응답: docs={}, next_cursor={:?}, estimated_total={:?}",
                    stream.documents.len(), stream.next_cursor, stream.estimated_total);
                if estimated_total == 0 {
                    if let Some(hint) = stream.estimated_total {
                        estimated_total = hint as usize;
                    }
                }
                let docs = stream.documents;
                // 빈 페이지: cursor 있으면 계속, 연속 5회면 API 버그로 간주하고 종료
                if docs.is_empty() {
                    consecutive_empty += 1;
                    if consecutive_empty >= 5 {
                        crate::log_d!("indexer", "[Core-Indexer] 연속 {}회 빈 페이지 — 루프 종료", consecutive_empty);
                        break;
                    }
                    cursor = stream.next_cursor;
                    if cursor.is_none() { break; }
                    continue;
                }
                consecutive_empty = 0;

                let mut batch_requests = Vec::new();
                for mut doc in docs {
                    let source_doc_id = doc.id.0.clone();

                    let content_hash_for_check = if !doc.content.is_empty() {
                        use sha2::{Sha256, Digest};
                        Some(format!("{:x}", Sha256::digest(doc.content.as_bytes())))
                    } else {
                        None
                    };
                    if !full && !self.needs_reindexing_with_hash(project_id, &source_doc_id, doc.updated_at, content_hash_for_check.as_deref()).await {
                        crate::log_d!("indexer", "[Core-Indexer][skip] source_doc_id={} updated_at={:?}", source_doc_id, doc.updated_at);
                        let _ = self.update_last_indexed(project_id, &source_doc_id, sync_start_time).await;
                        continue;
                    }
                    crate::log_d!("indexer", "[Core-Indexer][index] 재인덱싱 시작: source_doc_id={}", source_doc_id);
                    crate::log_d!("indexer", "[Core-Indexer][reindex] source_doc_id={} updated_at={:?}", source_doc_id, doc.updated_at);

                    if doc.content.is_empty() {
                        match plugin.fetch_document(&doc.id).await {
                            Ok(full_doc) => { doc = full_doc; }
                            Err(e) => {
                                crate::log_d!("indexer", "[Core-Indexer] Failed to fetch full content for {}: {}", source_doc_id, e);
                                continue;
                            }
                        }
                    }

                    let title = doc.title.as_deref()
                        .map(|s| s.to_string())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "Untitled".to_string());

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

                    batch_requests.push(crate::search::BatchIndexingRequest {
                        project_id,
                        source_doc_id,
                        title,
                        content: doc.content,
                        meta,
                    });
                }

                if !batch_requests.is_empty() {
                    let count = batch_requests.len();
                    match self.engine.index_documents_batch_async(batch_requests, sync_start_time).await {
                        Ok(_) => {
                            total += count;
                            on_progress(total, estimated_total);
                        }
                        Err(e) => {
                            crate::log_d!("indexer", "[Core-Indexer] Batch indexing error: {}", e);
                        }
                    }
                }

                cursor = stream.next_cursor;
                if cursor.is_none() { break; }
            }
            Ok::<(), String>(())
        }.await;

        // 3. 뒷정리: fetch가 완전히 성공한 경우에만 삭제 및 링크 해결
        if result.is_ok() {
            let _ = self.remove_deleted_documents(project_id, sync_start_time).await;
            if let Ok(conn) = self.conn.write_conn() {
                let _ = LinkResolver::resolve_project_links(&conn, project_id);
            }
        }
        // 인덱싱 종료 로그 및 DB 메모리 회수 (성공/실패와 무관하게 항상 실행)
        {
            if let Ok(conn) = self.conn.write_conn() {
                persist_audit(&conn, &AuditEvent::IndexComplete { project_id, docs_indexed: total });
                let _ = conn.execute("PRAGMA shrink_memory", []);
                let _ = conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []);
            }
        }

        match result {
            Ok(_) => Ok(total),
            Err(e) => Err(e),
        }
    }

    /// 증분 인덱싱: last_fetched_at 기준으로 fetch_changes를 호출해 변경 문서만 처리합니다.
    /// last_fetched_at이 없거나 플러그인이 incremental_sync를 미지원하면 fetch_all 방식으로 폴백합니다.
    pub async fn index_project_changes(
        &self,
        name: &str,
        on_progress: impl Fn(usize, usize) + Send,
    ) -> Result<usize, String> {
        let last_fetched_at: Option<i64> = {
            let conn = self.conn.read_conn().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT last_fetched_at FROM projects WHERE name = ?1",
                params![name],
                |r| r.get(0),
            ).ok().flatten()
        };

        let (project_id, plugin_id, config_json, project_path, _strategy, _policy) = self.get_project_config(name).await?;

        let mut plugin = self.plugin_manager.get_source(&plugin_id)
            .ok_or_else(|| format!("플러그인을 찾을 수 없습니다: {plugin_id}"))?;

        let mut config_fields = self.parse_config(&config_json);
        let mut secrets = PluginSecrets::default();
        if !project_path.is_empty() {
            config_fields.insert("path".to_string(), serde_json::Value::String(project_path));
        }
        let mut final_config = PluginConfig { fields: config_fields };
        inject_keychain_auth(&plugin_id, &mut final_config, &mut secrets).await;

        plugin.initialize(final_config, secrets).await
            .map_err(|e| format!("플러그인 초기화 실패: {e}"))?;

        if !plugin.capabilities().incremental_sync || last_fetched_at.is_none() {
            return self.index_project_with_progress(name, false, on_progress).await;
        }

        let since = last_fetched_at.unwrap();
        crate::log_d!("indexer", "[Core-Indexer] 증분 인덱싱 시작: {} (since={})", name, since);

        let mut total = 0usize;
        let mut cursor: Option<String> = None;

        loop {
            let opts = FetchChangesOpts { since, cursor: cursor.clone(), page_size: 100, known_ids: vec![] };
            let changeset = match plugin.fetch_changes(opts).await {
                Ok(cs) => cs,
                Err(e) => {
                    crate::log_d!("indexer", "[Core-Indexer] fetch_changes 오류, fetch_all로 폴백: {}", e);
                    return self.index_project_with_progress(name, false, on_progress).await;
                }
            };

            if !changeset.updated.is_empty() {
                let mut batch_requests = Vec::new();
                for doc in changeset.updated {
                    let title = doc.title.as_deref()
                        .map(|s| s.to_string())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "Untitled".to_string());
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
                    batch_requests.push(crate::search::BatchIndexingRequest {
                        project_id,
                        source_doc_id: doc.id.0,
                        title,
                        content: doc.content,
                        meta,
                    });
                }
                if !batch_requests.is_empty() {
                    let count = batch_requests.len();
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                    match self.engine.index_documents_batch_async(batch_requests, now).await {
                        Ok(_) => {
                            total += count;
                            on_progress(total, 0);
                        }
                        Err(e) => {
                            crate::log_d!("indexer", "[Core-Indexer] Incremental batch indexing error: {}", e);
                        }
                    }
                }
            }

            cursor = changeset.next_cursor;
            if cursor.is_none() { break; }
        }

        // last_fetched_at 갱신 및 DB 메모리 회수
        if let Ok(conn) = self.conn.write_conn() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let _ = conn.execute(
                "UPDATE projects SET last_fetched_at = ?1 WHERE name = ?2",
                params![now, name],
            );
            let _ = conn.execute("PRAGMA shrink_memory", []);
            let _ = conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []);
        }

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

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        self.engine.index_document_async_with_meta(crate::search::IndexParams {
            project_id,
            source_doc_id: &doc.id.0,
            title,
            content: &doc.content,
            meta,
            strategy,
            last_indexed: now,
        }).await.map_err(|e| format!("Indexing error: {e}"))?;

        // LTM 요약 생성 및 documents 테이블 UPDATE 연동
        let summary = crate::summarizer::lead3_extract(&doc.content);
        if let Ok(conn) = self.conn.write_conn() {
            let _ = conn.execute(
                "UPDATE documents SET summary = ?1 WHERE project_id = ?2 AND source_doc_id = ?3",
                params![summary, project_id, doc.id.0],
            );
        }

        Ok(())
    }

    /// 특정 문서가 재인덱싱이 필요한지 확인합니다.
    pub async fn needs_reindexing(&self, project_id: i64, source_doc_id: &str, new_updated_at: Option<i64>) -> bool {
        self.needs_reindexing_with_hash(project_id, source_doc_id, new_updated_at, None).await
    }

    pub async fn needs_reindexing_with_hash(
        &self,
        project_id: i64,
        source_doc_id: &str,
        new_updated_at: Option<i64>,
        new_content_hash: Option<&str>,
    ) -> bool {
        let conn = match self.conn.read_conn() {
            Ok(c) => c,
            Err(_) => return true,
        };

        let (old_updated_at, last_indexed, _chunk_count, old_content_hash): (Option<i64>, Option<i64>, i64, Option<String>) = conn.query_row(
            "SELECT d.updated_at, d.last_indexed, COUNT(c.id), d.content_hash
             FROM documents d
             LEFT JOIN chunks c ON d.id = c.document_id
             WHERE d.project_id = ?1 AND d.source_doc_id = ?2
             GROUP BY d.id",
            params![project_id, source_doc_id],
            |r| Ok((
                r.get(0).ok().flatten(),
                r.get(1).ok().flatten(),
                r.get::<_, i64>(2)?,
                r.get(3).ok().flatten(),
            ))
        ).unwrap_or((None, None, 0, None));

        // 1. 인덱싱된 기록이 아예 없는 경우
        if last_indexed.is_none() {
            crate::log_d!("indexer", "[Core-Indexer][decision] {} -> 재인덱싱 (기록 없음)", source_doc_id);
            return true;
        }

        // 2. 타임스탬프 비교 — None이면 content_hash로 fallback
        match (new_updated_at, old_updated_at) {
            (Some(new), Some(old)) => {
                if new != old {
                    crate::log_d!("indexer", "[Core-Indexer][decision] {} -> 재인덱싱 (시간 다름)", source_doc_id);
                    true
                } else {
                    false
                }
            }
            _ => {
                // 타임스탬프 정보 없음 → content_hash로 변경 감지
                match (new_content_hash, old_content_hash.as_deref()) {
                    (Some(new_h), Some(old_h)) => {
                        if new_h != old_h {
                            crate::log_d!("indexer", "[Core-Indexer][decision] {} -> 재인덱싱 (해시 다름)", source_doc_id);
                            true
                        } else {
                            false
                        }
                    }
                    _ => {
                        crate::log_d!("indexer", "[Core-Indexer][decision] {} -> 재인덱싱 (정보 부족: updated_at={:?}, hash={:?})", 
                            source_doc_id, old_updated_at, old_content_hash);
                        true
                    }
                }
            }
        }
    }

    pub async fn get_project_policy(&self, name: &str) -> Result<SyncPolicy, String> {
        let conn = self.conn.read_conn().map_err(|e| e.to_string())?;
        let policy_json: Option<String> = conn.query_row(
            "SELECT sync_policy_json FROM projects WHERE name = ?1",
            params![name],
            |r| r.get(0)
        ).map_err(|e| format!("정책 조회 실패: {e}"))?;

        match policy_json {
            Some(json) => serde_json::from_str::<SyncPolicy>(&json).map_err(|e| format!("정책 파싱 실패: {e}")),
            None => Ok(SyncPolicy::Interval { seconds: 7200 }), // Default to 2h
        }
    }

    async fn get_project_config(&self, name: &str) -> Result<(i64, String, String, String, String, SyncPolicy), String> {
        let conn = self.conn.read_conn().map_err(|e| e.to_string())?;
        let row = conn.query_row(
            "SELECT p.id, si.plugin_id, si.config_json, p.path, p.storage_strategy, p.sync_policy_json
             FROM projects p
             JOIN source_instances si ON p.id = si.project_id
             WHERE p.name = ?1
             LIMIT 1",
            params![name],
            |r: &rusqlite::Row<'_>| {
                let pid: i64 = r.get(0)?;
                let plugin_id: String = r.get(1)?;
                let config_json: String = r.get(2)?;
                let path: String = r.get(3)?;
                let strategy: String = r.get(4)?;
                let policy_json: Option<String> = r.get(5)?;
                let policy = policy_json
                    .and_then(|s| serde_json::from_str::<SyncPolicy>(&s).ok())
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
            |r: &rusqlite::Row<'_>| {
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
                    .and_then(|s| serde_json::from_str::<SyncPolicy>(&s).ok())
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
        let conn = self.conn.read_conn().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT name FROM projects WHERE status = 'active'")
            .map_err(|e| e.to_string())?;
        
        let projects = stmt.query_map([], |row: &rusqlite::Row<'_>| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r: Result<String, rusqlite::Error>| r.ok())
            .collect();
        
        Ok(projects)
    }

    /// 소스에서 사라진 문서를 DB에서 제거합니다. chunks/FTS/벡터는 CASCADE 트리거로 자동 정리됩니다.
    /// 이번 동기화 세션에서 발견되지 않은(삭제된) 문서를 인덱스에서 제거합니다.
    async fn remove_deleted_documents(
        &self,
        project_id: i64,
        sync_start_time: i64,
    ) -> Result<usize, String> {
        let conn = self.conn.write_conn().map_err(|e| e.to_string())?;

        // sync_start_time보다 이전에 인델싱된 문서는 이번 세션에서 발견되지 않은 것이므로 삭제합니다.
        let removed = conn.execute(
            "DELETE FROM documents WHERE project_id = ?1 AND (last_indexed < ?2 OR last_indexed IS NULL)",
            params![project_id, sync_start_time],
        ).map_err(|e| format!("삭제된 문서 정리 실패: {e}"))?;

        if removed > 0 {
            crate::log_d!("indexer", "[Core-Indexer] Cleaned up {} deleted documents from project {}", removed, project_id);
        }
        Ok(removed)
    }

    /// 문서의 인덱싱 시점(last_indexed)만 업데이트합니다. (내용 변경 없이 유지 시 사용)
    async fn update_last_indexed(&self, project_id: i64, source_doc_id: &str, timestamp: i64) -> Result<(), String> {
        let conn = self.conn.write_conn().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE documents SET last_indexed = ?1 WHERE project_id = ?2 AND source_doc_id = ?3",
            params![timestamp, project_id, source_doc_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use serial_test::serial;
    use std::sync::Arc;
    use async_trait::async_trait;
    use doxus_plugin_sdk::{
        DocSource, PluginMetadata, PluginKind, Capabilities, SyncPolicy,
        FetchAllOpts, DocumentStream, FetchChangesOpts, ChangeSet,
        PluginConfig, PluginSecrets, SourceDocId, RawDocument,
        HealthStatus, PluginError,
    };

    // ── Bug 1: inject_keychain_auth 이중 호출 테스트 ──────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_inject_keychain_auth_called_once_on_real_config() {
        // DOXUS_SKIP_KEYCHAIN=1 환경에서 inject_keychain_auth 단일/이중 호출 시
        // secrets에 api_token이 정확히 한 번만 주입되는지 확인 (멱등성)
        std::env::set_var("DOXUS_SKIP_KEYCHAIN", "1");
        std::env::set_var("DOXUS_CONFLUENCE_API_TOKEN", "test-token-123");

        let mut config = doxus_plugin_sdk::PluginConfig::default();
        let mut secrets = doxus_plugin_sdk::PluginSecrets::default();

        // 단일 호출
        crate::auth::inject_keychain_auth("com.doxus.confluence", &mut config, &mut secrets).await;

        let token_count = secrets.fields.values()
            .filter(|v| matches!(v, doxus_plugin_sdk::SecretValue::Text(t) if t == "test-token-123"))
            .count();
        assert_eq!(token_count, 1, "api_token은 한 번만 주입되어야 함");

        // 두 번 호출해도 중복 삽입 없어야 함 (HashMap이므로 overwrite됨 — 여전히 1)
        crate::auth::inject_keychain_auth("com.doxus.confluence", &mut config, &mut secrets).await;
        let token_count_after = secrets.fields.values()
            .filter(|v| matches!(v, doxus_plugin_sdk::SecretValue::Text(t) if t == "test-token-123"))
            .count();
        assert_eq!(token_count_after, 1, "두 번 호출해도 중복 주입 없어야 함");

        std::env::remove_var("DOXUS_SKIP_KEYCHAIN");
        std::env::remove_var("DOXUS_CONFLUENCE_API_TOKEN");
    }

    // ── Issue D: partial failure 시 기존 문서 삭제 방지 테스트 ───────────────

    /// 첫 번째 fetch_all 호출부터 즉시 에러를 반환하는 플러그인
    struct AlwaysFailPlugin;

    #[async_trait]
    impl DocSource for AlwaysFailPlugin {
        fn metadata(&self) -> &PluginMetadata {
            static META: std::sync::OnceLock<PluginMetadata> = std::sync::OnceLock::new();
            META.get_or_init(|| PluginMetadata {
                id: "test.alwaysfail".to_string(),
                name: "Always Fail Plugin".to_string(),
                version: "0.1.0".to_string(),
                kind: PluginKind::Builtin,
            })
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                incremental_sync: false,
                oauth: false,
                native_search: false,
                sync_policy: SyncPolicy::Manual,
            }
        }

        async fn validate_config(&self, _config: &PluginConfig) -> Result<(), PluginError> {
            Ok(())
        }

        async fn initialize(&mut self, _: PluginConfig, _: PluginSecrets) -> Result<(), PluginError> {
            Ok(())
        }

        async fn fetch_all(&self, _opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
            Err(PluginError::NetworkError("simulated network failure".to_string()))
        }

        async fn fetch_changes(&self, _: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
            Err(PluginError::Internal("not supported".to_string()))
        }

        async fn fetch_document(&self, id: &SourceDocId) -> Result<RawDocument, PluginError> {
            Err(PluginError::NotFound(id.0.clone()))
        }

        async fn health_check(&self) -> HealthStatus {
            HealthStatus { healthy: true, message: None }
        }
    }

    fn create_test_pool() -> (tempfile::TempDir, DbPool) {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = crate::db::create_pool(&db_path).unwrap();
        (temp_dir, pool)
    }

    #[tokio::test]
    async fn test_partial_failure_does_not_delete_existing_documents() {
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();

        // 프로젝트 삽입
        conn.execute(
            "INSERT INTO projects (name, display_name, path, source_type, created_at, updated_at) \
             VALUES ('test-proj', 'Test', '/tmp', 'obsidian', 0, 0)",
            [],
        ).unwrap();
        let project_id: i64 = conn
            .query_row("SELECT id FROM projects WHERE name='test-proj'", [], |r| r.get(0))
            .unwrap();

        // 기존 문서 10개 삽입 (last_indexed = 0, sync_start_time보다 이전이므로 삭제 대상)
        {
            for i in 0..10i64 {
                conn.execute(
                    "INSERT INTO documents (project_id, source_doc_id, content_hash, last_indexed) \
                     VALUES (?1, ?2, 'hash', 0)",
                    rusqlite::params![project_id, format!("existing-doc-{i}")],
                ).unwrap();
            }
        }
        drop(conn);

        let mut pm = PluginManager::new(std::path::PathBuf::from("/tmp/plugins"));
        pm.register_factory("com.doxus.obsidian", || {
            Box::new(AlwaysFailPlugin) as Box<dyn DocSource + Send + Sync>
        });
        let pm = Arc::new(pm);
        let engine = Arc::new(SearchEngine::with_embedder(
            pool.clone(),
            Arc::new(crate::embedding::NoOpEmbedder),
        ));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        // 인덱싱 실행 (첫 fetch_all부터 실패)
        let result = indexer.index_project("test-proj", false).await;
        assert!(result.is_err(), "fetch_all 에러 시 index_project는 Err를 반환해야 함");

        // 기존 문서 10개가 보존되어야 함 (partial failure 시 삭제하면 안 됨)
        let conn = pool.get().unwrap();
        let doc_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(doc_count, 10,
            "fetch 실패 시 기존 문서 10개가 보존되어야 함 (현재 버그: 삭제됨)");
    }

    #[tokio::test]
    async fn test_needs_reindexing_logic() {
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp/plugins")));
        let engine = Arc::new(SearchEngine::with_embedder(pool.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        // 테스트용 프로젝트 삽입
        conn.execute(
            "INSERT INTO projects (id, name, display_name, path, created_at, updated_at) \
             VALUES (1, 'test', 'Test', '/tmp', 0, 0)",
            [],
        ).unwrap();

        // 테스트용 문서 삽입 (updated_at = 100, last_indexed = 100)
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, updated_at, last_indexed) \
             VALUES (1, 'doc1', 'Doc1', 'hash', 100, 100)",
            [],
        ).unwrap();

        // chunk_count > 0 이어야 needs_reindexing이 타임스탬프 비교로 진행됨
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index) \
             SELECT id, 'content', 0 FROM documents WHERE source_doc_id = 'doc1'",
            [],
        ).unwrap();
        drop(conn);

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
    async fn test_untitled_doc_not_force_reindexed_when_timestamp_matches() {
        // Bug B: "Untitled" 제목이어도 타임스탬프가 동일하면 재인덱싱하면 안 됨
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp/plugins")));
        let engine = Arc::new(SearchEngine::with_embedder(pool.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        conn.execute(
            "INSERT INTO projects (id, name, display_name, path, created_at, updated_at) \
             VALUES (1, 'test', 'Test', '/tmp', 0, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, updated_at, last_indexed) \
             VALUES (1, 'untitled-doc', 'Untitled', 'hash_abc', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index) \
             SELECT id, 'content', 0 FROM documents WHERE source_doc_id = 'untitled-doc'",
            [],
        ).unwrap();
        drop(conn);

        // 동일 타임스탬프 → 변경 없음 → 재인덱싱 안 해야 함
        assert!(!indexer.needs_reindexing(1, "untitled-doc", Some(100)).await,
            "Untitled + 동일 타임스탬프인 경우 재인덱싱하지 않아야 함");
    }

    #[tokio::test]
    async fn test_needs_reindexing_uses_content_hash_when_timestamp_missing() {
        // Bug C: updated_at=None이어도 content_hash가 동일하면 재인덱싱하면 안 됨
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp/plugins")));
        let engine = Arc::new(SearchEngine::with_embedder(pool.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        conn.execute(
            "INSERT INTO projects (id, name, display_name, path, created_at, updated_at) \
             VALUES (1, 'test', 'Test', '/tmp', 0, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash, updated_at, last_indexed) \
             VALUES (1, 'no-ts-doc', 'SomeTitle', 'stable_hash', NULL, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO chunks (document_id, content, chunk_index) \
             SELECT id, 'content', 0 FROM documents WHERE source_doc_id = 'no-ts-doc'",
            [],
        ).unwrap();
        drop(conn);

        // updated_at=None이지만 content_hash 동일 → 재인덱싱 안 해야 함
        assert!(!indexer.needs_reindexing_with_hash(1, "no-ts-doc", None, Some("stable_hash")).await,
            "타임스탬프 없어도 content_hash 동일하면 재인덱싱하지 않아야 함");

        // content_hash 변경 → 재인덱싱 해야 함
        assert!(indexer.needs_reindexing_with_hash(1, "no-ts-doc", None, Some("new_hash")).await,
            "content_hash 변경 시 재인덱싱해야 함");
    }

    #[tokio::test]
    async fn test_get_project_policy() {
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(pool.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        conn.execute(
            "INSERT INTO projects (name, display_name, path, sync_policy_json, created_at, updated_at) \
             VALUES ('test-policy', 'Test', '/tmp', '{\"type\":\"on_focus\"}', 0, 0)",
            [],
        ).unwrap();
        drop(conn);

        let policy = indexer.get_project_policy("test-policy").await.unwrap();
        assert!(matches!(policy, SyncPolicy::OnFocus));

        let default_policy = indexer.get_project_policy("non-existent").await;
        assert!(default_policy.is_err());
    }

    #[tokio::test]
    async fn test_index_project_audits_on_plugin_not_found() {
        let (_temp_dir, pool) = create_test_pool();
        let conn = pool.get().unwrap();
        let pm = Arc::new(PluginManager::new(std::path::PathBuf::from("/tmp")));
        let engine = Arc::new(SearchEngine::with_embedder(pool.clone(), Arc::new(crate::embedding::NoOpEmbedder)));
        let indexer = IndexingService::new(pool.clone(), pm, engine);

        // 프로젝트 삽입 (source_type = 'non_existent')
        conn.execute(
            "INSERT INTO projects (name, display_name, path, source_type, created_at, updated_at) \
             VALUES ('my-proj', 'My Proj', '/tmp', 'non_existent', 0, 0)",
            [],
        ).unwrap();
        drop(conn);

        // 실행 (플러그인을 찾을 수 없으므로 에러 발생 예상)
        let result = indexer.index_project("my-proj", false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("플러그인을 찾을 수 없습니다"));

        // audit_log 확인
        let conn = pool.get().unwrap();
        let (event_type, message): (String, String) = conn.query_row(
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
