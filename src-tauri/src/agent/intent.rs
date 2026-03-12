use serde_json::json;

use super::types::{AgentPlan, AgentRoute, AgentToolStep};

const CONTROL_HINTS: &[&str] = &[
    "打开",
    "启动",
    "切到",
    "切换到",
    "聚焦",
    "窗口",
    "剪贴板",
    "输入到当前窗口",
    "当前窗口输入",
    "按一下",
    "快捷键",
    "ctrl+",
    "click",
    "点击",
];

pub fn looks_like_control_request(input: &str) -> bool {
    let lowered = input.trim().to_lowercase();
    if lowered.is_empty() {
        return false;
    }

    CONTROL_HINTS
        .iter()
        .any(|hint| lowered.contains(&hint.to_lowercase()))
}

pub fn parse_simple_control_plan(input: &str) -> Option<AgentPlan> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if contains_any(trimmed, &["看看我现在开了哪些窗口", "现在开了哪些窗口", "列出窗口", "窗口列表"]) {
        return Some(single_step("list_windows", json!({})));
    }

    if contains_any(trimmed, &["读取剪贴板", "看看剪贴板", "读一下剪贴板", "剪贴板里是什么"]) {
        return Some(single_step("read_clipboard", json!({})));
    }

    if contains_any(trimmed, &["打开记事本", "启动记事本"]) {
        return Some(single_step("open_app", json!({ "name": "notepad" })));
    }

    if let Some(window_title) = parse_focus_window_title(trimmed) {
        return Some(single_step(
            "focus_window",
            json!({
                "title": window_title,
                "match": "contains",
            }),
        ));
    }

    if let Some(text) = parse_current_window_text(trimmed) {
        return Some(single_step("type_text", json!({ "text": text })));
    }

    if let Some(keys) = parse_hotkey(trimmed) {
        return Some(single_step("send_hotkey", json!({ "keys": keys })));
    }

    None
}

fn contains_any(input: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| input.contains(token))
}

fn single_step(tool: &str, args: serde_json::Value) -> AgentPlan {
    AgentPlan {
        route: AgentRoute::Control,
        steps: vec![AgentToolStep {
            tool: tool.to_string(),
            args,
        }],
    }
}

fn parse_focus_window_title(input: &str) -> Option<String> {
    for keyword in ["切到", "切换到", "聚焦到", "聚焦", "切回", "帮我切到"] {
        if let Some(position) = input.find(keyword) {
            let tail = input[position + keyword.len()..].trim();
            let cleaned = clean_window_title(tail);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }

    None
}

fn clean_window_title(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            matches!(ch, ' ' | '，' | ',' | '。' | '“' | '”' | '"') || ch == '\''
        })
        .trim_end_matches("窗口")
        .trim_end_matches("软件")
        .trim()
        .to_string()
}

fn parse_current_window_text(input: &str) -> Option<String> {
    if let Some(colon) = input.find('：').or_else(|| input.find(':')) {
        let tail = input[colon + 1..].trim();
        if !tail.is_empty() {
            return Some(tail.to_string());
        }
    }

    for (prefix, suffix) in [
        ("把", "输入到当前窗口"),
        ("把", "输入到当前页面"),
        ("在当前窗口输入", ""),
        ("在当前页面输入", ""),
        ("输入", "到当前窗口"),
        ("输入", "到当前页面"),
    ] {
        if let Some(value) = between(input, prefix, suffix) {
            let cleaned = value
                .trim_matches(|ch: char| {
                    matches!(ch, ' ' | '“' | '”' | '"') || ch == '\''
                })
                .trim();
            if !cleaned.is_empty()
                && !matches!(cleaned, "这段话" | "这些话" | "这句话" | "文本")
            {
                return Some(cleaned.to_string());
            }
        }
    }

    None
}

fn between<'a>(input: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let start = input.find(prefix)?;
    let tail = &input[start + prefix.len()..];
    if suffix.is_empty() {
        return Some(tail);
    }

    let end = tail.find(suffix)?;
    Some(&tail[..end])
}

fn parse_hotkey(input: &str) -> Option<Vec<String>> {
    let lowered = input.to_lowercase();
    if contains_any(&lowered, &["ctrl+v", "ctrl + v", "control+v", "control + v", "按一下 ctrl+v"])
        || lowered.contains("ctrl v")
        || lowered.contains("按一下ctrl+v")
    {
        return Some(vec!["CTRL".to_string(), "V".to_string()]);
    }

    None
}
