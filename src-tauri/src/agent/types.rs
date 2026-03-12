use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control::types::ControlPendingRequest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentRoute {
    Chat,
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolStep {
    pub tool: String,
    #[serde(default = "empty_json_object")]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlan {
    pub route: AgentRoute,
    #[serde(default)]
    pub steps: Vec<AgentToolStep>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageMeta {
    pub route: AgentRoute,
    pub planned_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_request: Option<ControlPendingRequest>,
}

pub const AGENT_ALLOWED_TOOLS: &[&str] = &[
    "list_windows",
    "focus_window",
    "open_app",
    "read_clipboard",
    "type_text",
    "send_hotkey",
    "click_at",
    "find_element",
    "click_element",
    "set_element_value",
];

pub fn is_agent_tool_allowed(name: &str) -> bool {
    AGENT_ALLOWED_TOOLS.contains(&name)
}

pub fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}
