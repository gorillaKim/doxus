use crate::indexing::IndexingService;

pub struct JobResult {
    pub success: bool,
    pub message: String,
}

/// Executes background tasks handled directly by doxus core
pub async fn execute_system(
    action: &str,
    config: &serde_json::Value,
    indexer: &IndexingService,
) -> JobResult {
    match action {
        "full_index" | "incremental_sync" => {
            let project = config["project"].as_str().unwrap_or("");
            if project.is_empty() {
                return JobResult { success: false, message: "project not specified in config".into() };
            }
            
            let is_full = action == "full_index";
            match indexer.index_project(project, is_full).await {
                Ok(n) => JobResult { success: true, message: format!("{}: {} docs processed", project, n) },
                Err(e) => JobResult { success: false, message: e.to_string() },
            }
        }
        "freshness_batch" => {
            let service = crate::freshness::db::FreshnessService::new(indexer.conn().clone());
            match service.recalculate_all() {
                Ok(n) => JobResult { success: true, message: format!("Recalculated freshness for {n} documents") },
                Err(e) => JobResult { success: false, message: e.to_string() },
            }
        }
        _ => JobResult { success: false, message: format!("unknown system action: {action}") }
    }
}

/// Delegates complex workflows to the agent / sidecar
pub async fn execute_agent(
    action: &str,
    config: &serde_json::Value,
    project_name: Option<&str>,
) -> JobResult {
    let prompt = match action {
        "freshness_review" => format!(
            "Review freshness for project '{}'",
            project_name.unwrap_or("all")
        ),
        "custom_prompt" => config["prompt"].as_str().unwrap_or("").to_string(),
        _ => return JobResult { success: false, message: format!("unknown agent action: {action}") }
    };
    
    // (Phase 2): Send to sidecar
    JobResult { success: true, message: format!("agent prompt queued: {:.100}", prompt) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_agent_freshness_review() {
        let res = execute_agent("freshness_review", &json!({}), Some("my-vault")).await;
        assert!(res.success);
        assert!(res.message.contains("my-vault"));

        let res2 = execute_agent("unknown", &json!({}), None).await;
        assert!(!res2.success);
    }
    
    #[tokio::test]
    async fn test_agent_custom_prompt() {
        let res = execute_agent("custom_prompt", &json!({"prompt": "Hello"}), None).await;
        assert!(res.success);
        assert!(res.message.contains("Hello"));
    }
}
