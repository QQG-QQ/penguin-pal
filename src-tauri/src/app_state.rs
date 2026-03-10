use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub const HISTORY_LIMIT: usize = 24;
pub const AUDIT_LIMIT: usize = 12;
pub const DEFAULT_OAUTH_REDIRECT_URL: &str = "http://127.0.0.1:8976/oauth/callback";
const STATE_FILE: &str = "assistant-state.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PetMode {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Guarded,
}

impl Default for PetMode {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Mock,
    CodexCli,
    OpenAi,
    Anthropic,
    OpenAiCompatible,
}

impl ProviderKind {
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Mock => "penguin-guardian",
            Self::CodexCli => "gpt-5-codex",
            Self::OpenAi => "gpt-4.1-mini",
            Self::Anthropic => "claude-3-5-sonnet-latest",
            Self::OpenAiCompatible => "llama3.1",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Mock => "Mock",
            Self::CodexCli => "Codex CLI",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::OpenAiCompatible => "OpenAI-Compatible",
        }
    }
}

impl Default for ProviderKind {
    fn default() -> Self {
        Self::Mock
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthMode {
    #[serde(rename = "apiKey")]
    ApiKey,
    #[serde(rename = "oauth", alias = "oAuth", alias = "OAuth")]
    OAuth,
}

impl Default for AuthMode {
    fn default() -> Self {
        Self::ApiKey
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OAuthStatus {
    SignedOut,
    Pending,
    Authorized,
    Error,
}

impl Default for OAuthStatus {
    fn default() -> Self {
        Self::SignedOut
    }
}

pub fn default_system_prompt() -> String {
    "你是一只管理员企鹅桌宠，主要职责是陪伴、对话、提醒和执行经过白名单批准的桌面动作。\
    任何电脑控制都必须经过人工确认，绝不执行自由命令、自由脚本、自由下载或越权操作。\
    回复时优先解释风险与边界，再给出可执行建议。"
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthState {
    pub status: OAuthStatus,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub client_id: Option<String>,
    pub redirect_url: Option<String>,
    pub scopes: Vec<String>,
    pub account_hint: Option<String>,
    pub pending_auth_url: Option<String>,
    pub access_token_loaded: bool,
    pub last_error: Option<String>,
    pub started_at: Option<u64>,
    pub expires_at: Option<u64>,
}

impl Default for OAuthState {
    fn default() -> Self {
        Self {
            status: OAuthStatus::SignedOut,
            authorize_url: None,
            token_url: None,
            client_id: None,
            redirect_url: Some(DEFAULT_OAUTH_REDIRECT_URL.to_string()),
            scopes: vec![],
            account_hint: None,
            pending_auth_url: None,
            access_token_loaded: false,
            last_error: None,
            started_at: None,
            expires_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub allow_network: bool,
    pub voice_reply: bool,
    pub retain_history: bool,
    pub api_key_loaded: bool,
    #[serde(default)]
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub oauth: OAuthState,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::Mock,
            model: ProviderKind::Mock.default_model().to_string(),
            base_url: None,
            system_prompt: default_system_prompt(),
            allow_network: true,
            voice_reply: true,
            retain_history: true,
            api_key_loaded: false,
            auth_mode: AuthMode::ApiKey,
            oauth: OAuthState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: u64,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            id: format!("msg-{}", now_millis()),
            role: role.to_string(),
            content: content.into(),
            created_at: now_millis(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAction {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub risk_level: u8,
    pub minimum_level: u8,
    pub requires_confirmation: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: String,
    pub action: String,
    pub outcome: String,
    pub detail: String,
    pub created_at: u64,
    pub risk_level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStage {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioProfile {
    pub input_mode: String,
    pub output_mode: String,
    pub stages: Vec<AudioStage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConstraintItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConstraintProfile {
    pub label: String,
    pub version: String,
    pub summary: String,
    pub immutable_rules: Vec<AiConstraintItem>,
    pub capability_gates: Vec<AiConstraintItem>,
    pub runtime_boundaries: Vec<AiConstraintItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionApprovalCheck {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionApprovalRequest {
    pub id: String,
    pub action: DesktopAction,
    pub prompt: String,
    pub required_phrase: String,
    pub checks: Vec<ActionApprovalCheck>,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSnapshot {
    pub mode: PetMode,
    pub messages: Vec<ChatMessage>,
    pub provider: ProviderConfig,
    pub permission_level: u8,
    pub allowed_actions: Vec<DesktopAction>,
    pub audit_trail: Vec<AuditEntry>,
    pub audio_profile: AudioProfile,
    pub ai_constraints: AiConstraintProfile,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigInput {
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub allow_network: bool,
    pub voice_reply: bool,
    pub retain_history: bool,
    pub permission_level: u8,
    #[serde(default)]
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub oauth_authorize_url: Option<String>,
    #[serde(default)]
    pub oauth_token_url: Option<String>,
    #[serde(default)]
    pub oauth_client_id: Option<String>,
    #[serde(default)]
    pub oauth_redirect_url: Option<String>,
    #[serde(default)]
    pub oauth_scopes: String,
    pub api_key: Option<String>,
    pub clear_api_key: Option<bool>,
    #[serde(default)]
    pub clear_oauth_token: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub reply: ChatMessage,
    pub provider_label: String,
    pub snapshot: AssistantSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionExecutionResult {
    pub status: String,
    pub message: String,
    pub snapshot: AssistantSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_request: Option<ActionApprovalRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlowResult {
    pub message: String,
    pub authorization_url: Option<String>,
    pub snapshot: AssistantSnapshot,
}

#[derive(Debug, Clone)]
pub struct PendingOAuthState {
    pub state: String,
    pub verifier: String,
    pub authorization_url: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub mode: PetMode,
    pub messages: Vec<ChatMessage>,
    pub provider: ProviderConfig,
    pub permission_level: u8,
    pub audit_trail: Vec<AuditEntry>,
    pub api_key: Option<String>,
    pub oauth_access_token: Option<String>,
    pub oauth_refresh_token: Option<String>,
    pub oauth_access_expires_at: Option<u64>,
    pub oauth_account_hint: Option<String>,
    pub oauth_last_error: Option<String>,
    pub pending_oauth: Option<PendingOAuthState>,
    pub pending_action_approvals: Vec<ActionApprovalRequest>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            mode: PetMode::Idle,
            messages: vec![ChatMessage::assistant(
                "欢迎回来。我已经切到严格白名单模式，先把桌宠 UI、语音入口和安全边界搭好了，再接真实 AI API。",
            )],
            provider: ProviderConfig::default(),
            permission_level: 2,
            audit_trail: vec![AuditEntry {
                id: format!("audit-{}", now_millis()),
                action: "bootstrap".to_string(),
                outcome: "ok".to_string(),
                detail: "PenguinPal 已加载默认安全配置。".to_string(),
                created_at: now_millis(),
                risk_level: 0,
            }],
            api_key: None,
            oauth_access_token: None,
            oauth_refresh_token: None,
            oauth_access_expires_at: None,
            oauth_account_hint: None,
            oauth_last_error: None,
            pending_oauth: None,
            pending_action_approvals: vec![],
        }
    }
}

impl RuntimeState {
    pub fn to_snapshot(
        &self,
        audio_profile: AudioProfile,
        allowed_actions: Vec<DesktopAction>,
        ai_constraints: AiConstraintProfile,
    ) -> AssistantSnapshot {
        let mut provider = self.provider.clone();
        provider.api_key_loaded = self
            .api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty());
        provider.oauth.access_token_loaded = self
            .oauth_access_token
            .as_ref()
            .is_some_and(|token| !token.trim().is_empty());
        provider.oauth.account_hint = self.oauth_account_hint.clone();
        provider.oauth.last_error = self.oauth_last_error.clone();
        provider.oauth.pending_auth_url = self
            .pending_oauth
            .as_ref()
            .map(|pending| pending.authorization_url.clone());
        provider.oauth.started_at = self.pending_oauth.as_ref().map(|pending| pending.created_at);
        provider.oauth.expires_at = self
            .pending_oauth
            .as_ref()
            .map(|pending| pending.expires_at)
            .or(self.oauth_access_expires_at);
        provider.oauth.status = if self.pending_oauth.is_some() {
            OAuthStatus::Pending
        } else if provider.oauth.access_token_loaded {
            OAuthStatus::Authorized
        } else if self.oauth_last_error.is_some() {
            OAuthStatus::Error
        } else {
            OAuthStatus::SignedOut
        };

        AssistantSnapshot {
            mode: self.mode,
            messages: self.messages.clone(),
            provider,
            permission_level: self.permission_level,
            allowed_actions,
            audit_trail: self.audit_trail.clone(),
            audio_profile,
            ai_constraints,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    mode: PetMode,
    messages: Vec<ChatMessage>,
    provider: ProviderConfig,
    permission_level: u8,
    audit_trail: Vec<AuditEntry>,
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join(STATE_FILE))
}

pub fn load(app: &AppHandle) -> Result<RuntimeState, String> {
    let path = state_path(app)?;
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(RuntimeState::default()),
        Err(error) => return Err(error.to_string()),
    };

    let persisted = match serde_json::from_str::<PersistedState>(&content) {
        Ok(state) => state,
        Err(_) => return Ok(RuntimeState::default()),
    };

    let mut runtime = RuntimeState {
        mode: persisted.mode,
        messages: persisted.messages,
        provider: persisted.provider,
        permission_level: 2,
        audit_trail: persisted.audit_trail,
        api_key: None,
        oauth_access_token: None,
        oauth_refresh_token: None,
        oauth_access_expires_at: None,
        oauth_account_hint: None,
        oauth_last_error: None,
        pending_oauth: None,
        pending_action_approvals: vec![],
    };

    if runtime.messages.is_empty() {
        runtime.messages = RuntimeState::default().messages;
    }

    if runtime.audit_trail.is_empty() {
        runtime.audit_trail = RuntimeState::default().audit_trail;
    }

    runtime.mode = PetMode::Idle;
    runtime.provider.api_key_loaded = false;
    runtime.provider.oauth.access_token_loaded = false;
    runtime.provider.oauth.pending_auth_url = None;
    runtime.provider.oauth.account_hint = None;
    runtime.provider.oauth.last_error = None;
    runtime.provider.oauth.started_at = None;
    runtime.provider.oauth.expires_at = None;
    runtime.provider.oauth.status = OAuthStatus::SignedOut;
    runtime.provider.allow_network = true;

    Ok(runtime)
}

pub fn save(app: &AppHandle, runtime: &RuntimeState) -> Result<(), String> {
    let path = state_path(app)?;
    let mut provider = runtime.provider.clone();
    provider.api_key_loaded = false;
    provider.oauth.access_token_loaded = false;
    provider.oauth.account_hint = None;
    provider.oauth.pending_auth_url = None;
    provider.oauth.last_error = None;
    provider.oauth.started_at = None;
    provider.oauth.expires_at = None;
    provider.oauth.status = OAuthStatus::SignedOut;

    let messages = if runtime.provider.retain_history {
        runtime.messages.clone()
    } else {
        vec![ChatMessage::assistant(
            "当前处于临时会话模式，聊天历史不会在下次启动时恢复。",
        )]
    };

    let persisted = PersistedState {
        mode: PetMode::Idle,
        messages,
        provider,
        permission_level: runtime.permission_level.min(2),
        audit_trail: runtime.audit_trail.clone(),
    };

    let content = serde_json::to_string_pretty(&persisted).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}
