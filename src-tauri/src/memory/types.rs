use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Memory 模块 schema 版本
pub const MEMORY_SCHEMA_VERSION: &str = "1.0.0";

// ============================================================================
// Profile Memory - 用户偏好和常用配置
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileMemory {
    pub schema_version: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 常用应用偏好 (app_alias -> usage_count)
    pub preferred_apps: HashMap<String, u32>,
    /// 常用工作目录
    pub common_workdirs: Vec<String>,
    /// 语言风格偏好
    pub language_style: LanguageStyle,
    /// 风险偏好 - 只允许低风险自动执行
    pub risk_preference_low_level_only: bool,
    /// 常用文件路径
    pub frequently_used_paths: Vec<FrequentPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LanguageStyle {
    /// 首选语言 (zh-CN, en-US, etc.)
    pub preferred_language: String,
    /// 回复风格 (concise, detailed, technical)
    pub reply_style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequentPath {
    pub path: String,
    pub usage_count: u32,
    pub last_used_at: u64,
}

// ============================================================================
// Episodic Memory - 任务历史记录
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicMemory {
    pub schema_version: String,
    pub entries: Vec<EpisodicEntry>,
}

impl Default for EpisodicMemory {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicEntry {
    pub id: String,
    pub timestamp: u64,
    pub goal: String,
    pub intent: String,
    pub final_status: String,
    pub failure_reason_code: Option<String>,
    pub failure_stage: Option<String>,
    /// runtime context 摘要 (不存完整 context，只存关键信息)
    pub runtime_context_digest: RuntimeContextDigest,
    /// 关键实体引用
    pub key_entities: Vec<KeyEntity>,
    /// 使用的工具序列
    pub used_tools: Vec<String>,
    pub used_retry: bool,
    pub used_probe: bool,
    /// 步骤数
    pub steps_taken: usize,
    /// 相关性标签 (用于检索)
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeContextDigest {
    pub active_window_title: Option<String>,
    pub active_window_class: Option<String>,
    pub had_vision_context: bool,
    pub had_uia_context: bool,
    pub clipboard_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntity {
    pub entity_type: String,
    pub id: String,
    pub label: String,
}

// ============================================================================
// Procedural Memory - 稳定的操作路径和模式
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralMemory {
    pub schema_version: String,
    pub procedures: Vec<ProceduralEntry>,
}

impl Default for ProceduralMemory {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            procedures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralEntry {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 目标类型 (window, element, file, app)
    pub target_kind: String,
    /// 稳定的窗口特征
    pub stable_window_features: Option<StableWindowFeatures>,
    /// 稳定的元素特征
    pub stable_element_features: Option<StableElementFeatures>,
    /// 首选工具序列
    pub preferred_tool_sequence: Vec<String>,
    /// 成功次数
    pub success_count: u32,
    /// 失败次数
    pub failure_count: u32,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f64,
    /// 最后验证时间
    pub last_verified_at: u64,
    /// 相关目标 (如应用名、窗口标题模式)
    pub target_pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StableWindowFeatures {
    pub title_pattern: String,
    pub class_name: Option<String>,
    pub process_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StableElementFeatures {
    pub automation_id: Option<String>,
    pub name_pattern: Option<String>,
    pub control_type: Option<String>,
    pub class_name: Option<String>,
}

// ============================================================================
// Policy Memory - 软建议策略 (可被覆盖)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyMemory {
    pub schema_version: String,
    pub suggestions: Vec<PolicySuggestion>,
}

impl Default for PolicyMemory {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            suggestions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySuggestion {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 建议类型 (prefer_tool, avoid_action, default_value, etc.)
    pub suggestion_type: String,
    /// 作用域 (global, app:notepad, window:*, etc.)
    pub scope: String,
    /// 建议值
    pub value: String,
    /// 来源 (user, agent_learning, system)
    pub source: String,
    /// 置信度
    pub confidence: f64,
    /// 是否已被用户确认
    pub approved: bool,
}

// ============================================================================
// Memory Summary - 用于 prompt 注入
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySummary {
    /// 相关的 episodic 经验
    pub relevant_episodes: Vec<EpisodeSummary>,
    /// 相关的 procedural 知识
    pub relevant_procedures: Vec<ProcedureSummary>,
    /// 当前适用的 policy 建议
    pub active_policies: Vec<PolicySummary>,
    /// profile 摘要
    pub profile_hints: ProfileHints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeSummary {
    pub goal: String,
    pub final_status: String,
    pub key_insight: String,
    pub relevance_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureSummary {
    pub target_pattern: String,
    pub preferred_approach: String,
    pub confidence: f64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    pub suggestion_type: String,
    pub value: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileHints {
    pub preferred_apps: Vec<String>,
    pub risk_preference: String,
}

// ============================================================================
// Memory Query - 检索参数
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct MemoryQuery {
    pub goal: Option<String>,
    pub intent: Option<String>,
    pub window_title: Option<String>,
    pub app_name: Option<String>,
    pub tags: Vec<String>,
    pub limit: usize,
}

// ============================================================================
// Write-back Request - 写回请求
// ============================================================================

#[derive(Debug, Clone)]
pub struct WriteBackRequest {
    pub task_id: String,
    pub goal: String,
    pub intent: String,
    pub final_status: String,
    pub failure_reason_code: Option<String>,
    pub failure_stage: Option<String>,
    pub runtime_context_digest: RuntimeContextDigest,
    pub key_entities: Vec<KeyEntity>,
    pub used_tools: Vec<String>,
    pub used_retry: bool,
    pub used_probe: bool,
    pub steps_taken: usize,
}

// ============================================================================
// Utility Functions
// ============================================================================

/// 获取当前时间戳 (毫秒)
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
