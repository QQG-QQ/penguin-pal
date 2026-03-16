//! Memory Store - 本地 JSON 持久化存储
//!
//! 使用简单的 JSON 文件存储，不引入额外依赖。
//! 存储路径: $APP_DATA/penguin-pal/memory/

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{de::DeserializeOwned, Serialize};

use super::types::{
    now_millis, EpisodicMemory, PolicyMemory, ProceduralMemory, ProfileMemory,
    MEMORY_SCHEMA_VERSION,
};

/// Memory 存储目录名
const MEMORY_DIR: &str = "memory";

/// 各类 memory 的文件名
const PROFILE_FILE: &str = "profile.json";
const EPISODIC_FILE: &str = "episodic.json";
const PROCEDURAL_FILE: &str = "procedural.json";
const POLICY_FILE: &str = "policy.json";

/// Memory Store 单例
pub struct MemoryStore {
    base_path: PathBuf,
    profile: Mutex<Option<ProfileMemory>>,
    episodic: Mutex<Option<EpisodicMemory>>,
    procedural: Mutex<Option<ProceduralMemory>>,
    policy: Mutex<Option<PolicyMemory>>,
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
    }
}
