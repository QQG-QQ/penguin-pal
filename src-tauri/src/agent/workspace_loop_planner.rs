use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    types::{
        is_workspace_tool_allowed, AgentLoopDecision, AgentLoopSummary, AgentNextAction,
        AgentTaskRun, AgentTaskStatus, FailureReasonCode, RetryTarget, TopLevelIntent,
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

    let raw = provider::plan_control_request(
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
    let kind = next
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "workspace agent loop 返回缺少 next.kind。".to_string())?;
    let kind = kind.to_string();

    match kind.as_str() {
        "execute_tool" | "request_confirmation" | "retry_step" => {
            if let Some(step_summary) = next.remove("stepSummary") {
                next.entry("summary".to_string()).or_insert(step_summary);
            }
            ensure_step_summary(next, &kind);
            normalize_step_summary(next);
        }
        "finish_task" | "fail_task" => {
            if let Some(final_summary) = next.remove("finalSummary") {
                next.entry("summary".to_string()).or_insert(final_summary);
            }
            ensure_final_summary_seed(next, &kind);
            normalize_final_summary(next, &goal, &kind);
        }
        "respond_to_user" => {}
        _ => {}
    }

    Ok(payload)
}

fn validate_next_workspace_action(action: &AgentNextAction) -> Result<(), String> {
    match action {
        AgentNextAction::RespondToUser { message } => {
            if message.trim().is_empty() {
                return Err("workspace loop message 不能为空。".to_string());
            }
        }
        AgentNextAction::RequestConfirmation { tool, args, .. }
        | AgentNextAction::ExecuteTool { tool, args, .. } => {
            if !is_workspace_tool_allowed(tool) {
                return Err(format!("workspace loop 包含未授权工具：{tool}"));
            }
            if !args.is_object() {
                return Err("workspace loop args 必须是 object。".to_string());
            }
        }
        AgentNextAction::RetryStep { target, summary } => {
            if summary.trim().is_empty() {
                return Err("retry_step.summary 不能为空。".to_string());
            }
            if !matches!(target, RetryTarget::LastTool) {
                return Err("workspace loop 只允许 retry_step.target=last_tool。".to_string());
            }
        }
        AgentNextAction::FinishTask { message, summary }
        | AgentNextAction::FailTask { message, summary } => {
            if message.trim().is_empty() {
                return Err("finish_task/fail_task.message 不能为空。".to_string());
            }
            validate_summary(summary)?;
        }
        AgentNextAction::ObserveContext { .. } | AgentNextAction::AssertCondition { .. } => {
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

fn extract_json_value(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate).ok()
}

fn normalize_step_summary(next: &mut serde_json::Map<String, Value>) {
    let Some(summary) = next.get_mut("summary") else {
        return;
    };

    if summary.is_string() {
        return;
    }

    let normalized = summary
        .as_object()
        .and_then(|map| {
            map.get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| map.get("text").and_then(Value::as_str).map(ToString::to_string))
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.to_string());
    *summary = Value::String(normalized);
}

fn ensure_step_summary(next: &mut serde_json::Map<String, Value>, kind: &str) {
    if next.contains_key("summary") {
        return;
    }

    let fallback = next
        .get("message")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            next.get("tool")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("执行工具 {value}"))
        })
        .unwrap_or_else(|| kind.to_string());
    next.insert("summary".to_string(), Value::String(fallback));
}

fn ensure_final_summary_seed(next: &mut serde_json::Map<String, Value>, kind: &str) {
    if next.contains_key("summary") {
        return;
    }

    let fallback = next
        .get("message")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| kind.to_string());
    next.insert("summary".to_string(), Value::String(fallback));
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
            Value::String(if kind == "finish_task" {
                "completed".to_string()
            } else {
                "failed".to_string()
            })
        });
    object
        .entry("failureReasonCode".to_string())
        .or_insert_with(|| {
            Value::String(if kind == "finish_task" {
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

    if kind == "finish_task" {
        object.insert("failureStage".to_string(), Value::Null);
    } else if !object.contains_key("failureStage") && !message.is_empty() {
        object.insert("failureStage".to_string(), Value::String("finish".to_string()));
    }
}
