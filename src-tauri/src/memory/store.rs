//! Memory Store - 本地 JSON 持久化存储
//!
//! 使用简单的 JSON 文件存储，不引入额外依赖。
//! 存储路径: $APP_DATA/penguin-pal/memory/

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{de::DeserializeOwned, Serialize};

use super::types::{
    generate_id, now_millis, EpisodicMemory, MemoryStatus, MetaMemory, MetaPreference,
    PolicyMemory, ProceduralMemory, ProfileMemory, SemanticEntry, SemanticMemory,
    MEMORY_SCHEMA_VERSION,
};

/// Memory 存储目录名
const MEMORY_DIR: &str = "memory";

/// 各类 memory 的文件名
const PROFILE_FILE: &str = "profile.json";
const EPISODIC_FILE: &str = "episodic.json";
const PROCEDURAL_FILE: &str = "procedural.json";
const POLICY_FILE: &str = "policy.json";
const SEMANTIC_FILE: &str = "semantic.json";
const META_FILE: &str = "meta.json";

/// Memory Store 单例
pub struct MemoryStore {
    base_path: PathBuf,
    profile: Mutex<Option<ProfileMemory>>,
    episodic: Mutex<Option<EpisodicMemory>>,
    procedural: Mutex<Option<ProceduralMemory>>,
    policy: Mutex<Option<PolicyMemory>>,
    semantic: Mutex<Option<SemanticMemory>>,
    meta: Mutex<Option<MetaMemory>>,
}

impl MemoryStore {
    /// 创建新的 MemoryStore
    pub fn new(app_data_dir: &PathBuf) -> Self {
        let base_path = app_data_dir.join(MEMORY_DIR);
        Self {
            base_path,
            profile: Mutex::new(None),
            episodic: Mutex::new(None),
            procedural: Mutex::new(None),
            policy: Mutex::new(None),
            semantic: Mutex::new(None),
            meta: Mutex::new(None),
        }
    }

