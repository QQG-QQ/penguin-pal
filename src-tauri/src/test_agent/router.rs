use tauri::AppHandle;

use crate::{
    agent::types::{AgentMessageMeta, AgentRoute},
    control::types::ToolInvokeResponse,
    testing::harness,
};

use super::intent;

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
    user_input: &str,
) -> Result<Option<TestAgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if !intent::looks_like_test_request(trimmed) {
        return Ok(None);
    }

    let Some(request) = intent::parse_test_request(trimmed) else {
        return Ok(Some(TestAgentHandleResult {
            reply_text: "这句更像测试请求，但当前没有匹配到受控测试套件。请换成例如“跑一轮 smoke test”或“测试微信草稿输入”。".to_string(),
            provider_label: "Test Agent".to_string(),
            outcome: "test_selection_blocked".to_string(),
            detail: "没有匹配到测试套件".to_string(),
            meta: AgentMessageMeta {
                route: AgentRoute::Test,
                planned_tools: vec![],
                pending_request: None,
                task: None,
            },
        }));
    };

    let result = harness::execute_request(app, request)?;
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
        },
    }))
}

pub fn confirm_control_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    harness::confirm_pending(app, pending_id)
}

pub fn cancel_control_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    harness::cancel_pending(app, pending_id)
}
