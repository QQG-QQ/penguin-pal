use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::{
        registry,
        types::ControlRiskLevel,
        windows::adapters::browser::{self, BrowserPlanOutcome},
    },
};

use super::{
    intent,
    planner,
    prompt,
    screen_context::{render_screen_context_for_prompt, ScreenContext},
    vision_types::ScreenContextConsistencyKind,
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
    if matches!(
        context.consistency.status,
        ScreenContextConsistencyKind::HardConflict
    ) {
        let detail = if context.consistency.reasons.is_empty() {
            "当前 UIA 与视觉上下文存在硬冲突，已停止规划。".to_string()
        } else {
            context.consistency.reasons.join("；")
        };
        return Err(format!("当前界面上下文存在硬冲突：{detail}"));
    }

    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| super::types::is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();
    let plan = if let Some(outcome) = browser::try_build_browser_plan(user_input, context) {
        match outcome {
            BrowserPlanOutcome::Plan(plan) => plan,
            BrowserPlanOutcome::Reject(reason) => return Err(reason),
        }
    } else if let Some(plan) = intent::parse_simple_control_plan(user_input) {
        plan
    } else {
        let planner_prompt = prompt::build_screen_planner_prompt(&allowed_tools);
        let planner_input = format!(
            "用户原始请求：\n{}\n\n当前 screen context：\n{}\n\n其中 vision summary 来自独立视觉副通道，而不是主聊天 Provider。你必须先参考 screen context 再规划。如果上下文不足，优先输出 route=chat，而不是盲目操作。",
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
        .await?
    };

    validate_plan_against_context(plan, context)
}

fn validate_plan_against_context(
    plan: AgentPlan,
    context: &ScreenContext,
) -> Result<AgentPlan, String> {
    if plan.steps.is_empty() {
        return Ok(plan);
    }

    let allowed_tools = registry::tool_definitions()
        .into_iter()
        .filter(|tool| super::types::is_agent_tool_allowed(&tool.name))
        .collect::<Vec<_>>();

    for step in &plan.steps {
        let tool = allowed_tools
            .iter()
            .find(|item| item.name == step.tool)
            .ok_or_else(|| format!("规划结果包含未授权工具：{}", step.tool))?;

        match context.consistency.status {
            ScreenContextConsistencyKind::Consistent => {}
            ScreenContextConsistencyKind::UiaOnly
            | ScreenContextConsistencyKind::VisionOnly
            | ScreenContextConsistencyKind::SoftConflict => {
                if matches!(tool.risk_level, ControlRiskLevel::WriteHigh) {
                    return Err(format!(
                        "当前界面上下文只允许只读或低风险动作，不能执行高风险工具：{}。",
                        tool.name
                    ));
                }
            }
            ScreenContextConsistencyKind::HardConflict => {
                return Err("当前界面上下文存在硬冲突，已拒绝生成动作计划。".to_string())
            }
        }
    }

    Ok(plan)
}
