use doxus_plugin_confluence::ConfluencePlugin;
use doxus_plugin_sdk::{DocSource, FetchAllOpts, FetchChangesOpts, PluginError, SourceDocId, DocumentStream, ChangeSet, HealthStatus};
#[allow(unused_imports)]
use doxus_plugin_sdk::wasm_types::*;
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
    let mut plugin = ConfluencePlugin::new();
    plugin.set_test_config(
        server.uri().trim_end_matches('/').to_string(),
        space_key.to_string(),
        "test-token".to_string(),
    );
    plugin
}

async fn mock_v2_basics(server: &MockServer, space_key: &str, space_id: &str, space_name: &str) {
    let space_json = serde_json::json!({
        "results": [{"id": space_id, "key": space_key, "name": space_name}],
        "_links": {"next": null, "webui": null}
    });
    Mock::given(method("GET")).and(path("/api/v2/spaces")).respond_with(ResponseTemplate::new(200).set_body_json(&space_json)).mount(server).await;

    let empty_list = serde_json::json!({"results": [], "_links": {"next": null, "webui": null}});
    Mock::given(method("GET")).and(path(format!("/api/v2/spaces/{}/pages", space_id))).respond_with(ResponseTemplate::new(200).set_body_json(&empty_list)).mount(server).await;
    Mock::given(method("GET")).and(path(format!("/api/v2/spaces/{}/folders", space_id))).respond_with(ResponseTemplate::new(200).set_body_json(&empty_list)).mount(server).await;
}

#[tokio::test]
async fn fetch_all_returns_pages_from_api() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;

    let fixture = serde_json::json!({
        "results": [
            {"id": "1", "title": "Page 1", "_links": {"webui": "/p1"}, "body": {"storage": {"value": "c1"}}},
            {"id": "2", "title": "Page 2", "_links": {"webui": "/p2"}, "body": {"storage": {"value": "c2"}}},
            {"id": "3", "title": "Page 3", "_links": {"webui": "/p3"}, "body": {"storage": {"value": "c3"}}}
        ],
        "_links": {"next": null}
    });

    Mock::given(method("GET")).and(path("/api/v2/pages")).respond_with(ResponseTemplate::new(200).set_body_json(&fixture)).mount(&server).await;

    let plugin = make_plugin(&server, "TEAM");
    let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await.unwrap();
    assert_eq!(stream.documents.len(), 3);
}

#[tokio::test]
async fn fetch_all_paginates_with_cursor() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;

    let res1 = serde_json::json!({
        "results": [{"id": "1", "title": "A", "_links": {"webui": "/a"}}],
        "_links": {"next": "/next"}
    });
    let res2 = serde_json::json!({
        "results": [{"id": "2", "title": "B", "_links": {"webui": "/b"}}],
        "_links": {"next": null}
    });

    Mock::given(method("GET")).and(path("/api/v2/pages")).and(query_param("offset", "0")).respond_with(ResponseTemplate::new(200).set_body_json(&res1)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v2/pages")).and(query_param("offset", "25")).respond_with(ResponseTemplate::new(200).set_body_json(&res2)).mount(&server).await;

    let plugin = make_plugin(&server, "TEAM");
    let p1 = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await.unwrap();
    assert_eq!(p1.next_cursor.as_deref(), Some("25"));
}

#[tokio::test]
async fn fetch_changes_returns_updated_pages() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;

    let body = serde_json::json!({
        "results": [{"id": "200", "type": "page", "title": "Updated", "_links": {"webui": "/w"}, "body": {"storage": {"value": "v"}}}],
        "start": 0, "limit": 25, "size": 1
    });

    Mock::given(method("GET")).and(path("/rest/api/content/search")).respond_with(ResponseTemplate::new(200).set_body_json(&body)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v2/pages/200")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "id": "200", "title": "Updated", "parentId": null, "_links": {"webui": "/w"}
    }))).mount(&server).await;

    let plugin = make_plugin(&server, "TEAM");
    let cs = plugin.fetch_changes(FetchChangesOpts { since: 0, cursor: None, page_size: 25, known_ids: vec![] }).await.unwrap();
    assert_eq!(cs.updated.len(), 1);
}

#[tokio::test]
async fn health_check_ancestor_only_config_returns_healthy() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;

    Mock::given(method("GET")).and(path("/api/v2/pages/4667998225")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "id": "4667998225", "title": "Root", "_links": {"webui": "/r"}, "parentId": null
    }))).mount(&server).await;

    let plugin = make_ancestor_plugin(&server, "4667998225");
    assert!(plugin.health_check().await.healthy);
}

#[tokio::test]
async fn health_check_ancestor_only_returns_unhealthy_on_404() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;
    Mock::given(method("GET")).and(path("/api/v2/pages/999")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    let plugin = make_ancestor_plugin(&server, "999");
    assert!(!plugin.health_check().await.healthy);
}

#[tokio::test]
async fn fetch_all_ancestor_filters_out_folder_types() {
    let server = MockServer::start().await;
    mock_v2_basics(&server, "TEAM", "123", "Team Space").await;

    let body = serde_json::json!({
        "results": [
            {"id": "101", "type": "folder", "title": "F", "_links": {"webui": "/f"}},
            {"id": "200", "type": "page", "title": "P", "_links": {"webui": "/p"}}
        ],
        "start": 0, "size": 2, "limit": 25
    });

    Mock::given(method("GET")).and(path("/rest/api/content/search")).respond_with(ResponseTemplate::new(200).set_body_json(&body)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v2/pages")).and(query_param("id", "200")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "results": [{"id": "200", "title": "P", "parentId": null, "_links": {"webui": "/p"}}],
        "_links": {"next": null}
    }))).mount(&server).await;

    let plugin = make_ancestor_plugin(&server, "123");
    let stream = plugin.fetch_all(FetchAllOpts { cursor: None, page_size: 25 }).await.unwrap();
    assert_eq!(stream.documents.len(), 1);
}
