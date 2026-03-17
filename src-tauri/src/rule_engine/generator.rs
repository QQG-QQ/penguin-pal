//! Rule Generator - 从记忆生成规则

use crate::memory::{MemoryEntry, MemoryType, EpisodicEntry, ProceduralEntry, PolicySuggestion};
use super::types::*;
use serde_json::json;

/// 规则生成器
pub struct RuleGenerator;

impl RuleGenerator {
    /// 从记忆条目生成候选规则
    pub fn generate_from_memories(memories: &[MemoryEntry]) -> Vec<Rule> {
        let mut rules = Vec::new();

        // 按类型分组
        let episodic: Vec<_> = memories.iter().filter(|m| matches!(m.memory_type, MemoryType::Episodic)).collect();
        let procedural: Vec<_> = memories.iter().filter(|m| matches!(m.memory_type, MemoryType::Procedural)).collect();
        let policy: Vec<_> = memories.iter().filter(|m| matches!(m.memory_type, MemoryType::Policy)).collect();

        // 从失败经验生成风险规避规则
        rules.extend(Self::generate_risk_rules_from_failures(&episodic));

        // 从成功经验生成策略规则
        rules.extend(Self::generate_strategy_rules_from_success(&episodic));

        // 从程序性记忆生成工作流规则
        rules.extend(Self::generate_workflow_rules(&procedural));

        // 从策略记忆生成偏好规则
        rules.extend(Self::generate_preference_rules(&policy));

        rules
    }

    /// 从失败经验生成风险规避规则
    fn generate_risk_rules_from_failures(episodic: &[&MemoryEntry]) -> Vec<Rule> {
        let mut rules = Vec::new();

        // 按失败原因分组
        let mut failure_patterns: std::collections::HashMap<String, Vec<&MemoryEntry>> = std::collections::HashMap::new();

        for entry in episodic {
            if entry.summary.contains("failed") || entry.summary.contains("失败") {
                // 提取失败模式
                let pattern = Self::extract_failure_pattern(&entry.content);
                failure_patterns.entry(pattern).or_default().push(entry);
            }
        }

        // 如果同一模式失败 >= 2 次，生成规则
        for (pattern, entries) in failure_patterns {
            if entries.len() >= 2 {
                let mut rule = Rule::new(
                    format!("risk_avoid_{}", crate::memory::now_millis()),
                    format!("避免重复失败: {}", pattern),
                    RuleType::Risk,
                    RuleStrength::Soft,
                );

                rule.description = format!(
                    "此模式已失败 {} 次，建议谨慎处理",
                    entries.len()
                );
                rule.derived_from_memories = entries.iter().map(|e| e.id.clone()).collect();
                rule.confidence = (entries.len() as f64 / 5.0).min(0.9);
                rule.behavior_effect = BehaviorEffect::log_warning(&format!(
                    "警告: {} 在类似场景下曾失败 {} 次",
                    pattern, entries.len()
                ));

                rules.push(rule);
            }
        }

        rules
    }

    /// 从成功经验生成策略规则
    fn generate_strategy_rules_from_success(episodic: &[&MemoryEntry]) -> Vec<Rule> {
        let mut rules = Vec::new();

        // 按成功模式分组
        let mut success_patterns: std::collections::HashMap<String, Vec<&MemoryEntry>> = std::collections::HashMap::new();

        for entry in episodic {
            if entry.summary.contains("completed") || entry.summary.contains("成功") {
                let pattern = Self::extract_success_pattern(&entry.content);
                success_patterns.entry(pattern).or_default().push(entry);
            }
        }

        // 如果同一模式成功 >= 3 次，生成规则
        for (pattern, entries) in success_patterns {
            if entries.len() >= 3 {
                let mut rule = Rule::new(
                    format!("strategy_success_{}", crate::memory::now_millis()),
                    format!("推荐策略: {}", pattern),
                    RuleType::Strategy,
                    RuleStrength::Suggestion,
                );

                rule.description = format!(
                    "此方法已成功 {} 次",
                    entries.len()
                );
                rule.derived_from_memories = entries.iter().map(|e| e.id.clone()).collect();
                rule.confidence = (entries.len() as f64 / 10.0).min(0.95);
                rule.behavior_effect = BehaviorEffect::set_preference(
                    "preferred_strategy",
                    json!(pattern),
                );

                rules.push(rule);
            }
        }

        rules
    }

