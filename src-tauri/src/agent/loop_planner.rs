use serde_json::Value;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    loop_prompt,
    model_adapter,
    runtime_context::render_runtime_context_for_prompt,
    types::{
        empty_json_object, is_agent_tool_allowed, AgentAction, AgentActionPayload,
        AgentLoopDecision, AgentPlan, AgentRoute, AgentTaskRun, AgentTaskStatus,
        AgentLoopSummary, RuntimeContext, TopLevelIntent,
    },
};

pub async fn plan_next_action(
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
    context: &RuntimeContext,
    conversation_context: Option<&str>,
    memory_context: Option<&str>,
) -> Result<AgentLoopDecision, String> {
    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let prompt = loop_prompt::build_next_action_prompt(&allowed_tools);

    let memory_section = memory_context
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n\n{}\n", s))
        .unwrap_or_default();
    let conversation_section = conversation_context
        .filter(|s| !s.is_empty())
        .map(|s| format!("最近对话上下文：\n{s}\n\n"))
        .unwrap_or_default();

    let planner_input = format!(
        "用户原始请求：\n{}\n\n\
{}\
当前目标：\n{}\n\n\
当前任务状态：\n\
- intent: {:?}\n\
- mode: {:?}\n\
- stepBudget: {}\n\
- retryBudget: {}\n\
- recentSteps: {}\n\
- lastToolResult: {}\n\n\
当前 runtime context：\n{}{}\n",
        user_input.trim(),
        conversation_section,
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
        render_runtime_context_for_prompt(context),
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

    parse_next_action(&raw)
}

pub fn parse_next_action(raw: &str) -> Result<AgentLoopDecision, String> {
    let payload = extract_json_value(raw)
        .ok_or_else(|| format!("桌面 agent loop 没有返回可解析 JSON：{}", raw.trim()))?;
    let normalized = normalize_loop_decision(payload)?;
    let decision = serde_json::from_value::<AgentLoopDecision>(normalized)
        .map_err(|error| format!("桌面 agent loop JSON 解析失败：{error}"))?;

    if !matches!(decision.intent, TopLevelIntent::DesktopAction) {
        return Err("desktop_action loop 只接受 desktop_action 意图。".to_string());
    }

    validate_next_action(&decision.next)?;
    Ok(decision)
}

fn normalize_loop_decision(mut payload: Value) -> Result<Value, String> {
    let goal = payload
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let next = payload
        .get_mut("next")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "桌面 agent loop 返回缺少 next 对象。".to_string())?;
    let kind = normalize_next_action_protocol(next, "桌面 agent loop")?;

    match kind.as_str() {
        "tool" | "confirm" => {
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
        "observe" | "assert" | "retry" => {
            // 这些 kind 也需要 summary 字段
            ensure_step_summary(next, &kind);
        }
        "respond" => {}
        _ => {}
    }

    Ok(payload)
}

pub(crate) fn normalize_next_action_protocol(
    next: &mut serde_json::Map<String, Value>,
    scope: &str,
) -> Result<String, String> {
    if let Some(tool_args) = next.remove("toolArgs") {
        next.entry("args".to_string()).or_insert(tool_args);
    }
    if let Some(reply_message) = next.remove("replyText") {
        next.entry("message".to_string()).or_insert(reply_message);
    }

    let raw = next
        .get("action")
        .and_then(Value::as_str)
        .or_else(|| next.get("kind").and_then(Value::as_str))
        .ok_or_else(|| format!("{scope} 返回缺少 next.action/next.kind。"))?;

    let normalized = match raw.trim() {
        "respond" | "reply" | "respond_to_user" => "respond",
        "tool" | "execute" | "execute_tool" => "tool",
        "confirm" | "request_confirmation" => "confirm",
        "observe" | "observe_context" => "observe",
        "assert" | "assert_condition" => "assert",
        "retry" | "retry_step" => "retry",
        "finish" | "finish_task" => "finish",
        "fail" | "fail_task" => "fail",
        other => other,
    }
    .to_string();

    next.insert("action".to_string(), Value::String(normalized.clone()));
    Ok(normalized)
}

pub fn decision_from_plan(goal: &str, plan: AgentPlan) -> Result<AgentLoopDecision, String> {
    if matches!(plan.route, AgentRoute::Chat) || plan.steps.is_empty() {
        return Err("fallback 计划没有可执行步骤。".to_string());
    }

    let first = plan
        .steps
        .first()
        .cloned()
        .ok_or_else(|| "fallback 计划没有第一步。".to_string())?;

    Ok(AgentLoopDecision {
        intent: TopLevelIntent::DesktopAction,
        goal: goal.trim().to_string(),
        next: AgentActionPayload {
            action: AgentAction::Tool,
            message: None,
            tool: Some(first.tool),
            summary: Some(Value::String(
                first
                    .summary
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "执行桌面动作".to_string()),
            )),
            args: if first.args.is_null() {
                empty_json_object()
            } else {
                first.args
            },
            target: None,
            assertion_type: None,
            params: empty_json_object(),
        },
    })
}

