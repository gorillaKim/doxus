use std::sync::{Arc, Mutex};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

use crate::plugin::PluginManager;
use crate::search::{SearchEngine, DocMeta};
use crate::auth::inject_keychain_auth;
use doxus_plugin_sdk::{FetchAllOpts, PluginConfig, PluginSecrets};

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

    /// 프로젝트의 소스 타입 및 설정을 조회하여 인덱싱을 수행합니다.
    pub async fn index_project(&self, name: &str) -> Result<usize, String> {
        let (project_id, plugin_id, config_json, project_path, strategy) = self.get_project_config(name).await?;
        
        // 1. 플러그인 초기화
        let mut plugin = self.plugin_manager.get_source(&plugin_id)
            .ok_or_else(|| format!("플러그인을 찾을 수 없습니다: {plugin_id}"))?;

        let mut config_fields = self.parse_config(&config_json);
        let mut secrets = PluginSecrets::default();

        // DB의 path 컬럼 정보를 설정에 주입 (Obsidian 등 로컬 경로가 필요한 플러그인 대응)
        if !project_path.is_empty() {
            config_fields.insert("path".to_string(), serde_json::Value::String(project_path));
        }

        // 키체인 인증 정보 주입
        inject_keychain_auth(&plugin_id, &mut PluginConfig { fields: config_fields.clone() }, &mut secrets);
        
        // inject_keychain_auth가 config.fields를 직접 수정하므로, 동기화를 위해 다시 꺼내옴
        let mut final_config = PluginConfig { fields: config_fields };
        inject_keychain_auth(&plugin_id, &mut final_config, &mut secrets);

        plugin.initialize(final_config, secrets).await
            .map_err(|e| format!("플러그인 초기화 실패: {e}"))?;

        // 2. 인덱싱 루프
        let mut total = 0;
        let mut cursor = None;

        loop {
            let stream = plugin.fetch_all(FetchAllOpts { cursor, page_size: 50 }).await
                .map_err(|e| format!("문서 수집 실패: {e}"))?;
            
            let docs = stream.documents;
            crate::log_d!("indexer", "[Core-Indexer] Received {} documents from plugin", docs.len());
            if docs.is_empty() { break; }

            for doc in &docs {
                let title = doc.title.as_deref().unwrap_or("Untitled");
                crate::log_d!("indexer", "[Core-Indexer] Processing document: {} (ID: {})", title, doc.id.0);
                let meta = DocMeta {
                    url: doc.url.clone(),
                    tags: doc.tags.clone(),
                    metadata: doc.metadata.clone(),
                    created_at: doc.created_at,
                    updated_at: doc.updated_at,
                    relative_path: doc.relative_path.clone(),
                    ..Default::default()
                };

                if let Err(e) = self.engine.index_document_async_with_meta(
                    project_id,
                    &doc.id.0,
                    title,
                    &doc.content,
                    meta,
                    &strategy
                ).await {
                    crate::log_d!("indexer", "[Core-Indexer] Error indexing {}: {}", doc.id.0, e);
                    tracing::error!("Indexing error for {}: {}", doc.id.0, e);
                } else {
                    total += 1;
                }
            }

            cursor = stream.next_cursor;
            if cursor.is_none() { break; }
        }

        crate::log_d!("indexer", "[Core-Indexer] Cycle finished. Total indexed this run: {}", total);
        Ok(total)
    }

    async fn get_project_config(&self, name: &str) -> Result<(i64, String, String, String, String), String> {
        let conn = self.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
        // source_instances 테이블에서 우선 조회 (신규 구조)
        let row = conn.query_row(
            "SELECT p.id, si.plugin_id, si.config_json, p.path, p.storage_strategy
             FROM projects p
             JOIN source_instances si ON p.id = si.project_id
             WHERE p.name = ?1
             LIMIT 1",
            params![name],
            |r| Ok((
                r.get::<_, i64>(0)?, 
                r.get::<_, String>(1)?, 
                r.get::<_, String>(2)?, 
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?
            ))
        );

        if let Ok(r) = row {
            return Ok(r);
        }

        // 실패 시 projects 테이블에서 조회 (구조 호환성)
        conn.query_row(
            "SELECT id, COALESCE(source_type, 'obsidian'), COALESCE(config_json, '{}'), path, storage_strategy
             FROM projects WHERE name = ?1",
            params![name],
            |r| {
                let pid: i64 = r.get(0)?;
                let stype: String = r.get(1)?;
                let cjson: String = r.get(2)?;
                let ppath: String = r.get(3)?;
                let strategy: String = r.get(4)?;
                let plugin_id = if stype == "obsidian" || stype == "confluence" || stype == "github" {
                    format!("com.doxus.{stype}")
                } else {
                    stype
                };
                Ok((pid, plugin_id, cjson, ppath, strategy))
            }
        ).map_err(|e| format!("프로젝트 설정을 찾을 수 없습니다: {e}"))
    }

    fn parse_config(&self, json_str: &str) -> HashMap<String, Value> {
        let fields: HashMap<String, Value> = serde_json::from_str(json_str).unwrap_or_default();
        
        // "fields" 키가 있으면 추출 (Tauri 저장 형식 대응)
        if let Some(inner) = fields.get("fields").and_then(|v| v.as_object()) {
            return inner.clone().into_iter().collect();
        }
        
        fields
    }

    /// 인덱싱 가능한 모든 활성 프로젝트 목록을 반환합니다.
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
}
