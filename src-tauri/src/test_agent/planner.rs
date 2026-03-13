use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
    testing::{
        registry,
        types::{TestAssertion, TestCase, TestRunRequest},
    },
};

use super::{
    prompt,
    types::{is_exploratory_step_allowed, PlannedTestRequest},
};

pub async fn plan_test_request(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
) -> Result<TestRunRequest, String> {
    let cases = registry::builtin_cases();
    let planner_prompt = prompt::build_test_agent_prompt(&cases);
    let raw = provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        &planner_prompt,
        user_input,
    )
    .await?;

    parse_planned_request(&raw)
}

fn parse_planned_request(raw: &str) -> Result<TestRunRequest, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("测试规划模型没有返回可解析 JSON：{}", raw.trim()))?;
    let plan: PlannedTestRequest = serde_json::from_str(&payload)
        .map_err(|error| format!("测试规划 JSON 解析失败：{error}"))?;
    validate_plan(plan)
}

fn validate_plan(mut plan: PlannedTestRequest) -> Result<TestRunRequest, String> {
    if plan.title.trim().is_empty() {
        plan.title = "受控智能测试".to_string();
    }

    if plan.max_cases == 0 {
        plan.max_cases = 8;
    }
    plan.max_cases = plan.max_cases.clamp(1, 16);

    if plan.dynamic_cases.len() > 2 {
        return Err("受限探索测试最多只允许 2 个 dynamic case。".to_string());
    }

    for case in &plan.dynamic_cases {
        validate_dynamic_case(case)?;
    }

    if plan.selection.is_none() && plan.dynamic_cases.is_empty() {
        return Err("没有匹配到可执行测试。".to_string());
    }

    Ok(plan.into_run_request())
}

fn validate_dynamic_case(case: &TestCase) -> Result<(), String> {
    if case.id.trim().is_empty() || case.title.trim().is_empty() {
        return Err("dynamic case 缺少 id 或 title。".to_string());
    }

    if case.steps.is_empty() {
        return Err(format!("dynamic case {} 没有步骤。", case.id));
    }

    if case.steps.len() > 4 {
        return Err(format!("dynamic case {} 超过 4 步限制。", case.id));
    }

    if case.max_probes > 1 {
        return Err(format!("dynamic case {} 的 maxProbes 不能超过 1。", case.id));
    }

    for step in &case.steps {
        if !is_exploratory_step_allowed(step) {
            return Err(format!("dynamic case {} 包含未允许的测试步骤。", case.id));
        }
    }

    for assertion in &case.assertions {
        validate_assertion(case, assertion)?;
    }

    Ok(())
}

fn validate_assertion(case: &TestCase, assertion: &TestAssertion) -> Result<(), String> {
    let allowed = [
        "list_windows_non_empty",
        "screen_context_available",
        "vision_status_exposed",
        "consistency_state_known",
        "screen_context_browser_like",
        "var_present",
        "screen_context_active_title_contains_any",
        "last_result_field_contains",
        "last_result_field_non_empty",
    ];
    if allowed.contains(&assertion.kind.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "dynamic case {} 使用了未允许的断言：{}",
            case.id, assertion.kind
        ))
    }
}

fn extract_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value.to_string());
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate)
        .ok()
        .map(|value| value.to_string())
}
