//! Rule Engine - 规则引擎主逻辑

use std::path::Path;
use super::types::*;
use super::store::RuleStore;

/// 规则引擎
pub struct RuleEngine {
    rules: Vec<Rule>,
    store: RuleStore,
}

impl RuleEngine {
    /// 创建新的规则引擎
    pub fn new(store_path: &Path) -> Self {
        Self {
            rules: Vec::new(),
            store: RuleStore::new(store_path),
        }
    }

    /// 加载规则
    pub fn load(&mut self) -> Result<(), String> {
        self.rules = self.store.load()?;
        Ok(())
    }

    /// 保存规则
    pub fn save(&self) -> Result<(), String> {
        self.store.save(&self.rules)
    }

    /// 添加规则
    pub fn add_rule(&mut self, rule: Rule) {
        // 检查是否已存在同 ID 规则
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule.id) {
            self.rules[pos] = rule;
        } else {
            self.rules.push(rule);
        }
    }

    /// 移除规则
    pub fn remove_rule(&mut self, rule_id: &str) -> Option<Rule> {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule_id) {
            Some(self.rules.remove(pos))
        } else {
            None
        }
    }

    /// 获取所有规则
    pub fn all_rules(&self) -> &[Rule] {
        &self.rules
    }

    /// 获取活跃规则
    pub fn active_rules(&self) -> Vec<&Rule> {
        self.rules
            .iter()
            .filter(|r| matches!(r.status, RuleStatus::Active))
            .collect()
    }

    /// 激活符合条件的规则
    pub fn activate_rules(&self, context: &RuleContext) -> Vec<&Rule> {
        self.rules
            .iter()
            .filter(|rule| {
                // 只考虑活跃规则
                if !matches!(rule.status, RuleStatus::Active) {
                    return false;
                }

                // 检查作用域
                if !self.scope_matches(&rule.scope, context) {
                    return false;
                }

                // 检查所有激活条件
                rule.activation_conditions.iter().all(|cond| cond.evaluate(context))
            })
            .collect()
    }

    /// 应用规则
    pub fn apply_rules(&self, context: &RuleContext) -> RuleApplicationResult {
        let activated = self.activate_rules(context);

        if activated.is_empty() {
            return RuleApplicationResult::empty();
        }

        // 按优先级和强度排序
        let mut sorted: Vec<_> = activated.into_iter().collect();
        sorted.sort_by(|a, b| {
            // 先按强度，再按优先级
            match b.strength.cmp(&a.strength) {
                std::cmp::Ordering::Equal => b.priority.cmp(&a.priority),
                other => other,
            }
        });

        let mut result = RuleApplicationResult::empty();

        for rule in sorted {
            result.applied_rules.push(rule.id.clone());
            result.effects.push(AppliedEffect {
                rule_id: rule.id.clone(),
                effect: rule.behavior_effect.clone(),
            });

            // 如果是 Abort 效果，设置阻止标志
            if matches!(rule.behavior_effect.effect_type, EffectType::Abort) {
                result.blocked = true;
                result.block_reason = rule.behavior_effect.parameters.as_str().map(String::from);
                break;
            }
        }

        result
    }

    /// 更新规则置信度
    pub fn update_rule_confidence(&mut self, rule_id: &str, success: bool) {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            rule.update_confidence(success);

            // 自动升级/降级逻辑
            if rule.success_count >= 5 && rule.success_rate() >= 0.8 {
                if matches!(rule.status, RuleStatus::Candidate) {
                    rule.status = RuleStatus::Active;
                }
                if matches!(rule.strength, RuleStrength::Suggestion) && rule.success_count >= 10 {
                    rule.strength = RuleStrength::Soft;
                }
            } else if rule.failure_count >= 3 && rule.success_rate() < 0.3 {
                rule.status = RuleStatus::Suspended;
            }
        }
    }

    /// 激活候选规则
    pub fn promote_rule(&mut self, rule_id: &str) -> Result<(), String> {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            if matches!(rule.status, RuleStatus::Candidate) {
                rule.status = RuleStatus::Active;
                rule.updated_at = crate::memory::now_millis();
                Ok(())
            } else {
                Err("规则状态不是候选".to_string())
            }
        } else {
            Err("规则不存在".to_string())
        }
    }

    /// 暂停规则
    pub fn suspend_rule(&mut self, rule_id: &str) -> Result<(), String> {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            rule.status = RuleStatus::Suspended;
            rule.updated_at = crate::memory::now_millis();
            Ok(())
        } else {
            Err("规则不存在".to_string())
        }
    }

    /// 废弃规则
    pub fn deprecate_rule(&mut self, rule_id: &str) -> Result<(), String> {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            rule.status = RuleStatus::Deprecated;
            rule.updated_at = crate::memory::now_millis();
            Ok(())
        } else {
            Err("规则不存在".to_string())
        }
    }

    /// 检查作用域是否匹配
    fn scope_matches(&self, scope: &RuleScope, context: &RuleContext) -> bool {
        match scope {
            RuleScope::Global => true,
            RuleScope::App(app) => {
                context.app.as_ref().map(|a| a.contains(app)).unwrap_or(false)
            }
            RuleScope::Window(window) => {
                context.window.as_ref().map(|w| w.contains(window)).unwrap_or(false)
            }
            RuleScope::Task(task) => {
                context.goal.as_ref().map(|g| g.contains(task)).unwrap_or(false)
            }
        }
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            store: RuleStore::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_activation() {
        let mut engine = RuleEngine::default();

        let mut rule = Rule::new(
            "test_rule".to_string(),
            "Test Rule".to_string(),
            RuleType::Preference,
            RuleStrength::Soft,
        );
        rule.status = RuleStatus::Active;
        rule.activation_conditions.push(Condition::new(
            "app",
            ConditionOp::Contains,
            serde_json::json!("notepad"),
        ));

        engine.add_rule(rule);

        let context = RuleContext::new().with_app("notepad.exe");
        let activated = engine.activate_rules(&context);

        assert_eq!(activated.len(), 1);
    }

    #[test]
    fn test_rule_confidence_update() {
        let mut engine = RuleEngine::default();

        let rule = Rule::new(
            "test_rule".to_string(),
            "Test Rule".to_string(),
            RuleType::Strategy,
            RuleStrength::Suggestion,
        );

        engine.add_rule(rule);

        // 模拟多次成功
        for _ in 0..5 {
            engine.update_rule_confidence("test_rule", true);
        }

        let rule = engine.all_rules().first().unwrap();
        assert!(rule.confidence > 0.5);
        assert_eq!(rule.success_count, 5);
    }
}
