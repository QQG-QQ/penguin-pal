use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    runtime_context::render_runtime_context_for_prompt,
    test_loop_prompt,
    types::{
        empty_json_object, is_agent_tool_allowed, AgentLoopDecision, AgentLoopSummary,
        AgentNextAction, AgentTaskStatus, AssertionType, RetryTarget, RuntimeContext,
        TopLevelIntent,
    },
};

pub async fn plan_next_test_action(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    context: &RuntimeContext,
) -> Result<AgentLoopDecision, String> {
    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let prompt = test_loop_prompt::build_test_next_action_prompt(&allowed_tools);
    let planner_input = format!(
        "用户原始请求：\n{}\n\n\
当前测试目标：\n{}\n\n\
当前 runtime context：\n{}\n",
        user_input.trim(),
        context.normalized_goal,
        render_runtime_context_for_prompt(context),
    );

    let raw = provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        &prompt,
        &planner_input,
    )
    .await?;

    parse_next_test_action(&raw)
}

pub fn parse_next_test_action(raw: &str) -> Result<AgentLoopDecision, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("测试 agent loop 没有返回可解析 JSON：{}", raw.trim()))?;
    let decision = serde_json::from_str::<AgentLoopDecision>(&payload)
        .map_err(|error| format!("测试 agent loop JSON 解析失败：{error}"))?;

    if !matches!(decision.intent, TopLevelIntent::TestRequest) {
        return Err("test_request loop 只接受 test_request 意图。".to_string());
    }

    validate_next_test_action(&decision.next)?;
    Ok(decision)
}

fn validate_next_test_action(action: &AgentNextAction) -> Result<(), String> {
    match action {
        AgentNextAction::RespondToUser { message }
        | AgentNextAction::ObserveContext { summary: message } => {
            if message.trim().is_empty() {
                return Err("测试 loop 文本字段不能为空。".to_string());
            }
        }
        AgentNextAction::AssertCondition {
            assertion_type: _,
            summary,
            params,
        } => {
            if summary.trim().is_empty() {
                return Err("assert_condition.summary 不能为空。".to_string());
            }
            if !params.is_object() {
                return Err("assert_condition.params 必须是 object。".to_string());
            }
        }
        AgentNextAction::RequestConfirmation { tool, args, .. } => {
            if !is_agent_tool_allowed(tool) {
                return Err(format!("测试 loop 包含未授权工具：{tool}"));
            }
            if !args.is_object() {
                return Err("request_confirmation.args 必须是 object。".to_string());
            }
        }
        AgentNextAction::ExecuteTool { tool, args, .. } => {
            if !is_agent_tool_allowed(tool) {
                return Err(format!("测试 loop 包含未授权工具：{tool}"));
            }
            if !args.is_object() {
                return Err("execute_tool.args 必须是 object。".to_string());
            }
        }
        AgentNextAction::RetryStep { target, summary } => {
            if summary.trim().is_empty() {
                return Err("retry_step.summary 不能为空。".to_string());
            }
            if !matches!(target, RetryTarget::ObserveContext | RetryTarget::LastTool) {
                return Err("retry_step.target 非法。".to_string());
            }
        }
        AgentNextAction::FinishTask { message, summary }
        | AgentNextAction::FailTask { message, summary } => {
            if message.trim().is_empty() {
                return Err("finish_task/fail_task.message 不能为空。".to_string());
            }
            validate_summary(summary)?;
        }
    }

    Ok(())
}

fn validate_summary(summary: &AgentLoopSummary) -> Result<(), String> {
    if summary.goal.trim().is_empty() {
        return Err("summary.goal 不能为空。".to_string());
    }
    match summary.final_status {
        AgentTaskStatus::Running | AgentTaskStatus::WaitingConfirmation => {
            return Err("finish_task/fail_task 的 summary.finalStatus 不能是 running/waiting_confirmation。".to_string());
        }
        AgentTaskStatus::Completed | AgentTaskStatus::Failed | AgentTaskStatus::Cancelled => {}
    }
    Ok(())
}

#[allow(dead_code)]
pub fn assert_decision(goal: &str, assertion_type: AssertionType, summary: &str, params: Value) -> AgentLoopDecision {
    AgentLoopDecision {
        intent: TopLevelIntent::TestRequest,
        goal: goal.trim().to_string(),
        next: AgentNextAction::AssertCondition {
            assertion_type,
            summary: summary.to_string(),
            params: if params.is_null() { empty_json_object() } else { params },
        },
    }
}

fn extract_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value.to_string());
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate)
        .ok()
        .map(|value| value.to_string())
}
