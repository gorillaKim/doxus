use std::sync::Arc;
use std::path::PathBuf;
use doxus_core::scheduler::{AgentHandler, JobResult};
use crate::AppState;
use crate::commands::agent::{detect_cli_path, chat_start_session_impl, chat_send_message_impl};
use chrono::Local;

pub struct TauriAgentHandler {
    pub state: Arc<AppState>,
    pub app_handle: tauri::AppHandle,
}

#[async_trait::async_trait]
impl AgentHandler for TauriAgentHandler {
    async fn execute_agent(
        &self,
        job_name: &str,
        action: &str,
        config: &serde_json::Value,
    ) -> JobResult {
        if action != "ai_agent_report" {
            return JobResult { success: false, message: format!("Unsupported agent action: {}", action) };
        }

        match self.run_ai_report(job_name, config).await {
            Ok(msg) => JobResult { success: true, message: msg },
            Err(e) => JobResult { success: false, message: e },
        }
    }
}

impl TauriAgentHandler {
    async fn run_ai_report(&self, job_name: &str, config: &serde_json::Value) -> Result<String, String> {
        let model = config["model"].as_str().unwrap_or("claude-3-5-sonnet-latest");
        let persona = config["persona"].as_str().unwrap_or("devlog_specialist");
        let summary_style = config["summary_style"].as_str().unwrap_or("bullet_points");
        
        let scope = &config["scope"];
        let project_names = scope["project_names"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let tags = scope["tags"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let keywords = scope["keywords"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        let output = &config["output"];
        let target_project_name = output["project_name"].as_str().unwrap_or("");
        let sub_dir = output["sub_dir"].as_str().unwrap_or("reports");

        if project_names.is_empty() {
            return Err("Search scope projects are empty".into());
        }

        // 1. Find target project path
        let target_project_path = {
            let conn = self.state.conn.lock().map_err(|_| "DB lock failed")?;
            conn.query_row(
                "SELECT path FROM projects WHERE name = ?1",
                [target_project_name],
                |r| r.get::<_, String>(0)
            ).map_err(|e| format!("Target project '{}' not found: {}", target_project_name, e))?
        };

        // 2. Detect CLI
        let provider = if model.contains("gemini") { "gemini" } else { "claude" };
        let cli_info = detect_cli_path(provider.to_string()).await?;
        let cli_path = cli_info["cliPath"].as_str().ok_or("CLI path not found")?.to_string();

        // 3. Start Session & Register Collector
        let session_id = format!("scheduled-{}", uuid::Uuid::new_v4());
        {
            let mut coll = self.state.collected_messages.lock().map_err(|_| "Collector lock failed")?;
            coll.insert(session_id.clone(), String::new());
        }

        chat_start_session_impl(
            self.app_handle.clone(),
            &self.state,
            session_id.clone(),
            provider.to_string(),
            cli_path,
            model.to_string(),
        ).await?;

        // 4. Build Prompt
        let persona_desc = match persona {
            "devlog_specialist" => "전문 개발 로그 분석가로서, 기술적 결정 사항과 트러블슈팅 과정을 깊이 있게 정리합니다.",
            "knowledge_curator" => "지식 큐레이터로서, 방대한 문서들 사이의 핵심 인사이트와 숨겨진 연결고리를 찾아냅니다.",
            "research_assistant" => "리서치 전문가로서, 사실 관계를 명확히 하고 정보를 체계적으로 요약합니다.",
            _ => "AI 분석 전문가입니다.",
        };

        let style_desc = match summary_style {
            "bullet_points" => "간결하고 명확한 불렛 포인트 중심의 지표형 리포트",
            "narrative" => "흐름과 맥락을 중시하는 이야기 형태의 서사 리포트",
            "actionable" => "앞으로의 방향성과 실행 가능한 액션 아이템 중심의 통찰 리포트",
            "comparative" => "과거 데이터나 다른 프로젝트와의 차이점을 분석하는 비교 리포트",
            _ => "일반적인 상세 요약 리포트",
        };

        let main_prompt = format!(
            "당신은 {persona_desc}\n\n\
            ### 작업 지침\n\
            1. 다음 프로젝트들을 검색하여 최신 정보를 확보하십시오: [{project_names}]\n\
            2. 태그: [{tags}], 검색어: [{keywords}] 조건을 우선적으로 탐색하십시오.\n\
            3. 분석 결과를 '{style_desc}'로 작성하십시오.\n\n\
            ### 제약 사항\n\
            - 절대 나에게 확인 질문을 하지 마십시오.\n\
            - 즉시 분석을 시작하고 최종 결과인 Markdown 텍스트만 출력하십시오.\n\
            - 분석에 필요한 내용이 부족하다면 가능한 범위 내에서 최선을 다해 작성하십시오.\n\n\
            지금 바로 분석을 시작하십시오.",
            project_names = project_names.join(", "),
            tags = tags.join(", "),
            keywords = keywords.join(", ")
        );

        // 5. Send Message and Wait for Completion
        chat_send_message_impl(
            &self.state,
            session_id.clone(),
            main_prompt,
        ).await?;

        // 6. Collect final content
        let report_content = {
            let mut coll = self.state.collected_messages.lock().map_err(|_| "Collector lock failed")?;
            coll.remove(&session_id).unwrap_or_default()
        };

        if report_content.trim().is_empty() {
            return Err("AI generated empty report".into());
        }

        // 7. Save to File
        let date_str = Local::now().format("%Y-%m-%d").to_string();
        let file_name = format!("{}-{}.md", date_str, job_name);
        let report_dir = PathBuf::from(target_project_path).join(sub_dir);
        
        // Ensure directory exists
        std::fs::create_dir_all(&report_dir).map_err(|e| format!("Failed to create report directory: {}", e))?;
        
        let file_path = report_dir.join(file_name);
        std::fs::write(&file_path, report_content).map_err(|e| format!("Failed to write report file: {}", e))?;

        Ok(format!("Report saved to: {}", file_path.display()))
    }
}
