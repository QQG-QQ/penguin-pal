use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::{now_millis, DesktopAction, ProviderConfig, RuntimeState, VisionChannelConfig},
    control::registry as control_registry,
    control::{router as control_router, types::ToolInvokeResponse},
    history,
    test_agent,
    testing,
};

use super::{
    executor::{self, LoopToolExecution},
    intent,
    loop_planner,
    runtime_binding,
    runtime_context,
    screen_context,
    screen_planner,
    task_store,
    test_assertions,
    test_loop_planner,
    types::{
        AgentLoopSummary, AgentLoopTaskStatus, AgentMessageMeta, AgentNextAction, AgentRoute,
        AgentTaskProgress, AgentTaskRun, AgentTaskStatus, FailureReasonCode, FailureStage,
        RetryTarget, TopLevelIntent,
    },
};

const DEFAULT_LOOP_STEP_BUDGET: usize = 6;
const DEFAULT_LOOP_RETRY_BUDGET: usize = 1;
const TEST_LOOP_STEP_BUDGET: usize = 8;
const TEST_LOOP_RETRY_BUDGET: usize = 1;

#[derive(Debug, Clone)]
pub struct AgentHandleResult {
    pub reply_text: String,
    pub provider_label: String,
    pub outcome: String,
    pub detail: String,
    pub meta: AgentMessageMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmationIntent {
    Confirm,
    Cancel,
}

pub async fn maybe_handle_control_message(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    force_route: bool,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let looks_control = force_route
        || intent::parse_simple_control_plan(trimmed).is_some()
        || intent::looks_like_control_request(trimmed);
    if !looks_control {
        return Ok(None);
    }

    if task_store::has_active_task(app)? {
        return Ok(Some(blocked_result(
            "当前还有一个未完成的桌面任务，请先确认或取消。".to_string(),
        )));
    }

    let mut task = AgentTaskRun::new_loop(
        TopLevelIntent::DesktopAction,
        trimmed,
        DEFAULT_LOOP_STEP_BUDGET,
        DEFAULT_LOOP_RETRY_BUDGET,
    );
    let result = continue_desktop_loop(
        app,
        provider_config,
        api_key,
        oauth_access_token,
        vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        trimmed,
        &mut task,
    )
    .await?;
    Ok(Some(result))
}

pub async fn maybe_handle_test_message(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    force_route: bool,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let looks_test = force_route || test_agent::intent::looks_like_test_request(trimmed);
    if !looks_test {
        return Ok(None);
    }

    if task_store::has_active_task(app)? {
        return Ok(Some(blocked_result(
            "当前还有一个未完成的任务，请先确认或取消。".to_string(),
        )));
    }

    let mut task = AgentTaskRun::new_loop(
        TopLevelIntent::TestRequest,
        trimmed,
        TEST_LOOP_STEP_BUDGET,
        TEST_LOOP_RETRY_BUDGET,
    );
    let result = continue_test_loop(
        app,
        provider_config,
        api_key,
        oauth_access_token,
        vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        trimmed,
        &mut task,
    )
    .await?;
    Ok(Some(result))
}

pub async fn handle_debug_request(
    app: &AppHandle,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    user_input: &str,
) -> Result<AgentHandleResult, String> {
    let current_task = task_store::current_task(app)?;
    let task_progress = current_task.clone().map(|task| {
        task.progress(
            map_loop_status(&task.task_status),
            task.pending_action_summary.clone(),
            task.failure_reason.clone(),
        )
    });
    let pending = control_router::list_pending(app)
        .map(|items| items.len())
        .unwrap_or_default();
    let recent_failures = testing::history::recent_failed_summary(app).unwrap_or_default();
    let screen = screen_context::describe_current_screen(app, vision_channel, vision_api_key).await;

    let mut lines = vec![format!("我先按调试请求处理这句：{}", user_input.trim())];
    if let Some(task) = current_task {
        lines.push(format!(
            "当前桌面任务：{} / {:?} / 剩余 step budget={}",
            task.task_title, task.task_status, task.step_budget
        ));
        if let Some(reason) = task.failure_reason {
            lines.push(format!("最近失败原因：{reason}"));
        }
    } else {
        lines.push("当前没有进行中的桌面任务。".to_string());
    }
    lines.push(format!("当前待确认动作数：{pending}"));
    lines.push(format!(
        "当前活动窗口：{}",
        screen.active_window.title.trim()
    ));
    if !recent_failures.is_empty() {
        lines.push("最近失败摘要：".to_string());
        for item in recent_failures.iter().take(3) {
            lines.push(format!("- {item}"));
        }
    }

    Ok(AgentHandleResult {
        reply_text: lines.join("\n"),
        provider_label: "Debug Agent".to_string(),
        outcome: "debug_info".to_string(),
        detail: "top_level_intent=debug_request".to_string(),
        meta: AgentMessageMeta {
            route: AgentRoute::Chat,
            planned_tools: vec![],
            pending_request: None,
            task: task_progress,
            summary: None,
        },
    })
}

pub fn handle_memory_request(app: &AppHandle) -> Result<AgentHandleResult, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let input_history = history::get_input_history(app).unwrap_or_default();
    let reply_history = history::get_today_reply_history(app).unwrap_or_default();
    let recent_failures = testing::history::recent_failed_summary(app).unwrap_or_default();

