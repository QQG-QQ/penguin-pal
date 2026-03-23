use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

use crate::agent::{
    types::AgentMessageMeta,
    vision_types::{VisionProviderStatus, VisionProviderStatusKind},
};

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
#[serde(rename_all = "camelCase")]
pub enum VisionChannelKind {
    Disabled,
    OpenAi,
    OpenAiCompatible,
}

impl VisionChannelKind {
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Disabled => "gpt-4.1-mini",
            Self::OpenAi => "gpt-4.1-mini",
            Self::OpenAiCompatible => "gpt-4.1-mini",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "未启用",
            Self::OpenAi => "OpenAI",
            Self::OpenAiCompatible => "OpenAI-Compatible",
        }
    }
}

impl Default for VisionChannelKind {
    fn default() -> Self {
        Self::Disabled
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
    普通聊天时直接回答，不要在每次回复里重复安全边界。\
    只有涉及权限、隐私、电脑控制或受限能力时，才用一句话提醒限制，再给出可执行建议。\
    用户问你是什么模型或如何运行时，简短说明当前接入模型与运行方式。"
        .to_string()
}

fn default_true() -> bool {
    true
}

fn default_auto_update_codex() -> bool {
    true
}

fn default_auto_check_app_update() -> bool {
    true
}

fn migrate_system_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    let legacy_defaults = [
        "你是一只管理员企鹅桌宠，主要职责是陪伴、对话、提醒和执行经过白名单批准的桌面动作。\
    任何电脑控制都必须经过人工确认，绝不执行自由命令、自由脚本、自由下载或越权操作。\
    回复时优先解释风险与边界，再给出可执行建议。",
        "你是一只严格遵守白名单规则的管理员企鹅助手，任何桌面动作都必须经过人工确认。",
    ];

