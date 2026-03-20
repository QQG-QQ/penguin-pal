use serde::Deserialize;
use serde_json::Value;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
};

use super::{model_adapter, prompt, types::TopLevelIntent};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentDecision {
    pub route: TopLevelIntent,
}

pub async fn classify_user_intent(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
) -> Result<IntentDecision, String> {
    let raw = model_adapter::request_structured_agent_output(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &prompt::build_user_intent_classifier_prompt(),
        user_input,
    )
    .await?;

    parse_intent_decision(&raw)
}

fn parse_intent_decision(raw: &str) -> Result<IntentDecision, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("意图分类模型没有返回可解析 JSON：{}", raw.trim()))?;
    serde_json::from_str::<IntentDecision>(&payload)
        .map_err(|error| format!("意图分类 JSON 解析失败：{error}"))
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
