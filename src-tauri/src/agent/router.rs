use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::{load, now_millis, save, ChatMessage, DesktopAction, ProviderConfig, RuntimeState, VisionChannelConfig},
    control::registry as control_registry,
    control::{router as control_router, types::ToolInvokeResponse},
    history,
    testing,
};

use super::{
    agent_turn::AgentExecutionDomain,
    domain_loop_planner,
    executor::{self, LoopToolExecution},
    intent,
    runtime_binding,
    runtime_context,
    screen_context,
    task_store,
    test_assertions,
    workspace_context,
    types::{
        AgentAction, AgentActionPayload, AgentLoopSummary, AgentLoopTaskStatus,
        AgentMessageMeta, AgentRoute, AgentTaskProgress, AgentTaskRun, AgentTaskStatus,
        FailureReasonCode, FailureStage, RetryTarget, RuntimeContext, TopLevelIntent,
    },
};

// AI-first: 安全上限，不是主决策因素
const AI_FIRST_STEP_SAFETY_CAP: usize = 50;
const AI_FIRST_RETRY_BUDGET: usize = 3;
const TEST_LOOP_STEP_BUDGET: usize = 12;
const TEST_LOOP_RETRY_BUDGET: usize = 1;
const WORKSPACE_LOOP_STEP_BUDGET: usize = 24;
const WORKSPACE_LOOP_RETRY_BUDGET: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainLoopActionKind {
    RespondToUser,
    ObserveContext,
    AssertCondition,
    RequestConfirmation,
    ExecuteTool,
    RetryStep,
    FinishTask,
    FailTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainRuntimeContextMode {
    RefreshEachStep,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainMemoryContextMode {
    TaskMemory,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainWorkspaceDefaultsMode {
    DetectWorkspace,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainObservationPolicy {
    RuntimeContextSnapshot,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainAssertionPolicy {
    TestAssertion,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainToolArgsPolicy {
    WorkspaceDefaults,
    None,
}

#[derive(Clone, Copy)]
struct DomainPersistencePolicy {
    on_reply: bool,
    on_terminal: bool,
}

#[derive(Clone, Copy)]
struct DomainLoopSpec {
    domain: AgentExecutionDomain,
    route: AgentRoute,
    provider_label: &'static str,
    response_outcome: &'static str,
    planner_error_prefix: &'static str,
    budget_exhausted_reason: &'static str,
    budget_exhausted_message: &'static str,
    budget_exhausted_warning: Option<&'static str>,
    runtime_context_mode: DomainRuntimeContextMode,
    memory_context_mode: DomainMemoryContextMode,
    workspace_defaults_mode: DomainWorkspaceDefaultsMode,
    observation_policy: DomainObservationPolicy,
    assertion_policy: DomainAssertionPolicy,
    tool_args_policy: DomainToolArgsPolicy,
    allowed_actions: &'static [DomainLoopActionKind],
    persistence: DomainPersistencePolicy,
    auto_complete_on_budget_exhausted: bool,
}

struct DomainLoopStaticContext {
    memory_context: Option<String>,
    workspace_prompt: Option<String>,
    default_workdir: Option<String>,
    workspace_review_policy: Option<WorkspaceReviewPolicy>,
}

#[derive(Clone)]
struct WorkspaceReviewBootstrapFile {
    path: String,
    summary: String,
}

#[derive(Clone)]
struct WorkspaceReviewPolicy {
    bootstrap_files: Vec<WorkspaceReviewBootstrapFile>,
}

enum DomainLoopControl {
    Continue,
    Break,
    Return(AgentHandleResult),
}

const DESKTOP_ALLOWED_ACTIONS: &[DomainLoopActionKind] = &[
    DomainLoopActionKind::RespondToUser,
    DomainLoopActionKind::ObserveContext,
    DomainLoopActionKind::RequestConfirmation,
    DomainLoopActionKind::ExecuteTool,
    DomainLoopActionKind::RetryStep,
    DomainLoopActionKind::FinishTask,
    DomainLoopActionKind::FailTask,
];

const TEST_ALLOWED_ACTIONS: &[DomainLoopActionKind] = &[
    DomainLoopActionKind::RespondToUser,
    DomainLoopActionKind::ObserveContext,
    DomainLoopActionKind::AssertCondition,
    DomainLoopActionKind::RequestConfirmation,
    DomainLoopActionKind::ExecuteTool,
    DomainLoopActionKind::RetryStep,
    DomainLoopActionKind::FinishTask,
    DomainLoopActionKind::FailTask,
];

const WORKSPACE_ALLOWED_ACTIONS: &[DomainLoopActionKind] = &[
    DomainLoopActionKind::RespondToUser,
    DomainLoopActionKind::RequestConfirmation,
    DomainLoopActionKind::ExecuteTool,
    DomainLoopActionKind::RetryStep,
    DomainLoopActionKind::FinishTask,
    DomainLoopActionKind::FailTask,
];

const WORKSPACE_REVIEW_MAINLINE_FILES: &[(&str, &str)] = &[
    ("src-tauri/src/lib.rs", "读取主入口 lib.rs，确认当前统一 agent 主链"),
    (
        "src-tauri/src/agent/agent_turn.rs",
        "读取 agent_turn.rs，确认顶层统一回合决策协议",
    ),
    (
        "src-tauri/src/agent/router.rs",
        "读取 router.rs，确认当前生效的执行路由与统一执行器",
    ),
];

const WORKSPACE_REVIEW_SUPPORT_FILES: &[(&str, &str)] = &[
    ("README.md", "读取 README.md，确认仓库目标与运行方式"),
    ("package.json", "读取 package.json，确认前端脚本与构建入口"),
    (
        "src-tauri/src/agent/model_adapter.rs",
        "读取 model_adapter.rs，确认跨 provider 结构化输出适配层",
    ),
    (
        "src-tauri/src/ai/provider.rs",
        "读取 provider.rs，确认各 provider 的调用与恢复逻辑",
    ),
];

fn domain_loop_spec(domain: AgentExecutionDomain) -> DomainLoopSpec {
    match domain {
        AgentExecutionDomain::Desktop => DomainLoopSpec {
            domain,
            route: AgentRoute::Control,
            provider_label: "Desktop Agent",
            response_outcome: "agent_response",
            planner_error_prefix: "桌面 agent 没能基于当前上下文生成下一步动作。",
            budget_exhausted_reason: "当前桌面任务已经耗尽 step budget。",
            budget_exhausted_message: "当前桌面任务已经耗尽 step budget，已停止继续规划。",
            budget_exhausted_warning: Some("⚠️ step budget 已耗尽，请在下一轮输出 finish_task 或 fail_task"),
            runtime_context_mode: DomainRuntimeContextMode::RefreshEachStep,
            memory_context_mode: DomainMemoryContextMode::TaskMemory,
            workspace_defaults_mode: DomainWorkspaceDefaultsMode::None,
            observation_policy: DomainObservationPolicy::RuntimeContextSnapshot,
            assertion_policy: DomainAssertionPolicy::Unsupported,
            tool_args_policy: DomainToolArgsPolicy::None,
            allowed_actions: DESKTOP_ALLOWED_ACTIONS,
            persistence: DomainPersistencePolicy {
                on_reply: false,
                on_terminal: true,
            },
            auto_complete_on_budget_exhausted: false,
        },
        AgentExecutionDomain::Test => DomainLoopSpec {
            domain,
            route: AgentRoute::Test,
            provider_label: "Test Agent",
            response_outcome: "test_response",
            planner_error_prefix: "测试 agent 没能生成下一步动作：",
            budget_exhausted_reason: "当前测试任务已经耗尽 step budget。",
            budget_exhausted_message: "当前测试任务已经耗尽 step budget，已停止继续规划。",
            budget_exhausted_warning: None,
            runtime_context_mode: DomainRuntimeContextMode::RefreshEachStep,
            memory_context_mode: DomainMemoryContextMode::None,
            workspace_defaults_mode: DomainWorkspaceDefaultsMode::None,
            observation_policy: DomainObservationPolicy::RuntimeContextSnapshot,
            assertion_policy: DomainAssertionPolicy::TestAssertion,
            tool_args_policy: DomainToolArgsPolicy::None,
            allowed_actions: TEST_ALLOWED_ACTIONS,
            persistence: DomainPersistencePolicy {
                on_reply: false,
                on_terminal: false,
            },
            auto_complete_on_budget_exhausted: true,
        },
        AgentExecutionDomain::Workspace => DomainLoopSpec {
            domain,
            route: AgentRoute::Workspace,
            provider_label: "Workspace Agent",
            response_outcome: "workspace_response",
            planner_error_prefix: "workspace agent 没能生成下一步动作：",
            budget_exhausted_reason: "当前工作区任务已经耗尽 step budget。",
            budget_exhausted_message: "当前工作区任务已经耗尽 step budget，已停止继续规划。",
            budget_exhausted_warning: None,
            runtime_context_mode: DomainRuntimeContextMode::None,
            memory_context_mode: DomainMemoryContextMode::TaskMemory,
            workspace_defaults_mode: DomainWorkspaceDefaultsMode::DetectWorkspace,
            observation_policy: DomainObservationPolicy::Unsupported,
            assertion_policy: DomainAssertionPolicy::Unsupported,
            tool_args_policy: DomainToolArgsPolicy::WorkspaceDefaults,
            allowed_actions: WORKSPACE_ALLOWED_ACTIONS,
            persistence: DomainPersistencePolicy {
                on_reply: true,
                on_terminal: true,
            },
            auto_complete_on_budget_exhausted: false,
        },
        AgentExecutionDomain::Memory => DomainLoopSpec {
            domain,
            route: AgentRoute::Chat,
            provider_label: "Memory Agent",
            response_outcome: "memory_response",
            planner_error_prefix: "memory domain 不通过 loop 执行：",
            budget_exhausted_reason: "当前记忆查询任务已经耗尽 step budget。",
            budget_exhausted_message: "当前记忆查询任务已经耗尽 step budget。",
            budget_exhausted_warning: None,
            runtime_context_mode: DomainRuntimeContextMode::None,
            memory_context_mode: DomainMemoryContextMode::None,
            workspace_defaults_mode: DomainWorkspaceDefaultsMode::None,
            observation_policy: DomainObservationPolicy::Unsupported,
            assertion_policy: DomainAssertionPolicy::Unsupported,
            tool_args_policy: DomainToolArgsPolicy::None,
            allowed_actions: &[],
            persistence: DomainPersistencePolicy {
                on_reply: false,
                on_terminal: false,
            },
            auto_complete_on_budget_exhausted: false,
        },
    }
}

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
        Some(match task.intent {
            TopLevelIntent::DesktopAction => "desktop_action",
            TopLevelIntent::TestRequest => "test_request",
            TopLevelIntent::WorkspaceTask => "workspace_task",
            _ => "chat",
        }),
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

fn merge_assistant_preface(preface: Option<&str>, reply_text: String) -> String {
    let prefix = preface
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match prefix {
        Some(prefix) if !reply_text.trim().is_empty() => format!("{prefix}\n{reply_text}"),
        Some(prefix) => prefix.to_string(),
        None => reply_text,
    }
}

fn domain_to_intent(domain: AgentExecutionDomain) -> TopLevelIntent {
    match domain {
        AgentExecutionDomain::Desktop => TopLevelIntent::DesktopAction,
        AgentExecutionDomain::Test => TopLevelIntent::TestRequest,
        AgentExecutionDomain::Workspace => TopLevelIntent::WorkspaceTask,
        AgentExecutionDomain::Memory => TopLevelIntent::MemoryRequest,
    }
}

fn intent_to_domain(intent: TopLevelIntent) -> Option<AgentExecutionDomain> {
    match intent {
        TopLevelIntent::DesktopAction => Some(AgentExecutionDomain::Desktop),
        TopLevelIntent::TestRequest => Some(AgentExecutionDomain::Test),
        TopLevelIntent::WorkspaceTask => Some(AgentExecutionDomain::Workspace),
        TopLevelIntent::MemoryRequest => Some(AgentExecutionDomain::Memory),
        _ => None,
    }
}

fn domain_to_route(domain: AgentExecutionDomain) -> AgentRoute {
    match domain {
        AgentExecutionDomain::Desktop => AgentRoute::Control,
        AgentExecutionDomain::Test => AgentRoute::Test,
        AgentExecutionDomain::Workspace => AgentRoute::Workspace,
        AgentExecutionDomain::Memory => AgentRoute::Chat,
    }
}

fn domain_loop_budget(domain: AgentExecutionDomain) -> (usize, usize) {
    match domain {
        AgentExecutionDomain::Desktop => (AI_FIRST_STEP_SAFETY_CAP, AI_FIRST_RETRY_BUDGET),
        AgentExecutionDomain::Test => (TEST_LOOP_STEP_BUDGET, TEST_LOOP_RETRY_BUDGET),
        AgentExecutionDomain::Workspace => (WORKSPACE_LOOP_STEP_BUDGET, WORKSPACE_LOOP_RETRY_BUDGET),
        AgentExecutionDomain::Memory => (1, 0),
    }
}

fn blocked_message_for_domain(active_task: &AgentTaskRun, target_domain: AgentExecutionDomain) -> String {
    let target_label = match target_domain {
        AgentExecutionDomain::Desktop => "桌面动作",
        AgentExecutionDomain::Test => "测试任务",
        AgentExecutionDomain::Workspace => "工作区任务",
        AgentExecutionDomain::Memory => "记忆查询",
    };
    format!(
        "当前还有一个未完成的{}，请先完成当前任务后再发起新的{}。",
        task_kind_label(active_task.intent.clone()),
        target_label
    )
}

fn domain_action_kind(next: &AgentActionPayload) -> DomainLoopActionKind {
    match next.action {
        AgentAction::Respond => DomainLoopActionKind::RespondToUser,
        AgentAction::Observe => DomainLoopActionKind::ObserveContext,
        AgentAction::Assert => DomainLoopActionKind::AssertCondition,
        AgentAction::Confirm => DomainLoopActionKind::RequestConfirmation,
        AgentAction::Tool => DomainLoopActionKind::ExecuteTool,
        AgentAction::Retry => DomainLoopActionKind::RetryStep,
        AgentAction::Finish => DomainLoopActionKind::FinishTask,
        AgentAction::Fail => DomainLoopActionKind::FailTask,
    }
}

fn domain_supports_action(spec: DomainLoopSpec, action: DomainLoopActionKind) -> bool {
    spec.allowed_actions.contains(&action)
}

fn domain_action_label(action: DomainLoopActionKind) -> &'static str {
    match action {
        DomainLoopActionKind::RespondToUser => "respond_to_user",
        DomainLoopActionKind::ObserveContext => "observe_context",
        DomainLoopActionKind::AssertCondition => "assert_condition",
        DomainLoopActionKind::RequestConfirmation => "request_confirmation",
        DomainLoopActionKind::ExecuteTool => "execute_tool",
        DomainLoopActionKind::RetryStep => "retry_step",
        DomainLoopActionKind::FinishTask => "finish_task",
        DomainLoopActionKind::FailTask => "fail_task",
    }
}

async fn continue_domain_loop(
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
    domain: AgentExecutionDomain,
    task: &mut AgentTaskRun,
) -> Result<AgentHandleResult, String> {
    if matches!(domain, AgentExecutionDomain::Memory) {
        return Err("memory domain 不通过 loop 执行。".to_string());
    }

    continue_domain_loop_generic(
        app,
        domain_loop_spec(domain),
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
        conversation_context,
        task,
    )
    .await
}

async fn maybe_handle_domain_message(
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
    domain: AgentExecutionDomain,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if !force_route {
        return Ok(None);
    }

    let intent = domain_to_intent(domain);
    let route = domain_to_route(domain);

    if let Some(mut task) = task_store::current_task(app)? {
        if matches!(task.intent, TopLevelIntent::DesktopAction)
            && matches!(intent, TopLevelIntent::DesktopAction)
            || matches!(task.intent, TopLevelIntent::TestRequest)
                && matches!(intent, TopLevelIntent::TestRequest)
            || matches!(task.intent, TopLevelIntent::WorkspaceTask)
                && matches!(intent, TopLevelIntent::WorkspaceTask)
        {
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

        return Ok(Some(blocked_result_for_route(
            route,
            blocked_message_for_domain(&task, domain),
        )));
    }

    let (step_budget, retry_budget) = domain_loop_budget(domain);
    let mut task = AgentTaskRun::new_loop(intent, trimmed, step_budget, retry_budget);
    let result = continue_domain_loop(
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
        domain,
        &mut task,
    )
    .await?;
    Ok(Some(result))
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
    // AI-first: 完全依赖上游 lib.rs 的 AI 分类结果 (force_route)
    // 不再使用 looks_like_control_request() 关键词预检
    #[allow(deprecated)]
    let _ = intent::looks_like_control_request(user_input.trim()); // 保留调用以避免 dead_code 警告
    maybe_handle_domain_message(
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
        conversation_context,
        force_route,
        AgentExecutionDomain::Desktop,
    )
    .await
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
    let looks_test = force_route || looks_like_test_request(user_input.trim());
    if !looks_test {
        return Ok(None);
    }
    maybe_handle_domain_message(
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
        conversation_context,
        true,
        AgentExecutionDomain::Test,
    )
    .await
}

pub async fn maybe_handle_workspace_message(
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
    maybe_handle_domain_message(
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
        conversation_context,
        force_route,
        AgentExecutionDomain::Workspace,
    )
    .await
}

pub async fn execute_agent_turn_domain(
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
    domain: AgentExecutionDomain,
    assistant_message: Option<&str>,
) -> Result<AgentHandleResult, String> {
    let result = match domain {
        AgentExecutionDomain::Desktop => maybe_handle_control_message(
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
            conversation_context,
            true,
        )
        .await?
        .ok_or_else(|| "桌面 agent 没有返回结果。".to_string())?,
        AgentExecutionDomain::Test => maybe_handle_test_message(
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
            conversation_context,
            true,
        )
        .await?
        .ok_or_else(|| "测试 agent 没有返回结果。".to_string())?,
        AgentExecutionDomain::Workspace => maybe_handle_workspace_message(
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
            conversation_context,
            true,
        )
        .await?
        .ok_or_else(|| "workspace agent 没有返回结果。".to_string())?,
        AgentExecutionDomain::Memory => handle_memory_request(app)?,
    };

    Ok(AgentHandleResult {
        reply_text: merge_assistant_preface(assistant_message, result.reply_text),
        provider_label: result.provider_label,
        outcome: result.outcome,
        detail: result.detail,
        meta: result.meta,
    })
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
    let semantic = memory_service.load_semantic().unwrap_or_default();
    let meta = memory_service.load_meta().unwrap_or_default();

    let input_history = history::get_input_history(app).unwrap_or_default();
    let reply_history = history::get_today_reply_history(app).unwrap_or_default();
    let recent_failures = testing::history::recent_failed_summary(app).unwrap_or_default();
    let stable_semantic_count = semantic
        .entries
        .iter()
        .filter(|entry| entry.explicit || entry.mention_count >= 2)
        .count();
    let candidate_semantic_count = semantic.entries.len().saturating_sub(stable_semantic_count);

    let mut lines = vec![
        "## 持久化记忆系统 v2 状态".to_string(),
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
        "### Semantic Memory (语义知识)".to_string(),
        format!("- 语义条目：{} 条", semantic.entries.len()),
        format!("- 稳定长期记忆：{} 条", stable_semantic_count),
        format!("- 候选记忆：{} 条", candidate_semantic_count),
        "".to_string(),
        "### Meta Memory (记忆偏好)".to_string(),
        format!("- 偏好条目：{} 条", meta.preferences.len()),
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

async fn continue_domain_loop_generic(
    app: &AppHandle,
    spec: DomainLoopSpec,
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
    let static_context = build_domain_loop_static_context(spec, app, user_input, task);

    loop {
        match handle_domain_budget_boundary(spec, app, task) {
            DomainLoopControl::Continue => {}
            DomainLoopControl::Break => break,
            DomainLoopControl::Return(result) => return Ok(result),
        }

        if let Some(control) =
            maybe_run_workspace_review_bootstrap(app, spec, &static_context, task)?
        {
            match control {
                DomainLoopControl::Continue => continue,
                DomainLoopControl::Break => break,
                DomainLoopControl::Return(result) => return Ok(result),
            }
        }

        task.task_status = AgentLoopTaskStatus::Planning;
        let runtime_context = refresh_domain_runtime_context(
            spec,
            app,
            task,
            vision_channel,
            vision_api_key.clone(),
        )
        .await;
        task.updated_at = now_millis();

        let decision = match domain_loop_planner::plan_next_domain_action(
            spec.domain,
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
            runtime_context.as_ref(),
            conversation_context,
            static_context.memory_context.as_deref(),
            static_context.workspace_prompt.as_deref(),
            static_context.default_workdir.as_deref(),
        )
        .await
        {
            Ok(decision) => decision,
            Err(error) => {
                task.task_status = AgentLoopTaskStatus::Failed;
                task.failure_reason = Some(error.clone());
                task.failure_reason_code = FailureReasonCode::PlannerFailed;
                task.failure_stage = Some(FailureStage::Planning);
                persist_domain_memory(spec, app, task, false);
                return Ok(fail_result(
                    spec.route,
                    spec.provider_label,
                    task,
                    format_planner_error(spec, &error),
                ));
            }
        };

        match handle_domain_next_action(
            app,
            spec,
            &static_context,
            task,
            decision.next,
        )? {
            DomainLoopControl::Continue => {}
            DomainLoopControl::Break => break,
            DomainLoopControl::Return(result) => return Ok(result),
        }
    }

    task.task_status = AgentLoopTaskStatus::Failed;
    task.failure_reason = Some(spec.budget_exhausted_reason.to_string());
    task.failure_reason_code = FailureReasonCode::StepBudgetExceeded;
    task.failure_stage = Some(FailureStage::Planning);
    persist_domain_memory(spec, app, task, false);
    Ok(fail_result(
        spec.route,
        spec.provider_label,
        task,
        spec.budget_exhausted_message.to_string(),
    ))
}

fn build_domain_loop_static_context(
    spec: DomainLoopSpec,
    app: &AppHandle,
    user_input: &str,
    task: &AgentTaskRun,
) -> DomainLoopStaticContext {
    let memory_context = if matches!(spec.memory_context_mode, DomainMemoryContextMode::TaskMemory) {
        build_memory_context_for_task(app, user_input, task)
    } else {
        None
    };

    let (workspace_prompt, default_workdir, workspace_review_policy) = if matches!(
        spec.workspace_defaults_mode,
        DomainWorkspaceDefaultsMode::DetectWorkspace
    ) {
        let configured_workspace_root = load(app).ok().and_then(|runtime| runtime.workspace_root);
        let workspace = workspace_context::detect_workspace_context(configured_workspace_root.as_deref());
        let review_policy = workspace
            .as_ref()
            .and_then(|item| build_workspace_review_policy(item.default_workdir(), user_input, task));
        let prompt = workspace.as_ref().map(|item| {
            let mut rendered = item.render_for_prompt();
            if let Some(policy) = review_policy.as_ref() {
                rendered.push('\n');
                rendered.push_str(&render_workspace_review_policy(policy));
            }
            rendered
        });
        let workdir = workspace
            .as_ref()
            .map(|item| item.default_workdir().to_string_lossy().to_string())
            .or_else(|| Some(".".to_string()));
        (prompt, workdir, review_policy)
    } else {
        (None, None, None)
    };

    DomainLoopStaticContext {
        memory_context,
        workspace_prompt,
        default_workdir,
        workspace_review_policy,
    }
}

fn build_workspace_review_policy(
    workspace_root: &Path,
    user_input: &str,
    task: &AgentTaskRun,
) -> Option<WorkspaceReviewPolicy> {
    if !is_workspace_review_request(user_input, task) {
        return None;
    }

    let mut seen = HashSet::new();
    let mut bootstrap_files = Vec::new();

    if is_project_overview_request(user_input, task) {
        for (relative_path, summary) in WORKSPACE_REVIEW_SUPPORT_FILES
            .iter()
            .filter(|(path, _)| matches!(*path, "README.md" | "package.json"))
        {
            push_workspace_review_file(
                &mut bootstrap_files,
                &mut seen,
                workspace_root,
                relative_path,
                summary,
            );
        }
    }

    for (relative_path, summary) in WORKSPACE_REVIEW_MAINLINE_FILES {
        if input_mentions_workspace_file(user_input, task, relative_path) {
            push_workspace_review_file(
                &mut bootstrap_files,
                &mut seen,
                workspace_root,
                relative_path,
                summary,
            );
        }
    }

    for (relative_path, summary) in WORKSPACE_REVIEW_MAINLINE_FILES {
        push_workspace_review_file(
            &mut bootstrap_files,
            &mut seen,
            workspace_root,
            relative_path,
            summary,
        );
    }

    if is_provider_alignment_request(user_input, task) {
        for (relative_path, summary) in WORKSPACE_REVIEW_SUPPORT_FILES
            .iter()
            .filter(|(path, _)| {
                matches!(
                    *path,
                    "src-tauri/src/agent/model_adapter.rs" | "src-tauri/src/ai/provider.rs"
                )
            })
        {
            push_workspace_review_file(
                &mut bootstrap_files,
                &mut seen,
                workspace_root,
                relative_path,
                summary,
            );
        }
    }

    if bootstrap_files.is_empty() {
        None
    } else {
        Some(WorkspaceReviewPolicy { bootstrap_files })
    }
}

fn render_workspace_review_policy(policy: &WorkspaceReviewPolicy) -> String {
    let lines = policy
        .bootstrap_files
        .iter()
        .enumerate()
        .map(|(index, file)| format!("{}. {} ({})", index + 1, file.path, file.summary))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "## 代码审查 Bootstrap\n\
- 当前任务命中主链优先审查模式。\n\
- 在输出整体架构/风险结论前，必须先读取这些文件：\n{lines}\n\
- legacy / deprecated / fallback 文件只有在主入口或当前调用链明确引用后，才允许被当成当前实现的一部分讨论。\n"
    )
}

fn maybe_run_workspace_review_bootstrap(
    app: &AppHandle,
    spec: DomainLoopSpec,
    static_context: &DomainLoopStaticContext,
    task: &mut AgentTaskRun,
) -> Result<Option<DomainLoopControl>, String> {
    if !matches!(spec.domain, AgentExecutionDomain::Workspace) {
        return Ok(None);
    }

    let Some(policy) = static_context.workspace_review_policy.as_ref() else {
        return Ok(None);
    };

    let Some(file) = next_workspace_review_bootstrap_file(task, policy) else {
        return Ok(None);
    };

    let control = handle_domain_tool_step(
        app,
        spec,
        static_context,
        task,
        "read_file_text".to_string(),
        json!({ "path": file.path }),
        Some(file.summary),
        None,
    )?;
    Ok(Some(control))
}

fn next_workspace_review_bootstrap_file(
    task: &AgentTaskRun,
    policy: &WorkspaceReviewPolicy,
) -> Option<WorkspaceReviewBootstrapFile> {
    policy
        .bootstrap_files
        .iter()
        .find(|file| !workspace_review_file_already_read(task, &file.path))
        .cloned()
}

fn workspace_review_file_already_read(task: &AgentTaskRun, path: &str) -> bool {
    task.recent_steps.iter().any(|step| {
        step.tool.as_deref() == Some("read_file_text")
            && step
                .args
                .as_ref()
                .and_then(|value| value.get("path"))
                .and_then(Value::as_str)
                == Some(path)
    })
}

fn push_workspace_review_file(
    bootstrap_files: &mut Vec<WorkspaceReviewBootstrapFile>,
    seen: &mut HashSet<String>,
    workspace_root: &Path,
    relative_path: &str,
    summary: &str,
) {
    let absolute_path = workspace_root.join(relative_path);
    if !absolute_path.exists() {
        return;
    }
    let normalized = absolute_path.to_string_lossy().to_string();
    if !seen.insert(normalized.clone()) {
        return;
    }
    bootstrap_files.push(WorkspaceReviewBootstrapFile {
        path: normalized,
        summary: summary.to_string(),
    });
}

fn input_mentions_workspace_file(user_input: &str, task: &AgentTaskRun, relative_path: &str) -> bool {
    let basename = Path::new(relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative_path);
    let haystack = format!(
        "{}\n{}\n{}",
        user_input,
        task.original_request,
        task.goal
    )
    .to_lowercase();
    haystack.contains(&relative_path.to_lowercase()) || haystack.contains(&basename.to_lowercase())
}

fn is_workspace_review_request(user_input: &str, task: &AgentTaskRun) -> bool {
    if !matches!(task.intent, TopLevelIntent::WorkspaceTask) {
        return false;
    }

    let haystack = format!(
        "{}\n{}\n{}",
        user_input,
        task.original_request,
        task.goal
    )
    .to_lowercase();

    [
        "审查代码",
        "代码审查",
        "review",
        "分析项目",
        "分析这个项目",
        "看看仓库",
        "查看仓库",
        "看架构",
        "架构",
        "说明风险",
        "风险",
        "分析实现",
        "读取代码",
    ]
    .iter()
    .any(|keyword| haystack.contains(keyword))
}

fn is_project_overview_request(user_input: &str, task: &AgentTaskRun) -> bool {
    let haystack = format!(
        "{}\n{}\n{}",
        user_input,
        task.original_request,
        task.goal
    )
    .to_lowercase();
    ["分析项目", "分析这个项目", "看看仓库", "查看仓库", "项目结构", "仓库结构"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

fn is_provider_alignment_request(user_input: &str, task: &AgentTaskRun) -> bool {
    let haystack = format!(
        "{}\n{}\n{}",
        user_input,
        task.original_request,
        task.goal
    )
    .to_lowercase();
    ["provider", "api", "模型", "codex", "anthropic", "openai", "兼容"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

async fn refresh_domain_runtime_context(
    spec: DomainLoopSpec,
    app: &AppHandle,
    task: &mut AgentTaskRun,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
) -> Option<RuntimeContext> {
    if matches!(
        spec.runtime_context_mode,
        DomainRuntimeContextMode::RefreshEachStep
    ) {
        Some(
            runtime_context::refresh_runtime_context(app, task, vision_channel, vision_api_key).await,
        )
    } else {
        None
    }
}

fn handle_domain_budget_boundary(
    spec: DomainLoopSpec,
    app: &AppHandle,
    task: &mut AgentTaskRun,
) -> DomainLoopControl {
    if task.step_budget > 0 {
        return DomainLoopControl::Continue;
    }

    if spec.auto_complete_on_budget_exhausted && can_auto_complete_loop_task(task) {
        task.task_status = AgentLoopTaskStatus::Completed;
        let summary = build_auto_completion_summary(task);
        task.final_summary = Some(summary.clone());
        persist_domain_memory(spec, app, task, false);
        let message = if task.completed_notes.is_empty() {
            "测试任务已完成。".to_string()
        } else {
            task.completed_notes
                .last()
                .cloned()
                .unwrap_or_else(|| "测试任务已完成。".to_string())
        };
        return DomainLoopControl::Return(complete_result(
            spec.route,
            spec.provider_label,
            task,
            message,
            summary,
        ));
    }

    if let Some(warning) = spec.budget_exhausted_warning {
        task.completed_notes.push(warning.to_string());
    }

    DomainLoopControl::Break
}

fn handle_domain_next_action(
    app: &AppHandle,
    spec: DomainLoopSpec,
    static_context: &DomainLoopStaticContext,
    task: &mut AgentTaskRun,
    next: AgentActionPayload,
) -> Result<DomainLoopControl, String> {
    let action_kind = domain_action_kind(&next);
    if !domain_supports_action(spec, action_kind) {
        return Ok(DomainLoopControl::Return(invalid_action_result(
            spec,
            app,
            task,
            &format!(
                "{:?} loop 不支持 {}。",
                spec.domain,
                domain_action_label(action_kind)
            ),
        )));
    }

    match next.action {
        AgentAction::Respond => {
            let message = required_action_message(&next, "respond")?;
            task.task_status = AgentLoopTaskStatus::Completed;
            persist_domain_memory(spec, app, task, true);
            Ok(DomainLoopControl::Return(simple_result(
                spec.route,
                spec.provider_label,
                spec.response_outcome,
                message,
                task,
            )))
        }
        AgentAction::Finish => {
            let message = required_action_message(&next, "finish")?;
            let summary = required_action_final_summary(&next, "finish")?;
            task.task_status = AgentLoopTaskStatus::Completed;
            task.final_summary = Some(summary.clone());
            persist_domain_memory(spec, app, task, false);
            Ok(DomainLoopControl::Return(complete_result(
                spec.route,
                spec.provider_label,
                task,
                message,
                summary,
            )))
        }
        AgentAction::Fail => {
            let message = required_action_message(&next, "fail")?;
            let summary = required_action_final_summary(&next, "fail")?;
            task.task_status = AgentLoopTaskStatus::Failed;
            task.failure_reason = Some(message.clone());
            task.failure_reason_code = summary.failure_reason_code.clone();
            task.failure_stage = summary.failure_stage.clone();
            task.final_summary = Some(summary.clone());
            persist_domain_memory(spec, app, task, false);
            Ok(DomainLoopControl::Return(fail_result(
                spec.route,
                spec.provider_label,
                task,
                message,
            )))
        }
        AgentAction::Observe => {
            let summary = required_action_step_summary(&next, "observe")?;
            execute_domain_observation(spec, task, summary)
        }
        AgentAction::Retry => {
            let target = required_action_retry_target(&next)?;
            let summary = required_action_step_summary(&next, "retry")?;
            match perform_retry_step(
                app,
                task,
                target,
                summary,
                spec.route,
                spec.provider_label,
            )? {
                LoopContinuation::Continue => Ok(DomainLoopControl::Continue),
                LoopContinuation::Return(result) => Ok(DomainLoopControl::Return(result)),
            }
        }
        AgentAction::Assert => execute_domain_assertion(
            spec,
            task,
            required_action_assertion_type(&next)?,
            required_action_step_summary(&next, "assert")?,
            next.params,
        ),
        AgentAction::Confirm => {
            let tool = required_action_tool(&next, "confirm")?;
            let summary = next
                .summary
                .as_ref()
                .and_then(Value::as_str)
                .map(ToString::to_string);
            handle_domain_tool_step(
                app,
                spec,
                static_context,
                task,
                tool,
                next.args,
                summary,
                next.message,
            )
        }
        AgentAction::Tool => {
            let tool = required_action_tool(&next, "tool")?;
            let summary = next
                .summary
                .as_ref()
                .and_then(Value::as_str)
                .map(ToString::to_string);
            handle_domain_tool_step(
                app,
                spec,
                static_context,
                task,
                tool,
                next.args,
                summary,
                None,
            )
        }
    }
}

fn required_action_message(
    action: &AgentActionPayload,
    action_label: &str,
) -> Result<String, String> {
    action
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("{action_label} action 缺少 message。"))
}

fn required_action_tool(
    action: &AgentActionPayload,
    action_label: &str,
) -> Result<String, String> {
    action
        .tool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("{action_label} action 缺少 tool。"))
}

fn required_action_step_summary(
    action: &AgentActionPayload,
    action_label: &str,
) -> Result<String, String> {
    action
        .summary
        .as_ref()
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("{action_label} action 缺少 string summary。"))
}

fn required_action_final_summary(
    action: &AgentActionPayload,
    action_label: &str,
) -> Result<AgentLoopSummary, String> {
    let summary = action
        .summary
        .as_ref()
        .cloned()
        .ok_or_else(|| format!("{action_label} action 缺少 summary。"))?;
    serde_json::from_value(summary)
        .map_err(|_| format!("{action_label} action summary 必须是 AgentLoopSummary 对象。"))
}

fn required_action_retry_target(action: &AgentActionPayload) -> Result<RetryTarget, String> {
    action
        .target
        .clone()
        .ok_or_else(|| "retry action 缺少 target。".to_string())
}

fn required_action_assertion_type(
    action: &AgentActionPayload,
) -> Result<super::types::AssertionType, String> {
    action
        .assertion_type
        .clone()
        .ok_or_else(|| "assert action 缺少 assertion_type。".to_string())
}

fn handle_assert_condition(
    spec: DomainLoopSpec,
    task: &mut AgentTaskRun,
    assertion_type: super::types::AssertionType,
    summary: String,
    params: Value,
) -> Result<DomainLoopControl, String> {
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
        args: Some(params),
        outcome: if result.passed { "success" } else { "failure" }.to_string(),
        detail: Some(
            serde_json::to_string(&result).unwrap_or_else(|_| "assertion_result".to_string()),
        ),
    });
    if result.passed {
        task.step_budget = task.step_budget.saturating_sub(1);
        task.task_status = AgentLoopTaskStatus::Observing;
        task.completed_notes.push(format!("断言通过：{summary}"));
        Ok(DomainLoopControl::Continue)
    } else {
        task.failure_reason = Some(format!("断言失败：{summary}"));
        task.failure_reason_code = result.failure_reason_code.clone();
        task.failure_stage = Some(FailureStage::Assertion);
        if task.retry_budget > 0 {
            task.retry_budget -= 1;
            task.used_retry = true;
            task.task_status = AgentLoopTaskStatus::Retrying;
            task.completed_notes.push(format!("断言失败，允许一次补测：{summary}"));
            Ok(DomainLoopControl::Continue)
        } else {
            task.task_status = AgentLoopTaskStatus::Failed;
            Ok(DomainLoopControl::Return(fail_result(
                spec.route,
                spec.provider_label,
                task,
                format!("断言失败：{summary}"),
            )))
        }
    }
}

fn execute_domain_observation(
    spec: DomainLoopSpec,
    task: &mut AgentTaskRun,
    summary: String,
) -> Result<DomainLoopControl, String> {
    match spec.observation_policy {
        DomainObservationPolicy::RuntimeContextSnapshot => {
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
            Ok(DomainLoopControl::Continue)
        }
        DomainObservationPolicy::Unsupported => Err(format!(
            "{:?} loop 未配置 observe_context 策略。",
            spec.domain
        )),
    }
}

fn execute_domain_assertion(
    spec: DomainLoopSpec,
    task: &mut AgentTaskRun,
    assertion_type: super::types::AssertionType,
    summary: String,
    params: Value,
) -> Result<DomainLoopControl, String> {
    match spec.assertion_policy {
        DomainAssertionPolicy::TestAssertion => {
            handle_assert_condition(spec, task, assertion_type, summary, params)
        }
        DomainAssertionPolicy::Unsupported => Err(format!(
            "{:?} loop 未配置 assert_condition 策略。",
            spec.domain
        )),
    }
}

fn handle_domain_tool_step(
    app: &AppHandle,
    spec: DomainLoopSpec,
    static_context: &DomainLoopStaticContext,
    task: &mut AgentTaskRun,
    tool: String,
    args: Value,
    summary: Option<String>,
    message: Option<String>,
) -> Result<DomainLoopControl, String> {
    let action_args = apply_domain_tool_defaults(spec, &tool, args, static_context);
    match execute_tool_step(app, task, &tool, action_args.clone(), summary)? {
        LoopToolExecution::Success => {
            task.step_budget = task.step_budget.saturating_sub(1);
            task.task_status = AgentLoopTaskStatus::Observing;
            if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                task.completed_notes.push(message);
            }
            Ok(DomainLoopControl::Continue)
        }
        LoopToolExecution::Pending {
            note,
            pending_request,
        } => {
            if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
                task.completed_notes.push(message);
            }
            task_store::replace_active_task(app, Some(task.clone()))?;
            Ok(DomainLoopControl::Return(pending_result(
                task,
                pending_request,
                note,
                spec.route,
                spec.provider_label,
            )))
        }
        LoopToolExecution::Failure { reason } => {
            if let Some(result) = maybe_retry_or_fail(
                task,
                &tool,
                &reason,
                &action_args,
                spec.route,
                spec.provider_label,
            ) {
                Ok(DomainLoopControl::Return(result))
            } else {
                Ok(DomainLoopControl::Continue)
            }
        }
    }
}

fn apply_domain_tool_defaults(
    spec: DomainLoopSpec,
    tool: &str,
    args: Value,
    static_context: &DomainLoopStaticContext,
) -> Value {
    match spec.tool_args_policy {
        DomainToolArgsPolicy::WorkspaceDefaults => {
            apply_workspace_defaults(tool, args, static_context.default_workdir.as_deref())
        }
        DomainToolArgsPolicy::None => args,
    }
}

fn invalid_action_result(
    spec: DomainLoopSpec,
    app: &AppHandle,
    task: &mut AgentTaskRun,
    reason: &str,
) -> AgentHandleResult {
    task.task_status = AgentLoopTaskStatus::Failed;
    task.failure_reason = Some(reason.to_string());
    task.failure_reason_code = FailureReasonCode::InvalidAction;
    task.failure_stage = Some(FailureStage::Planning);
    persist_domain_memory(spec, app, task, false);
    fail_result(
        spec.route,
        spec.provider_label,
        task,
        reason.to_string(),
    )
}

fn persist_domain_memory(
    spec: DomainLoopSpec,
    app: &AppHandle,
    task: &AgentTaskRun,
    is_reply: bool,
) {
    let should_persist = if is_reply {
        spec.persistence.on_reply
    } else {
        spec.persistence.on_terminal
    };

    if should_persist {
        write_back_task_memory(app, task);
    }
}

fn format_planner_error(spec: DomainLoopSpec, error: &str) -> String {
    if spec.planner_error_prefix.ends_with('：') || spec.planner_error_prefix.ends_with(':') {
        format!("{}{}", spec.planner_error_prefix, error)
    } else {
        format!("{}\n主路径：{}", spec.planner_error_prefix, error)
    }
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

fn apply_workspace_defaults(tool: &str, args: Value, default_workdir: Option<&str>) -> Value {
    let Some(default_workdir) = default_workdir.filter(|value| !value.trim().is_empty()) else {
        return args;
    };

    let Some(mut map) = args.as_object().cloned() else {
        return args;
    };

    match tool {
        "run_shell_command" => {
            map.entry("workdir".to_string())
                .or_insert_with(|| Value::String(default_workdir.to_string()));
        }
        "list_directory" | "read_file_text" | "write_file_text" | "create_directory" | "delete_path" => {
            if let Some(path) = map.get("path").and_then(Value::as_str) {
                map.insert(
                    "path".to_string(),
                    Value::String(resolve_workspace_path(path, default_workdir)),
                );
            }
        }
        "move_path" => {
            if let Some(path) = map.get("fromPath").and_then(Value::as_str) {
                map.insert(
                    "fromPath".to_string(),
                    Value::String(resolve_workspace_path(path, default_workdir)),
                );
            }
            if let Some(path) = map.get("toPath").and_then(Value::as_str) {
                map.insert(
                    "toPath".to_string(),
                    Value::String(resolve_workspace_path(path, default_workdir)),
                );
            }
        }
        _ => {}
    }

    Value::Object(map)
}

fn resolve_workspace_path(path: &str, default_workdir: &str) -> String {
    let target = Path::new(path);
    if target.is_absolute() {
        return path.to_string();
    }

    PathBuf::from(default_workdir)
        .join(target)
        .to_string_lossy()
        .to_string()
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
    route: AgentRoute,
    provider_label: &str,
) -> Result<LoopContinuation, String> {
    if task.retry_budget == 0 {
        task.task_status = AgentLoopTaskStatus::Failed;
        task.failure_reason = Some("重试预算已耗尽。".to_string());
        task.failure_reason_code = FailureReasonCode::RetryExhausted;
        task.failure_stage = Some(FailureStage::Retry);
        return Ok(LoopContinuation::Return(fail_result(
            route,
            provider_label,
            task,
            "当前任务已经耗尽 retry budget。".to_string(),
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
                    route,
                    provider_label,
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
                        route,
                        provider_label,
                    )))
                }
                LoopToolExecution::Failure { reason } => {
                    task.task_status = AgentLoopTaskStatus::Failed;
                    task.failure_reason = Some(reason.clone());
                    task.failure_reason_code = FailureReasonCode::RetryExhausted;
                    task.failure_stage = Some(FailureStage::Retry);
                    Ok(LoopContinuation::Return(fail_result(
                        route,
                        provider_label,
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

fn provider_label_for_route(route: AgentRoute) -> &'static str {
    match route {
        AgentRoute::Chat => "Chat Agent",
        AgentRoute::Control => "Desktop Agent",
        AgentRoute::Test => "Test Agent",
        AgentRoute::Workspace => "Workspace Agent",
    }
}

fn route_outcome(prefix: &str, route: AgentRoute) -> String {
    match route {
        AgentRoute::Chat => format!("chat_{prefix}"),
        AgentRoute::Control => format!("control_{prefix}"),
        AgentRoute::Test => format!("test_{prefix}"),
        AgentRoute::Workspace => format!("workspace_{prefix}"),
    }
}

fn task_kind_label(intent: TopLevelIntent) -> &'static str {
    match intent {
        TopLevelIntent::DesktopAction => "桌面任务",
        TopLevelIntent::TestRequest => "测试任务",
        TopLevelIntent::WorkspaceTask => "工作区任务",
        _ => "任务",
    }
}

fn blocked_result(reason: String) -> AgentHandleResult {
    blocked_result_for_route(AgentRoute::Control, reason)
}

fn blocked_result_for_route(route: AgentRoute, reason: String) -> AgentHandleResult {
    AgentHandleResult {
        reply_text: format!("这次任务未执行。\n\n原因：{reason}"),
        provider_label: provider_label_for_route(route).to_string(),
        outcome: route_outcome("blocked", route),
        detail: reason,
        meta: AgentMessageMeta {
            route,
            planned_tools: vec![],
            pending_request: None,
            task: None,
            summary: None,
        },
    }
}

fn active_task_waiting_result(task: &AgentTaskRun) -> AgentHandleResult {
    let route = match task.intent {
        TopLevelIntent::TestRequest => AgentRoute::Test,
        TopLevelIntent::WorkspaceTask => AgentRoute::Workspace,
        _ => AgentRoute::Control,
    };
    let provider_label = provider_label_for_route(route);
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
        outcome: route_outcome("pending", route),
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
        outcome: route_outcome("ok", route),
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
        outcome: route_outcome("failed", route),
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
        outcome: route_outcome("pending", route),
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
            "workspace" => AgentRoute::Workspace,
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
    let Some(domain) = intent_to_domain(task.intent.clone()) else {
        return Err("当前 loop task 的 intent 不支持继续执行。".to_string());
    };
    let conversation_context = load_recent_conversation_context(app);
    continue_domain_loop(
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
        domain,
        task,
    )
    .await
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