    if trimmed.is_empty() || legacy_defaults.iter().any(|item| *item == trimmed) {
        default_system_prompt()
    } else {
        trimmed.to_string()
    }
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
    #[serde(default)]
    pub voice_input_mode: VoiceInputMode,
    #[serde(default = "default_push_to_talk_shortcut")]
    pub push_to_talk_shortcut: String,
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
            voice_input_mode: VoiceInputMode::default(),
            push_to_talk_shortcut: default_push_to_talk_shortcut(),
            api_key_loaded: false,
            auth_mode: AuthMode::ApiKey,
            oauth: OAuthState::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VoiceInputMode {
    Disabled,
    Continuous,
    PushToTalk,
}

impl Default for VoiceInputMode {
    fn default() -> Self {
        Self::Continuous
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionChannelConfig {
    pub enabled: bool,
    pub kind: VisionChannelKind,
    pub model: String,
    pub base_url: Option<String>,
    pub allow_network: bool,
    pub api_key_loaded: bool,
    pub timeout_ms: u64,
    pub max_image_bytes: u64,
    pub max_image_width: u32,
    pub max_image_height: u32,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Default for VisionChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: VisionChannelKind::Disabled,
            model: VisionChannelKind::OpenAi.default_model().to_string(),
            base_url: None,
            allow_network: true,
            api_key_loaded: false,
            timeout_ms: 12000,
            max_image_bytes: 3 * 1024 * 1024,
            max_image_width: 1600,
            max_image_height: 1200,
            last_error: None,
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
    pub launch_at_startup: bool,
    pub auto_update_codex: bool,
    pub auto_check_app_update: bool,
    pub research: ResearchConfig,
    pub workspace_root: Option<String>,
    pub vision_channel: VisionChannelConfig,
    pub vision_channel_status: VisionProviderStatus,
    pub permission_level: u8,
    pub allowed_actions: Vec<DesktopAction>,
    pub audit_trail: Vec<AuditEntry>,
    pub audio_profile: AudioProfile,
    pub ai_constraints: AiConstraintProfile,
    /// Shell Agent 权限设置
    pub shell_permissions: ShellPermissionSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigInput {
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub allow_network: bool,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default = "default_auto_update_codex")]
    pub auto_update_codex: bool,
    #[serde(default = "default_auto_check_app_update")]
    pub auto_check_app_update: bool,
    #[serde(default)]
    pub research: ResearchConfig,
    pub voice_reply: bool,
    pub retain_history: bool,
    #[serde(default)]
    pub voice_input_mode: VoiceInputMode,
    #[serde(default = "default_push_to_talk_shortcut")]
    pub push_to_talk_shortcut: String,
    #[serde(default)]
    pub workspace_root: Option<String>,
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
    #[serde(default)]
    pub vision_channel: VisionChannelConfigInput,
    /// Shell Agent 权限设置
    #[serde(default)]
    pub shell_permissions: ShellPermissionSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionChannelConfigInput {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub kind: VisionChannelKind,
    pub model: String,
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub allow_network: bool,
    #[serde(default = "default_vision_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_vision_max_image_bytes")]
    pub max_image_bytes: u64,
    #[serde(default = "default_vision_max_image_width")]
    pub max_image_width: u32,
    #[serde(default = "default_vision_max_image_height")]
    pub max_image_height: u32,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: Option<bool>,
}

impl Default for VisionChannelConfigInput {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: VisionChannelKind::Disabled,
            model: VisionChannelKind::OpenAi.default_model().to_string(),
            base_url: None,
            allow_network: true,
            timeout_ms: default_vision_timeout_ms(),
            max_image_bytes: default_vision_max_image_bytes(),
            max_image_width: default_vision_max_image_width(),
            max_image_height: default_vision_max_image_height(),
            api_key: None,
            clear_api_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub startup_popup: bool,
    #[serde(default = "default_true")]
    pub bubble_alerts: bool,
    #[serde(default)]
    pub watchlist: Vec<String>,
    #[serde(default)]
    pub funds: Vec<String>,
    #[serde(default = "default_research_themes")]
    pub themes: Vec<String>,
    #[serde(default)]
    pub habit_notes: String,
    #[serde(default = "default_research_decision_framework")]
    pub decision_framework: String,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            startup_popup: true,
            bubble_alerts: true,
            watchlist: Vec::new(),
            funds: Vec::new(),
            themes: default_research_themes(),
            habit_notes: String::new(),
            decision_framework: default_research_decision_framework(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResearchRuntimeStatus {
    #[serde(default)]
    pub last_daily_brief_day: Option<String>,
    #[serde(default)]
    pub last_alert_fingerprint: Option<String>,
    #[serde(default)]
    pub last_brief_generated_at: Option<u64>,
    #[serde(default)]
    pub last_startup_popup_day: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub reply: ChatMessage,
    pub provider_label: String,
    pub snapshot: AssistantSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentMessageMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_shell_confirmation: Option<PendingShellConfirmationInfo>,
}

/// Shell Agent 待确认命令信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingShellConfirmationInfo {
    pub id: String,
    pub command: String,
    pub risk_description: String,
    pub created_at: u64,
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
    pub session_thread_id: Option<String>,
    pub codex_thread_id: Option<String>,
    pub provider: ProviderConfig,
    pub launch_at_startup: bool,
    pub auto_update_codex: bool,
    pub auto_check_app_update: bool,
    pub research: ResearchConfig,
    pub research_status: ResearchRuntimeStatus,
    pub main_window_position: Option<SavedWindowPosition>,
    pub workspace_root: Option<String>,
    pub vision_channel: VisionChannelConfig,
    pub vision_channel_status: VisionProviderStatus,
    pub permission_level: u8,
    pub audit_trail: Vec<AuditEntry>,
    pub api_key: Option<String>,
    pub vision_api_key: Option<String>,
    pub oauth_access_token: Option<String>,
    pub oauth_refresh_token: Option<String>,
    pub oauth_access_expires_at: Option<u64>,
    pub oauth_account_hint: Option<String>,
    pub oauth_last_error: Option<String>,
    pub pending_oauth: Option<PendingOAuthState>,
    pub pending_action_approvals: Vec<ActionApprovalRequest>,
    /// Shell Agent 权限设置
    pub shell_permissions: ShellPermissionSettings,
    /// Shell Agent 待确认的命令（会话级别，不持久化）
    pub pending_shell_command: Option<PendingShellCommand>,
}

/// Shell Agent 待确认的命令
#[derive(Debug, Clone)]
pub struct PendingShellCommand {
    pub id: String,
    pub command: String,
    pub risk_description: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedWindowPosition {
    pub x: i32,
    pub y: i32,
}

/// Shell Agent 权限设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellPermissionSettings {
    /// 是否启用 Shell Agent
    pub enabled: bool,
    /// 基本执行权限
    pub allow_execute: bool,
    /// 文件修改权限
    pub allow_file_modify: bool,
    /// 文件删除权限
    pub allow_file_delete: bool,
    /// 网络访问权限
    pub allow_network: bool,
    /// 系统操作权限
    pub allow_system: bool,
    /// 权限有效期（小时，0 表示永久）
    pub duration_hours: u64,
}

impl Default for ShellPermissionSettings {
    fn default() -> Self {
        Self {
            enabled: false,          // 默认关闭，需要用户手动开启
            allow_execute: false,
            allow_file_modify: false,
            allow_file_delete: false,
            allow_network: false,
            allow_system: false,
            duration_hours: 24,      // 默认 24 小时
        }
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            mode: PetMode::Idle,
            messages: vec![ChatMessage::assistant(
                "欢迎回来。我已经切到严格白名单模式，先把桌宠 UI、语音入口和安全边界搭好了，再接真实 AI API。",
            )],
            session_thread_id: None,
            codex_thread_id: None,
            provider: ProviderConfig::default(),
            launch_at_startup: false,
            auto_update_codex: true,
            auto_check_app_update: true,
            research: ResearchConfig::default(),
            research_status: ResearchRuntimeStatus::default(),
            main_window_position: None,
            workspace_root: None,
            vision_channel: VisionChannelConfig::default(),
            vision_channel_status: current_vision_channel_status(&VisionChannelConfig::default(), None),
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
            vision_api_key: None,
            oauth_access_token: None,
            oauth_refresh_token: None,
            oauth_access_expires_at: None,
            oauth_account_hint: None,
            oauth_last_error: None,
            pending_oauth: None,
            pending_action_approvals: vec![],
            shell_permissions: ShellPermissionSettings::default(),
            pending_shell_command: None,
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
        let mut vision_channel = self.vision_channel.clone();
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
        vision_channel.api_key_loaded = self
            .vision_api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty());

        AssistantSnapshot {
            mode: self.mode,
            messages: self.messages.clone(),
            provider,
            launch_at_startup: self.launch_at_startup,
            auto_update_codex: self.auto_update_codex,
            auto_check_app_update: self.auto_check_app_update,
            research: self.research.clone(),
            workspace_root: self.workspace_root.clone(),
            vision_channel,
            vision_channel_status: self.vision_channel_status.clone(),
            permission_level: self.permission_level,
            allowed_actions,
            audit_trail: self.audit_trail.clone(),
            audio_profile,
            ai_constraints,
            shell_permissions: self.shell_permissions.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    mode: PetMode,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    session_thread_id: Option<String>,
    #[serde(default)]
    codex_thread_id: Option<String>,
    provider: ProviderConfig,
    #[serde(default)]
    launch_at_startup: bool,
    #[serde(default = "default_auto_update_codex")]
    auto_update_codex: bool,
    #[serde(default = "default_auto_check_app_update")]
    auto_check_app_update: bool,
    #[serde(default)]
    research: ResearchConfig,
    #[serde(default)]
    research_status: ResearchRuntimeStatus,
    #[serde(default)]
    main_window_position: Option<SavedWindowPosition>,
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    vision_channel: VisionChannelConfig,
    permission_level: u8,
    audit_trail: Vec<AuditEntry>,
    /// Shell Agent 权限设置
    #[serde(default)]
    shell_permissions: ShellPermissionSettings,
}

fn default_vision_timeout_ms() -> u64 {
    12_000
}

fn default_push_to_talk_shortcut() -> String {
    "CommandOrControl+Alt+Space".to_string()
}

fn default_research_themes() -> Vec<String> {
    vec![
        "地缘政治".to_string(),
        "财报".to_string(),
        "基金风格".to_string(),
    ]
}

fn default_research_decision_framework() -> String {
    "先看结论和证据，再看反证、风险、失效条件、跟踪指标，最后才决定是否继续研究。"
        .to_string()
}

fn default_vision_max_image_bytes() -> u64 {
    3 * 1024 * 1024
}

fn default_vision_max_image_width() -> u32 {
    1600
}

fn default_vision_max_image_height() -> u32 {
    1200
}

pub fn current_vision_channel_status(
    config: &VisionChannelConfig,
    api_key: Option<&String>,
) -> VisionProviderStatus {
    if !config.enabled || matches!(config.kind, VisionChannelKind::Disabled) {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::Unsupported,
            message: "视觉副通道未启用。".to_string(),
        };
    }

    if !config.allow_network {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::DisabledOffline,
            message: "视觉副通道已禁用网络访问。".to_string(),
        };
    }

    if matches!(config.kind, VisionChannelKind::OpenAi)
        && api_key
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
    {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::Unsupported,
            message: "视觉副通道缺少 OpenAI API Key。".to_string(),
        };
    }

    if let Some(error) = config.last_error.as_ref().filter(|value| !value.trim().is_empty()) {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::AnalysisFailed,
            message: error.clone(),
        };
    }