    let mut lines = vec![
        "当前还没有启用可写策略记忆，但本地会持久化保存若干历史数据。".to_string(),
        format!("appData 根目录：{}", app_data.to_string_lossy()),
        format!("输入历史条目：{}", input_history.len()),
        format!("今日回复历史条目：{}", reply_history.len()),
        format!("最近测试失败摘要条目：{}", recent_failures.len()),
        "记忆请求当前只读，不会改写核心策略、权限矩阵或硬边界。".to_string(),
    ];
    if !recent_failures.is_empty() {
        lines.push("最近失败摘要：".to_string());
        for item in recent_failures.iter().take(3) {
            lines.push(format!("- {item}"));
        }
    }

    Ok(AgentHandleResult {
        reply_text: lines.join("\n"),
        provider_label: "Memory Agent".to_string(),
        outcome: "memory_info".to_string(),
        detail: "top_level_intent=memory_request".to_string(),
        meta: AgentMessageMeta {
            route: AgentRoute::Chat,
            planned_tools: vec![],
            pending_request: None,
            task: None,
            summary: None,
        },
    })
}

pub async fn handle_confirmation_response(
    app: &AppHandle,
    user_input: &str,
) -> Result<AgentHandleResult, String> {
    let Some(intent) = parse_confirmation_intent(user_input) else {
        return Ok(AgentHandleResult {
            reply_text: "我识别到了确认语气，但这句还不足以判断是确认还是取消。请直接说“确认”或“取消”。".to_string(),
            provider_label: "Confirmation Agent".to_string(),
            outcome: "confirmation_ambiguous".to_string(),
            detail: "top_level_intent=confirmation_response".to_string(),
            meta: AgentMessageMeta {
                route: AgentRoute::Chat,
                planned_tools: vec![],
                pending_request: None,
                task: None,
                summary: None,
            },
        });
    };

    let pending = control_router::list_pending(app)
        .map_err(|error| error.to_string())?;
    if pending.is_empty() {
        return Ok(AgentHandleResult {
            reply_text: "当前没有待确认动作，所以这次确认/取消不会触发任何执行。".to_string(),
            provider_label: "Confirmation Agent".to_string(),
            outcome: "confirmation_no_pending".to_string(),
            detail: "pending_count=0".to_string(),
            meta: AgentMessageMeta {
                route: AgentRoute::Chat,
                planned_tools: vec![],
                pending_request: None,
                task: None,
                summary: None,
            },
        });
    }

    if pending.len() > 1 {
        return Ok(AgentHandleResult {
            reply_text: format!(
                "当前有 {} 个待确认动作。为了避免误执行，请继续使用界面上的确认条或 /confirm /cancel。",
                pending.len()
            ),
            provider_label: "Confirmation Agent".to_string(),
            outcome: "confirmation_ambiguous_pending".to_string(),
            detail: format!("pending_count={}", pending.len()),
            meta: AgentMessageMeta {
                route: AgentRoute::Chat,
                planned_tools: vec![],
                pending_request: None,
                task: None,
                summary: None,
            },
        });
    }

    let pending_id = pending[0].id.clone();
    let response = match intent {
        ConfirmationIntent::Confirm => {
            if let Some(response) = test_agent::router::confirm_control_pending(app, &pending_id).await? {
                response
            } else {
                confirm_control_pending(app, &pending_id).await?
            }
        }
        ConfirmationIntent::Cancel => {
            if let Some(response) = test_agent::router::cancel_control_pending(app, &pending_id).await? {
                response
            } else {
                cancel_control_pending(app, &pending_id).await?
            }
        }
    };

    Ok(tool_response_to_handle(
        "Confirmation Agent",
        if matches!(intent, ConfirmationIntent::Confirm) {
            "confirmation_confirmed"
        } else {
            "confirmation_cancelled"
        },
        response,
    ))
}

pub async fn confirm_control_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    if let Some(task) = task_store::peek_task_waiting_on_pending(app, pending_id)? {
        if executor::is_loop_task(&task) {
            return confirm_loop_pending(app, pending_id).await;
        }
    }

    control_router::confirm(app, pending_id).map_err(|error| error.to_string())
}

pub async fn cancel_control_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    if let Some(task) = task_store::peek_task_waiting_on_pending(app, pending_id)? {
        if executor::is_loop_task(&task) {
            return cancel_loop_pending(app, pending_id).await;
        }
    }

    control_router::cancel(app, pending_id).map_err(|error| error.to_string())
}

