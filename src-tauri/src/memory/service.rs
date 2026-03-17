//! Memory Service - 统一记忆服务层
//!
//! 提供高层 API，封装 store、retrieval、write_back 的交互。

use std::path::PathBuf;
use std::sync::Arc;

use super::core_policy::{self, CorePolicyCheck};
use super::retrieval::{build_memory_summary, render_memory_summary_for_prompt};
use super::store::MemoryStore;
use super::types::{
    now_millis, MemoryQuery, MemorySummary, PolicySuggestion, ProceduralEntry, ProfileMemory,
    WriteBackRequest,
};
use super::write_back;

/// 统一记忆服务
pub struct MemoryService {
    store: Arc<MemoryStore>,
}

impl MemoryService {
    /// 创建新的 MemoryService
    pub fn new(app_data_dir: &PathBuf) -> Self {
        Self {
            store: Arc::new(MemoryStore::new(app_data_dir)),
        }
    }

    /// 获取 store 引用（用于直接访问）
    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    // ========================================================================
    // Load / Save
    // ========================================================================

    /// 加载 Profile Memory
    pub fn load_profile(&self) -> Result<ProfileMemory, String> {
        self.store.load_profile()
    }

    /// 保存 Profile Memory
    pub fn save_profile(&self, profile: &ProfileMemory) -> Result<(), String> {
        self.store.save_profile(profile)
    }

    // ========================================================================
    // Retrieve / Rank
    // ========================================================================

    /// 检索相关记忆并构建摘要
    pub fn retrieve(&self, query: &MemoryQuery) -> Result<MemorySummary, String> {
        let profile = self.store.load_profile()?;
        let episodic = self.store.load_episodic()?;
        let procedural = self.store.load_procedural()?;
        let policy = self.store.load_policy()?;

        Ok(build_memory_summary(
            &profile,
            &episodic,
            &procedural,
            &policy,
            query,
        ))
    }

    /// 渲染记忆摘要为 prompt 文本
    pub fn render_for_prompt(&self, query: &MemoryQuery) -> Result<String, String> {
        let summary = self.retrieve(query)?;
        Ok(render_memory_summary_for_prompt(&summary))
    }

    // ========================================================================
    // Write-back
    // ========================================================================

    /// 写回任务结果
    pub fn write_back(&self, request: WriteBackRequest) -> Result<(), String> {
        write_back::write_back_task_result(&self.store, request)
    }

    /// 写回确认被拒绝的经验
    pub fn write_confirmation_rejected(
        &self,
        goal: &str,
        tool: &str,
        window_title: Option<&str>,
    ) -> Result<(), String> {
        write_back::write_confirmation_rejected(&self.store, goal, tool, window_title)
    }

    // ========================================================================
    // Policy
    // ========================================================================

    /// 检查动作是否被核心策略允许
    pub fn check_core_policy(&self, tool: &str, args: &serde_json::Value) -> CorePolicyCheck {
        core_policy::check_action(tool, args)
    }

    /// 获取核心策略摘要
    pub fn get_core_policy_summary(&self) -> String {
        core_policy::get_policy_summary()
    }

    /// 添加软策略建议
    pub fn add_policy_suggestion(&self, suggestion: PolicySuggestion) -> Result<(), String> {
        self.store.add_policy_suggestion(suggestion)
    }

    // ========================================================================
    // Procedural
    // ========================================================================

    /// 更新或插入 Procedural Entry
    pub fn upsert_procedural(&self, entry: ProceduralEntry) -> Result<(), String> {
        self.store.upsert_procedural_entry(entry)
    }

    // ========================================================================
    // Decay / Downgrade
    // ========================================================================

    /// 衰减过期的 procedural memory 置信度
    pub fn decay_procedural_confidence(&self, age_threshold_hours: u64) -> Result<u32, String> {
        let mut procedural = self.store.load_procedural()?;
        let now = now_millis();
        let threshold_millis = age_threshold_hours * 60 * 60 * 1000;
        let mut decayed_count = 0;

        for entry in &mut procedural.procedures {
            let age = now.saturating_sub(entry.last_verified_at);
            if age > threshold_millis && entry.confidence > 0.1 {
                // 每超过阈值一倍，衰减 0.1
                let decay_factor = (age as f64 / threshold_millis as f64).min(5.0);
                let decay_amount = 0.05 * decay_factor;
                entry.confidence = (entry.confidence - decay_amount).max(0.1);
                decayed_count += 1;
            }
        }

        if decayed_count > 0 {
            self.store.save_procedural(&procedural)?;
        }

        Ok(decayed_count)
    }

    // ========================================================================
    // Merge / Dedupe
    // ========================================================================

    /// 合并重复的 procedural entries
    pub fn merge_procedural_duplicates(&self) -> Result<u32, String> {
        let mut procedural = self.store.load_procedural()?;
        let original_count = procedural.procedures.len();

        // 按 target_pattern 分组
        let mut groups: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, entry) in procedural.procedures.iter().enumerate() {
            groups
                .entry(entry.target_pattern.clone())
                .or_default()
                .push(i);
        }

