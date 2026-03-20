use serde_json::Value;

use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
};

const STRUCTURED_OUTPUT_REPAIR_SUFFIX: &str = "你必须只返回一个 JSON 对象。不要输出 markdown、不要输出代码块、不要输出解释文字、不要输出多个 JSON。若不确定，也要返回一个符合 schema 的 JSON 对象。";

pub async fn request_structured_agent_output(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    prompt: &str,
    input: &str,
) -> Result<String, String> {
    let strict_prompt = format!("{prompt}\n\n{STRUCTURED_OUTPUT_REPAIR_SUFFIX}");
    let first = provider::plan_control_request(
        provider_config,
        api_key.clone(),
        oauth_access_token.clone(),
        codex_command.clone(),
        codex_home.clone(),
        codex_thread_id,
        permission_level,
        allowed_actions,
        &strict_prompt,
        input,
    )
    .await?;

    let normalized_first = normalize_structured_output(&first);
    if contains_json_object(&normalized_first) {
        return Ok(normalized_first);
    }

    let repair_input = format!(
        "上一次结构化输出不符合要求。\n\
原始任务输入：\n{input}\n\n\
上一次模型输出：\n{first}\n\n\
现在请修复为严格 JSON：只返回一个 JSON 对象，不要附带任何解释。"
    );

    let repaired = provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &strict_prompt,
        &repair_input,
    )
    .await?;

    let normalized_repaired = normalize_structured_output(&repaired);
    if contains_json_object(&normalized_repaired) {
        Ok(normalized_repaired)
    } else {
        Err(format!(
            "结构化输出修复失败，模型仍未返回可解析 JSON：{}",
            repaired.trim()
        ))
    }
}

fn normalize_structured_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(value) = extract_json_object(trimmed) {
        return value.to_string();
    }

    if trimmed.starts_with("```") {
        let without_fence = trimmed
            .lines()
            .filter(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
        let without_fence = without_fence.trim();
        if let Some(value) = extract_json_object(without_fence) {
            return value.to_string();
        }
        return without_fence.to_string();
    }

    trimmed.to_string()
}

fn contains_json_object(raw: &str) -> bool {
    extract_json_object(raw).is_some()
}

fn extract_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(Value::Object(value)) = serde_json::from_str::<Value>(trimmed) {
        return Some(Value::Object(value));
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let candidate = &trimmed[start..=end];
    match serde_json::from_str::<Value>(candidate).ok()? {
        Value::Object(value) => Some(Value::Object(value)),
        _ => None,
    }
}
