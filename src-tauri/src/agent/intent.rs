use serde_json::json;

// NOTE: adapters (notepad, wechat) 不再被 intent.rs 直接调用
// 主链已迁移到 unified loop，parse_simple_control_plan 只返回通用计划
// use crate::control::windows::adapters::{notepad, wechat};

use super::types::{AgentPlan, AgentRoute, AgentToolStep};

const CONTROL_HINTS: &[&str] = &[
    "打开",
    "启动",
    "切到",
    "切换到",
    "聚焦",
    "窗口",
    "剪贴板",
    "输入",
    "当前窗口",
    "按一下",
    "快捷键",
    "ctrl+",
    "click",
    "点击",
    "粘贴",
    "浏览器",
];

/// DEPRECATED: AI-first 架构不再使用关键词预检
/// 保留用于可选降级模式或调试
#[deprecated(since = "0.1.0", note = "AI-first 架构使用 force_route，不再依赖关键词预检")]
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

    parse_focus_and_type(trimmed)
        .or_else(|| parse_open_notepad_and_type(trimmed))
        .or_else(|| parse_paste_clipboard(trimmed))
        .or_else(|| parse_list_and_focus(trimmed))
        .or_else(|| parse_single_step_control(trimmed))
}

fn parse_single_step_control(input: &str) -> Option<AgentPlan> {
    if contains_any(input, &["看看我现在开了哪些窗口", "现在开了哪些窗口", "列出窗口", "窗口列表"]) {
        return Some(single_step_plan(
            "查看当前窗口",
            "list_windows",
            "列出窗口",
            json!({}),
        ));
    }

    if contains_any(input, &["读取剪贴板", "看看剪贴板", "读一下剪贴板", "剪贴板里是什么"]) {
        return Some(single_step_plan(
            "读取剪贴板",
            "read_clipboard",
            "读取剪贴板",
            json!({}),
        ));
    }

    if contains_any(input, &["打开记事本", "启动记事本"]) {
        return Some(single_step_plan(
            "打开记事本",
            "open_app",
            "打开记事本",
            json!({ "name": "notepad" }),
        ));
    }

    if let Some(window_title) = parse_focus_window_title(input) {
        return Some(single_step_plan(
            format!("切到 {window_title}"),
            "focus_window",
            format!("切到 {window_title}"),
            json!({
                "title": window_title,
                "match": "contains",
            }),
        ));
    }

    if let Some(text) = parse_current_window_text(input) {
        return Some(single_step_plan(
            "输入文本".to_string(),
            "type_text",
            "输入文本",
            json!({ "text": text }),
        ));
    }

    if let Some(keys) = parse_hotkey(input) {
        return Some(single_step_plan(
            "发送快捷键".to_string(),
            "send_hotkey",
            "发送快捷键",
            json!({ "keys": keys }),
        ));
    }

    None
}

fn parse_focus_and_type(input: &str) -> Option<AgentPlan> {
    let (window_title, text) = parse_window_and_inline_text(input)?;
    // NOTE: 不再为特定应用（微信）生成特化计划
    // 统一使用通用的 focus_window + type_text 流程
    // unified loop 会根据 runtime context 自行判断

    Some(plan(
        format!("切到 {window_title} 并输入文本"),
        vec![
            step(
                "focus_window",
                format!("切到 {window_title}"),
                json!({
                    "title": window_title,
                    "match": "contains",
                }),
            ),
            step("type_text", "输入文本", json!({ "text": text })),
        ],
    ))
}

fn parse_open_notepad_and_type(input: &str) -> Option<AgentPlan> {
    if !contains_any(input, &["打开记事本", "启动记事本"]) {
        return None;
    }

    let text = parse_inline_text_after_connector(input)?;
    // NOTE: 不再调用 notepad adapter 生成特化计划
    // 使用通用的 open_app + type_text 流程
    Some(plan(
        "打开记事本并输入文本",
        vec![
            step("open_app", "打开记事本", json!({ "name": "notepad" })),
            step("type_text", "输入文本", json!({ "text": text })),
        ],
    ))
}

fn parse_paste_clipboard(input: &str) -> Option<AgentPlan> {
    if !contains_any(
        input,
        &[
            "把剪贴板粘贴到当前窗口",
            "把剪贴板贴到当前窗口",
            "粘贴剪贴板到当前窗口",
            "把剪贴板粘贴到当前页面",
            "把剪贴板贴到当前页面",
        ],
    ) {
        return None;
    }

    Some(plan(
        "把剪贴板粘贴到当前窗口",
        vec![
            step("read_clipboard", "读取剪贴板", json!({})),
            step("send_hotkey", "发送 Ctrl+V", json!({ "keys": ["CTRL", "V"] })),
        ],
    ))
}

