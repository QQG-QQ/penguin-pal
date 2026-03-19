use serde::Deserialize;
use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
};

use super::prompt;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTurnKind {
    Reply,
    DesktopTask,
    TestTask,
    WorkspaceTask,
    MemoryRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnDecision {
    pub kind: AgentTurnKind,
    #[serde(default)]
    pub reply: Option<String>,
    #[serde(default)]
    pub task_title: Option<String>,
}

pub async fn decide_agent_turn(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    conversation_context: Option<&str>,
    task_context: Option<&str>,
    workspace_context: Option<&str>,
    pending_count: usize,
) -> Result<AgentTurnDecision, String> {
    let conversation_section = conversation_context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n"))
        .unwrap_or_default();
    let task_section = task_context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n"))
        .unwrap_or_else(|| "## 当前任务状态\n- 当前没有活动中的 agent 任务。\n".to_string());
    let workspace_section = workspace_context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n"))
        .unwrap_or_else(|| "## 当前工作区\n- 当前没有可用的工作区上下文。\n".to_string());
    let planner_input = format!(
        "{conversation_section}{task_section}{workspace_section}## 待确认动作\n- 当前待确认动作数：{pending_count}\n\n## 当前用户消息\n{message}",
        message = user_input.trim(),
    );

    let raw = provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &prompt::build_agent_turn_prompt(),
        &planner_input,
    )
    .await?;

    parse_agent_turn_decision(&raw)
}

fn parse_agent_turn_decision(raw: &str) -> Result<AgentTurnDecision, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("统一 agent turn 没有返回可解析 JSON：{}", raw.trim()))?;
    let decision = serde_json::from_str::<AgentTurnDecision>(&payload)
        .map_err(|error| format!("统一 agent turn JSON 解析失败：{error}"))?;

    if matches!(decision.kind, AgentTurnKind::Reply)
        && decision
            .reply
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
    {
        return Err("统一 agent turn 选择了 reply，但没有返回 reply 文本。".to_string());
    }

    Ok(decision)
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

#[cfg(test)]
mod tests {
    use super::{parse_agent_turn_decision, AgentTurnKind};

    #[test]
    fn parse_workspace_task_decision() {
        let raw =
            r#"{"kind":"workspace_task","reply":"我先检查仓库状态。","taskTitle":"审查当前项目"}"#;
        let decision = parse_agent_turn_decision(raw).expect("workspace decision should parse");
        assert_eq!(decision.kind, AgentTurnKind::WorkspaceTask);
        assert_eq!(decision.reply.as_deref(), Some("我先检查仓库状态。"));
        assert_eq!(decision.task_title.as_deref(), Some("审查当前项目"));
    }

    #[test]
    fn reject_reply_without_text() {
        let raw = r#"{"kind":"reply"}"#;
        let error = parse_agent_turn_decision(raw).expect_err("reply without text should fail");
        assert!(error.contains("reply"));
    }
}
