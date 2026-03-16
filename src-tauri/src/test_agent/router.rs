use tauri::AppHandle;

use crate::{
    agent::types::{AgentMessageMeta, AgentRoute},
    app_state::{DesktopAction, ProviderConfig},
    control::types::ToolInvokeResponse,
    testing::harness,
};

use super::{intent, planner};

#[derive(Debug, Clone)]
pub struct TestAgentHandleResult {
    pub reply_text: String,
    pub provider_label: String,
    pub outcome: String,
    pub detail: String,
    pub meta: AgentMessageMeta,
}

pub async fn maybe_handle_test_message(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    force_route: bool,
) -> Result<Option<TestAgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if !intent::prefers_harness_baseline(trimmed) {
        return Ok(None);
    }

    let request = if let Some(request) = intent::parse_test_request(trimmed) {
        request
    } else {
        match planner::plan_test_request(
            provider_config,
            api_key,
            oauth_access_token,
            codex_command,
            codex_home,
            permission_level,
            allowed_actions,
            trimmed,
        )
        .await
        {
            Ok(request) => request,
            Err(error) => {
                if !force_route {
                    return Ok(None);
                }
                return Ok(Some(TestAgentHandleResult {
                    reply_text: format!(
                        "这句更像测试请求，但当前没有匹配到合规的受控测试计划。\n\n原因：{}",
                        error
                    ),
                    provider_label: "Test Agent".to_string(),
                    outcome: "test_selection_blocked".to_string(),
                    detail: error,
                    meta: AgentMessageMeta {
                        route: AgentRoute::Test,
                        planned_tools: vec![],
                        pending_request: None,
                        task: None,
                        summary: None,
                    },
                }));
            }
        }
    };

    let result = harness::execute_request(app, request).await?;
    Ok(Some(TestAgentHandleResult {
        reply_text: result.reply_text,
        provider_label: "Test Agent".to_string(),
        outcome: if result.pending_request.is_some() {
            "test_pending".to_string()
        } else if result.report.failure_items.is_empty() {
            "test_ok".to_string()
        } else {
            "test_failed".to_string()
        },
        detail: format!(
            "run={} status={:?}",
            result.report.run_id, result.report.status
        ),
        meta: AgentMessageMeta {
            route: AgentRoute::Test,
            planned_tools: vec!["test_harness".to_string()],
            pending_request: result.pending_request,
            task: result.task,
            summary: None,
        },
    }))
}

pub async fn confirm_control_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    harness::confirm_pending(app, pending_id).await
}

pub async fn cancel_control_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    harness::cancel_pending(app, pending_id).await
}
