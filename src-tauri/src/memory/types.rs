//! Memory Types - 完整记忆系统 Schema v2
//!
//! 支持 6 种记忆类型和 18 字段标准结构

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Memory 模块 schema 版本
pub const MEMORY_SCHEMA_VERSION: &str = "2.0.0";

// ============================================================================
// 核心枚举类型
// ============================================================================

/// 记忆类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Profile,     // 用户偏好和常用配置
    Episodic,    // 任务历史记录
    Procedural,  // 稳定的操作路径和模式
    Policy,      // 软建议策略
    Semantic,    // 通用知识/项目知识摘要
    Meta,        // 关于记忆本身的记忆
}

/// 记忆作用域
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Global,
    User,
    Project,
    Task,
}

impl Default for MemoryScope {
    fn default() -> Self {
        Self::Global
    }
}

/// 记忆状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,      // 活跃
    Archived,    // 归档
    Deprecated,  // 废弃
    Conflicted,  // 冲突
}

impl Default for MemoryStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// 隐私级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    Public,     // 公开
    Sensitive,  // 敏感
    Forbidden,  // 禁止外发
}

impl Default for PrivacyLevel {
    fn default() -> Self {
        Self::Public
    }
}

/// 记忆操作类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryOperation {
    Merge { target_id: String, source_ids: Vec<String> },
    Promote { id: String, new_type: MemoryType },
    Demote { id: String, new_type: MemoryType },
    Archive { id: String },
    Expire { id: String },
    ConflictMark { ids: Vec<String>, reason: String },
}

// ============================================================================
// 通用记忆条目 - 18 字段标准结构
// ============================================================================

/// 通用记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub memory_type: MemoryType,
    pub content: String,
    pub summary: String,
    pub source: String,              // user, agent_learning, system
    pub created_at: u64,
    pub updated_at: u64,
    pub importance: f64,             // 0.0-1.0
    pub confidence: f64,             // 0.0-1.0
    pub recency: f64,                // 0.0-1.0, 动态计算
    pub frequency: u32,              // 命中次数
    pub scope: MemoryScope,
    pub tags: Vec<String>,
    pub related_memories: Vec<String>,
    pub status: MemoryStatus,
    pub privacy: PrivacyLevel,
    pub ttl: Option<u64>,            // 过期时间 (毫秒时间戳)
    pub retrieval_keys: Vec<String>,
}

impl MemoryEntry {
    pub fn new(
        id: String,
        memory_type: MemoryType,
        content: String,
        summary: String,
        source: String,
    ) -> Self {
        let now = now_millis();
        Self {
            id,
            memory_type,
            content,
            summary,
            source,
            created_at: now,
            updated_at: now,
            importance: 0.5,
            confidence: 0.5,
            recency: 1.0,
            frequency: 0,
            scope: MemoryScope::default(),
            tags: Vec::new(),
            related_memories: Vec::new(),
            status: MemoryStatus::Active,
            privacy: PrivacyLevel::Public,
            ttl: None,
            retrieval_keys: Vec::new(),
        }
    }

    /// 计算综合权重用于检索排序
    pub fn compute_weight(&self) -> f64 {
        self.importance * 0.3 + self.confidence * 0.3 + self.recency * 0.2 + (self.frequency as f64).min(10.0) / 10.0 * 0.2
    }

    /// 更新新鲜度
    pub fn update_recency(&mut self, current_time: u64) {
        let age_hours = (current_time.saturating_sub(self.updated_at)) as f64 / 3_600_000.0;
        // 指数衰减，24小时后约为 0.5
        self.recency = (-age_hours / 24.0).exp();
    }

    /// 检查是否过期
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.ttl.map(|ttl| current_time > ttl).unwrap_or(false)
    }

    /// 增加命中次数
    pub fn hit(&mut self) {
        self.frequency += 1;
        self.updated_at = now_millis();
    }
}

// ============================================================================
// Profile Memory - 用户偏好和常用配置
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileMemory {
    pub schema_version: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub preferred_apps: HashMap<String, u32>,
    pub common_workdirs: Vec<String>,
    pub language_style: LanguageStyle,
    pub risk_preference_low_level_only: bool,
    pub frequently_used_paths: Vec<FrequentPath>,
}