async fn continue_desktop_loop(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    while task.step_budget > 0 {
        task.task_status = AgentLoopTaskStatus::Planning;
        runtime_context::refresh_runtime_context(app, task, vision_channel, vision_api_key.clone()).await;
        task.updated_at = now_millis();

        let decision = match loop_planner::plan_next_action(
            provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            permission_level,
            allowed_actions,
            user_input,
            task,
            &screen_context::describe_current_screen(app, vision_channel, vision_api_key.clone()).await,
        )
        .await
        {
            Ok(decision) => decision,
            Err(primary_error) => {
                let fallback = screen_planner::plan_from_screen_context(
                    provider_config,
                    api_key.clone(),
                    oauth_access_token.clone(),
                    codex_command.clone(),
                    codex_home.clone(),
                    permission_level,
                    allowed_actions,
                    user_input,
                    &screen_context::describe_current_screen(app, vision_channel, vision_api_key.clone()).await,
                )
                .await
                .and_then(|plan| loop_planner::decision_from_plan(&task.goal, plan));

                match fallback {
                    Ok(decision) => decision,
                    Err(fallback_error) => {
                        task.task_status = AgentLoopTaskStatus::Failed;
                        task.failure_reason = Some(fallback_error.clone());
                        task.failure_reason_code = FailureReasonCode::PlannerFailed;
                        task.failure_stage = Some(FailureStage::Planning);
                        return Ok(fail_result(
                            AgentRoute::Control,
                            "Desktop Agent",
                            task,
                            format!(
                                "桌面 agent 没能基于当前上下文生成下一步动作。\n主路径：{primary_error}\nfallback：{fallback_error}"
                            ),
                        ));
                    }
                }
            }
        };

        match decision.next {
            AgentNextAction::RespondToUser { message } => {
                task.task_status = AgentLoopTaskStatus::Completed;
                return Ok(simple_result(
                    AgentRoute::Control,
                    "Desktop Agent",
                    "agent_response",
                    message,
                    task,
                ));
            }
            AgentNextAction::FinishTask { message, summary } => {
                task.task_status = AgentLoopTaskStatus::Completed;
                task.final_summary = Some(summary.clone());
                return Ok(complete_result(AgentRoute::Control, "Desktop Agent", task, message, summary));
            }
            AgentNextAction::FailTask { message, summary } => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(message.clone());
                task.failure_reason_code = summary.failure_reason_code.clone();
                task.failure_stage = summary.failure_stage.clone();
                task.final_summary = Some(summary.clone());
                return Ok(fail_result(AgentRoute::Control, "Desktop Agent", task, message));
            }
            AgentNextAction::ObserveContext { .. }
            | AgentNextAction::AssertCondition { .. }
            | AgentNextAction::RetryStep { .. } => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some("desktop_action loop 收到了测试专用动作。".to_string());
                task.failure_reason_code = FailureReasonCode::InvalidAction;
                task.failure_stage = Some(FailureStage::Planning);
                return Ok(fail_result(
                    AgentRoute::Control,
                    "Desktop Agent",
                    task,
                    "desktop_action loop 收到了测试专用动作，已停止。".to_string(),
                ));
            }
            AgentNextAction::RequestConfirmation {
                tool,
                summary,
                args,
                message,
            } => {
                let action_args = args.clone();
                match execute_tool_step(app, task, &tool, args, summary.clone())? {
                LoopToolExecution::Success => {
                    task.step_budget = task.step_budget.saturating_sub(1);
                    task.task_status = AgentLoopTaskStatus::Observing;
                    if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                        task.completed_notes.push(message);
                    }
                }
                LoopToolExecution::Pending {
                    note,
                    pending_request,
                } => {
                    if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                        task.completed_notes.push(message);
                    }
                    task_store::replace_active_task(app, Some(task.clone()))?;
                    return Ok(pending_result(
                        task,
                        pending_request,
                        note,
                        AgentRoute::Control,
                        "Desktop Agent",
                    ));
                }
                LoopToolExecution::Failure { reason } => {
                    if let Some(result) = maybe_retry_or_fail(
                        task,
                        &tool,
                        &reason,
                        &action_args,
                        AgentRoute::Control,
                        "Desktop Agent",
                    ) {
                        return Ok(result);
                    }
                }
            }
            }
            AgentNextAction::ExecuteTool {
                tool,
                summary,
                args,
            } => {
                let action_args = args.clone();
                match execute_tool_step(app, task, &tool, args, Some(summary.clone()))? {
                LoopToolExecution::Success => {
                    task.step_budget = task.step_budget.saturating_sub(1);
                    task.task_status = AgentLoopTaskStatus::Observing;
                }
                LoopToolExecution::Pending {
                    note,
                    pending_request,
                } => {
                    task_store::replace_active_task(app, Some(task.clone()))?;
                    return Ok(pending_result(
                        task,
                        pending_request,
                        note,
                        AgentRoute::Control,
                        "Desktop Agent",
                    ));
                }
                LoopToolExecution::Failure { reason } => {
                    if let Some(result) = maybe_retry_or_fail(
                        task,
                        &tool,
                        &reason,
                        &action_args,
                        AgentRoute::Control,
                        "Desktop Agent",
                    ) {
                        return Ok(result);
                    }
                }
            }
            }
        }
    }

    task.task_status = AgentLoopTaskStatus::Failed;
    task.failure_reason = Some("当前桌面任务已经耗尽 step budget。".to_string());
    task.failure_reason_code = FailureReasonCode::StepBudgetExceeded;
    task.failure_stage = Some(FailureStage::Planning);
    Ok(fail_result(
        AgentRoute::Control,
        "Desktop Agent",
        task,
        "当前桌面任务已经耗尽 step budget，已停止继续规划。".to_string(),
    ))
}