fn validate_next_action(action: &AgentActionPayload) -> Result<(), String> {
    match action.action {
        AgentAction::Respond => {
            if required_message(action)?.is_empty() {
                return Err("next action message 不能为空。".to_string());
            }
        }
        AgentAction::Finish | AgentAction::Fail => {
            if required_message(action)?.is_empty() {
                return Err("next action message 不能为空。".to_string());
            }
            let summary = required_final_summary(action)?;
            if summary.goal.trim().is_empty() {
                return Err("finish_task/fail_task.summary.goal 不能为空。".to_string());
            }
            if matches!(summary.final_status, AgentTaskStatus::Running | AgentTaskStatus::WaitingConfirmation) {
                return Err("finish_task/fail_task.summary.finalStatus 非法。".to_string());
            }
        }
        AgentAction::Observe => {
            if required_step_summary(action)?.is_empty() {
                return Err("observe_context.summary 不能为空。".to_string());
            }
        }
        AgentAction::Retry => {
            if required_step_summary(action)?.is_empty() {
                return Err("retry_step.summary 不能为空。".to_string());
            }
        }
        AgentAction::Assert => {
            return Err("desktop_action loop 不接受 assert_condition。".to_string());
        }
        AgentAction::Confirm => {
            let tool = required_tool(action)?;
            if !is_agent_tool_allowed(tool) {
                return Err(format!("request_confirmation 包含未授权工具：{tool}"));
            }
            if !action.args.is_object() {
                return Err("request_confirmation.args 必须是 object。".to_string());
            }
        }
        AgentAction::Tool => {
            let tool = required_tool(action)?;
            if !is_agent_tool_allowed(tool) {
                return Err(format!("execute_tool 包含未授权工具：{tool}"));
            }
            if required_step_summary(action)?.is_empty() {
                return Err("execute_tool.summary 不能为空。".to_string());
            }
            if !action.args.is_object() {
                return Err("execute_tool.args 必须是 object。".to_string());
            }
        }
    }

    Ok(())
}

fn required_message(action: &AgentActionPayload) -> Result<&str, String> {
    action
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "message 不能为空。".to_string())
}

fn required_tool(action: &AgentActionPayload) -> Result<&str, String> {
    action
        .tool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "tool 不能为空。".to_string())
}

fn required_step_summary(action: &AgentActionPayload) -> Result<&str, String> {
    action
        .summary
        .as_ref()
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "summary 不能为空。".to_string())
}

fn required_final_summary(action: &AgentActionPayload) -> Result<AgentLoopSummary, String> {
    let value = action
        .summary
        .as_ref()
        .cloned()
        .ok_or_else(|| "summary 不能为空。".to_string())?;
    serde_json::from_value::<AgentLoopSummary>(value)
        .map_err(|_| "summary 必须是对象。".to_string())
}

