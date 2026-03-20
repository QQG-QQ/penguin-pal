use serde_json::Value;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
};

use super::{model_adapter, types::{AgentPlan, AgentRoute}};

pub async fn plan_with_model_input(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    planner_prompt: &str,
    planner_input: &str,
) -> Result<AgentPlan, String> {
    let raw = model_adapter::request_structured_agent_output(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        planner_prompt,
        planner_input,
    )
    .await?;

    parse_plan(&raw)
}

pub(crate) fn parse_plan(raw: &str) -> Result<AgentPlan, String> {
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
        AgentRoute::Test => Err("桌面代理规划器不接受 test 路由输出。".to_string()),
        AgentRoute::Workspace => Err("桌面代理规划器不接受 workspace 路由输出。".to_string()),
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