async fn continue_test_loop(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    while task.step_budget > 0 {
        task.task_status = AgentLoopTaskStatus::Planning;
        let context = runtime_context::refresh_runtime_context(app, task, vision_channel, vision_api_key.clone()).await;
        task.updated_at = now_millis();

        let decision = match test_loop_planner::plan_next_test_action(
            provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            permission_level,
            allowed_actions,
            user_input,
            &context,
        )
        .await
        {
            Ok(decision) => decision,
            Err(error) => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(error.clone());
                task.failure_reason_code = FailureReasonCode::PlannerFailed;
                task.failure_stage = Some(FailureStage::Planning);
                return Ok(fail_result(
                    AgentRoute::Test,
                    "Test Agent",
                    task,
                    format!("测试 agent 没能生成下一步动作：{error}"),
                ));
            }
        };

        match decision.next {
            AgentNextAction::RespondToUser { message } => {
                task.task_status = AgentLoopTaskStatus::Completed;
                return Ok(simple_result(
                    AgentRoute::Test,
                    "Test Agent",
                    "test_response",
                    message,
                    task,
                ));
            }
            AgentNextAction::ObserveContext { summary } => {
                task.task_status = AgentLoopTaskStatus::Observing;
                task.used_probe = true;
                task.step_budget = task.step_budget.saturating_sub(1);
                task.completed_notes.push(summary.clone());
                task.recent_steps.push(super::types::AgentStepRecord {
                    summary: summary.clone(),
                    tool: None,
                    args: None,
                    outcome: "success".to_string(),
                    detail: Some("已刷新 runtime context。".to_string()),
                });
                runtime_context::append_runtime_observation(
                    task,
                    "observe_context",
                    summary,
                    task.runtime_context
                        .as_ref()
                        .and_then(|context| serde_json::to_value(context).ok()),
                );
            }
            AgentNextAction::ExecuteTool { tool, summary, args } => {
                let action_args = args.clone();
                match execute_tool_step(app, task, &tool, args, Some(summary.clone()))? {
                    LoopToolExecution::Success => {
                        task.step_budget = task.step_budget.saturating_sub(1);
                        task.task_status = AgentLoopTaskStatus::Observing;
                    }
                    LoopToolExecution::Pending { note, pending_request } => {
                        task_store::replace_active_task(app, Some(task.clone()))?;
                        return Ok(pending_result(task, pending_request, note, AgentRoute::Test, "Test Agent"));
                    }
                    LoopToolExecution::Failure { reason } => {
                    if let Some(result) = maybe_retry_or_fail(
                        task,
                        &tool,
                        &reason,
                        &action_args,
                        AgentRoute::Test,
                        "Test Agent",
                    ) {
                        return Ok(result);
                    }
                }
                }
            }
            AgentNextAction::RequestConfirmation {
                tool,
                summary,
                args,
                message,
            } => {
                let action_args = args.clone();
                match execute_tool_step(app, task, &tool, args, summary.clone())? {
                    LoopToolExecution::Success => {
                        task.step_budget = task.step_budget.saturating_sub(1);
                        task.task_status = AgentLoopTaskStatus::Observing;
                        if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                            task.completed_notes.push(message);
                        }
                    }
                    LoopToolExecution::Pending { note, pending_request } => {
                        if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                            task.completed_notes.push(message);
                        }
                        task_store::replace_active_task(app, Some(task.clone()))?;
                        return Ok(pending_result(task, pending_request, note, AgentRoute::Test, "Test Agent"));
                    }
                    LoopToolExecution::Failure { reason } => {
                    if let Some(result) = maybe_retry_or_fail(
                        task,
                        &tool,
                        &reason,
                        &action_args,
                        AgentRoute::Test,
                        "Test Agent",
                    ) {
                        return Ok(result);
                    }
                }
                }
            }
            AgentNextAction::AssertCondition {
                assertion_type,
                summary,
                params,
            } => {
                let result = test_assertions::evaluate(
                    &assertion_type,
                    &params,
                    task.runtime_context
                        .as_ref()
                        .ok_or_else(|| "断言执行时缺少 runtime context。".to_string())?,
                    task.pending_action_id.is_some(),
                );
                task.last_tool_result = serde_json::to_value(&result).ok();
                runtime_context::append_runtime_observation(
                    task,
                    "assert_condition",
                    summary.clone(),
                    task.last_tool_result.clone(),
                );
                task.recent_steps.push(super::types::AgentStepRecord {
                    summary: summary.clone(),
                    tool: None,
                    args: Some(params.clone()),
                    outcome: if result.passed { "success" } else { "failure" }.to_string(),
                    detail: Some(
                        serde_json::to_string(&result)
                            .unwrap_or_else(|_| "assertion_result".to_string()),
                    ),
                });
                if result.passed {
                    task.step_budget = task.step_budget.saturating_sub(1);
                    task.task_status = AgentLoopTaskStatus::Observing;
                    task.completed_notes.push(format!("断言通过：{summary}"));
                } else {
                    task.failure_reason = Some(format!("断言失败：{summary}"));
                    task.failure_reason_code = result.failure_reason_code.clone();
                    task.failure_stage = Some(FailureStage::Assertion);
                    if task.retry_budget > 0 {
                        task.retry_budget -= 1;
                        task.used_retry = true;
                        task.task_status = AgentLoopTaskStatus::Retrying;
                        task.completed_notes.push(format!("断言失败，允许一次补测：{summary}"));
                    } else {
                        task.task_status = AgentLoopTaskStatus::Failed;
                        return Ok(fail_result(
                            AgentRoute::Test,
                            "Test Agent",
                            task,
                            format!("断言失败：{summary}"),
                        ));
                    }
                }
            }
            AgentNextAction::RetryStep { target, summary } => {
                match perform_retry_step(app, task, target, summary)? {
                    LoopContinuation::Continue => {}
                    LoopContinuation::Return(result) => return Ok(result),
                }
            }
            AgentNextAction::FinishTask { message, summary } => {
                task.task_status = AgentLoopTaskStatus::Completed;
                task.final_summary = Some(summary.clone());
                return Ok(complete_result(AgentRoute::Test, "Test Agent", task, message, summary));
            }
            AgentNextAction::FailTask { message, summary } => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(message.clone());
                task.failure_reason_code = summary.failure_reason_code.clone();
                task.failure_stage = summary.failure_stage.clone();
                task.final_summary = Some(summary.clone());
                return Ok(fail_result(AgentRoute::Test, "Test Agent", task, message));
            }
        }
    }

    task.task_status = AgentLoopTaskStatus::Failed;
    task.failure_reason = Some("当前测试任务已经耗尽 step budget。".to_string());
    task.failure_reason_code = FailureReasonCode::StepBudgetExceeded;
    task.failure_stage = Some(FailureStage::Planning);
    Ok(fail_result(
        AgentRoute::Test,
        "Test Agent",
        task,
        "当前测试任务已经耗尽 step budget，已停止继续规划。".to_string(),
    ))
}