impl ProfileMemory {
    pub fn to_entry(&self) -> MemoryEntry {
        MemoryEntry {
            id: "profile_main".to_string(),
            memory_type: MemoryType::Profile,
            content: serde_json::to_string(self).unwrap_or_default(),
            summary: format!(
                "用户偏好: {} 个常用应用, {} 个工作目录",
                self.preferred_apps.len(),
                self.common_workdirs.len()
            ),
            source: "system".to_string(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            importance: 0.9,
            confidence: 1.0,
            recency: 1.0,
            frequency: 0,
            scope: MemoryScope::User,
            tags: vec!["profile".to_string(), "preference".to_string()],
            related_memories: Vec::new(),
            status: MemoryStatus::Active,
            privacy: PrivacyLevel::Sensitive,
            ttl: None,
            retrieval_keys: vec!["用户".to_string(), "偏好".to_string(), "配置".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LanguageStyle {
    pub preferred_language: String,
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
    pub runtime_context_digest: RuntimeContextDigest,
    pub key_entities: Vec<KeyEntity>,
    pub used_tools: Vec<String>,
    pub used_retry: bool,
    pub used_probe: bool,
    pub steps_taken: usize,
    pub tags: Vec<String>,
}

impl EpisodicEntry {
    pub fn to_memory_entry(&self) -> MemoryEntry {
        MemoryEntry {
            id: self.id.clone(),
            memory_type: MemoryType::Episodic,
            content: serde_json::to_string(self).unwrap_or_default(),
            summary: format!("{}: {}", self.goal, self.final_status),
            source: "agent_learning".to_string(),
            created_at: self.timestamp,
            updated_at: self.timestamp,
            importance: if self.final_status == "completed" { 0.6 } else { 0.8 },
            confidence: 1.0,
            recency: 1.0,
            frequency: 0,
            scope: MemoryScope::Task,
            tags: self.tags.clone(),
            related_memories: Vec::new(),
            status: MemoryStatus::Active,
            privacy: PrivacyLevel::Public,
            ttl: Some(self.timestamp + 30 * 24 * 3600 * 1000), // 30天后过期
            retrieval_keys: vec![self.goal.clone(), self.intent.clone()],
        }
    }
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
    pub target_kind: String,
    pub stable_window_features: Option<StableWindowFeatures>,
    pub stable_element_features: Option<StableElementFeatures>,
    pub preferred_tool_sequence: Vec<String>,
    pub success_count: u32,
    pub failure_count: u32,
    pub confidence: f64,
    pub last_verified_at: u64,
    pub target_pattern: String,
}

impl ProceduralEntry {
    pub fn to_memory_entry(&self) -> MemoryEntry {
        let success_rate = if self.success_count + self.failure_count > 0 {
            self.success_count as f64 / (self.success_count + self.failure_count) as f64
        } else {
            0.5
        };

        MemoryEntry {
            id: self.id.clone(),
            memory_type: MemoryType::Procedural,
            content: serde_json::to_string(self).unwrap_or_default(),
            summary: format!(
                "{}: {} 步骤, {:.0}% 成功率",
                self.target_pattern,
                self.preferred_tool_sequence.len(),
                success_rate * 100.0
            ),
            source: "agent_learning".to_string(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            importance: 0.7,
            confidence: self.confidence,
            recency: 1.0,
            frequency: self.success_count,
            scope: MemoryScope::Global,
            tags: vec!["procedure".to_string(), self.target_kind.clone()],
            related_memories: Vec::new(),
            status: MemoryStatus::Active,
            privacy: PrivacyLevel::Public,
            ttl: None,
            retrieval_keys: vec![self.target_pattern.clone(), self.target_kind.clone()],
        }
    }
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
    pub suggestion_type: String,
    pub scope: String,
    pub value: String,
    pub source: String,
    pub confidence: f64,
    pub approved: bool,
}

impl PolicySuggestion {
    pub fn to_memory_entry(&self) -> MemoryEntry {
        MemoryEntry {
            id: self.id.clone(),
            memory_type: MemoryType::Policy,
            content: serde_json::to_string(self).unwrap_or_default(),
            summary: format!("{}: {}", self.suggestion_type, self.value),
            source: self.source.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            importance: if self.approved { 0.8 } else { 0.5 },
            confidence: self.confidence,
            recency: 1.0,
            frequency: 0,
            scope: MemoryScope::Global,
            tags: vec!["policy".to_string(), self.suggestion_type.clone()],
            related_memories: Vec::new(),
            status: MemoryStatus::Active,
            privacy: PrivacyLevel::Public,
            ttl: None,
            retrieval_keys: vec![self.suggestion_type.clone(), self.scope.clone()],
        }
    }
}

// ============================================================================
// Semantic Memory - 通用知识/项目知识摘要 (NEW)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub schema_version: String,
    pub entries: Vec<SemanticEntry>,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntry {
    pub id: String,
    #[serde(default)]
    pub memory_key: String,
    pub topic: String,
    pub knowledge: String,
    pub source_type: String,  // project_structure, tool_usage, config_file, api_doc
    pub confidence: f64,
    pub created_at: u64,
    pub updated_at: u64,
    pub tags: Vec<String>,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default = "default_mention_count")]
    pub mention_count: u32,
    #[serde(default)]
    pub ttl: Option<u64>,
    #[serde(default)]
    pub status: MemoryStatus,
    #[serde(default)]
    pub conflict_group: Option<String>,
}

impl SemanticEntry {
    pub fn to_memory_entry(&self) -> MemoryEntry {
        MemoryEntry {
            id: self.id.clone(),
            memory_type: MemoryType::Semantic,
            content: self.knowledge.clone(),
            summary: format!("[{}] {}", self.source_type, self.topic),
            source: if self.explicit {
                "conversation_explicit".to_string()
            } else {
                "conversation_inferred".to_string()
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
            importance: if self.explicit { 0.8 } else { 0.45 },
            confidence: self.confidence,
            recency: 1.0,
            frequency: self.mention_count,
            scope: MemoryScope::Project,
            tags: self.tags.clone(),
            related_memories: Vec::new(),
            status: self.status,
            privacy: PrivacyLevel::Public,
            ttl: self.ttl,
            retrieval_keys: vec![self.topic.clone(), self.source_type.clone()],
        }
    }
}

// ============================================================================
// Meta Memory - 关于记忆本身的记忆 (NEW)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaMemory {
    pub schema_version: String,
    pub preferences: Vec<MetaPreference>,
}

impl Default for MetaMemory {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            preferences: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaPreference {
    pub id: String,
    pub category: String,      // retention, retrieval, cleanup, trust
    pub preference: String,
    pub value: Value,
    pub confidence: f64,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub ttl: Option<u64>,
    #[serde(default)]
    pub status: MemoryStatus,
    #[serde(default)]
    pub conflict_group: Option<String>,
}

impl MetaPreference {
    pub fn to_memory_entry(&self) -> MemoryEntry {
        MemoryEntry {
            id: self.id.clone(),
            memory_type: MemoryType::Meta,
            content: serde_json::to_string(self).unwrap_or_default(),
            summary: format!("[{}] {}", self.category, self.preference),
            source: if self.explicit {
                "conversation_explicit".to_string()
            } else {
                "system".to_string()
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
            importance: 0.9,
            confidence: self.confidence,
            recency: 1.0,
            frequency: 0,
            scope: MemoryScope::Global,
            tags: vec!["meta".to_string(), self.category.clone()],
            related_memories: Vec::new(),
            status: self.status,
            privacy: PrivacyLevel::Public,
            ttl: self.ttl,
            retrieval_keys: vec![self.category.clone(), self.preference.clone()],
        }
    }
}

// ============================================================================
// Memory Summary - 用于 prompt 注入
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySummary {
    pub relevant_episodes: Vec<EpisodeSummary>,
    pub relevant_procedures: Vec<ProcedureSummary>,
    pub active_policies: Vec<PolicySummary>,
    pub semantic_context: Vec<SemanticSummary>,
    pub meta_preferences: Vec<MetaSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSummary {
    pub topic: String,
    pub knowledge: String,
    pub relevance_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaSummary {
    pub category: String,
    pub preference: String,
    pub value: String,
    pub confidence: f64,
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
    pub memory_types: Vec<MemoryType>,  // 过滤特定类型
    pub min_importance: Option<f64>,
    pub min_confidence: Option<f64>,
    pub scope: Option<MemoryScope>,
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

/// 生成唯一 ID
pub fn generate_id(prefix: &str) -> String {
    format!("{}_{}", prefix, now_millis())
}

fn default_mention_count() -> u32 {
    1
}

// ============================================================================
// Memory Management View Types
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ManagedMemoryKind {
    Semantic,
    Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedMemoryRecord {
    pub id: String,
    pub memory_type: ManagedMemoryKind,
    pub title: String,
    pub summary: String,
    pub detail: String,
    pub confidence: f64,
    pub explicit: bool,
    pub mention_count: u32,
    pub status: MemoryStatus,
    pub source: String,
    pub updated_at: u64,
    pub expires_at: Option<u64>,
    pub tags: Vec<String>,
    pub conflict_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryManagementStats {
    pub profile_count: usize,
    pub episodic_count: usize,
    pub procedural_count: usize,
    pub policy_count: usize,
    pub semantic_count: usize,
    pub meta_count: usize,
    pub stable_count: usize,
    pub candidate_count: usize,
    pub conflict_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConflictGroup {
    pub id: String,
    pub memory_type: ManagedMemoryKind,
    pub title: String,
    pub entries: Vec<ManagedMemoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryManagementSnapshot {
    pub stats: MemoryManagementStats,
    pub stable_records: Vec<ManagedMemoryRecord>,
    pub candidate_records: Vec<ManagedMemoryRecord>,
    pub conflicts: Vec<MemoryConflictGroup>,
}
