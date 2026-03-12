use serde_json::json;

use crate::agent::types::{AgentPlan, AgentRoute, AgentToolStep};

pub fn build_open_and_type_plan(text: &str) -> AgentPlan {
    plan(
        "打开记事本并输入文本",
        vec![
            step("open_app", "打开记事本", json!({ "name": "notepad" })),
            step("list_windows", "刷新窗口列表", json!({})),
            step(
                "focus_window",
                "切到记事本窗口",
                json!({
                    "titleCandidates": ["Notepad", "记事本"],
                    "match": "contains",
                }),
            ),
            step("type_text", "输入文本", json!({ "text": text })),
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
