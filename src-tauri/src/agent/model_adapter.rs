use crate::{
    ai::provider,
    app_state::{DesktopAction, ProviderConfig},
};

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
    provider::plan_control_request(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        prompt,
        input,
    )
    .await
}
