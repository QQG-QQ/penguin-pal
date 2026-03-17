//! Codex 规则系统
//!
//! 从 rules/ 目录加载规则，支持自动批准命令。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 规则决策
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDecision {
    /// 允许执行
    Allow,
    /// 拒绝执行
    Deny,
    /// 需要确认
    Confirm,
}

impl Default for RuleDecision {
    fn default() -> Self {
        Self::Confirm
    }
}

/// 单条规则
#[derive(Debug, Clone)]
pub struct Rule {
    /// 规则类型
    pub rule_type: RuleType,
    /// 匹配模式
    pub pattern: Vec<String>,
    /// 决策
    pub decision: RuleDecision,
}

/// 规则类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleType {
    /// 前缀匹配
    Prefix,
    /// 精确匹配
    Exact,
    /// 正则匹配
    Regex,
}

/// 规则集合
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
    rules_dir: PathBuf,
}

impl RuleSet {
    /// 创建新的规则集合
    pub fn new(codex_home: &Path) -> Self {
        Self {
            rules: Vec::new(),
            rules_dir: codex_home.join("rules"),
        }
    }

    /// 从目录加载所有规则
    pub fn load(&mut self) -> Result<(), String> {
        self.rules.clear();

        if !self.rules_dir.exists() {
            fs::create_dir_all(&self.rules_dir)
                .map_err(|e| format!("创建 rules 目录失败: {}", e))?;
            return Ok(());
        }

        let entries = fs::read_dir(&self.rules_dir)
            .map_err(|e| format!("读取 rules 目录失败: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rules").unwrap_or(false) {
                self.load_rules_file(&path)?;
            }
        }

        Ok(())
    }

