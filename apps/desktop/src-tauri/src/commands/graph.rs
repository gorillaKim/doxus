// No imports needed here except serde provided by graph.rs structure

#[derive(serde::Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: String, // "doc" or "tag"
    pub project: Option<String>,
    pub plugin_id: Option<String>,
}

#[derive(serde::Serialize)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    pub link_type: String, // "link" or "tag_rel"
}

#[derive(serde::Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

pub(crate) fn get_graph_data_impl(conn: &rusqlite::Connection) -> Result<GraphData, String> {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // 1. 문서 노드 수집
    let mut stmt = conn.prepare(
        "SELECT d.id, COALESCE(d.title, d.source_doc_id), p.name, d.plugin_id 
         FROM documents d
         JOIN projects p ON d.project_id = p.id"
    ).map_err(|e| e.to_string())?;

    let doc_nodes: Vec<GraphNode> = stmt.query_map([], |r| {
        let id: i64 = r.get(0)?;
        Ok(GraphNode {
            id: id.to_string(),
            label: r.get(1)?,
            node_type: "doc".to_string(),
            project: Some(r.get(2)?),
            plugin_id: r.get(3)?,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    nodes.extend(doc_nodes);

    // 2. 태그 노드 수집 (유니크한 태그들)
    let mut stmt = conn.prepare("SELECT DISTINCT tag FROM document_tags")
        .map_err(|e| e.to_string())?;
    let tags: Vec<String> = stmt.query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    for tag in tags {
        nodes.push(GraphNode {
            id: format!("tag:{}", tag),
            label: tag,
            node_type: "tag".to_string(),
            project: None,
            plugin_id: None,
        });
    }

    // 3. 문서-문서 링크 (document_links)
    // resolved된 target_id가 있는 것만 수집
    let mut stmt = conn.prepare("SELECT source_id, target_id FROM document_links WHERE target_id IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let doc_links: Vec<GraphLink> = stmt.query_map([], |r| {
        let sid: i64 = r.get(0)?;
        let tid: i64 = r.get(1)?;
        Ok(GraphLink {
            source: sid.to_string(),
            target: tid.to_string(),
            link_type: "link".to_string(),
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    links.extend(doc_links);

    // 4. 문서-태그 링크 (document_tags)
    let mut stmt = conn.prepare("SELECT document_id, tag FROM document_tags")
        .map_err(|e| e.to_string())?;
    let tag_links: Vec<GraphLink> = stmt.query_map([], |r| {
        let sid: i64 = r.get(0)?;
        let tag: String = r.get(1)?;
        Ok(GraphLink {
            source: sid.to_string(),
            target: format!("tag:{}", tag),
            link_type: "tag_rel".to_string(),
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    links.extend(tag_links);

    Ok(GraphData { nodes, links })
}

#[tauri::command]
pub async fn get_graph_data(
    state: tauri::State<'_, crate::AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    let data = get_graph_data_impl(&conn)?;
    Ok(serde_json::to_value(data).map_err(|e| e.to_string())?)
}