    /// 从程序性记忆生成工作流规则
    fn generate_workflow_rules(procedural: &[&MemoryEntry]) -> Vec<Rule> {
        let mut rules = Vec::new();

        for entry in procedural {
            // 只处理高置信度的程序性记忆
            if entry.confidence >= 0.7 && entry.frequency >= 3 {
                let mut rule = Rule::new(
                    format!("workflow_{}", entry.id),
                    format!("工作流: {}", entry.summary),
                    RuleType::Workflow,
                    RuleStrength::Soft,
                );

                rule.description = entry.content.clone();
                rule.derived_from_memories = vec![entry.id.clone()];
                rule.confidence = entry.confidence;
                rule.behavior_effect = BehaviorEffect::set_preference(
                    "preferred_workflow",
                    json!(entry.content),
                );

                // 从 tags 提取作用域
                if let Some(app) = entry.tags.iter().find(|t| t.starts_with("app:")) {
                    rule.scope = RuleScope::App(app.trim_start_matches("app:").to_string());
                }

                rules.push(rule);
            }
        }

        rules
    }

    /// 从策略记忆生成偏好规则
    fn generate_preference_rules(policy: &[&MemoryEntry]) -> Vec<Rule> {
        let mut rules = Vec::new();

        for entry in policy {
            // 只处理已批准或高置信度的策略
            if entry.confidence >= 0.6 {
                let mut rule = Rule::new(
                    format!("preference_{}", entry.id),
                    format!("偏好: {}", entry.summary),
                    RuleType::Preference,
                    RuleStrength::Suggestion,
                );

                rule.description = entry.content.clone();
                rule.derived_from_memories = vec![entry.id.clone()];
                rule.confidence = entry.confidence;
                rule.behavior_effect = BehaviorEffect::set_preference(
                    "user_preference",
                    json!(entry.content),
                );

                rules.push(rule);
            }
        }

        rules
    }

    /// 提取失败模式
    fn extract_failure_pattern(content: &str) -> String {
        // 简化实现：提取前 50 个字符作为模式
        content.chars().take(50).collect::<String>()
    }

    /// 提取成功模式
    fn extract_success_pattern(content: &str) -> String {
        // 简化实现：提取前 50 个字符作为模式
        content.chars().take(50).collect::<String>()
    }

    /// 判断是否应该升级规则
    pub fn should_promote(rule: &Rule) -> bool {
        rule.success_count >= 5 && rule.success_rate() >= 0.8
    }

    /// 判断是否应该降级规则
    pub fn should_demote(rule: &Rule) -> bool {
        rule.failure_count >= 3 && rule.success_rate() < 0.3
    }

    /// 判断是否应该废弃规则
    pub fn should_deprecate(rule: &Rule) -> bool {
        rule.failure_count >= 5 && rule.success_rate() < 0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryScope;

    #[test]
    fn test_should_promote() {
        let mut rule = Rule::new(
            "test".to_string(),
            "Test".to_string(),
            RuleType::Strategy,
            RuleStrength::Suggestion,
        );

        rule.success_count = 5;
        rule.failure_count = 1;

        assert!(RuleGenerator::should_promote(&rule));
    }

    #[test]
    fn test_should_demote() {
        let mut rule = Rule::new(
            "test".to_string(),
            "Test".to_string(),
            RuleType::Strategy,
            RuleStrength::Soft,
        );

        rule.success_count = 1;
        rule.failure_count = 5;

        assert!(RuleGenerator::should_demote(&rule));
    }
}
