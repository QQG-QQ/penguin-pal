use serde::{Deserialize, Serialize};

use crate::{
    agent::types::AgentRoute,
    testing::types::{TestCase, TestRunRequest, TestSelection, TestStep},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTestRequest {
    #[serde(default = "default_test_route")]
    pub route: AgentRoute,
    pub title: String,
    #[serde(default)]
    pub selection: Option<TestSelection>,
    #[serde(default)]
    pub dynamic_cases: Vec<TestCase>,
    #[serde(default = "default_max_cases")]
    pub max_cases: usize,
    #[serde(default)]
    pub allow_supplementary_rerun: bool,
}

impl PlannedTestRequest {
    pub fn into_run_request(self) -> TestRunRequest {
        TestRunRequest {
            title: self.title,
            selection: self.selection.unwrap_or_default(),
            dynamic_cases: self.dynamic_cases,
            max_cases: self.max_cases,
            allow_supplementary_rerun: self.allow_supplementary_rerun,
        }
    }
}

pub fn default_test_route() -> AgentRoute {
    AgentRoute::Test
}

pub const EXPLORATORY_ALLOWED_TOOLS: &[&str] = &[
    "list_windows",
    "focus_window",
    "open_app",
    "capture_active_window",
    "read_clipboard",
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

pub fn default_max_cases() -> usize {
    8
}

pub fn is_exploratory_step_allowed(step: &TestStep) -> bool {
    match step {
        TestStep::ControlInvoke { tool, .. } => EXPLORATORY_ALLOWED_TOOLS.contains(&tool.as_str()),
        TestStep::SeedClipboardText { .. } | TestStep::CaptureScreenContext { .. } => true,
    }
}
