use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    prompt,
    types::{is_agent_tool_allowed, AgentPlan, AgentRoute},
};

pub async fn plan_with_model(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
) -> Result<AgentPlan, String> {
    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let prompt = prompt::build_planner_prompt(&allowed_tools);
    let raw = provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        &prompt,
        user_input,
    )
    .await?;

    parse_plan(&raw)
}

fn parse_plan(raw: &str) -> Result<AgentPlan, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("规划模型没有返回可解析的 JSON：{}", raw.trim()))?;
    let plan: AgentPlan =
        serde_json::from_str(&payload).map_err(|error| format!("动作规划 JSON 解析失败：{error}"))?;

    match plan.route {
        AgentRoute::Chat => Ok(AgentPlan {
            route: AgentRoute::Chat,
            task_title: None,
            stop_on_error: true,
            steps: vec![],
        }),
        AgentRoute::Control => {
            if plan.steps.is_empty() {
                return Err("规划模型返回了 control，但没有提供 steps。".to_string());
            }

            if plan.steps.len() > 4 {
                return Err("第一版桌面代理只允许最多 4 个规划步骤。".to_string());
            }

            Ok(plan)
        }
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
