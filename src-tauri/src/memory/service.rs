//! Memory Service - 统一记忆服务层
//!
//! 提供高层 API，封装 store、retrieval、write_back 的交互。

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::collections::BTreeMap;

use super::core_policy::{self, CorePolicyCheck};
use super::retrieval::{build_memory_summary, render_memory_summary_for_prompt};
use super::store::MemoryStore;
use super::types::{
    now_millis, ManagedMemoryKind, ManagedMemoryRecord, MemoryConflictGroup,
    MemoryManagementSnapshot, MemoryManagementStats, MemoryQuery, MemoryStatus, MemorySummary,
    MetaMemory, MetaPreference, PolicySuggestion, ProceduralEntry, ProfileMemory,
    SemanticEntry, SemanticMemory, WriteBackRequest,
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
        self.store.as_ref()
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

    /// 加载 Semantic Memory
    pub fn load_semantic(&self) -> Result<SemanticMemory, String> {
        self.store.load_semantic()
    }

    /// 加载 Meta Memory
    pub fn load_meta(&self) -> Result<MetaMemory, String> {
        self.store.load_meta()
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
        let semantic = self.store.load_semantic()?;
        let meta = self.store.load_meta()?;

        Ok(build_memory_summary(
            &profile,
            &episodic,
            &procedural,
            &policy,
            &semantic,
            &meta,
            query,
        ))
    }

    /// 渲染记忆摘要为 prompt 文本
    pub fn render_for_prompt(&self, query: &MemoryQuery) -> Result<String, String> {
        let summary = self.retrieve(query)?;
        Ok(render_memory_summary_for_prompt(&summary))
    }

    /// 获取记忆管理视图快照
    pub fn management_snapshot(&self) -> Result<MemoryManagementSnapshot, String> {
        let profile = self.store.load_profile()?;
        let episodic = self.store.load_episodic()?;
        let procedural = self.store.load_procedural()?;
        let policy = self.store.load_policy()?;
        let semantic = self.store.load_semantic()?;
        let meta = self.store.load_meta()?;

        let mut stable_records = Vec::new();
        let mut candidate_records = Vec::new();
        let mut semantic_conflicts: BTreeMap<String, Vec<ManagedMemoryRecord>> = BTreeMap::new();
        let mut meta_conflicts: BTreeMap<String, Vec<ManagedMemoryRecord>> = BTreeMap::new();

        for entry in &semantic.entries {
            let record = managed_semantic_record(entry);
            match entry.status {
                MemoryStatus::Active => {
                    if entry.explicit || entry.mention_count >= 2 {
                        stable_records.push(record);
                    } else {
                        candidate_records.push(record);
                    }
                }
                MemoryStatus::Conflicted => {
                    if let Some(group) = &entry.conflict_group {
                        semantic_conflicts
                            .entry(group.clone())
                            .or_default()
                            .push(record);
                    }
                }
                MemoryStatus::Archived | MemoryStatus::Deprecated => {}
            }
        }

        for entry in &meta.preferences {
            let record = managed_meta_record(entry);
            match entry.status {
                MemoryStatus::Active => stable_records.push(record),
                MemoryStatus::Conflicted => {
                    if let Some(group) = &entry.conflict_group {
                        meta_conflicts.entry(group.clone()).or_default().push(record);
                    }
                }
                MemoryStatus::Archived | MemoryStatus::Deprecated => {}
            }
        }

        stable_records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        candidate_records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        let mut conflicts = Vec::new();
        for (group, mut entries) in semantic_conflicts {
            entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            let title = entries
                .first()
                .map(|entry| entry.title.clone())
                .unwrap_or_else(|| "语义记忆冲突".to_string());
            conflicts.push(MemoryConflictGroup {
                id: group,
                memory_type: ManagedMemoryKind::Semantic,
                title,
                entries,
            });
        }
        for (group, mut entries) in meta_conflicts {
            entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            let title = entries
                .first()
                .map(|entry| entry.title.clone())
                .unwrap_or_else(|| "交互偏好冲突".to_string());
            conflicts.push(MemoryConflictGroup {
                id: group,
                memory_type: ManagedMemoryKind::Meta,
                title,
                entries,
            });
        }
        conflicts.sort_by(|a, b| a.title.cmp(&b.title));

        Ok(MemoryManagementSnapshot {
            stats: MemoryManagementStats {
                profile_count: profile.preferred_apps.len()
                    + profile.common_workdirs.len()
                    + profile.frequently_used_paths.len(),
                episodic_count: episodic.entries.len(),
                procedural_count: procedural.procedures.len(),
                policy_count: policy.suggestions.len(),
                semantic_count: semantic.entries.len(),
                meta_count: meta.preferences.len(),
                stable_count: stable_records.len(),
                candidate_count: candidate_records.len(),
                conflict_count: conflicts.len(),
            },
            stable_records,
            candidate_records,
            conflicts,
        })
    }

    pub fn delete_managed_memory(
        &self,
        kind: ManagedMemoryKind,
        id: &str,
    ) -> Result<MemoryManagementSnapshot, String> {
        let removed = match kind {
            ManagedMemoryKind::Semantic => self.store.delete_semantic_entry(id)?,
            ManagedMemoryKind::Meta => self.store.delete_meta_preference(id)?,
        };

        if !removed {
            return Err("未找到要删除的记忆条目".to_string());
        }

        self.management_snapshot()
    }

    pub fn promote_memory_candidate(
        &self,
        id: &str,
    ) -> Result<MemoryManagementSnapshot, String> {
        if !self.store.promote_semantic_entry(id)? {
            return Err("未找到可提升的候选记忆".to_string());
        }

        self.management_snapshot()
    }

    pub fn resolve_memory_conflict(
        &self,
        kind: ManagedMemoryKind,
        group: &str,
        keep_id: &str,
    ) -> Result<MemoryManagementSnapshot, String> {
        let resolved = match kind {
            ManagedMemoryKind::Semantic => self.store.resolve_semantic_conflict(group, keep_id)?,
            ManagedMemoryKind::Meta => self.store.resolve_meta_conflict(group, keep_id)?,
        };

        if !resolved {
            return Err("未找到需要处理的冲突记忆".to_string());
        }

        self.management_snapshot()
    }

    // ========================================================================
    // Write-back
    // ========================================================================

    /// 写回任务结果
    pub fn write_back(&self, request: WriteBackRequest) -> Result<(), String> {
        write_back::write_back_task_result(&self.store, request)
    }

    /// 写回普通对话中的长期记忆
    pub fn write_conversation_turn(
        &self,
        user_input: &str,
        assistant_reply: &str,
    ) -> Result<(), String> {
        write_back::write_back_conversation_turn(&self.store, user_input, assistant_reply)
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
        let semantic_merge_count = self.merge_semantic_duplicates().unwrap_or(0);
        let semantic_prune_count = self.prune_low_confidence_semantic(0.2).unwrap_or(0);

        MaintenanceResult {
            decayed: decay_count,
            merged: merge_count + semantic_merge_count,
            pruned: prune_count + semantic_prune_count,
        }
    }

    fn merge_semantic_duplicates(&self) -> Result<u32, String> {
        let mut semantic = self.store.load_semantic()?;
        let original_count = semantic.entries.len();
        let mut merged: std::collections::HashMap<String, super::types::SemanticEntry> =
            std::collections::HashMap::new();

        for entry in semantic.entries.drain(..) {
            let identity = if entry.memory_key.trim().is_empty() {
                normalize_key(&entry.topic)
            } else {
                normalize_key(&entry.memory_key)
            };
            let key = format!(
                "{}::{}::{}::{:?}",
                identity,
                entry.source_type,
                normalize_key(&entry.knowledge),
                entry.status
            );
            if let Some(existing) = merged.get_mut(&key) {
                if entry.knowledge.len() > existing.knowledge.len() {
                    existing.knowledge = entry.knowledge.clone();
                }
                existing.confidence = existing.confidence.max(entry.confidence);
                existing.updated_at = existing.updated_at.max(entry.updated_at);
                existing.mention_count = existing.mention_count.max(entry.mention_count);
                existing.explicit = existing.explicit || entry.explicit;
                if existing.ttl.is_none() || entry.ttl.is_none() {
                    existing.ttl = None;
                } else if let Some(incoming_ttl) = entry.ttl {
                    existing.ttl = Some(existing.ttl.unwrap_or(incoming_ttl).max(incoming_ttl));
                }
                for tag in entry.tags {
                    if !existing.tags.iter().any(|existing_tag| existing_tag == &tag) {
                        existing.tags.push(tag);
                    }
                }
            } else {
                merged.insert(key, entry);
            }
        }

        semantic.entries = merged.into_values().collect();
        let merged_count = (original_count - semantic.entries.len()) as u32;
        if merged_count > 0 {
            self.store.save_semantic(&semantic)?;
        }
        Ok(merged_count)
    }

    fn prune_low_confidence_semantic(&self, threshold: f64) -> Result<u32, String> {
        let mut semantic = self.store.load_semantic()?;
        let original_count = semantic.entries.len();
        semantic.entries.retain(|entry| entry.confidence >= threshold);
        let pruned_count = (original_count - semantic.entries.len()) as u32;
        if pruned_count > 0 {
            self.store.save_semantic(&semantic)?;
        }
        Ok(pruned_count)
    }
}