    /// 确保存储目录存在
    fn ensure_dir(&self) -> Result<(), String> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path)
                .map_err(|e| format!("创建 memory 目录失败: {}", e))?;
        }
        Ok(())
    }

    /// 读取 JSON 文件
    fn read_json<T: DeserializeOwned + Default>(&self, filename: &str) -> Result<T, String> {
        let path = self.base_path.join(filename);
        if !path.exists() {
            return Ok(T::default());
        }
        let content =
            fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败: {}", filename, e))?;
        serde_json::from_str(&content).map_err(|e| format!("解析 {} 失败: {}", filename, e))
    }

    /// 写入 JSON 文件
    fn write_json<T: Serialize>(&self, filename: &str, data: &T) -> Result<(), String> {
        self.ensure_dir()?;
        let path = self.base_path.join(filename);
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| format!("序列化 {} 失败: {}", filename, e))?;
        fs::write(&path, content).map_err(|e| format!("写入 {} 失败: {}", filename, e))?;
        Ok(())
    }

    // ========================================================================
    // Profile Memory
    // ========================================================================

    /// 加载 Profile Memory
    pub fn load_profile(&self) -> Result<ProfileMemory, String> {
        let mut cache = self.profile.lock().map_err(|_| "锁定 profile 失败")?;
        if let Some(ref profile) = *cache {
            return Ok(profile.clone());
        }
        let profile: ProfileMemory = self.read_json(PROFILE_FILE)?;
        *cache = Some(profile.clone());
        Ok(profile)
    }

    /// 保存 Profile Memory
    pub fn save_profile(&self, profile: &ProfileMemory) -> Result<(), String> {
        self.write_json(PROFILE_FILE, profile)?;
        let mut cache = self.profile.lock().map_err(|_| "锁定 profile 失败")?;
        *cache = Some(profile.clone());
        Ok(())
    }

    /// 更新 Profile Memory (读取-修改-写入)
    pub fn update_profile<F>(&self, updater: F) -> Result<(), String>
    where
        F: FnOnce(&mut ProfileMemory),
    {
        let mut profile = self.load_profile()?;
        if profile.schema_version.is_empty() {
            profile.schema_version = MEMORY_SCHEMA_VERSION.to_string();
            profile.created_at = now_millis();
        }
        updater(&mut profile);
        profile.updated_at = now_millis();
        self.save_profile(&profile)
    }

    // ========================================================================
    // Episodic Memory
    // ========================================================================

    /// 加载 Episodic Memory
    pub fn load_episodic(&self) -> Result<EpisodicMemory, String> {
        let mut cache = self.episodic.lock().map_err(|_| "锁定 episodic 失败")?;
        if let Some(ref episodic) = *cache {
            return Ok(episodic.clone());
        }
        let episodic: EpisodicMemory = self.read_json(EPISODIC_FILE)?;
        *cache = Some(episodic.clone());
        Ok(episodic)
    }

    /// 保存 Episodic Memory
    pub fn save_episodic(&self, episodic: &EpisodicMemory) -> Result<(), String> {
        self.write_json(EPISODIC_FILE, episodic)?;
        let mut cache = self.episodic.lock().map_err(|_| "锁定 episodic 失败")?;
        *cache = Some(episodic.clone());
        Ok(())
    }

    /// 添加 Episodic Entry
    pub fn add_episodic_entry(
        &self,
        entry: super::types::EpisodicEntry,
    ) -> Result<(), String> {
        let mut episodic = self.load_episodic()?;
        episodic.entries.push(entry);
        // 保持最近 500 条记录
        if episodic.entries.len() > 500 {
            episodic.entries = episodic.entries.split_off(episodic.entries.len() - 500);
        }
        self.save_episodic(&episodic)
    }

    // ========================================================================
    // Procedural Memory
    // ========================================================================

    /// 加载 Procedural Memory
    pub fn load_procedural(&self) -> Result<ProceduralMemory, String> {
        let mut cache = self.procedural.lock().map_err(|_| "锁定 procedural 失败")?;
        if let Some(ref procedural) = *cache {
            return Ok(procedural.clone());
        }
        let procedural: ProceduralMemory = self.read_json(PROCEDURAL_FILE)?;
        *cache = Some(procedural.clone());
        Ok(procedural)
    }

    /// 保存 Procedural Memory
    pub fn save_procedural(&self, procedural: &ProceduralMemory) -> Result<(), String> {
        self.write_json(PROCEDURAL_FILE, procedural)?;
        let mut cache = self.procedural.lock().map_err(|_| "锁定 procedural 失败")?;
        *cache = Some(procedural.clone());
        Ok(())
    }

    /// 更新或插入 Procedural Entry
    pub fn upsert_procedural_entry(
        &self,
        entry: super::types::ProceduralEntry,
    ) -> Result<(), String> {
        let mut procedural = self.load_procedural()?;
        if let Some(existing) = procedural
            .procedures
            .iter_mut()
            .find(|p| p.target_pattern == entry.target_pattern && p.target_kind == entry.target_kind)
        {
            // 更新现有条目
            existing.success_count = entry.success_count;
            existing.failure_count = entry.failure_count;
            existing.confidence = entry.confidence;
            existing.last_verified_at = entry.last_verified_at;
            existing.updated_at = now_millis();
            if !entry.preferred_tool_sequence.is_empty() {
                existing.preferred_tool_sequence = entry.preferred_tool_sequence;
            }
        } else {
            // 插入新条目
            procedural.procedures.push(entry);
        }
        // 保持最多 200 条
        if procedural.procedures.len() > 200 {
            // 按 confidence 排序，保留高置信度的
            procedural
                .procedures
                .sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
            procedural.procedures.truncate(200);
        }
        self.save_procedural(&procedural)
    }

    // ========================================================================
    // Policy Memory
    // ========================================================================

    /// 加载 Policy Memory
    pub fn load_policy(&self) -> Result<PolicyMemory, String> {
        let mut cache = self.policy.lock().map_err(|_| "锁定 policy 失败")?;
        if let Some(ref policy) = *cache {
            return Ok(policy.clone());
        }
        let policy: PolicyMemory = self.read_json(POLICY_FILE)?;
        *cache = Some(policy.clone());
        Ok(policy)
    }

    /// 保存 Policy Memory
    pub fn save_policy(&self, policy: &PolicyMemory) -> Result<(), String> {
        self.write_json(POLICY_FILE, policy)?;
        let mut cache = self.policy.lock().map_err(|_| "锁定 policy 失败")?;
        *cache = Some(policy.clone());
        Ok(())
    }

    /// 添加 Policy Suggestion
    pub fn add_policy_suggestion(
        &self,
        suggestion: super::types::PolicySuggestion,
    ) -> Result<(), String> {
        let mut policy = self.load_policy()?;
        // 检查是否已存在相同 scope + type 的建议
        if let Some(existing) = policy.suggestions.iter_mut().find(|s| {
            s.scope == suggestion.scope && s.suggestion_type == suggestion.suggestion_type
        }) {
            existing.value = suggestion.value;
            existing.confidence = suggestion.confidence;
            existing.updated_at = now_millis();
        } else {
            policy.suggestions.push(suggestion);
        }
        // 保持最多 100 条
        if policy.suggestions.len() > 100 {
            policy.suggestions = policy.suggestions.split_off(policy.suggestions.len() - 100);
        }
        self.save_policy(&policy)
    }

    // ========================================================================
    // Semantic Memory
    // ========================================================================

    /// 加载 Semantic Memory
    pub fn load_semantic(&self) -> Result<SemanticMemory, String> {
        let mut cache = self.semantic.lock().map_err(|_| "锁定 semantic 失败")?;
        if let Some(ref semantic) = *cache {
            return Ok(semantic.clone());
        }
        let semantic: SemanticMemory = self.read_json(SEMANTIC_FILE)?;
        *cache = Some(semantic.clone());
        Ok(semantic)
    }

    /// 保存 Semantic Memory
    pub fn save_semantic(&self, semantic: &SemanticMemory) -> Result<(), String> {
        self.write_json(SEMANTIC_FILE, semantic)?;
        let mut cache = self.semantic.lock().map_err(|_| "锁定 semantic 失败")?;
        *cache = Some(semantic.clone());
        Ok(())
    }

    /// 更新 Semantic Memory
    pub fn update_semantic<F>(&self, updater: F) -> Result<(), String>
    where
        F: FnOnce(&mut SemanticMemory),
    {
        let mut semantic = self.load_semantic()?;
        if semantic.schema_version.is_empty() {
            semantic.schema_version = MEMORY_SCHEMA_VERSION.to_string();
        }
        updater(&mut semantic);
        self.save_semantic(&semantic)
    }

    /// 更新或插入 Semantic Entry
    pub fn upsert_semantic_entry(&self, entry: SemanticEntry) -> Result<(), String> {
        self.update_semantic(|semantic| {
            let mut entry = with_semantic_defaults(entry);

            if let Some(existing) = semantic.entries.iter_mut().find(|candidate| {
                semantic_identity_key(candidate) == semantic_identity_key(&entry)
                    && semantic_knowledge_key(candidate) == semantic_knowledge_key(&entry)
                    && candidate.status != MemoryStatus::Archived
                    && candidate.status != MemoryStatus::Deprecated
            }) {
                merge_semantic_entry(existing, &entry);
            } else if let Some(existing_index) = semantic.entries.iter().position(|candidate| {
                semantic_identity_key(candidate) == semantic_identity_key(&entry)
                    && candidate.status != MemoryStatus::Archived
                    && candidate.status != MemoryStatus::Deprecated
            }) {
                let should_keep_existing =
                    semantic.entries[existing_index].explicit && !entry.explicit;

                if should_keep_existing {
                    let existing = &mut semantic.entries[existing_index];
                    existing.updated_at = existing.updated_at.max(entry.updated_at);
                } else if entry.explicit {
                    let previous = &mut semantic.entries[existing_index];
                    previous.status = MemoryStatus::Deprecated;
                    previous.conflict_group = None;
                    previous.updated_at = now_millis();
                    semantic.entries.push(entry);
                } else {
                    let group = semantic.entries[existing_index]
                        .conflict_group
                        .clone()
                        .unwrap_or_else(|| generate_id("semantic_conflict"));
                    let previous = &mut semantic.entries[existing_index];
                    previous.status = MemoryStatus::Conflicted;
                    previous.conflict_group = Some(group.clone());
                    previous.updated_at = now_millis();
                    entry.status = MemoryStatus::Conflicted;
                    entry.conflict_group = Some(group);
                    semantic.entries.push(entry);
                }
            } else {
                semantic.entries.push(entry);
            }

            if semantic.entries.len() > 200 {
                semantic.entries.sort_by(|a, b| {
                    status_rank(&a.status)
                        .cmp(&status_rank(&b.status))
                        .then_with(|| {
                            b.updated_at
                                .cmp(&a.updated_at)
                                .then_with(|| {
                                    b.confidence
                                        .partial_cmp(&a.confidence)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                })
                        })
                });
                semantic.entries.truncate(200);
            }
        })
    }

    /// 删除指定 Semantic Entry
    pub fn delete_semantic_entry(&self, id: &str) -> Result<bool, String> {
        let mut removed = false;
        self.update_semantic(|semantic| {
            let before = semantic.entries.len();
            semantic.entries.retain(|entry| entry.id != id);
            removed = before != semantic.entries.len();
        })?;
        Ok(removed)
    }

    /// 提升候选 Semantic Entry 为稳定长期记忆
    pub fn promote_semantic_entry(&self, id: &str) -> Result<bool, String> {
        let mut promoted = false;
        self.update_semantic(|semantic| {
            if let Some(entry) = semantic.entries.iter_mut().find(|entry| entry.id == id) {
                entry.explicit = true;
                entry.mention_count = entry.mention_count.max(2);
                entry.confidence = entry.confidence.max(0.75);
                entry.ttl = None;
                entry.status = MemoryStatus::Active;
                entry.conflict_group = None;
                entry.updated_at = now_millis();
                if !entry.tags.iter().any(|tag| tag == "promoted") {
                    entry.tags.push("promoted".to_string());
                }
                promoted = true;
            }
        })?;
        Ok(promoted)
    }

    /// 解决 Semantic 冲突，保留指定条目
    pub fn resolve_semantic_conflict(
        &self,
        group: &str,
        keep_id: &str,
    ) -> Result<bool, String> {
        let mut resolved = false;
        self.update_semantic(|semantic| {
            for entry in &mut semantic.entries {
                if entry.conflict_group.as_deref() != Some(group) {
                    continue;
                }
                entry.conflict_group = None;
                entry.updated_at = now_millis();
                if entry.id == keep_id {
                    entry.status = MemoryStatus::Active;
                    entry.mention_count = entry.mention_count.max(2);
                    entry.ttl = None;
                } else {
                    entry.status = MemoryStatus::Deprecated;
                }
                resolved = true;
            }
        })?;
        Ok(resolved)
    }

    /// 删除匹配的 Semantic Entries
    pub fn forget_semantic_entries(&self, query: &str) -> Result<usize, String> {
        let normalized = normalize_key(query);
        if normalized.is_empty() {
            return Ok(0);
        }

        let mut removed = 0usize;
        self.update_semantic(|semantic| {
            let before = semantic.entries.len();
            semantic.entries.retain(|entry| {
                !semantic_entry_matches(entry, &normalized)
            });
            removed = before.saturating_sub(semantic.entries.len());
        })?;
        Ok(removed)
    }

    // ========================================================================
    // Meta Memory
    // ========================================================================

    /// 加载 Meta Memory
    pub fn load_meta(&self) -> Result<MetaMemory, String> {
        let mut cache = self.meta.lock().map_err(|_| "锁定 meta 失败")?;
        if let Some(ref meta) = *cache {
            return Ok(meta.clone());
        }
        let meta: MetaMemory = self.read_json(META_FILE)?;
        *cache = Some(meta.clone());
        Ok(meta)
    }

    /// 保存 Meta Memory
    pub fn save_meta(&self, meta: &MetaMemory) -> Result<(), String> {
        self.write_json(META_FILE, meta)?;
        let mut cache = self.meta.lock().map_err(|_| "锁定 meta 失败")?;
        *cache = Some(meta.clone());
        Ok(())
    }

    /// 更新或插入 Meta Preference
    pub fn upsert_meta_preference(&self, preference: MetaPreference) -> Result<(), String> {
        let mut meta = self.load_meta()?;
        let preference = with_meta_defaults(preference);
        if let Some(existing) = meta.preferences.iter_mut().find(|item| {
            item.category == preference.category
                && item.preference == preference.preference
                && meta_value_key(&item.value) == meta_value_key(&preference.value)
                && item.status != MemoryStatus::Archived
                && item.status != MemoryStatus::Deprecated
        }) {
            existing.confidence = existing.confidence.max(preference.confidence);
            existing.updated_at = now_millis();
            existing.explicit = existing.explicit || preference.explicit;
            existing.ttl = match (existing.ttl, preference.ttl) {
                (None, _) | (_, None) => None,
                (Some(current), Some(incoming)) => Some(current.max(incoming)),
            };
        } else if let Some(existing_index) = meta.preferences.iter().position(|item| {
            item.category == preference.category
                && item.preference == preference.preference
                && item.status != MemoryStatus::Archived
                && item.status != MemoryStatus::Deprecated
        }) {
            if preference.explicit {
                let previous = &mut meta.preferences[existing_index];
                previous.status = MemoryStatus::Deprecated;
                previous.conflict_group = None;
                previous.updated_at = now_millis();
                meta.preferences.push(preference);
            } else {
                let group = meta.preferences[existing_index]
                    .conflict_group
                    .clone()
                    .unwrap_or_else(|| generate_id("meta_conflict"));
                let previous = &mut meta.preferences[existing_index];
                previous.status = MemoryStatus::Conflicted;
                previous.conflict_group = Some(group.clone());
                previous.updated_at = now_millis();
                let mut incoming = preference;
                incoming.status = MemoryStatus::Conflicted;
                incoming.conflict_group = Some(group);
                meta.preferences.push(incoming);
            }
        } else {
            meta.preferences.push(preference);
        }

        if meta.preferences.len() > 64 {
            meta.preferences.sort_by(|a, b| {
                status_rank(&a.status)
                    .cmp(&status_rank(&b.status))
                    .then_with(|| {
                        b.updated_at
                            .cmp(&a.updated_at)
                            .then_with(|| {
                                b.confidence
                                    .partial_cmp(&a.confidence)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                    })
            });
            meta.preferences.truncate(64);
        }

        self.save_meta(&meta)
    }

    /// 删除指定 Meta Preference
    pub fn delete_meta_preference(&self, id: &str) -> Result<bool, String> {
        let mut meta = self.load_meta()?;
        let before = meta.preferences.len();
        meta.preferences.retain(|entry| entry.id != id);
        let removed = before != meta.preferences.len();
        if removed {
            self.save_meta(&meta)?;
        }
        Ok(removed)
    }

    /// 解决 Meta 冲突，保留指定条目
    pub fn resolve_meta_conflict(&self, group: &str, keep_id: &str) -> Result<bool, String> {
        let mut meta = self.load_meta()?;
        let mut resolved = false;
        for entry in &mut meta.preferences {
            if entry.conflict_group.as_deref() != Some(group) {
                continue;
            }
            entry.conflict_group = None;
            entry.updated_at = now_millis();
            if entry.id == keep_id {
                entry.status = MemoryStatus::Active;
                entry.ttl = None;
            } else {
                entry.status = MemoryStatus::Deprecated;
            }
            resolved = true;
        }
        if resolved {
            self.save_meta(&meta)?;
        }
        Ok(resolved)
    }

    // ========================================================================
    // 清除缓存
    // ========================================================================

    /// 清除所有缓存
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.profile.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.episodic.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.procedural.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.policy.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.semantic.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.meta.lock() {
            *cache = None;
        }
    }

    // ========================================================================
    // 统一入口
    // ========================================================================

    /// 加载所有记忆条目（用于规则生成）
    pub fn load_all_entries(&self) -> Result<Vec<super::types::MemoryEntry>, String> {
        use super::types::{MemoryEntry, MemoryType, MemoryScope, MemoryStatus, PrivacyLevel};

        let mut entries = Vec::new();

        // 0. 从 Profile 转换
        let profile = self.load_profile()?;
        entries.push(profile.to_entry());

        // 1. 从 Episodic 转换
        let episodic = self.load_episodic()?;
        for entry in episodic.entries {
            entries.push(MemoryEntry {
                id: entry.id.clone(),
                memory_type: MemoryType::Episodic,
                content: format!("Goal: {}; Status: {}", entry.goal, entry.final_status),
                summary: entry.goal.clone(),
                source: "episodic".to_string(),
                created_at: entry.timestamp,
                updated_at: entry.timestamp,
                importance: if entry.final_status == "completed" { 0.6 } else { 0.4 },
                confidence: 0.8,
                recency: 1.0,
                frequency: 1,
                scope: MemoryScope::Task,
                tags: entry.tags.clone(),
                related_memories: Vec::new(),
                status: MemoryStatus::Active,
                privacy: PrivacyLevel::Public,
                ttl: None,
                retrieval_keys: vec![entry.goal, entry.intent],
            });
        }

        // 2. 从 Procedural 转换
        let procedural = self.load_procedural()?;
        for entry in procedural.procedures {
            entries.push(MemoryEntry {
                id: entry.id.clone(),
                memory_type: MemoryType::Procedural,
                content: format!("Pattern: {}; Tools: {:?}", entry.target_pattern, entry.preferred_tool_sequence),
                summary: entry.target_pattern.clone(),
                source: "procedural".to_string(),
                created_at: entry.created_at,
                updated_at: entry.updated_at,
                importance: entry.confidence,
                confidence: entry.confidence,
                recency: 0.8,
                frequency: entry.success_count + entry.failure_count,
                scope: MemoryScope::Project,
                tags: vec![entry.target_kind.clone()],
                related_memories: Vec::new(),
                status: MemoryStatus::Active,
                privacy: PrivacyLevel::Public,
                ttl: None,
                retrieval_keys: vec![entry.target_pattern, entry.target_kind],
            });
        }

        // 3. 从 Policy 转换
        let policy = self.load_policy()?;
        for entry in policy.suggestions {
            // 解析 scope 字符串到枚举
            let scope = match entry.scope.as_str() {
                "global" => MemoryScope::Global,
                "user" => MemoryScope::User,
                "project" => MemoryScope::Project,
                "task" => MemoryScope::Task,
                _ => MemoryScope::Global,
            };
            entries.push(MemoryEntry {
                id: entry.id.clone(),
                memory_type: MemoryType::Policy,
                content: format!("{}: {}", entry.suggestion_type, entry.value),
                summary: entry.suggestion_type.clone(),
                source: "policy".to_string(),
                created_at: entry.created_at,
                updated_at: entry.updated_at,
                importance: entry.confidence,
                confidence: entry.confidence,
                recency: 0.7,
                frequency: 1,
                scope,
                tags: Vec::new(),
                related_memories: Vec::new(),
                status: MemoryStatus::Active,
                privacy: PrivacyLevel::Public,
                ttl: None,
                retrieval_keys: vec![entry.suggestion_type],
            });
        }

        // 4. 从 Semantic 转换
        let semantic = self.load_semantic()?;
        for entry in semantic.entries {
            if entry.status != MemoryStatus::Active {
                continue;
            }
            entries.push(entry.to_memory_entry());
        }

        // 5. 从 Meta 转换
        let meta = self.load_meta()?;
        for entry in meta.preferences {
            if entry.status != MemoryStatus::Active {
                continue;
            }
            entries.push(entry.to_memory_entry());
        }

        Ok(entries)
    }
}

