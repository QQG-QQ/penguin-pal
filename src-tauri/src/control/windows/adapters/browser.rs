use serde_json::json;

use crate::agent::{
    screen_context::{ScreenContext, ScreenRect},
    types::{AgentPlan, AgentRoute, AgentToolStep},
    vision_types::ScreenContextConsistencyKind,
};

const BROWSER_TITLE_CANDIDATES: &[&str] = &[
    "Chrome",
    "Google Chrome",
    "Edge",
    "Microsoft Edge",
    "Firefox",
    "Brave",
    "Opera",
    "Vivaldi",
    "Chromium",
    "浏览器",
];

pub enum BrowserPlanOutcome {
    Plan(AgentPlan),
    Reject(String),
}

pub fn try_build_browser_plan(user_input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_lowercase();
    if !looks_like_browser_request(&lowered, context) {
        return None;
    }

    if is_forbidden_browser_request(&lowered) {
        return Some(BrowserPlanOutcome::Reject(
            "browser automation v1 只支持打开浏览器、聚焦窗口、新建标签页、聚焦地址栏、输入/粘贴文本、基础滚动和受控页面中间点击；不支持登录、支付、下载、安装或复杂表单提交。"
                .to_string(),
        ));
    }

    if lowered.contains("打开浏览器")
        && contains_any(
            &lowered,
            &[
                "输入",
                "粘贴",
                "地址栏",
                "ctrl+l",
                "ctrl+t",
                "回车",
                "标签页",
                "滚动",
                "点击",
                "点一下",
            ],
        )
    {
        return Some(BrowserPlanOutcome::Reject(
            "browser automation v1 仍遵守 2~4 步约束；“打开浏览器并继续输入/导航”这类组合暂不支持。请先说“打开浏览器”，等浏览器起来后再继续下一句。".to_string(),
        ));
    }

    if let Some(outcome) = parse_center_click(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_scroll(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_focus_and_paste(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_focus_and_input(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_focus_and_ctrl_l(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_new_tab(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_focus_address_bar(trimmed, context) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_focus_browser(trimmed) {
        return Some(outcome);
    }

    if let Some(outcome) = parse_open_browser(trimmed) {
        return Some(outcome);
    }

    Some(BrowserPlanOutcome::Reject(
        "当前浏览器请求不在 v1 支持范围内。请改成更明确的说法，例如“打开浏览器”“切到浏览器并输入 https://example.com”“聚焦浏览器地址栏”或“向下滚动页面”。"
            .to_string(),
    ))
}

fn parse_open_browser(input: &str) -> Option<BrowserPlanOutcome> {
    if !contains_any(input, &["打开浏览器", "启动浏览器"]) {
        return None;
    }

    Some(BrowserPlanOutcome::Plan(single_step_plan(
        "打开浏览器",
        "open_app",
        "打开浏览器",
        json!({ "name": "browser" }),
    )))
}

fn parse_focus_browser(input: &str) -> Option<BrowserPlanOutcome> {
    if !contains_any(input, &["切到浏览器", "切换到浏览器", "聚焦浏览器", "切回浏览器"]) {
        return None;
    }

    Some(BrowserPlanOutcome::Plan(plan(
        "切到浏览器",
        focus_browser_steps(),
    )))
}

fn parse_new_tab(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    if !contains_any(input, &["新建标签页", "新建一个标签页", "打开新标签页", "开一个新标签页", "新建标签"]) {
        return None;
    }

    let mut steps = ensure_browser_focus_steps(context);
    steps.push(step(
        "send_hotkey",
        "发送 Ctrl+T",
        json!({ "keys": ["CTRL", "T"] }),
    ));
    Some(BrowserPlanOutcome::Plan(plan("新建浏览器标签页", steps)))
}

fn parse_focus_address_bar(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    if !contains_any(
        input,
        &["聚焦浏览器地址栏", "聚焦地址栏", "切到浏览器地址栏", "激活浏览器地址栏"],
    ) {
        return None;
    }

    Some(BrowserPlanOutcome::Plan(build_focus_address_bar_plan(context)))
}

fn parse_focus_and_ctrl_l(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    let lowered = input.to_lowercase();
    if !lowered.contains("浏览器")
        || !contains_any(&lowered, &["ctrl+l", "ctrl + l", "ctrl l", "control+l", "control + l"])
    {
        return None;
    }

    Some(BrowserPlanOutcome::Plan(build_focus_address_bar_plan(context)))
}

fn parse_focus_and_input(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    let lowered = input.to_lowercase();
    if !lowered.contains("浏览器") && !lowered.contains("地址栏") {
        return None;
    }
    let text = parse_input_text(input)?;
    let mut steps = build_focus_address_bar_steps(context);
    steps.push(step("type_text", "输入文本", json!({ "text": text })));
    Some(BrowserPlanOutcome::Plan(plan(
        "切到浏览器并输入文本",
        steps,
    )))
}

fn parse_focus_and_paste(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    if !contains_any(
        input,
        &[
            "切到浏览器并粘贴剪贴板",
            "把剪贴板粘贴到浏览器地址栏",
            "把剪贴板粘贴到浏览器",
            "向浏览器地址栏粘贴剪贴板",
        ],
    ) {
        return None;
    }

    let mut steps = build_focus_address_bar_steps(context);
    steps.push(step(
        "send_hotkey",
        "发送 Ctrl+V",
        json!({ "keys": ["CTRL", "V"] }),
    ));
    Some(BrowserPlanOutcome::Plan(plan(
        "切到浏览器并粘贴剪贴板",
        steps,
    )))
}

fn parse_scroll(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    let lowered = input.to_lowercase();
    let mentions_browser_surface =
        lowered.contains("浏览器") || lowered.contains("网页") || lowered.contains("地址栏") || lowered.contains("标签页");
    let mentions_scroll = lowered.contains("滚动");
    if !mentions_scroll || (!mentions_browser_surface && !active_window_is_browser_like(context)) {
        return None;
    }

    let delta = if lowered.contains("向上") || lowered.contains("往上") {
        120
    } else {
        -120
    };

    let mut steps = ensure_browser_focus_steps(context);
    steps.push(step(
        "scroll_at",
        if delta > 0 { "向上滚动页面" } else { "向下滚动页面" },
        json!({ "delta": delta, "steps": 4 }),
    ));
    Some(BrowserPlanOutcome::Plan(plan("滚动浏览器页面", steps)))
}

fn parse_center_click(input: &str, context: &ScreenContext) -> Option<BrowserPlanOutcome> {
    if !contains_any(
        input,
        &[
            "在浏览器页面中间点一下",
            "在浏览器页面中央点一下",
            "在页面中间点一下",
            "在页面中央点一下",
        ],
    ) {
        return None;
    }

    if !active_window_is_browser_like(context) {
        return Some(BrowserPlanOutcome::Reject(
            "受控页面点击只支持当前前台浏览器窗口。请先切到浏览器后再试。".to_string(),
        ));
    }

    if !matches!(context.consistency.status, ScreenContextConsistencyKind::Consistent) {
        return Some(BrowserPlanOutcome::Reject(
            "当前浏览器界面上下文不够稳定，已拒绝页面点击。请先让浏览器窗口保持前台并重试。".to_string(),
        ));
    }

    let Some(bounds) = context.active_window.bounds.as_ref() else {
        return Some(BrowserPlanOutcome::Reject(
            "当前浏览器窗口缺少可用尺寸信息，无法计算受控点击位置。".to_string(),
        ));
    };
    let Some((x, y)) = safe_page_center_click(bounds) else {
        return Some(BrowserPlanOutcome::Reject(
            "当前浏览器窗口尺寸异常，无法计算受控点击位置。".to_string(),
        ));
    };

    Some(BrowserPlanOutcome::Plan(single_step_plan(
        "在浏览器页面中间点击",
        "click_at",
        "点击页面中间安全区域",
        json!({ "x": x, "y": y, "button": "left" }),
    )))
}

fn build_focus_address_bar_plan(context: &ScreenContext) -> AgentPlan {
    plan("聚焦浏览器地址栏", build_focus_address_bar_steps(context))
}

fn build_focus_address_bar_steps(context: &ScreenContext) -> Vec<AgentToolStep> {
    let mut steps = ensure_browser_focus_steps(context);
    steps.push(step(
        "send_hotkey",
        "发送 Ctrl+L",
        json!({ "keys": ["CTRL", "L"] }),
    ));
    steps
}

fn ensure_browser_focus_steps(context: &ScreenContext) -> Vec<AgentToolStep> {
    if active_window_is_browser_like(context) {
        Vec::new()
    } else {
        focus_browser_steps()
    }
}

fn focus_browser_steps() -> Vec<AgentToolStep> {
    vec![
        step("list_windows", "列出窗口", json!({})),
        step(
            "focus_window",
            "切到浏览器窗口",
            json!({
                "windowCategory": "browser",
                "titleCandidates": BROWSER_TITLE_CANDIDATES,
                "match": "contains",
            }),
        ),
    ]
}

fn active_window_is_browser_like(context: &ScreenContext) -> bool {
    let title = context.active_window.title.to_lowercase();
    if BROWSER_TITLE_CANDIDATES
        .iter()
        .map(|item| item.to_lowercase())
        .any(|candidate| title.contains(&candidate))
    {
        return true;
    }

    if let Some(class_name) = &context.active_window.class_name {
        let lowered = class_name.to_lowercase();
        if ["chrome_widgetwin", "mozillawindowclass"]
            .iter()
            .any(|hint| lowered.contains(hint))
        {
            return true;
        }
    }

    if let Some(vision) = &context.vision {
        let window_kind = vision.window_kind.to_lowercase();
        if window_kind.contains("browser") || window_kind.contains("web") {
            return true;
        }

        if vision
            .primary_regions
            .iter()
            .any(|region| matches!(region.region_type.as_str(), "address_bar" | "tab_strip" | "toolbar"))
        {
            return true;
        }
    }

    false
}

fn safe_page_center_click(bounds: &ScreenRect) -> Option<(i64, i64)> {
    if bounds.width < 200 || bounds.height < 200 {
        return None;
    }

    let min_x = 120_i64.min(bounds.width / 2);
    let max_x = (bounds.width - 120).max(min_x);
    let min_y = 160_i64.min(bounds.height / 2);
    let max_y = (bounds.height - 100).max(min_y);
    let x = (bounds.width / 2).clamp(min_x, max_x);
    let y = ((bounds.height * 2) / 3).clamp(min_y, max_y);
    Some((x, y))
}

fn parse_input_text(input: &str) -> Option<String> {
    for connector in ["并输入", "然后输入", "再输入", "地址栏输入", "输入到地址栏"] {
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
    if cleaned.is_empty() || matches!(cleaned, "这段话" | "这些话" | "这句话" | "文本" | "网址" | "链接")
    {
        None
    } else {
        Some(cleaned.to_string())
    }
}

fn looks_like_browser_request(input: &str, context: &ScreenContext) -> bool {
    contains_any(
        input,
        &[
            "浏览器",
            "地址栏",
            "标签页",
            "ctrl+t",
            "ctrl+l",
            "网址",
            "网页",
        ],
    ) || (active_window_is_browser_like(context)
        && contains_any(input, &["滚动", "点一下", "中央", "中间", "页面"]))
}

fn is_forbidden_browser_request(input: &str) -> bool {
    contains_any(input, &["登录", "支付", "验证码", "表单", "提交", "下载", "安装"])
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

fn single_step_plan(
    task_title: impl Into<String>,
    tool: &str,
    summary: impl Into<String>,
    args: serde_json::Value,
) -> AgentPlan {
    plan(task_title, vec![step(tool, summary, args)])
}

fn step(tool: &str, summary: impl Into<String>, args: serde_json::Value) -> AgentToolStep {
    AgentToolStep {
        id: None,
        summary: Some(summary.into()),
        tool: tool.to_string(),
        args,
    }
}
