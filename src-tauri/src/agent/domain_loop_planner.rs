use crate::app_state::{DesktopAction, ProviderConfig};

use super::{
    agent_turn::AgentExecutionDomain,
    loop_planner,
    test_loop_planner,
    types::{AgentLoopDecision, AgentTaskRun, RuntimeContext},
    workspace_loop_planner,
};

pub async fn plan_next_domain_action(
    domain: AgentExecutionDomain,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    task: &AgentTaskRun,
    runtime_context: Option<&RuntimeContext>,
    conversation_context: Option<&str>,
    memory_context: Option<&str>,
    workspace_context: Option<&str>,
    default_workdir: Option<&str>,
) -> Result<AgentLoopDecision, String> {
    match domain {
        AgentExecutionDomain::Desktop => {
            let context = runtime_context
                .ok_or_else(|| "desktop domain 缺少 runtime context。".to_string())?;
            loop_planner::plan_next_action(
                provider_config,
                api_key,
                oauth_access_token,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                user_input,
                task,
                context,
                conversation_context,
                memory_context,
            )
            .await
        }
        AgentExecutionDomain::Test => {
            let context = runtime_context
                .ok_or_else(|| "test domain 缺少 runtime context。".to_string())?;
            test_loop_planner::plan_next_test_action(
                provider_config,
                api_key,
                oauth_access_token,
                codex_command,
                codex_home,
                codex_thread_id,
                permission_level,
                allowed_actions,
                user_input,
                context,
                conversation_context,
            )
            .await
        }
        AgentExecutionDomain::Workspace => workspace_loop_planner::plan_next_workspace_action(
            provider_config,
            api_key,
            oauth_access_token,
            codex_command,
            codex_home,
            codex_thread_id,
            permission_level,
            allowed_actions,
            user_input,
            task,
            conversation_context,
            memory_context,
            workspace_context,
            default_workdir.unwrap_or("."),
        )
        .await,
        AgentExecutionDomain::Memory => {
            Err("memory domain 不通过 loop planner 规划。".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::plan_next_domain_action;
    use crate::{
        agent::{
            agent_turn::AgentExecutionDomain,
            types::{AgentTaskRun, RuntimeContext, TopLevelIntent},
        },
        app_state::ProviderConfig,
    };

    #[tokio::test]
    async fn reject_memory_domain_planner() {
        let provider = ProviderConfig::default();
        let mut thread_id = None;
        let task = AgentTaskRun::new_loop(TopLevelIntent::WorkspaceTask, "审查代码", 4, 1);
        let context = RuntimeContext {
            raw_user_input: "审查代码".to_string(),
            normalized_goal: "审查代码".to_string(),
            task_status: crate::agent::types::AgentLoopTaskStatus::Planning,
            active_window: None,
            window_inventory: vec![],
            uia_summary: None,
            vision_summary: None,
            clipboard: None,
            recent_tool_results: vec![],
            recent_observations: vec![],
            discovered_entities: vec![],
            consistency: None,
            last_error: None,
        };
        let error = plan_next_domain_action(
            AgentExecutionDomain::Memory,
            &provider,
            None,
            None,
            None,
            None,
            &mut thread_id,
            2,
            &[],
            "审查代码",
            &task,
            Some(&context),
            None,
            None,
            None,
            None,
        )
        .await
        .expect_err("memory domain should not use loop planner");
        assert!(error.contains("memory domain"));
    }
}
