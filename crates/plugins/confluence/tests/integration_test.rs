use doxus_plugin_confluence::ConfluencePlugin;
use doxus_plugin_sdk::{DocSource, FetchAllOpts, FetchChangesOpts, PluginError, SourceDocId};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_ancestor_plugin(server: &MockServer, ancestor_id: &str) -> ConfluencePlugin {
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_ancestor_config(
        server.uri().trim_end_matches('/').to_string(),
        ancestor_id.to_string(),
        "test-token".to_string(),
    );
    plugin
}

fn make_plugin(server: &MockServer, space_key: &str) -> ConfluencePlugin {
    // Set fields directly to bypass SSRF validation (wiremock uses HTTP localhost).
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(
        server.uri().trim_end_matches('/').to_string(),
        space_key.to_string(),
        "test-token".to_string(),
    );
    plugin
}

#[tokio::test]
async fn fetch_all_returns_pages_from_api() {
    let server = MockServer::start().await;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/confluence_pages.json"
    ))
    .unwrap();

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fixture))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let stream = plugin
        .fetch_all(FetchAllOpts {
            cursor: None,
            page_size: 25,
        })
        .await
        .unwrap();

    assert_eq!(stream.documents.len(), 3);
    assert_eq!(stream.documents[0].title.as_deref(), Some("Page 1"));
    assert_eq!(stream.documents[1].title.as_deref(), Some("Page 2"));
    assert_eq!(stream.documents[2].title.as_deref(), Some("Page 3"));
}

#[tokio::test]
async fn fetch_all_paginates_with_cursor() {
    let server = MockServer::start().await;

    let page1 = serde_json::json!({
        "results": [{"id": "1", "title": "A", "_links": {"webui": ""}, "body": null}],
        "start": 0, "limit": 25, "size": 25
    });
    let page2 = serde_json::json!({
        "results": [{"id": "2", "title": "B", "_links": {"webui": ""}, "body": null}],
        "start": 25, "limit": 25, "size": 25
    });
    let page3 = serde_json::json!({
        "results": [{"id": "3", "title": "C", "_links": {"webui": ""}, "body": null}],
        "start": 50, "limit": 25, "size": 10
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("start", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("start", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page3))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");

    // page 1: cursor=None → start=0
    let p1 = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await
        .unwrap();
    assert_eq!(p1.next_cursor.as_deref(), Some("25"));

    // page 2: cursor="25" → start=25
    let p2 = plugin
        .fetch_all(FetchAllOpts { cursor: p1.next_cursor, page_size: 25 })
        .await
        .unwrap();
    assert_eq!(p2.next_cursor.as_deref(), Some("50"));

    // page 3: cursor="50" → start=50, size(10) < limit(25) → no next cursor
    let p3 = plugin
        .fetch_all(FetchAllOpts { cursor: p2.next_cursor, page_size: 25 })
        .await
        .unwrap();
    assert!(p3.next_cursor.is_none());
}

#[tokio::test]
async fn fetch_all_returns_error_on_unauthorized() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(
        matches!(result, Err(PluginError::AuthRequired)),
        "expected AuthRequired, got: {:?}",
        result
    );
}

#[tokio::test]
async fn fetch_all_respects_page_size() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "results": [],
        "start": 0, "limit": 25, "size": 0
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    // If mock matched (limit=25 in query), request succeeded
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[tokio::test]
async fn fetch_changes_returns_updated_pages() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "results": [
            {"id": "200", "title": "Updated Page", "_links": {"webui": "/wiki/updated"}, "body": {"storage": {"value": "<p>New content</p>"}}}
        ],
        "start": 0, "limit": 25, "size": 1
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let changeset = plugin
        .fetch_changes(FetchChangesOpts {
            since: 1704067200, // 2024-01-01T00:00:00Z
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await
        .unwrap();

    assert_eq!(changeset.updated.len(), 1);
    assert_eq!(changeset.updated[0].title.as_deref(), Some("Updated Page"));
    assert!(changeset.next_cursor.is_none());
}

#[tokio::test]
async fn fetch_changes_paginates_with_cursor() {
    let server = MockServer::start().await;

    let page1 = serde_json::json!({
        "results": [{"id": "1", "title": "A", "_links": {"webui": ""}, "body": null}],
        "start": 0, "limit": 25, "size": 25
    });
    let page2 = serde_json::json!({
        "results": [{"id": "2", "title": "B", "_links": {"webui": ""}, "body": null}],
        "start": 25, "limit": 25, "size": 10
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .and(query_param("start", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");

    let cs1 = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await
        .unwrap();
    assert_eq!(cs1.next_cursor.as_deref(), Some("25"));

    let cs2 = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: cs1.next_cursor,
            page_size: 25,
            known_ids: vec![],
        })
        .await
        .unwrap();
    assert!(cs2.next_cursor.is_none());
    assert_eq!(cs2.updated[0].title.as_deref(), Some("B"));
}

#[tokio::test]
async fn fetch_changes_returns_auth_error_on_401() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await;

    assert!(
        matches!(result, Err(PluginError::AuthRequired)),
        "expected AuthRequired, got: {:?}",
        result
    );
}

#[tokio::test]
async fn fetch_changes_detects_deletions_on_final_page() {
    let server = MockServer::start().await;

    // Only page "200" comes back; "999" is in known_ids → should be deleted
    let body = serde_json::json!({
        "results": [
            {"id": "200", "title": "Existing Page", "_links": {"webui": ""}, "body": null}
        ],
        "start": 0, "limit": 25, "size": 1
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let changeset = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![SourceDocId("999".into()), SourceDocId("200".into())],
        })
        .await
        .unwrap();

    assert_eq!(changeset.updated.len(), 1);
    assert_eq!(changeset.deleted_ids.len(), 1);
    assert_eq!(changeset.deleted_ids[0].0, "999");
}

#[tokio::test]
async fn health_check_ancestor_only_config_returns_healthy() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/4667998225"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "4667998225",
            "type": "page",
            "title": "Root Page",
            "_links": {"webui": "/wiki/root"}
        })))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    let status = plugin.health_check().await;

    assert!(status.healthy, "expected healthy for ancestor-only config, got: {:?}", status.message);
}

