use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

use doxus_core::db::create_pool;
use doxus_core::indexing::IndexingService;
use doxus_core::plugin::PluginManager;
use doxus_core::search::{SearchEngine, SearchQuery};

#[tokio::test]
async fn test_db_concurrency_read_write_separation() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("concurrency_test.db");

    // 1. 커넥션 풀 및 서비스 초기화
    let pool = create_pool(&db_path).unwrap();

    let pm = Arc::new(PluginManager::new(temp_dir.path().join("plugins")));
    let search_engine = Arc::new(SearchEngine::new_fts_only(pool.clone()));
    let _indexing_service = Arc::new(IndexingService::new(
        pool.clone(),
        pm.clone(),
        search_engine.clone(),
    ));

    // 2. 초기 데이터 및 프로젝트 생성
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('test-project', 'Test Project', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
    }

    // 3. 백그라운드 스레드에서 쓰기(인덱싱)를 계속 진행
    let search_engine_clone = search_engine.clone();
    let write_handle = tokio::spawn(async move {
        // 많은 양의 문서를 연속으로 인덱싱
        for i in 0..100 {
            let doc_id = format!("doc-{}", i);
            let content = format!(
                "This is the content of document number {}. It contains some text for search.",
                i
            );
            search_engine_clone
                .index_document_async(
                    1, // project_id
                    &doc_id,
                    &format!("Title {}", i),
                    &content,
                    "fts",
                )
                .await
                .expect("인덱싱 실패");
            sleep(Duration::from_millis(5)).await;
        }
    });

    // 4. 동시에 다른 스레드에서 읽기(검색)를 지속적으로 시도
    let search_engine_clone2 = search_engine.clone();
    let read_handle = tokio::spawn(async move {
        for _ in 0..50 {
            let query = SearchQuery {
                text: "content".to_string(),
                project_ids: vec![1],
                limit: 10,
                offset: 0,
                mode: doxus_core::search::SearchMode::Fts,
                created_after: None,
                created_before: None,
                updated_after: None,
                updated_before: None,
                tags: vec![],
            };
            let results = search_engine_clone2.search_async(&query).await;
            assert!(
                results.is_ok(),
                "검색 작업은 쓰기 중에도 항상 성공해야 합니다. 에러: {:?}",
                results.err()
            );
            sleep(Duration::from_millis(10)).await;
        }
    });

    // 두 작업이 모두 완료될 때까지 대기
    let (write_res, read_res) = tokio::join!(write_handle, read_handle);
    write_res.expect("쓰기 태스크 패닉");
    read_res.expect("읽기 태스크 패닉");
}