enum LoopContinuation {
    Continue,
    Return(AgentHandleResult),
}

fn execute_tool_step(
    app: &AppHandle,
    task: &mut AgentTaskRun,
    tool: &str,
    args: Value,
    summary: Option<String>,
) -> Result<LoopToolExecution, String> {
    let materialized_args = if let Some(context) = task.runtime_context.as_ref() {
        runtime_binding::materialize_tool_args(context, tool, &args)?
    } else {
        args
    };
    if task.task_status != AgentLoopTaskStatus::Retrying
        && would_repeat_failed_action(task, tool, &materialized_args)
    {
        return Ok(LoopToolExecution::Failure {
            reason: format!("上一轮已经对 {tool} 执行过相同失败动作，已停止重复尝试。"),
        });
    }

    task.task_status = AgentLoopTaskStatus::Executing;
    if let Some(definition) = control_registry::find_tool_definition(tool) {
        if is_retryable_risk(&definition.risk_level, definition.requires_confirmation) {
            task.last_retryable_tool = Some(tool.to_string());
            task.last_retryable_args = Some(materialized_args.clone());
            task.last_retryable_summary = summary.clone();
            task.last_retryable_risk = Some(definition.risk_level);
        } else {
            task.last_retryable_tool = None;
            task.last_retryable_args = None;
            task.last_retryable_summary = None;
            task.last_retryable_risk = None;
        }
    }
    executor::execute_loop_tool(app, task, tool, materialized_args, summary)
}

fn maybe_retry_or_fail(
    task: &mut AgentTaskRun,
    tool: &str,
    reason: &str,
    _args: &Value,
    route: AgentRoute,
    provider_label: &str,
) -> Option<AgentHandleResult> {
    task.failure_reason = Some(reason.to_string());
    task.failure_reason_code = FailureReasonCode::ToolFailed;
    task.failure_stage = Some(FailureStage::ExecuteTool);
    if task.retry_budget > 0 {
        task.retry_budget -= 1;
        task.used_retry = true;
        task.task_status = AgentLoopTaskStatus::Retrying;
        task.completed_notes
            .push(format!("步骤 {} 失败，准备基于最新观测重试一次。", tool));
        None
    } else {
        task.task_status = AgentLoopTaskStatus::Failed;
        Some(fail_result(route, provider_label, task, reason.to_string()))
    }
}