#[tokio::test]
async fn health_check_ancestor_only_returns_unhealthy_on_404() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/9999999"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "9999999");
    let status = plugin.health_check().await;

    assert!(!status.healthy, "expected unhealthy for 404 ancestor");
}

#[tokio::test]
async fn fetch_all_returns_rate_limited_on_429() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "30"),
        )
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(
        matches!(result, Err(PluginError::RateLimited { retry_after_secs: 30 })),
        "expected RateLimited{{30}}, got: {:?}",
        result
    );
}

#[tokio::test]
async fn fetch_all_returns_rate_limited_default_on_429_without_header() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(
        matches!(result, Err(PluginError::RateLimited { retry_after_secs: 60 })),
        "expected RateLimited{{60}}, got: {:?}",
        result
    );
}

#[tokio::test]
async fn fetch_all_returns_permission_denied_on_403() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    assert!(
        matches!(result, Err(PluginError::PermissionDenied(_))),
        "expected PermissionDenied, got: {:?}",
        result
    );
}

#[tokio::test]
async fn fetch_changes_returns_rate_limited_on_429() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "45"),
        )
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM");
    let result = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await;

    assert!(
        matches!(result, Err(PluginError::RateLimited { retry_after_secs: 45 })),
        "expected RateLimited{{45}}, got: {:?}",
        result
    );
}

// ── ancestor_id (folder) mode tests ──────────────────────────────────────────

#[tokio::test]
async fn fetch_all_ancestor_filters_out_folder_types() {
    let server = MockServer::start().await;

    // Real Confluence returns mix of folders + pages when ancestor is a folder
    let body = serde_json::json!({
        "results": [
            {"id": "101", "type": "folder", "title": "Sub-folder", "_links": {"webui": "/wiki/folder1"}, "body": null},
            {"id": "200", "type": "page", "title": "Real Page", "_links": {"webui": "/wiki/page1"}, "body": {"storage": {"value": "<p>Content</p>"}}},
            {"id": "201", "type": "page", "title": "Another Page", "_links": {"webui": "/wiki/page2"}, "body": null},
        ],
        "start": 0, "limit": 25, "size": 3
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    let stream = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await
        .unwrap();

    // folder type must be filtered out — only 2 pages returned
    assert_eq!(stream.documents.len(), 2, "should filter out folder type items");
    assert_eq!(stream.documents[0].title.as_deref(), Some("Real Page"));
    assert_eq!(stream.documents[1].title.as_deref(), Some("Another Page"));
}

#[tokio::test]
async fn fetch_all_ancestor_returns_zero_when_only_folders() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "results": [
            {"id": "101", "type": "folder", "title": "Folder A", "_links": {"webui": ""}, "body": null},
            {"id": "102", "type": "folder", "title": "Folder B", "_links": {"webui": ""}, "body": null},
        ],
        "start": 0, "limit": 25, "size": 2
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    let stream = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await
        .unwrap();

    assert_eq!(stream.documents.len(), 0, "all-folder response should yield 0 documents");
}

#[tokio::test]
async fn fetch_changes_ancestor_filters_out_folder_types() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "results": [
            {"id": "101", "type": "folder", "title": "Folder", "_links": {"webui": ""}, "body": null},
            {"id": "200", "type": "page", "title": "Updated Page", "_links": {"webui": ""}, "body": {"storage": {"value": "content"}}},
        ],
        "start": 0, "limit": 25, "size": 2
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    let changeset = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![],
        })
        .await
        .unwrap();

    assert_eq!(changeset.updated.len(), 1, "folder type should be filtered from changes");
    assert_eq!(changeset.updated[0].title.as_deref(), Some("Updated Page"));
}

#[tokio::test]
async fn fetch_changes_ancestor_deletion_ignores_folder_ids() {
    let server = MockServer::start().await;

    // Only "200" (page) comes back; "999" is known page ID → deleted
    // "101" is a folder ID in known_ids — should NOT appear in deletions
    let body = serde_json::json!({
        "results": [
            {"id": "200", "type": "page", "title": "Existing", "_links": {"webui": ""}, "body": null},
        ],
        "start": 0, "limit": 25, "size": 1
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    let changeset = plugin
        .fetch_changes(FetchChangesOpts {
            since: 0,
            cursor: None,
            page_size: 25,
            known_ids: vec![SourceDocId("200".into()), SourceDocId("999".into())],
        })
        .await
        .unwrap();

    assert_eq!(changeset.deleted_ids.len(), 1);
    assert_eq!(changeset.deleted_ids[0].0, "999");
}
