//! Memory Module - 持久化记忆系统
//!
//! 提供四类记忆存储和检索：
//! - Profile Memory: 用户偏好和常用配置
//! - Episodic Memory: 任务历史记录
//! - Procedural Memory: 稳定的操作路径和模式
//! - Policy Memory: 软建议策略（可被覆盖）
//!
//! 另有不可变的 Core Policy（硬编码安全策略）。

pub mod core_policy;
pub mod retrieval;
pub mod service;
pub mod store;
pub mod types;
pub mod write_back;

#[cfg(test)]
mod tests;

pub use core_policy::{check_action, get_policy_summary, CorePolicyCheck};
pub use retrieval::{build_memory_summary, render_memory_summary_for_prompt};
pub use service::{MaintenanceResult, MemoryService};
pub use store::MemoryStore;
pub use types::{
    now_millis, EpisodicEntry, EpisodicMemory, MemoryQuery, MemorySummary, PolicyMemory,
    PolicySuggestion, ProceduralEntry, ProceduralMemory, ProfileMemory, WriteBackRequest,
};
pub use write_back::{write_back_task_result, write_confirmation_rejected};