        // 找出需要合并的组
        let mut indices_to_remove: Vec<usize> = Vec::new();
        for (_pattern, indices) in groups.iter() {
            if indices.len() > 1 {
                // 保留置信度最高的，合并其他
                let mut best_idx = indices[0];
                let mut best_confidence = procedural.procedures[best_idx].confidence;
                for &idx in &indices[1..] {
                    if procedural.procedures[idx].confidence > best_confidence {
                        indices_to_remove.push(best_idx);
                        best_idx = idx;
                        best_confidence = procedural.procedures[idx].confidence;
                    } else {
                        indices_to_remove.push(idx);
                    }
                    // 合并成功/失败计数
                    procedural.procedures[best_idx].success_count +=
                        procedural.procedures[idx].success_count;
                    procedural.procedures[best_idx].failure_count +=
                        procedural.procedures[idx].failure_count;
                }
            }
        }

        // 移除重复项
        indices_to_remove.sort_unstable();
        indices_to_remove.reverse();
        for idx in &indices_to_remove {
            procedural.procedures.remove(*idx);
        }

        let merged_count = (original_count - procedural.procedures.len()) as u32;
        if merged_count > 0 {
            self.store.save_procedural(&procedural)?;
        }

        Ok(merged_count)
    }

    /// 清除低置信度的 procedural entries
    pub fn prune_low_confidence_procedural(&self, threshold: f64) -> Result<u32, String> {
        let mut procedural = self.store.load_procedural()?;
        let original_count = procedural.procedures.len();

        procedural
            .procedures
            .retain(|e| e.confidence >= threshold || e.success_count > 3);

        let pruned_count = (original_count - procedural.procedures.len()) as u32;
        if pruned_count > 0 {
            self.store.save_procedural(&procedural)?;
        }

        Ok(pruned_count)
    }

    // ========================================================================
    // Cache Management
    // ========================================================================

    /// 清除所有内存缓存
    pub fn clear_cache(&self) {
        self.store.clear_cache();
    }

    // ========================================================================
    // Maintenance
    // ========================================================================

    /// 执行定期维护任务
    /// - 衰减过期 procedural memory 置信度
    /// - 合并重复条目
    /// - 清理低置信度条目
    pub fn run_maintenance(&self) -> MaintenanceResult {
        let decay_count = self.decay_procedural_confidence(168).unwrap_or(0); // 一周阈值
        let merge_count = self.merge_procedural_duplicates().unwrap_or(0);
        let prune_count = self.prune_low_confidence_procedural(0.1).unwrap_or(0);

        MaintenanceResult {
            decayed: decay_count,
            merged: merge_count,
            pruned: prune_count,
        }
    }
}

/// 维护任务执行结果
#[derive(Debug, Clone, Default)]
pub struct MaintenanceResult {
    pub decayed: u32,
    pub merged: u32,
    pub pruned: u32,
}

impl MaintenanceResult {
    pub fn total_changes(&self) -> u32 {
        self.decayed + self.merged + self.pruned
    }
}


// ============================================================================
// 便捷函数：用于 agent 模块快速构建查询
// ============================================================================

/// 从任务上下文构建 MemoryQuery
pub fn build_query_from_task(
    goal: &str,
    intent: Option<&str>,
    window_title: Option<&str>,
    app_name: Option<&str>,
) -> MemoryQuery {
    MemoryQuery {
        goal: Some(goal.to_string()),
        intent: intent.map(String::from),
        window_title: window_title.map(String::from),
        app_name: app_name.map(String::from),
        tags: Vec::new(),
        memory_types: Vec::new(),
        min_importance: None,
        min_confidence: None,
        scope: None,
        limit: 5,
    }
}

/// 从任务结果构建 WriteBackRequest
pub fn build_write_back_request(
    task_id: &str,
    goal: &str,
    intent: &str,
    final_status: &str,
    failure_reason_code: Option<&str>,
    failure_stage: Option<&str>,
    window_title: Option<&str>,
    window_class: Option<&str>,
    used_tools: Vec<String>,
    used_retry: bool,
    used_probe: bool,
    steps_taken: usize,
) -> WriteBackRequest {
    use super::types::RuntimeContextDigest;

    WriteBackRequest {
        task_id: task_id.to_string(),
        goal: goal.to_string(),
        intent: intent.to_string(),
        final_status: final_status.to_string(),
        failure_reason_code: failure_reason_code.map(String::from),
        failure_stage: failure_stage.map(String::from),
        runtime_context_digest: RuntimeContextDigest {
            active_window_title: window_title.map(String::from),
            active_window_class: window_class.map(String::from),
            had_vision_context: false,
            had_uia_context: false,
            clipboard_preview: None,
        },
        key_entities: Vec::new(),
        used_tools,
        used_retry,
        used_probe,
        steps_taken,
    }
}