fn perform_retry_step(
    app: &AppHandle,
    task: &mut AgentTaskRun,
    target: RetryTarget,
    summary: String,
) -> Result<LoopContinuation, String> {
    if task.retry_budget == 0 {
        task.task_status = AgentLoopTaskStatus::Failed;
        task.failure_reason = Some("重试预算已耗尽。".to_string());
        task.failure_reason_code = FailureReasonCode::RetryExhausted;
        task.failure_stage = Some(FailureStage::Retry);
        return Ok(LoopContinuation::Return(fail_result(
            AgentRoute::Test,
            "Test Agent",
            task,
            "当前测试任务已经耗尽 retry budget。".to_string(),
        )));
    }

    match target {
        RetryTarget::ObserveContext => {
            task.retry_budget -= 1;
            task.used_retry = true;
            task.used_probe = true;
            task.task_status = AgentLoopTaskStatus::Observing;
            task.recent_steps.push(super::types::AgentStepRecord {
                summary: summary.clone(),
                tool: None,
                args: None,
                outcome: "success".to_string(),
                detail: Some("已执行一次 observe_context 重试。".to_string()),
            });
            runtime_context::append_runtime_observation(
                task,
                "retry_step",
                summary,
                task.runtime_context
                    .as_ref()
                    .and_then(|context| serde_json::to_value(context).ok()),
            );
            Ok(LoopContinuation::Continue)
        }
        RetryTarget::LastTool => {
            let Some(tool) = task.last_retryable_tool.clone() else {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some("当前没有可重试的低风险动作。".to_string());
                task.failure_reason_code = FailureReasonCode::RetryExhausted;
                task.failure_stage = Some(FailureStage::Retry);
                return Ok(LoopContinuation::Return(fail_result(
                    AgentRoute::Test,
                    "Test Agent",
                    task,
                    "当前没有可重试的低风险动作。".to_string(),
                )));
            };
            let args = task
                .last_retryable_args
                .clone()
                .unwrap_or_else(|| serde_json::json!({}));
            let step_summary = task
                .last_retryable_summary
                .clone()
                .unwrap_or_else(|| summary.clone());
            task.retry_budget -= 1;
            task.used_retry = true;
            match executor::execute_loop_tool(app, task, &tool, args, Some(step_summary))? {
                LoopToolExecution::Success => {
                    task.step_budget = task.step_budget.saturating_sub(1);
                    task.task_status = AgentLoopTaskStatus::Observing;
                    Ok(LoopContinuation::Continue)
                }
                LoopToolExecution::Pending { note, pending_request } => {
                    task_store::replace_active_task(app, Some(task.clone()))?;
                    Ok(LoopContinuation::Return(pending_result(
                        task,
                        pending_request,
                        note,
                        AgentRoute::Test,
                        "Test Agent",
                    )))
                }
                LoopToolExecution::Failure { reason } => {
                    task.task_status = AgentLoopTaskStatus::Failed;
                    task.failure_reason = Some(reason.clone());
                    task.failure_reason_code = FailureReasonCode::RetryExhausted;
                    task.failure_stage = Some(FailureStage::Retry);
                    Ok(LoopContinuation::Return(fail_result(
                        AgentRoute::Test,
                        "Test Agent",
                        task,
                        reason,
                    )))
                }
            }
        }
    }
}

async fn confirm_loop_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    let Some(mut task) = task_store::take_task_waiting_on_pending(app, pending_id)? else {
        return control_router::confirm(app, pending_id).map_err(|error| error.to_string());
    };

    let confirmed = control_router::confirm(app, pending_id).map_err(|error| error.to_string())?;
    let confirmed_result = confirmed.result.clone().unwrap_or_else(|| json!({}));
    let note = task
        .pending_action_summary
        .clone()
        .map(|summary| format!("{summary} 已确认执行。"))
        .unwrap_or_else(|| "高风险动作已确认执行。".to_string());
    if let Some(last) = task.recent_steps.last_mut() {
        if last.outcome == "pending" {
            last.outcome = "success".to_string();
            last.detail = Some(note.clone());
        }
    }
    task.completed_notes.push(note);
    task.last_tool_result = Some(confirmed_result.clone());
    task.completed_results.push(confirmed_result);
    let confirmed_tool = task.recent_steps.last().and_then(|step| step.tool.clone());
    if let Some(tool) = confirmed_tool {
        runtime_context::append_runtime_tool_result(&mut task, &tool, "success", task.last_tool_result.clone());
    }
    task.step_budget = task.step_budget.saturating_sub(1);
    executor::clear_loop_pending(&mut task);
    task.task_status = AgentLoopTaskStatus::Observing;
    let goal = task.goal.clone();

    let (
        provider_config,
        api_key,
        oauth_access_token,
        vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
    ) = runtime_inputs_for_agent(app)?;

    let result = continue_loop_for_task(
        app,
        &provider_config,
        api_key,
        oauth_access_token,
        &vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        permission_level,
        &allowed_actions,
        &goal,
        &mut task,
    )
    .await?;
    Ok(handle_to_tool_response(result))
}

