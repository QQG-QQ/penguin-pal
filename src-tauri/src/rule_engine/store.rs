//! Rule Store - 规则持久化存储

use std::fs;
use std::path::{Path, PathBuf};
use super::types::*;

/// 规则存储
pub struct RuleStore {
    store_path: PathBuf,
}

impl RuleStore {
    /// 创建新的规则存储
    pub fn new(store_path: &Path) -> Self {
        Self {
            store_path: store_path.to_path_buf(),
        }
    }

    /// 加载规则
    pub fn load(&self) -> Result<Vec<Rule>, String> {
        let rules_file = self.store_path.join("rules.json");

        if !rules_file.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&rules_file)
            .map_err(|e| format!("读取规则文件失败: {}", e))?;

        serde_json::from_str(&content)
            .map_err(|e| format!("解析规则文件失败: {}", e))
    }

    /// 保存规则
    pub fn save(&self, rules: &[Rule]) -> Result<(), String> {
        // 确保目录存在
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败: {}", e))?;
        }

        fs::create_dir_all(&self.store_path)
            .map_err(|e| format!("创建规则目录失败: {}", e))?;

        let rules_file = self.store_path.join("rules.json");
        let content = serde_json::to_string_pretty(rules)
            .map_err(|e| format!("序列化规则失败: {}", e))?;

        fs::write(&rules_file, content)
            .map_err(|e| format!("写入规则文件失败: {}", e))
    }

    /// 备份当前规则
    pub fn backup(&self) -> Result<PathBuf, String> {
        let rules_file = self.store_path.join("rules.json");
        if !rules_file.exists() {
            return Err("没有规则文件需要备份".to_string());
        }

        let timestamp = crate::memory::now_millis();
        let backup_file = self.store_path.join(format!("rules_backup_{}.json", timestamp));

        fs::copy(&rules_file, &backup_file)
            .map_err(|e| format!("备份规则失败: {}", e))?;

        Ok(backup_file)
    }

    /// 从备份恢复
    pub fn restore(&self, backup_path: &Path) -> Result<(), String> {
        if !backup_path.exists() {
            return Err("备份文件不存在".to_string());
        }

        let rules_file = self.store_path.join("rules.json");
        fs::copy(backup_path, &rules_file)
            .map_err(|e| format!("恢复规则失败: {}", e))?;

        Ok(())
    }

    /// 获取存储路径
    pub fn path(&self) -> &Path {
        &self.store_path
    }

    /// 导出规则为 YAML 格式（便于人工审核）
    pub fn export_yaml(&self, rules: &[Rule]) -> Result<String, String> {
        // 简化的 YAML 导出（不依赖 serde_yaml）
        let mut yaml = String::new();
        yaml.push_str("# PenguinPal Rules Export\n");
        yaml.push_str(&format!("# Generated at: {}\n\n", crate::memory::now_millis()));

        for rule in rules {
            yaml.push_str(&format!("- id: {}\n", rule.id));
            yaml.push_str(&format!("  name: {}\n", rule.name));
            yaml.push_str(&format!("  type: {:?}\n", rule.rule_type));
            yaml.push_str(&format!("  strength: {:?}\n", rule.strength));
            yaml.push_str(&format!("  status: {:?}\n", rule.status));
            yaml.push_str(&format!("  priority: {}\n", rule.priority));
            yaml.push_str(&format!("  confidence: {:.2}\n", rule.confidence));
            yaml.push_str(&format!("  success_count: {}\n", rule.success_count));
            yaml.push_str(&format!("  failure_count: {}\n", rule.failure_count));
            if !rule.description.is_empty() {
                yaml.push_str(&format!("  description: |\n    {}\n", rule.description.replace('\n', "\n    ")));
            }
            yaml.push_str("\n");
        }

        Ok(yaml)
    }

    /// 列出所有备份
    pub fn list_backups(&self) -> Result<Vec<PathBuf>, String> {
        let mut backups = Vec::new();

        let entries = fs::read_dir(&self.store_path)
            .map_err(|e| format!("读取目录失败: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("rules_backup_") && name.ends_with(".json") {
                    backups.push(path);
                }
            }
        }

        // 按时间排序（最新的在前）
        backups.sort_by(|a, b| b.cmp(a));

        Ok(backups)
    }

    /// 清理旧备份（保留最近 N 个）
    pub fn cleanup_backups(&self, keep_count: usize) -> Result<usize, String> {
        let backups = self.list_backups()?;

        if backups.len() <= keep_count {
            return Ok(0);
        }

        let mut removed = 0;
        for backup in backups.into_iter().skip(keep_count) {
            if fs::remove_file(&backup).is_ok() {
                removed += 1;
            }
        }

        Ok(removed)
    }
}

impl Default for RuleStore {
    fn default() -> Self {
        Self {
            store_path: PathBuf::from("./rules"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_store_save_load() {
        let temp_dir = env::temp_dir().join(format!("rule_store_test_{}", crate::memory::now_millis()));
        let store = RuleStore::new(&temp_dir);

        let rules = vec![
            Rule::new("1".into(), "Test Rule".into(), RuleType::Preference, RuleStrength::Soft),
        ];

        store.save(&rules).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "1");

        // 清理
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_export_yaml() {
        let store = RuleStore::default();
        let rules = vec![
            Rule::new("1".into(), "Test Rule".into(), RuleType::Preference, RuleStrength::Soft),
        ];

        let yaml = store.export_yaml(&rules).unwrap();
        assert!(yaml.contains("id: 1"));
        assert!(yaml.contains("name: Test Rule"));
    }
}
