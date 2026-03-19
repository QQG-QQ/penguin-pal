//! Memory Module - 持久化记忆系统 v2
//!
//! 支持 6 种记忆类型：
//! - Profile Memory: 用户偏好和常用配置
//! - Episodic Memory: 任务历史记录
//! - Procedural Memory: 稳定的操作路径和模式
//! - Policy Memory: 软建议策略（可被覆盖）
//! - Semantic Memory: 通用知识/项目知识摘要
//! - Meta Memory: 关于记忆本身的记忆
//!
//! 另有不可变的 Core Policy（硬编码安全策略）。

#![allow(unused_imports)]

pub mod core_policy;
pub mod retrieval;
pub mod service;
pub mod store;
pub mod types;
pub mod write_back;

#[cfg(test)]
mod tests;

// 核心类型导出
pub use types::{
    // 枚举
    MemoryType,
    MemoryScope,
    MemoryStatus,
    PrivacyLevel,
    MemoryOperation,
    // 通用记忆条目
    MemoryEntry,
    // 6 种记忆类型
    ProfileMemory,
    EpisodicMemory,
    EpisodicEntry,
    ProceduralMemory,
    ProceduralEntry,
    PolicyMemory,
    PolicySuggestion,
    SemanticMemory,
    SemanticEntry,
    MetaMemory,
    MetaPreference,
    ManagedMemoryKind,
    ManagedMemoryRecord,
    MemoryManagementStats,
    MemoryConflictGroup,
    MemoryManagementSnapshot,
    // 辅助类型
    LanguageStyle,
    FrequentPath,
    RuntimeContextDigest,
    KeyEntity,
    StableWindowFeatures,
    StableElementFeatures,
    // Summary 类型
    MemorySummary,
    EpisodeSummary,
    ProcedureSummary,
    PolicySummary,
    SemanticSummary,
    MetaSummary,
    ProfileHints,
    // Query 和 WriteBack
    MemoryQuery,
    WriteBackRequest,
    // 工具函数
    now_millis,
    generate_id,
};

// Core Policy 导出
pub use core_policy::{check_action, get_policy_summary, CorePolicyCheck};

// Retrieval 导出
pub use retrieval::{build_memory_summary, render_memory_summary_for_prompt};

// Service 导出
pub use service::{MaintenanceResult, MemoryService};

// Store 导出
pub use store::MemoryStore;

// WriteBack 导出
pub use write_back::{write_back_task_result, write_confirmation_rejected};