fn normalize_key(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '：' && *ch != ':' && *ch != '，' && *ch != ',')
        .collect()
}

fn choose_richer_text(existing: &str, incoming: &str) -> String {
    if incoming.trim().len() > existing.trim().len() {
        incoming.trim().to_string()
    } else {
        existing.trim().to_string()
    }
}

fn merge_unique_strings(target: &mut Vec<String>, incoming: &[String]) {
    for item in incoming {
        if !target.iter().any(|existing| existing == item) {
            target.push(item.clone());
        }
    }
}

fn semantic_entry_matches(entry: &SemanticEntry, query: &str) -> bool {
    let topic = normalize_key(&entry.topic);
    let knowledge = normalize_key(&entry.knowledge);
    topic.contains(query)
        || knowledge.contains(query)
        || entry
            .tags
            .iter()
            .map(|tag| normalize_key(tag))
            .any(|tag| tag.contains(query))
}

fn semantic_identity_key(entry: &SemanticEntry) -> String {
    if !entry.memory_key.trim().is_empty() {
        normalize_key(&entry.memory_key)
    } else {
        format!("{}::{}", normalize_key(&entry.topic), entry.source_type)
    }
}

fn semantic_knowledge_key(entry: &SemanticEntry) -> String {
    normalize_key(&entry.knowledge)
}

