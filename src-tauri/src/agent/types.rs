use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control::types::{ControlPendingRequest, ControlRiskLevel};
use crate::app_state::now_millis;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentRoute {
    Chat,
    Control,
    Test,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopLevelIntent {
    Chat,
    DesktopAction,
    TestRequest,
    WorkspaceTask,
    DebugRequest,
    ConfirmationResponse,
    MemoryRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskMode {
    Plan,
    Loop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentLoopTaskStatus {
    Planning,
    Executing,
    Observing,
    WaitingConfirmation,
    Retrying,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertionType {
    WindowExists,
    ActiveWindowMatches,
    TextContains,
    ScreenContextState,
    PendingState,
    ConsistencyState,
    FileExists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Planning,
    Observation,
    ExecuteTool,
    Assertion,
    Confirmation,
    Retry,
    Finish,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureReasonCode {
    None,
    PlannerFailed,
    ContextUnavailable,
    ToolFailed,
    AssertionFailed,
    ConfirmationRequired,
    ConfirmationRejected,
    RetryExhausted,
    StepBudgetExceeded,
    PolicyBlocked,
    InvalidAction,
    FileMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryTarget {
    ObserveContext,
    LastTool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveredEntitySource {
    ScreenContext,
    ToolResult,
    Assertion,
    UserInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityPayloadType {
    WindowRef,
    ElementRef,
    TextValue,
    FileRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveredEntityPayload {
    WindowRef {
        title: String,
        #[serde(default)]
        class_name: Option<String>,
        #[serde(default)]
        kind: Option<String>,
    },
    ElementRef {
        #[serde(default)]
        window_title: Option<String>,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        automation_id: Option<String>,
        #[serde(default)]
        class_name: Option<String>,
    },
    TextValue {
        text: String,
    },
    FileRef {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredEntity {
    pub id: String,
    pub label: String,
    pub payload_type: EntityPayloadType,
    pub payload: DiscoveredEntityPayload,
    pub created_at_step: usize,
    pub last_seen_step: usize,
    pub source: DiscoveredEntitySource,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeObservation {
    pub step: usize,
    pub source: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolResult {
    pub step: usize,
    pub tool: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContext {
    pub raw_user_input: String,
    pub normalized_goal: String,
    pub task_status: AgentLoopTaskStatus,
    #[serde(default)]
    pub active_window: Option<Value>,
    #[serde(default)]
    pub window_inventory: Vec<Value>,
    #[serde(default)]
    pub uia_summary: Option<Value>,
    #[serde(default)]
    pub vision_summary: Option<Value>,
    #[serde(default)]
    pub clipboard: Option<String>,
    #[serde(default)]
    pub recent_tool_results: Vec<RuntimeToolResult>,
    #[serde(default)]
    pub recent_observations: Vec<RuntimeObservation>,
    #[serde(default)]
    pub discovered_entities: Vec<DiscoveredEntity>,
    #[serde(default)]
    pub consistency: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResult {
    pub assertion_type: AssertionType,
    pub passed: bool,
    #[serde(default)]
    pub observed_value: Value,
    #[serde(default)]
    pub expected_value: Value,
    pub failure_reason_code: FailureReasonCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLoopSummary {
    pub goal: String,
    pub steps_taken: usize,
    pub final_status: AgentTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<FailureStage>,
    pub failure_reason_code: FailureReasonCode,
    pub used_probe: bool,
    pub used_retry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolStep {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    pub tool: String,
    #[serde(default = "empty_json_object")]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepRecord {
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLoopDecision {
    pub intent: TopLevelIntent,
    pub goal: String,
    pub next: AgentActionPayload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAction {
    Respond,
    Observe,
    Assert,
    Confirm,
    Tool,
    Retry,
    Finish,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionPayload {
    pub action: AgentAction,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub summary: Option<Value>,
    #[serde(default = "empty_json_object")]
    pub args: Value,
    #[serde(default)]
    pub target: Option<RetryTarget>,
    #[serde(default)]
    pub assertion_type: Option<AssertionType>,
    #[serde(default = "empty_json_object")]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlan {
    pub route: AgentRoute,
    #[serde(default)]
    pub task_title: Option<String>,
    #[serde(default = "default_true")]
    pub stop_on_error: bool,
    #[serde(default)]
    pub steps: Vec<AgentToolStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskStatus {
    Running,
    WaitingConfirmation,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskProgress {
    pub task_id: String,
    pub task_title: String,
    pub step_index: usize,
    pub step_count: usize,
    pub status: AgentTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentTaskRun {
    pub task_id: String,
    pub mode: AgentTaskMode,
    pub intent: TopLevelIntent,
    pub task_title: String,
    pub original_request: String,
    pub goal: String,
    pub max_steps: usize,
    pub step_budget: usize,
    pub retry_budget: usize,
    pub pending_action_id: Option<String>,
    pub pending_action_summary: Option<String>,
    pub last_observation: Option<Value>,
    pub last_tool_result: Option<Value>,
    pub runtime_context: Option<RuntimeContext>,
    pub task_status: AgentLoopTaskStatus,
    pub recent_steps: Vec<AgentStepRecord>,
    pub failure_reason: Option<String>,
    pub failure_reason_code: FailureReasonCode,
    pub failure_stage: Option<FailureStage>,
    pub used_probe: bool,
    pub used_retry: bool,
    pub plan: AgentPlan,
    pub next_step_index: usize,
    pub waiting_step_index: Option<usize>,
    pub waiting_pending_id: Option<String>,
    pub completed_notes: Vec<String>,
    pub completed_results: Vec<Value>,
    pub last_retryable_tool: Option<String>,
    pub last_retryable_args: Option<Value>,
    pub last_retryable_summary: Option<String>,
    pub last_retryable_risk: Option<ControlRiskLevel>,
    pub final_summary: Option<AgentLoopSummary>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageMeta {
    pub route: AgentRoute,
    pub planned_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_request: Option<ControlPendingRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<AgentTaskProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<AgentLoopSummary>,
}

pub const AGENT_ALLOWED_TOOLS: &[&str] = &[
    "list_windows",
    "focus_window",
    "open_app",
    "capture_active_window",
    "read_clipboard",
    "list_directory",
    "read_file_text",
    "write_file_text",
    "create_directory",
    "move_path",
    "delete_path",
    "run_shell_command",
    "launch_installer_file",
    "query_registry_key",
    "read_registry_value",
    "write_registry_value",
    "delete_registry_value",
    "type_text",
    "send_hotkey",
    "scroll_at",
    "click_at",
    "find_element",
    "click_element",
    "get_element_text",
    "set_element_value",
    "wait_for_element",
];

pub const WORKSPACE_ALLOWED_TOOLS: &[&str] = &[
    "list_directory",
    "read_file_text",
    "write_file_text",
    "create_directory",
    "move_path",
    "delete_path",
    "run_shell_command",
];

pub fn is_agent_tool_allowed(name: &str) -> bool {
    AGENT_ALLOWED_TOOLS.contains(&name)
}

pub fn is_workspace_tool_allowed(name: &str) -> bool {
    WORKSPACE_ALLOWED_TOOLS.contains(&name)
}

pub fn default_true() -> bool {
    true
}

pub fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}

impl AgentTaskRun {
    pub fn new_loop(intent: TopLevelIntent, goal: &str, max_steps: usize, retry_budget: usize) -> Self {
        let task_id = format!("agent-task-{}", now_millis());
        let timestamp = now_millis();
        let task_title = truncate_task_title(goal);
        let route = if matches!(intent, TopLevelIntent::TestRequest) {
            AgentRoute::Test
        } else if matches!(intent, TopLevelIntent::WorkspaceTask) {
            AgentRoute::Workspace
        } else {
            AgentRoute::Control
        };

        Self {
            task_id,
            mode: AgentTaskMode::Loop,
            intent,
            task_title: task_title.clone(),
            original_request: goal.trim().to_string(),
            goal: goal.trim().to_string(),
            max_steps: max_steps.max(1),
            step_budget: max_steps.max(1),
            retry_budget,
            pending_action_id: None,
            pending_action_summary: None,
            last_observation: None,
            last_tool_result: None,
            runtime_context: None,
            task_status: AgentLoopTaskStatus::Planning,
            recent_steps: vec![],
            failure_reason: None,
            failure_reason_code: FailureReasonCode::None,
            failure_stage: None,
            used_probe: false,
            used_retry: false,
            plan: AgentPlan {
                route,
                task_title: Some(task_title.clone()),
                stop_on_error: true,
                steps: vec![],
            },
            next_step_index: 0,
            waiting_step_index: None,
            waiting_pending_id: None,
            completed_notes: vec![],
            completed_results: vec![],
            last_retryable_tool: None,
            last_retryable_args: None,
            last_retryable_summary: None,
            last_retryable_risk: None,
            final_summary: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    pub fn planned_tools(&self) -> Vec<String> {
        match self.mode {
            AgentTaskMode::Plan => self
                .plan
                .steps
                .iter()
                .map(|step| step.tool.clone())
                .collect::<Vec<_>>(),
            AgentTaskMode::Loop => self
                .recent_steps
                .iter()
                .filter_map(|step| step.tool.clone())
                .collect::<Vec<_>>(),
        }
    }

    pub fn step_count(&self) -> usize {
        match self.mode {
            AgentTaskMode::Plan => self.plan.steps.len().max(1),
            AgentTaskMode::Loop => self.max_steps.max(1),
        }
    }

    pub fn waiting_progress(&self) -> AgentTaskProgress {
        let (step_index, step_summary) = match self.mode {
            AgentTaskMode::Plan => {
                let step_index = self
                    .waiting_step_index
                    .map(|index| index + 1)
                    .unwrap_or_else(|| self.next_step_index.saturating_add(1).min(self.step_count().max(1)));
                let step_summary = self
                    .waiting_step_index
                    .and_then(|index| self.plan.steps.get(index))
                    .and_then(|step| step.summary.clone())
                    .or_else(|| {
                        self.waiting_step_index
                            .and_then(|index| self.plan.steps.get(index))
                            .map(|step| step.tool.clone())
                    });
                (step_index, step_summary)
            }
            AgentTaskMode::Loop => (
                self.recent_steps.len().saturating_add(1).min(self.step_count()),
                self.pending_action_summary.clone(),
            ),
        };
        AgentTaskProgress {
            task_id: self.task_id.clone(),
            task_title: self.task_title.clone(),
            step_index,
            step_count: self.step_count(),
            status: AgentTaskStatus::WaitingConfirmation,
            step_summary,
            detail: Some("等待本地控制确认。".to_string()),
        }
    }

    pub fn progress(
        &self,
        status: AgentTaskStatus,
        step_summary: Option<String>,
        detail: Option<String>,
    ) -> AgentTaskProgress {
        let step_index = match self.mode {
            AgentTaskMode::Plan => self.next_step_index.min(self.step_count()).max(1),
            AgentTaskMode::Loop => self.recent_steps.len().max(1).min(self.step_count()),
        };
        AgentTaskProgress {
            task_id: self.task_id.clone(),
            task_title: self.task_title.clone(),
            step_index,
            step_count: self.step_count(),
            status,
            step_summary,
            detail,
        }
    }
}

fn truncate_task_title(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "桌面任务".to_string();
    }

    let mut title = trimmed.chars().take(40).collect::<String>();
    if trimmed.chars().count() > 40 {
        title.push('…');
    }
    title
}
