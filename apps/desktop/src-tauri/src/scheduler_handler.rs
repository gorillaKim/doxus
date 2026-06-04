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
        let model = config["model"].as_str().unwrap_or("claude-sonnet-4-6").to_string();
        
        let persona = config["persona"].as_str().unwrap_or("devlog_specialist");
        let summary_style = config["summary_style"].as_str().unwrap_or("bullet_points");
        let custom_prompt = config["custom_prompt"].as_str().unwrap_or("");
        
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

        eprintln!("[scheduler] Running AI report job: {} (model: {}, persona: {})", job_name, model, persona);

        if project_names.is_empty() {
            return Err("Search scope projects are empty".into());
        }

        // 1. Find target project path
        let target_project_path = {
            let conn = self.state.conn.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT path FROM projects WHERE name = ?1",
                [target_project_name],
                |r: &rusqlite::Row<'_>| r.get::<_, String>(0)
            ).map_err(|e| format!("Target project '{}' not found: {}", target_project_name, e))?
        };

        // 2. Detect CLI
        let provider = if model.contains("gemini") { "gemini" } else { "claude" };
        let cli_info = detect_cli_path(provider.to_string()).await?;
        let cli_path = cli_info["cliPath"].as_str().ok_or("CLI path not found")?.to_string();

        // 3. Start Session & Register Collector
        let session_id = format!("scheduled-{}", uuid::Uuid::new_v4());
        eprintln!("[scheduler] Starting agent session: {}", session_id);
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
            "custom" => "사용자가 정의한 특수 임무를 수행하는 AI 전문가입니다.",
            _ => "AI 분석 전문가입니다.",
        };

        let style_desc = match summary_style {
            "bullet_points" => "간결하고 명확한 불렛 포인트 중심의 지표형 리포트",
            "narrative" => "흐름과 맥락을 중시하는 이야기 형태의 서사 리포트",
            "actionable" => "앞으로의 방향성과 실행 가능한 액션 아이템 중심의 통찰 리포트",
            "comparative" => "과거 데이터나 다른 프로젝트와의 차이점을 분석하는 비교 리포트",
            _ => "일반적인 상세 요약 리포트",
        };

        let instruction = if !custom_prompt.is_empty() {
            format!("### 사용자의 특별 지시사항\n{}\n\n위 지시사항을 최우선으로 반영하여 분석을 수행하십시오.", custom_prompt)
        } else {
            format!("분석 결과를 '{}'로 작성하십시오.", style_desc)
        };

        let main_prompt = format!(
            "당신은 {persona_desc}\n\n\
            ### 작업 데이터 소스\n\
            1. 다음 프로젝트들을 검색하여 필요한 정보를 확보하십시오: [{project_names}]\n\
            2. 연관 태그: [{tags}], 주요 검색어: [{keywords}]를 활용하여 문서를 색인하십시오.\n\n\
            ### 최종 작업 지침\n\
            {instruction}\n\n\
            ### 제약 사항\n\
            - 분석에 필요한 내용이 부족하다면 가능한 범위 내에서 최선을 다해 작성하십시오.\n\
            - 절대 나에게 확인 질문을 하거나 중간 과정을 설명하지 마십시오.\n\
            - 분석을 완료한 후, 오직 최종 결과인 Markdown 텍스트만을 답변으로 출력하십시오.\n\n\
            지금 바로 분석을 시작하십시오.",
            persona_desc = persona_desc,
            project_names = project_names.join(", "),
            tags = tags.join(", "),
            keywords = keywords.join(", "),
            instruction = instruction
        );

        // 5. Send Message and Wait for Completion
        eprintln!("[scheduler] Sending main prompt to agent...");
        chat_send_message_impl(
            &self.state,
            session_id.clone(),
            main_prompt,
        ).await?;

        // 6. Collect final content
        eprintln!("[scheduler] Agent finished. Collecting output for session {}", session_id);
        let report_content = {
            let mut coll = self.state.collected_messages.lock().map_err(|_| "Collector lock failed")?;
            coll.remove(&session_id).unwrap_or_default()
        };

        if report_content.trim().is_empty() {
            eprintln!("[scheduler] Error: AI generated empty report for job {}", job_name);
            return Err("AI generated empty report. The agent might have failed or didn't produce final text.".into());
        }

        eprintln!("[scheduler] Report content received ({} characters). Saving to file...", report_content.len());

        // 7. Save to File
        let date_str = Local::now().format("%Y-%m-%d").to_string();
        let file_name = format!("{}-{}.md", date_str, job_name);
        let report_dir = PathBuf::from(target_project_path).join(sub_dir);
        
        // Ensure directory exists
        std::fs::create_dir_all(&report_dir).map_err(|e| format!("Failed to create report directory: {}", e))?;
        
        let file_path = report_dir.join(file_name);
        std::fs::write(&file_path, report_content).map_err(|e| format!("Failed to write report file: {}", e))?;

        eprintln!("[scheduler] Success: Report saved to {}", file_path.display());
        Ok(format!("Report saved to: {}", file_path.display()))
    }
}