fn managed_semantic_record(entry: &SemanticEntry) -> ManagedMemoryRecord {
    ManagedMemoryRecord {
        id: entry.id.clone(),
        memory_type: ManagedMemoryKind::Semantic,
        title: entry.topic.clone(),
        summary: entry.knowledge.clone(),
        detail: format!(
            "来源：{} · {}",
            entry.source_type,
            if entry.explicit { "显式记忆" } else { "候选记忆" }
        ),
        confidence: entry.confidence,
        explicit: entry.explicit,
        mention_count: entry.mention_count,
        status: entry.status,
        source: entry.source_type.clone(),
        updated_at: entry.updated_at,
        expires_at: entry.ttl,
        tags: entry.tags.clone(),
        conflict_group: entry.conflict_group.clone(),
    }
}

fn managed_meta_record(entry: &MetaPreference) -> ManagedMemoryRecord {
    ManagedMemoryRecord {
        id: entry.id.clone(),
        memory_type: ManagedMemoryKind::Meta,
        title: format!("{} / {}", entry.category, entry.preference),
        summary: meta_value_to_string(&entry.value),
        detail: if entry.explicit {
            "显式交互偏好".to_string()
        } else {
            "系统交互偏好".to_string()
        },
        confidence: entry.confidence,
        explicit: entry.explicit,
        mention_count: 1,
        status: entry.status,
        source: entry.category.clone(),
        updated_at: entry.updated_at,
        expires_at: entry.ttl,
        tags: vec!["meta".to_string(), entry.category.clone()],
        conflict_group: entry.conflict_group.clone(),
    }
}

fn meta_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
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

fn normalize_key(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace() && ch.is_alphanumeric())
        .collect()
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
