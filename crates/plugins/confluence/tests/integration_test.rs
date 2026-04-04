use doxus_plugin_confluence::ConfluencePlugin;
use doxus_plugin_sdk::{
    DocSource, FetchAllOpts, PluginConfig, PluginError, PluginSecrets, SecretValue,
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn make_plugin(server: &MockServer, space_key: &str) -> ConfluencePlugin {
    let mut plugin = ConfluencePlugin::new();
    let mut config = PluginConfig::default();
    config
        .fields
        .insert("base_url".into(), serde_json::json!(server.uri()));
    config
        .fields
        .insert("space_key".into(), serde_json::json!(space_key));
    let mut secrets = PluginSecrets::default();
    secrets
        .fields
        .insert("api_token".into(), SecretValue::Text("test-token".into()));
    plugin.initialize(config, secrets).await.unwrap();
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
        .and(path("/rest/api/content"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fixture))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM").await;
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
        .and(path("/rest/api/content"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/content"))
        .and(query_param("start", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/content"))
        .and(query_param("start", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page3))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM").await;

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
        .and(path("/rest/api/content"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM").await;
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
        .and(path("/rest/api/content"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let plugin = make_plugin(&server, "TEAM").await;
    let result = plugin
        .fetch_all(FetchAllOpts { cursor: None, page_size: 25 })
        .await;

    // If mock matched (limit=25 in query), request succeeded
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}