pub(crate) fn extract_json_value(raw: &str) -> Option<Value> {
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

pub(crate) fn normalize_step_summary(next: &mut serde_json::Map<String, Value>) {
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

pub(crate) fn ensure_step_summary(next: &mut serde_json::Map<String, Value>, kind: &str) {
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

pub(crate) fn ensure_final_summary_seed(next: &mut serde_json::Map<String, Value>, kind: &str) {
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

pub(crate) fn normalize_final_summary(
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
    let current_summary = next.get("summary").cloned();
    let Some(summary_value) = current_summary else {
        return;
    };

    if let Some(summary) = summary_value.as_object() {
        let mut normalized = summary.clone();
        normalized.insert("goal".to_string(), Value::String(goal.to_string()));
        normalized.insert(
            "stepsTaken".to_string(),
            normalized
                .get("stepsTaken")
                .cloned()
                .filter(|value| value.is_u64() || value.is_i64())
                .unwrap_or_else(|| Value::from(0)),
        );
        normalized.insert(
            "finalStatus".to_string(),
            normalize_final_status_value(normalized.get("finalStatus"), kind),
        );
        normalized.insert(
            "failureStage".to_string(),
            normalize_failure_stage_value(normalized.get("failureStage"), kind),
        );
        normalized.insert(
            "failureReasonCode".to_string(),
            normalize_failure_reason_code_value(
                normalized.get("failureReasonCode"),
                &message,
                kind,
            ),
        );
        normalized.insert(
            "usedProbe".to_string(),
            normalize_bool_value(normalized.get("usedProbe")),
        );
        normalized.insert(
            "usedRetry".to_string(),
            normalize_bool_value(normalized.get("usedRetry")),
        );
        next.insert("summary".to_string(), Value::Object(normalized));
        return;
    }

    let fallback_message = if message.is_empty() {
        summary_value
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        message
    };
    let final_status = if kind == "finish" {
        "completed"
    } else {
        "failed"
    };
    let failure_stage = if kind == "finish" {
        Value::Null
    } else {
        Value::String("finish".to_string())
    };
    let failure_reason_code = if kind == "finish" {
        "none"
    } else {
        map_failure_reason_code(&fallback_message)
    };

    next.insert(
        "summary".to_string(),
        serde_json::json!({
            "goal": goal,
            "stepsTaken": 0,
            "finalStatus": final_status,
            "failureStage": failure_stage,
            "failureReasonCode": failure_reason_code,
            "usedProbe": false,
            "usedRetry": false,
        }),
    );
}

fn normalize_final_status_value(value: Option<&Value>, kind: &str) -> Value {
    let fallback = if kind == "finish" {
        "completed"
    } else {
        "failed"
    };
    let next = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| matches!(*item, "completed" | "failed" | "cancelled"))
        .unwrap_or(fallback);
    Value::String(next.to_string())
}

fn normalize_failure_stage_value(value: Option<&Value>, kind: &str) -> Value {
    if kind == "finish" {
        return Value::Null;
    }

    let next = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| {
            // 显式拒绝字符串 "null"，避免 JSON 解析问题
            *item != "null" && matches!(
                *item,
                "planning" | "observation" | "execute_tool" | "assertion" | "confirmation" | "retry" | "finish"
            )
        })
        .unwrap_or("finish");
    Value::String(next.to_string())
}

fn normalize_failure_reason_code_value(value: Option<&Value>, message: &str, kind: &str) -> Value {
    if kind == "finish" {
        return Value::String("none".to_string());
    }

    let next = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| {
            matches!(
                *item,
                "none"
                    | "planner_failed"
                    | "context_unavailable"
                    | "tool_failed"
                    | "assertion_failed"
                    | "confirmation_required"
                    | "confirmation_rejected"
                    | "retry_exhausted"
                    | "step_budget_exceeded"
                    | "policy_blocked"
                    | "invalid_action"
                    | "file_missing"
            )
        })
        .map(ToString::to_string)
        .unwrap_or_else(|| map_failure_reason_code(message).to_string());
    Value::String(next)
}

fn normalize_bool_value(value: Option<&Value>) -> Value {
    Value::Bool(value.and_then(Value::as_bool).unwrap_or(false))
}

fn map_failure_reason_code(message: &str) -> &'static str {
    let lowered = message.to_lowercase();
    if lowered.contains("context") || lowered.contains("上下文") {
        "context_unavailable"
    } else if lowered.contains("policy") || lowered.contains("权限") || lowered.contains("blocked")
    {
        "policy_blocked"
    } else if lowered.contains("tool") || lowered.contains("执行") || lowered.contains("失败") {
        "tool_failed"
    } else {
        "invalid_action"
    }
}

#[cfg(test)]
mod tests {
    use super::parse_next_action;
    use crate::agent::types::AgentAction;

    #[test]
    fn parse_generic_action_protocol_for_tool_step() {
        let raw = r#"{
          "intent":"desktop_action",
          "goal":"打开记事本",
          "next":{
            "action":"tool",
            "tool":"launch_program",
            "stepSummary":"启动记事本",
            "toolArgs":{"program":"notepad"}
          }
        }"#;

        let decision = parse_next_action(raw).expect("generic action protocol should parse");
        assert_eq!(decision.next.action, AgentAction::Tool);
        assert_eq!(decision.next.tool.as_deref(), Some("launch_program"));
        assert_eq!(
            decision.next.summary.as_ref().and_then(|v| v.as_str()),
            Some("启动记事本")
        );
        assert_eq!(
            decision.next.args.get("program").and_then(|v| v.as_str()),
            Some("notepad")
        );
    }

    #[test]
    fn parse_generic_action_protocol_for_reply() {
        let raw = r#"{
          "intent":"desktop_action",
          "goal":"解释状态",
          "next":{
            "action":"respond",
            "replyText":"当前没有可执行动作。"
          }
        }"#;

        let decision = parse_next_action(raw).expect("generic respond action should parse");
        assert_eq!(decision.next.action, AgentAction::Respond);
        assert_eq!(decision.next.message.as_deref(), Some("当前没有可执行动作。"));
    }
}