async fn cancel_loop_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    let Some(mut task) = task_store::take_task_waiting_on_pending(app, pending_id)? else {
        return control_router::cancel(app, pending_id).map_err(|error| error.to_string());
    };

    let _ = control_router::cancel(app, pending_id).map_err(|error| error.to_string())?;
    executor::clear_loop_pending(&mut task);
    task.task_status = AgentLoopTaskStatus::Cancelled;
    task.failure_reason = Some("用户取消了当前待确认动作。".to_string());

    Ok(ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(json!({
            "task": task.progress(
                AgentTaskStatus::Cancelled,
                None,
                Some("当前桌面任务已取消。".to_string()),
            ),
        })),
        message: Some(format!("任务“{}”已取消。", task.task_title)),
        pending_request: None,
        error: None,
    })
}

fn runtime_inputs_for_agent(
    app: &AppHandle,
) -> Result<
    (
        ProviderConfig,
        Option<String>,
        Option<String>,
        VisionChannelConfig,
        Option<String>,
        Option<String>,
        Option<String>,
        u8,
        Vec<DesktopAction>,
    ),
    String,
> {
    let state: State<'_, Mutex<RuntimeState>> = app.state();
    let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    let allowed_actions = crate::security::policy::actions_for_level(runtime.permission_level);
    let codex_runtime = crate::codex_runtime::resolve_for_app(app).ok();

    Ok((
        runtime.provider.clone(),
        runtime.api_key.clone(),
        runtime.oauth_access_token.clone(),
        runtime.vision_channel.clone(),
        runtime.vision_api_key.clone(),
        codex_runtime
            .as_ref()
            .and_then(|item| item.command.as_ref())
            .map(|path| path.to_string_lossy().to_string()),
        codex_runtime
            .as_ref()
            .map(|item| item.home_root.to_string_lossy().to_string()),
        runtime.permission_level,
        allowed_actions,
    ))
}

fn parse_confirmation_intent(input: &str) -> Option<ConfirmationIntent> {
    let normalized = input.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if [
        "确认",
        "可以",
        "继续",
        "执行",
        "yes",
        "y",
        "ok",
        "好的",
    ]
    .iter()
    .any(|item| normalized == *item)
    {
        return Some(ConfirmationIntent::Confirm);
    }

    if [
        "取消",
        "不要",
        "停止",
        "no",
        "n",
        "算了",
    ]
    .iter()
    .any(|item| normalized == *item)
    {
        return Some(ConfirmationIntent::Cancel);
    }

    None
}

fn would_repeat_failed_action(task: &AgentTaskRun, tool: &str, args: &Value) -> bool {
    task.recent_steps.last().is_some_and(|step| {
        step.outcome == "failure"
            && step.tool.as_deref() == Some(tool)
            && step.args.as_ref() == Some(args)
    })
}

fn map_loop_status(status: &AgentLoopTaskStatus) -> AgentTaskStatus {
    match status {
        AgentLoopTaskStatus::WaitingConfirmation => AgentTaskStatus::WaitingConfirmation,
        AgentLoopTaskStatus::Completed => AgentTaskStatus::Completed,
        AgentLoopTaskStatus::Failed => AgentTaskStatus::Failed,
        AgentLoopTaskStatus::Cancelled => AgentTaskStatus::Cancelled,
        AgentLoopTaskStatus::Planning
        | AgentLoopTaskStatus::Executing
        | AgentLoopTaskStatus::Observing
        | AgentLoopTaskStatus::Retrying => AgentTaskStatus::Running,
    }
}

fn blocked_result(reason: String) -> AgentHandleResult {
    AgentHandleResult {
        reply_text: format!("这次桌面任务未执行。\n\n原因：{reason}"),
        provider_label: "Desktop Agent".to_string(),
        outcome: "control_blocked".to_string(),
        detail: reason,
        meta: AgentMessageMeta {
            route: AgentRoute::Control,
            planned_tools: vec![],
            pending_request: None,
            task: None,
            summary: None,
        },
    }
}

fn simple_result(
    route: AgentRoute,
    provider_label: &str,
    outcome: &str,
    reply_text: String,
    task: &AgentTaskRun,
) -> AgentHandleResult {
    AgentHandleResult {
        reply_text,
        provider_label: provider_label.to_string(),
        outcome: outcome.to_string(),
        detail: format!("task={} status={:?}", task.task_id, task.task_status),
        meta: AgentMessageMeta {
            route,
            planned_tools: task.planned_tools(),
            pending_request: None,
            task: Some(task.progress(
                map_loop_status(&task.task_status),
                task.pending_action_summary.clone(),
                task.failure_reason.clone(),
            )),
            summary: task.final_summary.clone(),
        },
    }
}

fn complete_result(
    route: AgentRoute,
    provider_label: &str,
    task: &AgentTaskRun,
    message: String,
    summary: AgentLoopSummary,
) -> AgentHandleResult {
    let mut lines = task.completed_notes.clone();
    if !message.trim().is_empty() {
        lines.push(message.clone());
    }
    AgentHandleResult {
        reply_text: lines.join("\n"),
        provider_label: provider_label.to_string(),
        outcome: if matches!(route, AgentRoute::Test) {
            "test_ok".to_string()
        } else {
            "control_ok".to_string()
        },
        detail: format!("task={} status=completed", task.task_id),
        meta: AgentMessageMeta {
            route,
            planned_tools: task.planned_tools(),
            pending_request: None,
            task: Some(task.progress(
                AgentTaskStatus::Completed,
                task.pending_action_summary.clone(),
                Some("任务已完成。".to_string()),
            )),
            summary: Some(summary),
        },
    }
}

