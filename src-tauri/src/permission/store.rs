//! Permission Store - 权限持久化存储

use std::fs;
use std::path::{Path, PathBuf};
use super::types::*;

/// 权限存储
pub struct PermissionStore {
    store_path: PathBuf,
}

/// 可序列化的权限状态
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionState {
    pub permissions: Vec<Permission>,
    pub policies: Vec<PermissionPolicy>,
    pub schema_version: String,
}

impl Default for PermissionState {
    fn default() -> Self {
        Self {
            permissions: Vec::new(),
            policies: default_policies(),
            schema_version: "1.0.0".to_string(),
        }
    }
}

impl PermissionStore {
    pub fn new(store_path: &Path) -> Self {
        Self {
            store_path: store_path.to_path_buf(),
        }
    }

    /// 加载权限状态
    pub fn load(&self) -> Result<PermissionState, String> {
        let perm_file = self.store_path.join("permissions.json");

        if !perm_file.exists() {
            return Ok(PermissionState::default());
        }

        let content = fs::read_to_string(&perm_file)
            .map_err(|e| format!("读取权限文件失败: {}", e))?;

        serde_json::from_str(&content)
            .map_err(|e| format!("解析权限文件失败: {}", e))
    }

    /// 保存权限状态
    pub fn save(&self, state: &PermissionState) -> Result<(), String> {
        fs::create_dir_all(&self.store_path)
            .map_err(|e| format!("创建权限目录失败: {}", e))?;

        let perm_file = self.store_path.join("permissions.json");
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| format!("序列化权限失败: {}", e))?;

        fs::write(&perm_file, content)
            .map_err(|e| format!("写入权限文件失败: {}", e))
    }

    /// 获取存储路径
    pub fn path(&self) -> &Path {
        &self.store_path
    }
}

impl Default for PermissionStore {
    fn default() -> Self {
        Self {
            store_path: PathBuf::from("./permissions"),
        }
    }
}

/// 默认权限策略
fn default_policies() -> Vec<PermissionPolicy> {
    vec![
        // Shell 命令默认策略：需要用户确认
        PermissionPolicy {
            id: "shell_default".to_string(),
            name: "Shell 命令默认策略".to_string(),
            description: "所有 Shell 命令默认需要用户确认".to_string(),
            rules: vec![
                PolicyRule {
                    category: Some(PermissionCategory::Shell),
                    permission_pattern: Some("shell:*".to_string()),
                    action: PolicyAction::RequireConfirmation,
                    conditions: Vec::new(),
                },
            ],
            priority: 10,
            enabled: true,
        },
        // 只读操作默认允许
        PermissionPolicy {
            id: "readonly_allow".to_string(),
            name: "只读操作默认允许".to_string(),
            description: "只读操作不需要确认".to_string(),
            rules: vec![
                PolicyRule {
                    category: None,
                    permission_pattern: Some("*:read".to_string()),
                    action: PolicyAction::Allow,
                    conditions: Vec::new(),
                },
            ],
            priority: 20,
            enabled: true,
        },
        // 系统级操作默认拒绝
        PermissionPolicy {
            id: "system_deny".to_string(),
            name: "系统级操作默认拒绝".to_string(),
            description: "系统级危险操作默认拒绝".to_string(),
            rules: vec![
                PolicyRule {
                    category: Some(PermissionCategory::System),
                    permission_pattern: None,
                    action: PolicyAction::Deny,
                    conditions: Vec::new(),
                },
            ],
            priority: 100,
            enabled: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_permission_store_save_load() {
        let temp_dir = env::temp_dir().join(format!("perm_store_test_{}", crate::memory::now_millis()));
        let store = PermissionStore::new(&temp_dir);

        let mut state = PermissionState::default();
        state.permissions.push(Permission::new(
            "shell:execute",
            "Shell 执行权限",
            PermissionCategory::Shell,
            PermissionLevel::Standard,
        ));

        store.save(&state).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.permissions.len(), 1);
        assert_eq!(loaded.permissions[0].id, "shell:execute");

        // 清理
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
