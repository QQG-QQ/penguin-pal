use serde_json::json;

use crate::agent::types::{AgentPlan, AgentRoute, AgentToolStep};

pub fn build_focus_and_draft_plan(text: &str) -> AgentPlan {
    plan(
        "切到微信并输入草稿",
        vec![
            step("list_windows", "列出窗口", json!({})),
            step(
                "focus_window",
                "切到微信窗口",
                json!({
                    "titleCandidates": ["微信", "WeChat"],
                    "match": "contains",
                }),
            ),
            step("type_text", "输入草稿文本", json!({ "text": text })),
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
