use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::{load, now_millis, save, ChatMessage, DesktopAction, ProviderConfig, RuntimeState, VisionChannelConfig},
    control::registry as control_registry,
    control::{router as control_router, types::ToolInvokeResponse},
    history,
    testing,
};

use super::{
    executor::{self, LoopToolExecution},
    intent,
    loop_planner,
    runtime_binding,
    runtime_context,
    screen_context,
    task_store,
    test_assertions,
    test_loop_planner,
    types::{
        AgentLoopSummary, AgentLoopTaskStatus, AgentMessageMeta, AgentNextAction, AgentRoute,
        AgentTaskProgress, AgentTaskRun, AgentTaskStatus, FailureReasonCode, FailureStage,
        RetryTarget, TopLevelIntent,
    },
};

// AI-first: 安全上限，不是主决策因素
const AI_FIRST_STEP_SAFETY_CAP: usize = 50;
const AI_FIRST_RETRY_BUDGET: usize = 3;
const TEST_LOOP_STEP_BUDGET: usize = 12;
const TEST_LOOP_RETRY_BUDGET: usize = 1;

fn render_recent_conversation_context(messages: &[ChatMessage]) -> Option<String> {
    let rendered = messages
        .iter()
        .filter(|message| message.role == "user" || message.role == "assistant")
        .map(|message| {
            let role = if message.role == "user" { "用户" } else { "助手" };
            format!("{role}：{}", message.content.trim())
        })
        .collect::<Vec<_>>();

    if rendered.is_empty() {
        None
    } else {
        Some(format!("## 最近聊天上下文\n{}\n", rendered.join("\n\n")))
    }
}

fn load_recent_conversation_context(app: &AppHandle) -> Option<String> {
    let runtime = load(app).ok()?;
    let start = runtime.messages.len().saturating_sub(12);
    render_recent_conversation_context(&runtime.messages[start..])
}

/// 构建任务的 memory context (用于 prompt 注入)
fn build_memory_context_for_task(
    app: &AppHandle,
    user_input: &str,
    task: &AgentTaskRun,
) -> Option<String> {
    let app_data = app.path().app_data_dir().ok()?;
    let memory_service = crate::memory::MemoryService::new(&app_data);

    // 从 task 中提取窗口信息用于查询
    let window_title = task
        .runtime_context
        .as_ref()
        .and_then(|ctx| ctx.active_window.as_ref())
        .and_then(|w| w.get("title"))
        .and_then(|v| v.as_str());

    let query = crate::memory::service::build_query_from_task(
        user_input,
        Some("desktop_action"),
        window_title,
        None,
    );

    memory_service.render_for_prompt(&query).ok()
}

