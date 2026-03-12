use serde_json::json;

use crate::agent::types::{AgentPlan, AgentRoute, AgentToolStep};

pub fn build_focus_and_ctrl_l_plan() -> AgentPlan {
    plan(
        "切到浏览器并按 Ctrl+L",
        vec![
            step("list_windows", "列出窗口", json!({})),
            step(
                "focus_window",
                "切到浏览器窗口",
                json!({
                    "windowCategory": "browser",
                    "titleCandidates": ["Chrome", "Edge", "Firefox", "浏览器"],
                    "match": "contains",
                }),
            ),
            step("send_hotkey", "发送 Ctrl+L", json!({ "keys": ["CTRL", "L"] })),
        ],
    )
}

fn plan(task_title: impl Into<String>, steps: Vec<AgentToolStep>) -> AgentPlan {
    AgentPlan {
        route: AgentRoute::Control,
        task_title: Some(task_title.into()),
        stop_on_error: true,
        steps,
    }
}

fn step(tool: &str, summary: impl Into<String>, args: serde_json::Value) -> AgentToolStep {
    AgentToolStep {
        id: None,
        summary: Some(summary.into()),
        tool: tool.to_string(),
        args,
    }
}