fn parse_list_and_focus(input: &str) -> Option<AgentPlan> {
    if !contains_any(input, &["列出窗口", "看看窗口", "看看我现在开了哪些窗口"]) {
        return None;
    }

    let window_title = parse_focus_window_title(input)?;
    Some(plan(
        format!("列出窗口并切到 {window_title}"),
        vec![
            step("list_windows", "列出窗口", json!({})),
            step(
                "focus_window",
                format!("切到 {window_title}"),
                json!({
                    "title": window_title,
                    "match": "contains",
                }),
            ),
        ],
    ))
}

fn contains_any(input: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| input.contains(token))
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

fn single_step_plan(
    task_title: impl Into<String>,
    tool: &str,
    summary: impl Into<String>,
    args: serde_json::Value,
) -> AgentPlan {
    plan(task_title, vec![step(tool, summary, args)])
}

fn parse_window_and_inline_text(input: &str) -> Option<(String, String)> {
    let (window_title, remainder) = parse_focus_window_and_remainder(input)?;
    let text = clean_inline_text(remainder)?;
    Some((window_title, text))
}

fn parse_focus_window_and_remainder(input: &str) -> Option<(String, &str)> {
    for keyword in ["帮我切到", "切换到", "切到", "聚焦到", "聚焦", "切回"] {
        let Some(position) = input.find(keyword) else {
            continue;
        };
        let tail = &input[position + keyword.len()..];
        for connector in ["并输入", "然后输入", "再输入"] {
            if let Some(index) = tail.find(connector) {
                let title = clean_window_title(&tail[..index]);
                if !title.is_empty() {
                    return Some((title, &tail[index + connector.len()..]));
                }
            }
        }
    }

    None
}

fn parse_focus_window_title(input: &str) -> Option<String> {
    for keyword in ["帮我切到", "切换到", "切到", "聚焦到", "聚焦", "切回"] {
        if let Some(position) = input.find(keyword) {
            let tail = input[position + keyword.len()..].trim();
            let title_part = split_by_tokens(tail, &["并输入", "然后输入", "再输入", "并按", "然后按"]);
            let cleaned = clean_window_title(title_part);
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
            matches!(ch, ' ' | '，' | ',' | '。' | '“' | '”' | '"' | '：' | ':') || ch == '\''
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
            let cleaned = clean_inline_text(value)?;
            return Some(cleaned);
        }
    }

    None
}

fn parse_inline_text_after_connector(input: &str) -> Option<String> {
    for connector in ["并输入", "然后输入", "再输入"] {
        if let Some(index) = input.find(connector) {
            return clean_inline_text(&input[index + connector.len()..]);
        }
    }

    None
}

fn clean_inline_text(value: &str) -> Option<String> {
    let cleaned = value
        .trim_matches(|ch: char| {
            matches!(ch, ' ' | '“' | '”' | '"' | '：' | ':') || ch == '\''
        })
        .trim();
    if cleaned.is_empty() || matches!(cleaned, "这段话" | "这些话" | "这句话" | "文本") {
        None
    } else {
        Some(cleaned.to_string())
    }
}

fn split_by_tokens<'a>(input: &'a str, tokens: &[&str]) -> &'a str {
    let mut end = input.len();
    for token in tokens {
        if let Some(index) = input.find(token) {
            end = end.min(index);
        }
    }
    &input[..end]
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
    if contains_any(
        &lowered,
        &["ctrl+v", "ctrl + v", "control+v", "control + v", "按一下 ctrl+v"],
    ) || lowered.contains("ctrl v")
        || lowered.contains("按一下ctrl+v")
    {
        return Some(vec!["CTRL".to_string(), "V".to_string()]);
    }

    if contains_any(
        &lowered,
        &["ctrl+l", "ctrl + l", "control+l", "control + l", "按一下 ctrl+l"],
    ) || lowered.contains("ctrl l")
        || lowered.contains("按一下ctrl+l")
    {
        return Some(vec!["CTRL".to_string(), "L".to_string()]);
    }

    None
}

// NOTE: is_wechat_title 不再使用，主链已迁移到 unified loop
// 保留注释以备参考
// fn is_wechat_title(title: &str) -> bool {
//     let lowered = title.trim().to_lowercase();
//     lowered.contains("微信") || lowered.contains("wechat")
// }