fn fail_result(route: AgentRoute, provider_label: &str, task: &AgentTaskRun, reason: String) -> AgentHandleResult {
    let mut lines = task.completed_notes.clone();
    lines.push(format!("任务“{}”已停止。\n原因：{}", task.task_title, reason));
    let summary = task.final_summary.clone().unwrap_or_else(|| AgentLoopSummary {
        goal: task.goal.clone(),
        steps_taken: task.recent_steps.len(),
        final_status: AgentTaskStatus::Failed,
        failure_stage: task.failure_stage.clone(),
        failure_reason_code: task.failure_reason_code.clone(),
        used_probe: task.used_probe,
        used_retry: task.used_retry,
    });
    AgentHandleResult {
        reply_text: lines.join("\n"),
        provider_label: provider_label.to_string(),
        outcome: if matches!(route, AgentRoute::Test) {
            "test_failed".to_string()
        } else {
            "control_failed".to_string()
        },
        detail: reason.clone(),
        meta: AgentMessageMeta {
            route,
            planned_tools: task.planned_tools(),
            pending_request: None,
            task: Some(task.progress(
                AgentTaskStatus::Failed,
                task.pending_action_summary.clone(),
                Some(reason),
            )),
            summary: Some(summary),
        },
    }
}

fn pending_result(
    task: &AgentTaskRun,
    pending_request: crate::control::types::ControlPendingRequest,
    note: String,
    route: AgentRoute,
    provider_label: &str,
) -> AgentHandleResult {
    let mut lines = task.completed_notes.clone();
    lines.push(note);
    lines.push(pending_request.prompt.clone());
    AgentHandleResult {
        reply_text: lines.join("\n"),
        provider_label: provider_label.to_string(),
        outcome: if matches!(route, AgentRoute::Test) {
            "test_pending".to_string()
        } else {
            "control_pending".to_string()
        },
        detail: format!(
            "task={} pending_id={}",
            task.task_id,
            pending_request.id
        ),
        meta: AgentMessageMeta {
            route,
            planned_tools: task.planned_tools(),
            pending_request: Some(pending_request),
            task: Some(task.waiting_progress()),
            summary: task.final_summary.clone(),
        },
    }
}

fn tool_response_to_handle(
    provider_label: &str,
    outcome: &str,
    response: ToolInvokeResponse,
) -> AgentHandleResult {
    let ToolInvokeResponse {
        status,
        result,
        message,
        pending_request,
        ..
    } = response;

    let task = result
        .as_ref()
        .and_then(|value| value.get("task"))
        .cloned()
        .and_then(|value| serde_json::from_value::<AgentTaskProgress>(value).ok());
    let route = result
        .as_ref()
        .and_then(|value| value.get("route"))
        .and_then(Value::as_str)
        .map(|value| match value {
            "test" => AgentRoute::Test,
            "control" => AgentRoute::Control,
            _ => AgentRoute::Control,
        })
        .unwrap_or(AgentRoute::Control);
    AgentHandleResult {
        reply_text: message.unwrap_or_else(|| "动作已处理。".to_string()),
        provider_label: provider_label.to_string(),
        outcome: outcome.to_string(),
        detail: format!("status={status}"),
        meta: AgentMessageMeta {
            route,
            planned_tools: vec![],
            pending_request,
            task,
            summary: None,
        },
    }
}

fn handle_to_tool_response(result: AgentHandleResult) -> ToolInvokeResponse {
    let AgentHandleResult {
        reply_text,
        outcome,
        meta,
        ..
    } = result;
    let AgentMessageMeta {
        route,
        planned_tools,
        pending_request,
        task,
        summary,
        ..
    } = meta;

    ToolInvokeResponse {
        status: if pending_request.is_some() {
            "pending_confirmation".to_string()
        } else if outcome.contains("failed") || outcome.contains("blocked") {
            "error".to_string()
        } else {
            "success".to_string()
        },
        result: Some(json!({
            "route": route,
            "task": task,
            "plannedTools": planned_tools,
            "summary": summary,
        })),
        message: Some(reply_text),
        pending_request,
        error: None,
    }
}

async fn continue_loop_for_task(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    match task.intent {
        TopLevelIntent::DesktopAction => {
            continue_desktop_loop(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                permission_level,
                allowed_actions,
                user_input,
                task,
            )
            .await
        }
        TopLevelIntent::TestRequest => {
            continue_test_loop(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                permission_level,
                allowed_actions,
                user_input,
                task,
            )
            .await
        }
        _ => Err("当前 loop task 的 intent 不支持继续执行。".to_string()),
    }
}

fn is_retryable_risk(
    risk: &crate::control::types::ControlRiskLevel,
    requires_confirmation: bool,
) -> bool {
    !requires_confirmation && !matches!(risk, crate::control::types::ControlRiskLevel::WriteHigh)
}
