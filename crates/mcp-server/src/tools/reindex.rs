use crate::server::McpServer;
use crate::types::McpResponse;
use doxus_core::reindex::{ReindexOptions, ReindexScope, ReindexService};
use rusqlite::params;
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn reindex_documents(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let scope = match args["scope"].as_str().unwrap_or("full") {
        "document" => {
            let doc_id = match args["document_id"].as_str() {
                Some(s) => s.to_string(),
                None => return McpResponse::err(id, -32602, "scope=document requires document_id"),
            };
            ReindexScope::Document(doc_id)
        }
        "documents" => {
            let ids: Vec<String> = match args["document_ids"].as_array() {
                Some(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
                None => {
                    return McpResponse::err(
                        id,
                        -32602,
                        "scope=documents requires document_ids array",
                    )
                }
            };
            ReindexScope::Documents(ids)
        }
        "date_range" => ReindexScope::DateRange {
            created_after: args["created_after"].as_i64(),
            created_before: args["created_before"].as_i64(),
        },
        _ => ReindexScope::Full,
    };

    let options = ReindexOptions {
        force: args["force"].as_bool().unwrap_or(false),
        dry_run: args["dry_run"].as_bool().unwrap_or(false),
        batch_size: args["batch_size"].as_u64().unwrap_or(50) as usize,
    };

    let indexing = Arc::new(server.indexer());
    let service = ReindexService::new(server.conn(), indexing);

    match service.reindex(project, scope, options).await {
        Err(e) => McpResponse::err(id, -32603, e),
        Ok(result) => {
            let mut resp = json!({
                "total": result.total,
                "processed": result.processed,
                "skipped": result.skipped,
                "errors": result.errors,
                "duration_ms": result.duration_ms,
            });
            if let Some(targets) = result.dry_run_targets {
                resp["dry_run_targets"] = json!(targets);
            }
            McpResponse::ok(
                id,
                json!({
                    "content": [{"type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default()}]
                }),
            )
        }
    }
}

pub fn reindex_status(server: &McpServer, id: Value, args: &Value) -> McpResponse {
    let project = match args["project"].as_str() {
        Some(p) => p,
        None => return McpResponse::err(id, -32602, "missing required arg: project"),
    };

    let conn = server.conn();
    let conn_lock = match conn.get() {
        Ok(l) => l,
        Err(e) => return McpResponse::err(id, -32603, format!("db pool error: {e}")),
    };

    let pid: Result<i64, _> = conn_lock.query_row(
        "SELECT id FROM projects WHERE name=?1",
        params![project],
        |r: &rusqlite::Row<'_>| r.get(0),
    );
    let pid = match pid {
        Ok(p) => p,
        Err(_) => return McpResponse::err(id, -32602, format!("project '{}' not found", project)),
    };

    let mut stmt = match conn_lock.prepare(
        "SELECT id, scope, status, total_docs, processed_docs, error_message, started_at, completed_at \
         FROM reindex_history WHERE project_id=?1 ORDER BY started_at DESC LIMIT 10"
    ) {
        Ok(s) => s,
        Err(e) => return McpResponse::err(id, -32603, e.to_string()),
    };

    let rows: Result<Vec<Value>, _> = stmt
        .query_map(params![pid], |r: &rusqlite::Row<'_>| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "scope": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "total_docs": r.get::<_, i64>(3)?,
                "processed_docs": r.get::<_, i64>(4)?,
                "error_message": r.get::<_, Option<String>>(5)?,
                "started_at": r.get::<_, i64>(6)?,
                "completed_at": r.get::<_, Option<i64>>(7)?,
            }))
        })
        .and_then(|it| it.collect());

    match rows {
        Err(e) => McpResponse::err(id, -32603, e.to_string()),
        Ok(rows) => McpResponse::ok(
            id,
            json!({
                "content": [{"type": "text", "text": serde_json::to_string_pretty(&json!({
                    "project": project,
                    "history": rows,
                })).unwrap_or_default()}]
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServer;
    use doxus_core::search::{DocMeta, SyncSearchEngine};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct TestServer {
        _temp_dir: tempfile::TempDir,
        server: McpServer,
        pid: i64,
    }

    fn make_server() -> TestServer {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = doxus_core::db::create_pool(&db_path).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "INSERT INTO projects(name, display_name, path, status, storage_strategy, created_at, updated_at) \
                 VALUES ('rp', 'ReindexProj', '/tmp', 'active', 'full', unixepoch(), unixepoch())",
                [],
            ).unwrap();
        }
        let pid: i64 = {
            let conn = pool.get().unwrap();
            conn.query_row("SELECT id FROM projects WHERE name='rp'", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
        };
        let pm = Arc::new(doxus_core::plugin::PluginManager::new(PathBuf::from(
            "/tmp",
        )));
        let server = McpServer::new(pool, db_path, None, pm, PathBuf::from("/tmp"));
        TestServer {
            _temp_dir: temp_dir,
            server,
            pid,
        }
    }

    fn insert_doc(server: &McpServer, pid: i64, sid: &str, title: &str) {
        let conn = server.conn();
        let c = conn.get().unwrap();
        let engine = SyncSearchEngine::from_conn(&c);
        let meta = DocMeta {
            created_at: Some(1000),
            updated_at: Some(1000),
            ..Default::default()
        };
        engine
            .index_document_with_meta(pid, sid, title, title, &meta, "full")
            .unwrap();
    }

    fn get_text(resp: &McpResponse) -> String {
        let v = serde_json::to_value(resp).unwrap();
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    // ── Step 5 TDD 테스트 ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_reindex_dry_run_returns_targets() {
        let ts = make_server();
        insert_doc(&ts.server, ts.pid, "d1", "Doc One");
        insert_doc(&ts.server, ts.pid, "d2", "Doc Two");

        let args = json!({
            "project": "rp",
            "scope": "full",
            "dry_run": true,
        });
        let resp = reindex_documents(&ts.server, json!(1), &args).await;
        assert!(resp.error.is_none(), "오류 없음");
        let text = get_text(&resp);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["total"].as_i64(), Some(2), "대상 2건");
        assert_eq!(v["processed"].as_i64(), Some(0), "dry_run이므로 처리 0");
        assert!(v["dry_run_targets"].is_array(), "dry_run_targets 반환");
        assert_eq!(v["dry_run_targets"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_reindex_status_returns_history() {
        let ts = make_server();
        insert_doc(&ts.server, ts.pid, "d1", "Doc One");

        // 먼저 dry_run으로 이력 생성
        let args = json!({ "project": "rp", "scope": "full", "dry_run": true });
        reindex_documents(&ts.server, json!(1), &args).await;

        // status 조회
        let resp = reindex_status(&ts.server, json!(2), &json!({ "project": "rp" }));
        assert!(resp.error.is_none(), "status 오류 없음");
        let text = get_text(&resp);
        let v: Value = serde_json::from_str(&text).unwrap();
        let history = v["history"].as_array().expect("history array");
        assert!(!history.is_empty(), "이력이 있어야 함");
        assert_eq!(history[0]["status"].as_str(), Some("dry_run"));
    }

    #[tokio::test]
    async fn test_reindex_missing_project_returns_error() {
        let ts = make_server();
        let args = json!({ "project": "nonexistent", "scope": "full" });
        let resp = reindex_documents(&ts.server, json!(1), &args).await;
        assert!(resp.error.is_some(), "존재하지 않는 프로젝트는 오류 반환");
    }

    #[tokio::test]
    async fn test_reindex_status_missing_project_returns_error() {
        let ts = make_server();
        let resp = reindex_status(&ts.server, json!(1), &json!({ "project": "ghost" }));
        assert!(resp.error.is_some(), "존재하지 않는 프로젝트는 오류 반환");
    }
}
