use tauri::AppHandle;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::types::ToolInvokeResponse,
};

use super::{
    executor,
    intent,
    screen_context,
    screen_planner,
    types::{AgentMessageMeta, AgentRoute},
};

#[derive(Debug, Clone)]
pub struct AgentHandleResult {
    pub reply_text: String,
    pub provider_label: String,
    pub outcome: String,
    pub detail: String,
    pub meta: AgentMessageMeta,
}

pub async fn maybe_handle_control_message(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let looks_control = intent::parse_simple_control_plan(trimmed).is_some()
        || intent::looks_like_control_request(trimmed);

    let plan = if looks_control {
        let screen_context = screen_context::describe_current_screen(
            app,
            provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
        )
        .await;
        let plan_result = screen_planner::plan_from_screen_context(
            provider_config,
            api_key,
            oauth_access_token,
            codex_command,
            codex_home,
            permission_level,
            allowed_actions,
            trimmed,
            &screen_context,
        )
        .await;

        match plan_result {
            Ok(plan) => Some(plan),
            Err(error) => {
                return Ok(Some(AgentHandleResult {
                    reply_text: format!(
                        "这句更像桌面控制请求，但我这次没能基于当前 screen context 生成安全动作计划。请换成更明确的说法，例如“打开记事本并输入 hello”或“切到微信并输入你好”。\n\n详细原因：{}",
                        error
                    ),
                    provider_label: "Desktop Agent".to_string(),
                    outcome: "planner_error".to_string(),
                    detail: error,
                    meta: AgentMessageMeta {
                        route: AgentRoute::Control,
                        planned_tools: vec![],
                        pending_request: None,
                        task: None,
                    },
                }));
            }
        }
    } else {
        None
    };

    let Some(plan) = plan else {
        return Ok(None);
    };

    if matches!(plan.route, AgentRoute::Chat) {
        return Ok(None);
    }

    let result = executor::execute_plan(app, plan, trimmed)?;
    Ok(Some(AgentHandleResult {
        reply_text: result.reply_text,
        provider_label: "Desktop Agent".to_string(),
        outcome: result.outcome,
        detail: result.detail,
        meta: AgentMessageMeta {
            route: AgentRoute::Control,
            planned_tools: result.planned_tools,
            pending_request: result.pending_request,
            task: result.task,
        },
    }))
}

pub fn confirm_control_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    executor::confirm_pending(app, pending_id)
}

pub fn cancel_control_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    executor::cancel_pending(app, pending_id)
}
