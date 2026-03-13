use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control::types::ControlPendingRequest;
use crate::app_state::now_millis;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentRoute {
    Chat,
    Control,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopLevelIntent {
    Chat,
    DesktopAction,
    TestRequest,
    DebugRequest,
    ConfirmationResponse,
    MemoryRequest,
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

#[derive(Debug, Clone, Serialize)]
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
    pub task_title: String,
    pub original_request: String,
    pub plan: AgentPlan,
    pub next_step_index: usize,
    pub waiting_step_index: Option<usize>,
    pub waiting_pending_id: Option<String>,
    pub completed_notes: Vec<String>,
    pub completed_results: Vec<Value>,
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
}

pub const AGENT_ALLOWED_TOOLS: &[&str] = &[
    "list_windows",
    "focus_window",
    "open_app",
    "read_clipboard",
    "type_text",
    "send_hotkey",
    "scroll_at",
    "click_at",
];

pub fn is_agent_tool_allowed(name: &str) -> bool {
    AGENT_ALLOWED_TOOLS.contains(&name)
}

pub fn default_true() -> bool {
    true
}

pub fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}

impl AgentTaskRun {
    pub fn new(plan: AgentPlan, original_request: &str) -> Self {
        let task_id = format!("agent-task-{}", now_millis());
        let task_title = plan
            .task_title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| truncate_task_title(original_request));
        let timestamp = now_millis();

        Self {
            task_id,
            task_title,
            original_request: original_request.trim().to_string(),
            plan,
            next_step_index: 0,
            waiting_step_index: None,
            waiting_pending_id: None,
            completed_notes: vec![],
            completed_results: vec![],
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    pub fn planned_tools(&self) -> Vec<String> {
        self.plan
            .steps
            .iter()
            .map(|step| step.tool.clone())
            .collect::<Vec<_>>()
    }

    pub fn step_count(&self) -> usize {
        self.plan.steps.len()
    }

    pub fn waiting_progress(&self) -> AgentTaskProgress {
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