    fn load_rules_file(&mut self, path: &Path) -> Result<(), String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("读取规则文件失败: {}", e))?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }

            if let Some(rule) = self.parse_rule(line) {
                self.rules.push(rule);
            }
        }

        Ok(())
    }

    fn parse_rule(&self, line: &str) -> Option<Rule> {
        // 解析格式: rule_type(pattern=[...], decision="...")
        // 例如: prefix_rule(pattern=["git", "status"], decision="allow")

        let line = line.trim();

        // 解析规则类型
        let rule_type = if line.starts_with("prefix_rule") {
            RuleType::Prefix
        } else if line.starts_with("exact_rule") {
            RuleType::Exact
        } else if line.starts_with("regex_rule") {
            RuleType::Regex
        } else {
            return None;
        };

        // 提取参数部分
        let start = line.find('(')?;
        let end = line.rfind(')')?;
        let params = &line[start + 1..end];

        // 解析 pattern
        let pattern = self.extract_pattern(params)?;

        // 解析 decision
        let decision = self.extract_decision(params)?;

        Some(Rule {
            rule_type,
            pattern,
            decision,
        })
    }

    fn extract_pattern(&self, params: &str) -> Option<Vec<String>> {
        // 查找 pattern=[...]
        let pattern_start = params.find("pattern=")?;
        let after_pattern = &params[pattern_start + 8..];

        let bracket_start = after_pattern.find('[')?;
        let bracket_end = after_pattern.find(']')?;
        let array_content = &after_pattern[bracket_start + 1..bracket_end];

        // 解析数组元素
        let elements: Vec<String> = array_content
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if elements.is_empty() {
            None
        } else {
            Some(elements)
        }
    }

    fn extract_decision(&self, params: &str) -> Option<RuleDecision> {
        // 查找 decision="..."
        let decision_start = params.find("decision=")?;
        let after_decision = &params[decision_start + 9..];

        let value = after_decision
            .trim()
            .trim_start_matches('"')
            .trim_start_matches('\'')
            .split(|c| c == '"' || c == '\'' || c == ',' || c == ')')
            .next()?
            .to_lowercase();

        match value.as_str() {
            "allow" => Some(RuleDecision::Allow),
            "deny" => Some(RuleDecision::Deny),
            "confirm" => Some(RuleDecision::Confirm),
            _ => None,
        }
    }

    /// 检查命令是否匹配规则
    pub fn check_command(&self, command: &[String]) -> Option<RuleDecision> {
        for rule in &self.rules {
            if self.matches_rule(command, rule) {
                return Some(rule.decision.clone());
            }
        }
        None
    }

    /// 检查命令字符串是否匹配规则
    pub fn check_command_str(&self, command: &str) -> Option<RuleDecision> {
        let parts: Vec<String> = command
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        self.check_command(&parts)
    }

    fn matches_rule(&self, command: &[String], rule: &Rule) -> bool {
        match rule.rule_type {
            RuleType::Prefix => {
                if command.len() < rule.pattern.len() {
                    return false;
                }
                rule.pattern
                    .iter()
                    .zip(command.iter())
                    .all(|(p, c)| p == c || p == "*")
            }
            RuleType::Exact => {
                if command.len() != rule.pattern.len() {
                    return false;
                }
                rule.pattern
                    .iter()
                    .zip(command.iter())
                    .all(|(p, c)| p == c || p == "*")
            }
            RuleType::Regex => {
                // 简化的正则匹配（只支持 * 通配符）
                let pattern_str = rule.pattern.join(" ");
                let command_str = command.join(" ");
                self.simple_wildcard_match(&pattern_str, &command_str)
            }
        }
    }

    fn simple_wildcard_match(&self, pattern: &str, text: &str) -> bool {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.is_empty() {
            return true;
        }

        let mut remaining = text;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i == 0 {
                // 第一部分必须是前缀
                if !remaining.starts_with(part) {
                    return false;
                }
                remaining = &remaining[part.len()..];
            } else if i == parts.len() - 1 {
                // 最后一部分必须是后缀
                if !remaining.ends_with(part) {
                    return false;
                }
            } else {
                // 中间部分只需要存在
                if let Some(pos) = remaining.find(part) {
                    remaining = &remaining[pos + part.len()..];
                } else {
                    return false;
                }
            }
        }

        true
    }

    /// 添加规则
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// 保存规则到文件
    pub fn save(&self, filename: &str) -> Result<(), String> {
        let path = self.rules_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建规则目录失败: {}", e))?;
        }

        let mut lines = Vec::new();
        lines.push("# Codex 自动批准规则".to_string());
        lines.push("# 格式: rule_type(pattern=[...], decision=\"...\")".to_string());
        lines.push("".to_string());

        for rule in &self.rules {
            let rule_type = match rule.rule_type {
                RuleType::Prefix => "prefix_rule",
                RuleType::Exact => "exact_rule",
                RuleType::Regex => "regex_rule",
            };
            let pattern = rule
                .pattern
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(", ");
            let decision = match rule.decision {
                RuleDecision::Allow => "allow",
                RuleDecision::Deny => "deny",
                RuleDecision::Confirm => "confirm",
            };
            lines.push(format!(
                "{}(pattern=[{}], decision=\"{}\")",
                rule_type, pattern, decision
            ));
        }

        fs::write(&path, lines.join("\n"))
            .map_err(|e| format!("保存规则文件失败: {}", e))
    }

    /// 获取规则数量
    pub fn count(&self) -> usize {
        self.rules.len()
    }

    /// 检查是否允许自动执行
    pub fn is_auto_allowed(&self, command: &str) -> bool {
        self.check_command_str(command) == Some(RuleDecision::Allow)
    }
}

/// 从 Codex home 目录加载规则
pub fn load_rules(codex_home: &Path) -> Result<RuleSet, String> {
    let mut rule_set = RuleSet::new(codex_home);
    rule_set.load()?;
    Ok(rule_set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prefix_rule() {
        let rule_set = RuleSet::default();
        let rule = rule_set
            .parse_rule("prefix_rule(pattern=[\"git\", \"status\"], decision=\"allow\")")
            .unwrap();
        assert_eq!(rule.rule_type, RuleType::Prefix);
        assert_eq!(rule.pattern, vec!["git", "status"]);
        assert_eq!(rule.decision, RuleDecision::Allow);
    }

    #[test]
    fn test_check_command() {
        let mut rule_set = RuleSet::default();
        rule_set.add_rule(Rule {
            rule_type: RuleType::Prefix,
            pattern: vec!["git".to_string(), "status".to_string()],
            decision: RuleDecision::Allow,
        });

        let cmd = vec!["git".to_string(), "status".to_string()];
        assert_eq!(rule_set.check_command(&cmd), Some(RuleDecision::Allow));

        let cmd2 = vec!["git".to_string(), "push".to_string()];
        assert_eq!(rule_set.check_command(&cmd2), None);
    }

    #[test]
    fn test_wildcard_match() {
        let rule_set = RuleSet::default();
        assert!(rule_set.simple_wildcard_match("git *", "git status"));
        assert!(rule_set.simple_wildcard_match("npm * build", "npm run build"));
        assert!(!rule_set.simple_wildcard_match("git push", "git status"));
    }
}
