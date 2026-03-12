use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::registry,
};

use super::{
    intent,
    planner,
    prompt,
    screen_context::{render_screen_context_for_prompt, ScreenContext},
    types::AgentPlan,
};

pub async fn plan_from_screen_context(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    context: &ScreenContext,
) -> Result<AgentPlan, String> {
    if let Some(plan) = intent::parse_simple_control_plan(user_input) {
        return Ok(plan);
    }

    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| super::types::is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let planner_prompt = prompt::build_screen_planner_prompt(&allowed_tools);
    let planner_input = format!(
        "用户原始请求：\n{}\n\n当前 screen context：\n{}\n\n你必须先参考 screen context 再规划。如果上下文不足，优先输出 route=chat，而不是盲目操作。",
        user_input.trim(),
        render_screen_context_for_prompt(context)
    );

    planner::plan_with_model_input(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        &planner_prompt,
        &planner_input,
    )
    .await
}