fn meta_value_key(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => normalize_key(text),
        _ => normalize_key(&value.to_string()),
    }
}

fn with_semantic_defaults(mut entry: SemanticEntry) -> SemanticEntry {
    if entry.memory_key.trim().is_empty() {
        entry.memory_key = entry.topic.clone();
    }
    if entry.mention_count == 0 {
        entry.mention_count = 1;
    }
    if matches!(entry.status, MemoryStatus::Active) && entry.conflict_group.is_some() {
        entry.status = MemoryStatus::Conflicted;
    }
    entry
}

fn with_meta_defaults(mut preference: MetaPreference) -> MetaPreference {
    if matches!(preference.status, MemoryStatus::Active) && preference.conflict_group.is_some() {
        preference.status = MemoryStatus::Conflicted;
    }
    preference
}

fn merge_semantic_entry(existing: &mut SemanticEntry, incoming: &SemanticEntry) {
    existing.knowledge = choose_richer_text(&existing.knowledge, &incoming.knowledge);
    existing.confidence = existing.confidence.max(incoming.confidence);
    existing.updated_at = now_millis();
    existing.explicit = existing.explicit || incoming.explicit;
    existing.mention_count = existing
        .mention_count
        .saturating_add(incoming.mention_count.max(1));
    if incoming.ttl.is_none() || existing.explicit {
        existing.ttl = None;
    } else if let Some(incoming_ttl) = incoming.ttl {
        existing.ttl = Some(
            existing
                .ttl
                .map(|ttl| ttl.max(incoming_ttl))
                .unwrap_or(incoming_ttl),
        );
    }
    existing.status = MemoryStatus::Active;
    existing.conflict_group = None;
    merge_unique_strings(&mut existing.tags, &incoming.tags);
}

fn status_rank(status: &MemoryStatus) -> u8 {
    match status {
        MemoryStatus::Active => 0,
        MemoryStatus::Conflicted => 1,
        MemoryStatus::Archived => 2,
        MemoryStatus::Deprecated => 3,
    }
}
