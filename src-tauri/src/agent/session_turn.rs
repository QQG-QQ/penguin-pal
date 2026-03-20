use serde::Deserialize;
use serde_json::Value;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
};

use super::{model_adapter, prompt};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTurnKind {
    Reply,
    DesktopAction,
    TestRequest,
    MemoryRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurnDecision {
    pub kind: SessionTurnKind,
    #[serde(default)]
    pub reply: Option<String>,
}

pub async fn decide_session_turn(
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
    pending_count: usize,
) -> Result<SessionTurnDecision, String> {
    let conversation_section = conversation_context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n"))
        .unwrap_or_default();
    let task_section = task_context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n"))
        .unwrap_or_else(|| "## 当前任务状态\n- 当前没有活动中的桌面/测试任务。\n".to_string());
    let planner_input = format!(
        "{conversation_section}{task_section}## 待确认动作\n- 当前待确认动作数：{pending_count}\n\n## 当前用户消息\n{message}",
        message = user_input.trim(),
    );

    let raw = model_adapter::request_structured_agent_output(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &prompt::build_session_turn_prompt(),
        &planner_input,
    )
    .await?;

    parse_session_turn_decision(&raw)
}

fn parse_session_turn_decision(raw: &str) -> Result<SessionTurnDecision, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("统一会话回合没有返回可解析 JSON：{}", raw.trim()))?;
    let decision = serde_json::from_str::<SessionTurnDecision>(&payload)
        .map_err(|error| format!("统一会话回合 JSON 解析失败：{error}"))?;

    if matches!(decision.kind, SessionTurnKind::Reply)
        && decision
            .reply
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
    {
        return Err("统一会话回合选择了 reply，但没有返回 reply 文本。".to_string());
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
