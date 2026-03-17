//! Codex 技能系统
//!
//! 从 skills/ 目录加载自定义技能，注入到 system prompt。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 单个技能定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 技能名称
    pub name: String,
    /// 技能描述
    #[serde(default)]
    pub description: String,
    /// 技能指令 (注入到 system prompt)
    pub instructions: String,
    /// 触发关键词
    #[serde(default)]
    pub triggers: Vec<String>,
    /// 技能是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 优先级 (数字越大越优先)
    #[serde(default)]
    pub priority: i32,
}

fn default_enabled() -> bool {
    true
}

/// 技能集合
#[derive(Debug, Clone, Default)]
pub struct SkillSet {
    /// 按名称索引的技能
    pub skills: HashMap<String, Skill>,
    /// 技能目录路径
    skills_dir: PathBuf,
}

impl SkillSet {
    /// 创建新的技能集合
    pub fn new(codex_home: &Path) -> Self {
        Self {
            skills: HashMap::new(),
            skills_dir: codex_home.join("skills"),
        }
    }

    /// 从目录加载所有技能
    pub fn load(&mut self) -> Result<(), String> {
        self.skills.clear();

        if !self.skills_dir.exists() {
            // 创建默认目录结构
            fs::create_dir_all(&self.skills_dir)
                .map_err(|e| format!("创建 skills 目录失败: {}", e))?;
            return Ok(());
        }

        self.load_skills_recursive(&self.skills_dir.clone())?;
        Ok(())
    }

    fn load_skills_recursive(&mut self, dir: &Path) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("读取 skills 目录失败: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();

            // 跳过隐藏文件和目录
            if path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }

            if path.is_dir() {
                // 检查是否是技能目录 (包含 skill.json 或 skill.toml)
                let skill_json = path.join("skill.json");
                let skill_toml = path.join("skill.toml");
                let instructions_md = path.join("instructions.md");

                if skill_json.exists() {
                    self.load_skill_from_json(&skill_json, &path)?;
                } else if skill_toml.exists() {
                    self.load_skill_from_toml(&skill_toml, &path)?;
                } else if instructions_md.exists() {
                    // 简单模式：只有 instructions.md
                    self.load_skill_from_instructions(&instructions_md, &path)?;
                } else {
                    // 递归搜索子目录
                    self.load_skills_recursive(&path)?;
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // 单文件技能 (*.md)
                self.load_skill_from_single_md(&path)?;
            }
        }

        Ok(())
    }

    fn load_skill_from_json(&mut self, json_path: &Path, skill_dir: &Path) -> Result<(), String> {
        let content = fs::read_to_string(json_path)
            .map_err(|e| format!("读取 skill.json 失败: {}", e))?;

        let mut skill: Skill =
            serde_json::from_str(&content).map_err(|e| format!("解析 skill.json 失败: {}", e))?;

        // 如果 instructions 为空，尝试从 instructions.md 加载
        if skill.instructions.is_empty() {
            let instructions_path = skill_dir.join("instructions.md");
            if instructions_path.exists() {
                skill.instructions = fs::read_to_string(&instructions_path)
                    .map_err(|e| format!("读取 instructions.md 失败: {}", e))?;
            }
        }

        self.skills.insert(skill.name.clone(), skill);
        Ok(())
    }

    fn load_skill_from_toml(&mut self, toml_path: &Path, skill_dir: &Path) -> Result<(), String> {
        let content = fs::read_to_string(toml_path)
            .map_err(|e| format!("读取 skill.toml 失败: {}", e))?;

        let mut skill: Skill =
            toml::from_str(&content).map_err(|e| format!("解析 skill.toml 失败: {}", e))?;

        // 如果 instructions 为空，尝试从 instructions.md 加载
        if skill.instructions.is_empty() {
            let instructions_path = skill_dir.join("instructions.md");
            if instructions_path.exists() {
                skill.instructions = fs::read_to_string(&instructions_path)
                    .map_err(|e| format!("读取 instructions.md 失败: {}", e))?;
            }
        }

        self.skills.insert(skill.name.clone(), skill);
        Ok(())
    }

    fn load_skill_from_instructions(
        &mut self,
        instructions_path: &Path,
        skill_dir: &Path,
    ) -> Result<(), String> {
        let instructions = fs::read_to_string(instructions_path)
            .map_err(|e| format!("读取 instructions.md 失败: {}", e))?;

        let name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let skill = Skill {
            name: name.clone(),
            description: String::new(),
            instructions,
            triggers: Vec::new(),
            enabled: true,
            priority: 0,
        };

        self.skills.insert(name, skill);
        Ok(())
    }

    fn load_skill_from_single_md(&mut self, md_path: &Path) -> Result<(), String> {
        let instructions = fs::read_to_string(md_path)
            .map_err(|e| format!("读取技能文件失败: {}", e))?;

        let name = md_path
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let skill = Skill {
            name: name.clone(),
            description: String::new(),
            instructions,
            triggers: Vec::new(),
            enabled: true,
            priority: 0,
        };

        self.skills.insert(name, skill);
        Ok(())
    }

    /// 获取所有启用的技能
    pub fn enabled_skills(&self) -> Vec<&Skill> {
        let mut skills: Vec<&Skill> = self.skills.values().filter(|s| s.enabled).collect();
        skills.sort_by(|a, b| b.priority.cmp(&a.priority));
        skills
    }

    /// 根据触发词匹配技能
    pub fn match_skill(&self, input: &str) -> Option<&Skill> {
        let input_lower = input.to_lowercase();
        for skill in self.enabled_skills() {
            for trigger in &skill.triggers {
                if input_lower.contains(&trigger.to_lowercase()) {
                    return Some(skill);
                }
            }
        }
        None
    }

    /// 构建技能注入的 prompt
    pub fn build_skills_prompt(&self) -> String {
        let enabled = self.enabled_skills();
        if enabled.is_empty() {
            return String::new();
        }

        let mut lines = vec!["## 可用技能".to_string()];
        for skill in enabled {
            lines.push(format!("\n### {}", skill.name));
            if !skill.description.is_empty() {
                lines.push(skill.description.clone());
            }
            lines.push(skill.instructions.clone());
        }

        lines.join("\n")
    }

    /// 获取技能数量
    pub fn count(&self) -> usize {
        self.skills.len()
    }
}

/// 从 Codex home 目录加载技能
pub fn load_skills(codex_home: &Path) -> Result<SkillSet, String> {
    let mut skill_set = SkillSet::new(codex_home);
    skill_set.load()?;
    Ok(skill_set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_default() {
        let skill = Skill {
            name: "test".to_string(),
            description: "Test skill".to_string(),
            instructions: "Do something".to_string(),
            triggers: vec!["test".to_string()],
            enabled: true,
            priority: 0,
        };
        assert!(skill.enabled);
    }

    #[test]
    fn test_skill_set_empty() {
        let skill_set = SkillSet::default();
        assert_eq!(skill_set.count(), 0);
    }
}