/// 任务完成/失败后写回记忆
fn write_back_task_memory(app: &AppHandle, task: &AgentTaskRun) {
    let Some(app_data) = app.path().app_data_dir().ok() else {
        return;
    };
    let memory_service = crate::memory::MemoryService::new(&app_data);

    // 从 task 中提取信息构建 write-back request
    let final_status = match task.task_status {
        AgentLoopTaskStatus::Completed => "completed",
        AgentLoopTaskStatus::Failed => "failed",
        AgentLoopTaskStatus::Cancelled => "cancelled",
        _ => "running",
    };

    let failure_reason_code = match task.failure_reason_code {
        FailureReasonCode::None => None,
        ref code => Some(format!("{:?}", code)),
    };
    let failure_stage = task.failure_stage.as_ref().map(|s| format!("{:?}", s));

    let window_title = task
        .runtime_context
        .as_ref()
        .and_then(|ctx| ctx.active_window.as_ref())
        .and_then(|w| w.get("title"))
        .and_then(|v| v.as_str());
    let window_class = task
        .runtime_context
        .as_ref()
        .and_then(|ctx| ctx.active_window.as_ref())
        .and_then(|w| w.get("className"))
        .and_then(|v| v.as_str());

    let used_tools: Vec<String> = task
        .recent_steps
        .iter()
        .filter_map(|step| step.tool.clone())
        .collect();

    let request = crate::memory::service::build_write_back_request(
        &task.task_id,
        &task.goal,
        &format!("{:?}", task.intent),
        final_status,
        failure_reason_code.as_deref(),
        failure_stage.as_deref(),
        window_title,
        window_class,
        used_tools,
        task.used_retry,
        task.used_probe,
        task.recent_steps.len(),
    );

    // 写回记忆 (忽略错误，不影响主流程)
    let _ = memory_service.write_back(request);
}

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
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    conversation_context: Option<&str>,
    force_route: bool,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    // AI-first: 完全依赖上游 lib.rs 的 AI 分类结果 (force_route)
    // 不再使用 looks_like_control_request() 关键词预检
    #[allow(deprecated)]
    let _ = intent::looks_like_control_request(trimmed); // 保留调用以避免 dead_code 警告
    if !force_route {
        return Ok(None);
    }

    if let Some(mut task) = task_store::current_task(app)? {
        if matches!(task.intent, TopLevelIntent::DesktopAction) {
            if task.pending_action_id.is_some() || task.waiting_pending_id.is_some() {
                return Ok(Some(active_task_waiting_result(&task)));
            }

            let result = continue_loop_for_task(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                trimmed,
                &mut task,
            )
            .await?;
            return Ok(Some(result));
        }

        return Ok(Some(blocked_result(
            "当前还有一个未完成的测试任务，请先完成当前任务后再发起新的桌面动作。".to_string(),
        )));
    }

    let mut task = AgentTaskRun::new_loop(
        TopLevelIntent::DesktopAction,
        trimmed,
        AI_FIRST_STEP_SAFETY_CAP,
        AI_FIRST_RETRY_BUDGET,
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
        codex_thread_id,
        permission_level,
        allowed_actions,
        trimmed,
        conversation_context,
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
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    conversation_context: Option<&str>,
    force_route: bool,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let looks_test = force_route || looks_like_test_request(trimmed);
    if !looks_test {
        return Ok(None);
    }

    if let Some(mut task) = task_store::current_task(app)? {
        if matches!(task.intent, TopLevelIntent::TestRequest) {
            if task.pending_action_id.is_some() || task.waiting_pending_id.is_some() {
                return Ok(Some(active_task_waiting_result(&task)));
            }

            let result = continue_loop_for_task(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                trimmed,
                &mut task,
            )
            .await?;
            return Ok(Some(result));
        }

        return Ok(Some(blocked_result(
            "当前还有一个未完成的桌面任务，请先完成当前任务后再发起新的测试请求。".to_string(),
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
        codex_thread_id,
        permission_level,
        allowed_actions,
        trimmed,
        conversation_context,
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

    // 初始化 memory service
    let memory_service = crate::memory::MemoryService::new(&app_data);

    // 加载各类 memory
    let profile = memory_service.load_profile().unwrap_or_default();
    let episodic = memory_service.store().load_episodic().unwrap_or_default();
    let procedural = memory_service.store().load_procedural().unwrap_or_default();
    let policy = memory_service.store().load_policy().unwrap_or_default();

    let input_history = history::get_input_history(app).unwrap_or_default();
    let reply_history = history::get_today_reply_history(app).unwrap_or_default();
    let recent_failures = testing::history::recent_failed_summary(app).unwrap_or_default();

    let mut lines = vec![
        "## 持久化记忆系统 v1 状态".to_string(),
        format!("存储路径：{}/memory/", app_data.to_string_lossy()),
        "".to_string(),
        "### Profile Memory (用户偏好)".to_string(),
        format!("- 常用应用：{} 个", profile.preferred_apps.len()),
        format!("- 常用路径：{} 个", profile.frequently_used_paths.len()),
        format!("- 风险偏好：{}", if profile.risk_preference_low_level_only { "保守" } else { "平衡" }),
        "".to_string(),
        "### Episodic Memory (任务历史)".to_string(),
        format!("- 历史条目：{} 条", episodic.entries.len()),
        "".to_string(),
        "### Procedural Memory (操作模式)".to_string(),
        format!("- 已知路径：{} 条", procedural.procedures.len()),
        "".to_string(),
        "### Policy Memory (软建议)".to_string(),
        format!("- 策略建议：{} 条", policy.suggestions.len()),
        "".to_string(),
        "### 其他历史".to_string(),
        format!("- 输入历史：{} 条", input_history.len()),
        format!("- 今日回复：{} 条", reply_history.len()),
        format!("- 测试失败摘要：{} 条", recent_failures.len()),
        "".to_string(),
        "### 核心安全策略 (不可变)".to_string(),
    ];

    // 添加核心策略摘要
    lines.push(memory_service.get_core_policy_summary());

    if !recent_failures.is_empty() {
        lines.push("".to_string());
        lines.push("### 最近失败摘要".to_string());
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
        ConfirmationIntent::Confirm => confirm_control_pending(app, &pending_id).await?,
        ConfirmationIntent::Cancel => cancel_control_pending(app, &pending_id).await?,
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
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    conversation_context: Option<&str>,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    // 初始化 memory service 并构建 memory context
    let memory_context = build_memory_context_for_task(app, user_input, task);

    loop {
        if task.step_budget == 0 {
            // AI-first: budget 耗尽时注入警告到 completed_notes，让 AI 最终决定
            // 不再自动调用 can_auto_complete_loop_task()
            task.completed_notes.push("⚠️ step budget 已耗尽，请在下一轮输出 finish_task 或 fail_task".to_string());
            break;
        }
        task.task_status = AgentLoopTaskStatus::Planning;
        let context =
            runtime_context::refresh_runtime_context(app, task, vision_channel, vision_api_key.clone())
                .await;
        task.updated_at = now_millis();

        let decision = match loop_planner::plan_next_action(
            provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            codex_thread_id,
            permission_level,
            allowed_actions,
            user_input,
            task,
            &context,
            conversation_context,
            memory_context.as_deref(),
        )
        .await
        {
            Ok(decision) => decision,
            Err(primary_error) => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(primary_error.clone());
                task.failure_reason_code = FailureReasonCode::PlannerFailed;
                task.failure_stage = Some(FailureStage::Planning);
                // Write-back: 记录失败经验
                write_back_task_memory(app, task);
                return Ok(fail_result(
                    AgentRoute::Control,
                    "Desktop Agent",
                    task,
                    format!("桌面 agent 没能基于当前上下文生成下一步动作。\n主路径：{primary_error}"),
                ));
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
                // Write-back: 记录成功经验
                write_back_task_memory(app, task);
                return Ok(complete_result(AgentRoute::Control, "Desktop Agent", task, message, summary));
            }
            AgentNextAction::FailTask { message, summary } => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(message.clone());
                task.failure_reason_code = summary.failure_reason_code.clone();
                task.failure_stage = summary.failure_stage.clone();
                task.final_summary = Some(summary.clone());
                // Write-back: 记录失败经验
                write_back_task_memory(app, task);
                return Ok(fail_result(AgentRoute::Control, "Desktop Agent", task, message));
            }
            AgentNextAction::ObserveContext { summary } => {
                // AI-first: desktop loop 现在支持 observe_context
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
            AgentNextAction::RetryStep { target, summary } => {
                // AI-first: desktop loop 现在支持 retry_step
                match perform_retry_step(app, task, target, summary)? {
                    LoopContinuation::Continue => {}
                    LoopContinuation::Return(result) => return Ok(result),
                }
            }
            AgentNextAction::AssertCondition { .. } => {
                // assert_condition 仅测试循环使用
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some("desktop_action loop 不支持 assert_condition。".to_string());
                task.failure_reason_code = FailureReasonCode::InvalidAction;
                task.failure_stage = Some(FailureStage::Planning);
                return Ok(fail_result(
                    AgentRoute::Control,
                    "Desktop Agent",
                    task,
                    "desktop_action loop 不支持 assert_condition。".to_string(),
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
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    conversation_context: Option<&str>,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    loop {
        if task.step_budget == 0 {
            if can_auto_complete_loop_task(task) {
                task.task_status = AgentLoopTaskStatus::Completed;
                let summary = build_auto_completion_summary(task);
                task.final_summary = Some(summary.clone());
                let message = if task.completed_notes.is_empty() {
                    "测试任务已完成。".to_string()
                } else {
                    task.completed_notes
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "测试任务已完成。".to_string())
                };
                return Ok(complete_result(
                    AgentRoute::Test,
                    "Test Agent",
                    task,
                    message,
                    summary,
                ));
            }
            break;
        }
        task.task_status = AgentLoopTaskStatus::Planning;
        let context = runtime_context::refresh_runtime_context(app, task, vision_channel, vision_api_key.clone()).await;
        task.updated_at = now_millis();

        let decision = match test_loop_planner::plan_next_test_action(
            provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            codex_thread_id,
            permission_level,
            allowed_actions,
            user_input,
            &context,
            conversation_context,
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

fn can_auto_complete_loop_task(task: &AgentTaskRun) -> bool {
    if task.pending_action_id.is_some() || task.waiting_pending_id.is_some() {
        return false;
    }
    if matches!(
        task.task_status,
        AgentLoopTaskStatus::Failed | AgentLoopTaskStatus::Cancelled | AgentLoopTaskStatus::WaitingConfirmation
    ) {
        return false;
    }
    if matches!(
        task.failure_reason_code,
        FailureReasonCode::PlannerFailed
            | FailureReasonCode::ContextUnavailable
            | FailureReasonCode::ToolFailed
            | FailureReasonCode::AssertionFailed
            | FailureReasonCode::ConfirmationRejected
            | FailureReasonCode::RetryExhausted
            | FailureReasonCode::PolicyBlocked
            | FailureReasonCode::InvalidAction
            | FailureReasonCode::FileMissing
    ) {
        return false;
    }
    // 强化检查：必须有至少一个成功的工具执行步骤
    // 避免只做了无关 observe_context 就被标记为 completed
    task.recent_steps.iter().any(|step| {
        step.outcome == "success" && step.tool.is_some()
    })
}

fn build_auto_completion_summary(task: &AgentTaskRun) -> AgentLoopSummary {
    AgentLoopSummary {
        goal: task.goal.clone(),
        steps_taken: task.recent_steps.len(),
        final_status: AgentTaskStatus::Completed,
        failure_stage: None,
        failure_reason_code: FailureReasonCode::None,
        used_probe: task.used_probe,
        used_retry: task.used_retry,
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
    let confirmed_payload = task.last_tool_result.clone();
    if let Some(tool) = confirmed_tool {
        runtime_context::append_runtime_tool_result(&mut task, &tool, "success", confirmed_payload);
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
    let mut codex_thread_id = load(app)
        .ok()
        .and_then(|runtime| runtime.codex_thread_id);

    let result = continue_loop_for_task(
        app,
        &provider_config,
        api_key,
        oauth_access_token,
        &vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        &mut codex_thread_id,
        permission_level,
        &allowed_actions,
        &goal,
        &mut task,
    )
    .await?;
    if let Some(thread_id) = codex_thread_id {
        let state: State<'_, Mutex<RuntimeState>> = app.state();
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.codex_thread_id = Some(thread_id);
        save(app, &runtime)?;
    }
    Ok(handle_to_tool_response(result))
}

async fn cancel_loop_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    let Some(mut task) = task_store::take_task_waiting_on_pending(app, pending_id)? else {
        return control_router::cancel(app, pending_id).map_err(|error| error.to_string());
    };

    let _ = control_router::cancel(app, pending_id).map_err(|error| error.to_string())?;

    // Write-back: 记录确认被拒绝的经验
    if let Some(ref tool) = task.last_retryable_tool {
        let window_title = task
            .runtime_context
            .as_ref()
            .and_then(|ctx| ctx.active_window.as_ref())
            .and_then(|w| w.get("title"))
            .and_then(|v| v.as_str());

        if let Ok(app_data) = app.path().app_data_dir() {
            let memory_service = crate::memory::MemoryService::new(&app_data);
            let _ = memory_service.write_confirmation_rejected(
                &task.goal,
                tool,
                window_title,
            );
        }
    }

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
        message: Some(format!("任务 \"{}\" 已取消。", task.task_title)),
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

fn active_task_waiting_result(task: &AgentTaskRun) -> AgentHandleResult {
    let route = if matches!(task.intent, TopLevelIntent::TestRequest) {
        AgentRoute::Test
    } else {
        AgentRoute::Control
    };
    let provider_label = if matches!(route, AgentRoute::Test) {
        "Test Agent"
    } else {
        "Desktop Agent"
    };
    let pending_summary = task
        .pending_action_summary
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "当前任务正在等待一个待确认动作。".to_string());
    let reply_text = format!(
        "当前任务还没有结束。\n\n正在等待确认：{pending_summary}\n请直接回复“确认”或“取消”；如果你只是想了解当前卡在哪，也可以继续问我。"
    );

    AgentHandleResult {
        reply_text,
        provider_label: provider_label.to_string(),
        outcome: if matches!(route, AgentRoute::Test) {
            "test_pending".to_string()
        } else {
            "control_pending".to_string()
        },
        detail: format!(
            "task={} waiting_pending={}",
            task.task_id,
            task.waiting_pending_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        ),
        meta: AgentMessageMeta {
            route,
            planned_tools: task.planned_tools(),
            pending_request: None,
            task: Some(task.waiting_progress()),
            summary: task.final_summary.clone(),
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
    lines.push(format!("任务 \"{}\" 已停止。\n原因：{}", task.task_title, reason));
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
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    match task.intent {
        TopLevelIntent::DesktopAction => {
            let conversation_context = load_recent_conversation_context(app);
            continue_desktop_loop(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                user_input,
                conversation_context.as_deref(),
                task,
            )
            .await
        }
        TopLevelIntent::TestRequest => {
            let conversation_context = load_recent_conversation_context(app);
            continue_test_loop(
                app,
                provider_config,
                api_key,
                oauth_access_token,
                vision_channel,
                vision_api_key,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                user_input,
                conversation_context.as_deref(),
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

fn looks_like_test_request(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    [
        "测试",
        "验证",
        "测一下",
        "帮我测",
        "回归",
        "重测",
        "retest",
        "smoke",
    ]
    .iter()
    .any(|token| trimmed.contains(token))
}