    VisionProviderStatus {
        kind: VisionProviderStatusKind::Supported,
        message: format!("视觉副通道已启用：{}。", config.kind.label()),
    }
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

    let vision_channel = persisted.vision_channel.clone();
    let vision_channel_status =
        current_vision_channel_status(&persisted.vision_channel, None);
    let mut runtime = RuntimeState {
        mode: persisted.mode,
        messages: persisted.messages,
        session_thread_id: persisted.session_thread_id,
        codex_thread_id: persisted.codex_thread_id,
        provider: persisted.provider,
        launch_at_startup: persisted.launch_at_startup,
        auto_update_codex: persisted.auto_update_codex,
        auto_check_app_update: persisted.auto_check_app_update,
        research: persisted.research,
        research_status: persisted.research_status,
        main_window_position: persisted.main_window_position,
        workspace_root: persisted.workspace_root,
        vision_channel,
        vision_channel_status,
        permission_level: 2,
        audit_trail: persisted.audit_trail,
        api_key: None,
        vision_api_key: None,
        oauth_access_token: None,
        oauth_refresh_token: None,
        oauth_access_expires_at: None,
        oauth_account_hint: None,
        oauth_last_error: None,
        pending_oauth: None,
        pending_action_approvals: vec![],
        shell_permissions: persisted.shell_permissions,
        pending_shell_command: None,
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
    runtime.provider.system_prompt = migrate_system_prompt(&runtime.provider.system_prompt);
    runtime.provider.push_to_talk_shortcut = if runtime.provider.push_to_talk_shortcut.trim().is_empty() {
        default_push_to_talk_shortcut()
    } else {
        runtime.provider.push_to_talk_shortcut.trim().to_string()
    };
    runtime.vision_channel.api_key_loaded = false;
    runtime.vision_channel.last_error = None;
    runtime.vision_channel_status =
        current_vision_channel_status(&runtime.vision_channel, runtime.vision_api_key.as_ref());

    Ok(runtime)
}

pub fn save(app: &AppHandle, runtime: &RuntimeState) -> Result<(), String> {
    let path = state_path(app)?;
    let mut provider = runtime.provider.clone();
    let mut vision_channel = runtime.vision_channel.clone();
    provider.api_key_loaded = false;
    provider.oauth.access_token_loaded = false;
    provider.oauth.account_hint = None;
    provider.oauth.pending_auth_url = None;
    provider.oauth.last_error = None;
    provider.oauth.started_at = None;
    provider.oauth.expires_at = None;
    provider.oauth.status = OAuthStatus::SignedOut;
    vision_channel.api_key_loaded = false;

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
        session_thread_id: if runtime.provider.retain_history {
            runtime.session_thread_id.clone()
        } else {
            None
        },
        codex_thread_id: if runtime.provider.retain_history {
            runtime.codex_thread_id.clone()
        } else {
            None
        },
        provider,
        launch_at_startup: runtime.launch_at_startup,
        auto_update_codex: runtime.auto_update_codex,
        auto_check_app_update: runtime.auto_check_app_update,
        research: runtime.research.clone(),
        research_status: runtime.research_status.clone(),
        main_window_position: runtime.main_window_position.clone(),
        workspace_root: runtime.workspace_root.clone(),
        vision_channel,
        permission_level: runtime.permission_level.min(2),
        audit_trail: runtime.audit_trail.clone(),
        shell_permissions: runtime.shell_permissions.clone(),
    };

    let content = serde_json::to_string_pretty(&persisted).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}
