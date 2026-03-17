//! Behavior State - 三层架构统一状态管理
//!
//! 管理记忆层、规则层、权限层的初始化、持久化和生命周期。
//!
//! ## 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    BehaviorState                        │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐       │
//! │  │MemoryService│ │ RuleEngine  │ │PermChecker │       │
//! │  └─────────────┘ └─────────────┘ └─────────────┘       │
//! │         ↑               ↑               ↑               │
//! │         │ 检索/写回      │ 应用/更新      │ 检查/授权     │
//! │         ↓               ↓               ↓               │
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │                  Shell Agent                        ││
//! │  └─────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::memory::MemoryService;
use crate::rule_engine::{RuleEngine, RuleGenerator};
use crate::permission::{PermissionChecker, PermissionStore, PermissionState, PermissionScope, GrantSource};

/// 三层架构统一状态
pub struct BehaviorState {
    /// 应用数据目录
    app_data_dir: PathBuf,
    /// 记忆服务
    memory_service: MemoryService,
    /// 规则引擎
    rule_engine: Mutex<RuleEngine>,
    /// 权限检查器
    permission_checker: Mutex<PermissionChecker>,
    /// 权限存储
    permission_store: PermissionStore,
}

impl BehaviorState {
    /// 创建新的行为状态
    pub fn new(app_data_dir: &Path) -> Self {
        let memory_service = MemoryService::new(&app_data_dir.to_path_buf());

        let rule_store_path = app_data_dir.join("rules");
        let mut rule_engine = RuleEngine::new(&rule_store_path);
        // 尝试加载已保存的规则
        let _ = rule_engine.load();

        let permission_store_path = app_data_dir.join("permissions");
        let permission_store = PermissionStore::new(&permission_store_path);
        let mut permission_checker = PermissionChecker::new();

        // 加载持久化的权限状态
        if let Ok(state) = permission_store.load() {
            for permission in state.permissions {
                if permission.is_valid() {
                    permission_checker.restore_permission(permission);
                }
            }
            for policy in state.policies {
                permission_checker.add_policy(policy);
            }
        }

        Self {
            app_data_dir: app_data_dir.to_path_buf(),
            memory_service,
            rule_engine: Mutex::new(rule_engine),
            permission_checker: Mutex::new(permission_checker),
            permission_store,
        }
    }

    /// 获取记忆服务
    pub fn memory_service(&self) -> &MemoryService {
        &self.memory_service
    }

    /// 获取规则引擎（需要锁）
    pub fn rule_engine(&self) -> std::sync::MutexGuard<'_, RuleEngine> {
        self.rule_engine.lock().unwrap()
    }

    /// 获取权限检查器（需要锁）
    pub fn permission_checker(&self) -> std::sync::MutexGuard<'_, PermissionChecker> {
        self.permission_checker.lock().unwrap()
    }

    /// 从记忆生成新规则
    pub fn generate_rules_from_memories(&self) -> Result<usize, String> {
        // 加载所有记忆
        let entries = self.memory_service.store().load_all_entries()?;

        // 生成候选规则
        let new_rules = RuleGenerator::generate_from_memories(&entries);
        let count = new_rules.len();

        // 添加到规则引擎
        let mut engine = self.rule_engine.lock().unwrap();
        for rule in new_rules {
            engine.add_rule(rule);
        }

        // 保存规则
        let _ = engine.save();

        Ok(count)
    }

    /// 运行维护任务
    pub fn run_maintenance(&self) -> MaintenanceResult {
        let mut result = MaintenanceResult::default();

        // 1. 运行记忆维护
        let memory_result = self.memory_service.run_maintenance();
        result.memory_decayed = memory_result.decayed as usize;
        result.memory_merged = memory_result.merged as usize;
        result.memory_pruned = memory_result.pruned as usize;

        // 2. 从记忆生成规则
        if let Ok(count) = self.generate_rules_from_memories() {
            result.rules_generated = count;
        }

        // 3. 清理过期权限
        {
            let mut checker = self.permission_checker.lock().unwrap();
            checker.cleanup_expired();
        }

        // 4. 保存状态
        let _ = self.save();

        result
    }

    /// 保存所有状态
    pub fn save(&self) -> Result<(), String> {
        // 保存规则
        {
            let engine = self.rule_engine.lock().unwrap();
            engine.save()?;
        }

        // 保存权限状态
        {
            let checker = self.permission_checker.lock().unwrap();
            let state = PermissionState {
                permissions: checker.all_permissions().into_iter().cloned().collect(),
                policies: checker.all_policies().into_iter().cloned().collect(),
                schema_version: "1.0.0".to_string(),
            };
            self.permission_store.save(&state)?;
        }

        Ok(())
    }

    /// 授予权限（从设置同步时调用）
    pub fn grant_permission(
        &self,
        permission_id: &str,
        scope: PermissionScope,
        duration_ms: Option<u64>,
    ) -> Result<(), String> {
        let mut checker = self.permission_checker.lock().unwrap();
        checker.grant(permission_id, GrantSource::User, scope, duration_ms)?;
        drop(checker);
        self.save()
    }

    /// 撤销权限（从设置同步时调用）
    pub fn revoke_permission(&self, permission_id: &str) -> Result<(), String> {
        let mut checker = self.permission_checker.lock().unwrap();
        // 忽略不存在的权限
        let _ = checker.revoke(permission_id, "settings");
        drop(checker);
        self.save()
    }

    /// 撤销所有 Shell 相关权限
    pub fn revoke_all_shell_permissions(&self) -> Result<(), String> {
        let shell_permissions = [
            "shell:execute",
            "shell:modify",
            "shell:delete",
            "shell:network",
            "shell:system",
            "shell:registry",
        ];

        let mut checker = self.permission_checker.lock().unwrap();
        for permission_id in &shell_permissions {
            let _ = checker.revoke(permission_id, "settings");
        }
        drop(checker);
        self.save()
    }
}

/// 维护结果
#[derive(Debug, Default)]
pub struct MaintenanceResult {
    pub memory_decayed: usize,
    pub memory_merged: usize,
    pub memory_pruned: usize,
    pub rules_generated: usize,
}

impl MaintenanceResult {
    pub fn total_changes(&self) -> usize {
        self.memory_decayed + self.memory_merged + self.memory_pruned + self.rules_generated
    }
}
