use serde_json::Value;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    loop_planner::{
        ensure_final_summary_seed, ensure_step_summary, extract_json_value,
        normalize_next_action_protocol, normalize_step_summary,
    },
    model_adapter,
    types::{
        is_workspace_tool_allowed, AgentAction, AgentActionPayload, AgentLoopDecision,
        AgentLoopSummary, AgentTaskRun, AgentTaskStatus, FailureReasonCode, RetryTarget,
        TopLevelIntent,
    },
    workspace_loop_prompt,
};

pub async fn plan_next_workspace_action(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &AgentTaskRun,
    conversation_context: Option<&str>,
    memory_context: Option<&str>,
    workspace_context: Option<&str>,
    default_workdir: &str,
) -> Result<AgentLoopDecision, String> {
    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| is_workspace_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let prompt = workspace_loop_prompt::build_workspace_next_action_prompt(&allowed_tools, default_workdir);

    let conversation_section = conversation_context
        .filter(|s| !s.is_empty())
        .map(|s| format!("最近对话上下文：\n{s}\n\n"))
        .unwrap_or_default();
    let workspace_section = workspace_context
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}\n"))
        .unwrap_or_default();
    let memory_section = memory_context
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n{}\n", s))
        .unwrap_or_default();

    let planner_input = format!(
        "用户原始请求：\n{}\n\n\
{}{}\
当前工作区目标：\n{}\n\n\
当前任务状态：\n\
- intent: {:?}\n\
- mode: {:?}\n\
- stepBudget: {}\n\
- retryBudget: {}\n\
- recentSteps: {}\n\
- lastToolResult: {}\n{}\n",
        user_input.trim(),
        conversation_section,
        workspace_section,
        task.goal.trim(),
        task.intent,
        task.mode,
        task.step_budget,
        task.retry_budget,
        serde_json::to_string(&task.recent_steps).unwrap_or_else(|_| "[]".to_string()),
        task.last_tool_result
            .as_ref()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string()),
        memory_section,
    );

    let raw = model_adapter::request_structured_agent_output(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &prompt,
        &planner_input,
    )
    .await?;

    parse_next_workspace_action(&raw)
}

pub fn parse_next_workspace_action(raw: &str) -> Result<AgentLoopDecision, String> {
    let payload = extract_json_value(raw)
        .ok_or_else(|| format!("workspace agent loop 没有返回可解析 JSON：{}", raw.trim()))?;
    let normalized = normalize_workspace_loop_decision(payload)?;
    let decision = serde_json::from_value::<AgentLoopDecision>(normalized)
        .map_err(|error| format!("workspace agent loop JSON 解析失败：{error}"))?;

    if !matches!(decision.intent, TopLevelIntent::WorkspaceTask) {
        return Err("workspace_task loop 只接受 workspace_task 意图。".to_string());
    }

    validate_next_workspace_action(&decision.next)?;
    Ok(decision)
}

fn normalize_workspace_loop_decision(mut payload: Value) -> Result<Value, String> {
    let goal = payload
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let next = payload
        .get_mut("next")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "workspace agent loop 返回缺少 next 对象。".to_string())?;
    let kind = normalize_next_action_protocol(next, "workspace agent loop")?;

    match kind.as_str() {
        "tool" | "confirm" | "retry" => {
            if let Some(step_summary) = next.remove("stepSummary") {
                next.entry("summary".to_string()).or_insert(step_summary);
            }
            ensure_step_summary(next, &kind);
            normalize_step_summary(next);
        }
        "finish" | "fail" => {
            if let Some(final_summary) = next.remove("finalSummary") {
                next.entry("summary".to_string()).or_insert(final_summary);
            }
            ensure_final_summary_seed(next, &kind);
            normalize_final_summary(next, &goal, &kind);
        }
        "respond" => {}
        _ => {}
    }

    Ok(payload)
}

fn validate_next_workspace_action(action: &AgentActionPayload) -> Result<(), String> {
    match action.action {
        AgentAction::Respond => {
            let message = action.message.as_deref().map(str::trim).unwrap_or_default();
            if message.trim().is_empty() {
                return Err("workspace loop message 不能为空。".to_string());
            }
        }
        AgentAction::Confirm | AgentAction::Tool => {
            let tool = action.tool.as_deref().unwrap_or_default();
            if !is_workspace_tool_allowed(tool) {
                return Err(format!("workspace loop 包含未授权工具：{tool}"));
            }
            if !action.args.is_object() {
                return Err("workspace loop args 必须是 object。".to_string());
            }
        }
        AgentAction::Retry => {
            let summary = action
                .summary
                .as_ref()
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if summary.trim().is_empty() {
                return Err("retry_step.summary 不能为空。".to_string());
            }
            if !matches!(action.target, Some(RetryTarget::LastTool)) {
                return Err("workspace loop 只允许 retry_step.target=last_tool。".to_string());
            }
        }
        AgentAction::Finish | AgentAction::Fail => {
            let message = action.message.as_deref().map(str::trim).unwrap_or_default();
            if message.trim().is_empty() {
                return Err("finish_task/fail_task.message 不能为空。".to_string());
            }
            let summary = action
                .summary
                .as_ref()
                .cloned()
                .ok_or_else(|| "finish/fail.summary 不能为空。".to_string())
                .and_then(|value| {
                    serde_json::from_value::<AgentLoopSummary>(value)
                        .map_err(|_| "finish/fail.summary 必须是对象。".to_string())
                })?;
            validate_summary(&summary)?;
        }
        AgentAction::Observe | AgentAction::Assert => {
            return Err("workspace loop 不接受 observe_context/assert_condition。".to_string());
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
            return Err(
                "finish_task/fail_task 的 summary.finalStatus 不能是 running/waiting_confirmation。"
                    .to_string(),
            );
        }
        AgentTaskStatus::Completed | AgentTaskStatus::Failed | AgentTaskStatus::Cancelled => {}
    }
    if matches!(summary.failure_reason_code, FailureReasonCode::ContextUnavailable) {
        return Err("workspace loop 不应输出 context_unavailable。".to_string());
    }
    Ok(())
}

fn normalize_final_summary(
    next: &mut serde_json::Map<String, Value>,
    goal: &str,
    kind: &str,
) {
    let message = next
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let summary = next
        .entry("summary".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));

    let object = summary
        .as_object_mut()
        .expect("summary object must exist after insertion");

    object
        .entry("goal".to_string())
        .or_insert_with(|| Value::String(goal.to_string()));
    object
        .entry("stepsTaken".to_string())
        .or_insert(Value::Number(0.into()));
    object
        .entry("finalStatus".to_string())
        .or_insert_with(|| {
            Value::String(if kind == "finish" {
                "completed".to_string()
            } else {
                "failed".to_string()
            })
        });
    object
        .entry("failureReasonCode".to_string())
        .or_insert_with(|| {
            Value::String(if kind == "finish" {
                "none".to_string()
            } else {
                "tool_failed".to_string()
            })
        });
    object
        .entry("usedProbe".to_string())
        .or_insert(Value::Bool(false));
    object
        .entry("usedRetry".to_string())
        .or_insert(Value::Bool(false));

    if kind == "finish" {
        object.insert("failureStage".to_string(), Value::Null);
    } else if !object.contains_key("failureStage") && !message.is_empty() {
        object.insert("failureStage".to_string(), Value::String("finish".to_string()));
    }
}
