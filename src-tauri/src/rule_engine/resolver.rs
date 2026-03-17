//! Rule Resolver - 规则冲突处理

use super::types::*;

/// 规则冲突解决器
pub struct RuleResolver;

impl RuleResolver {
    /// 解决规则冲突，返回最终有效规则列表
    ///
    /// 优先级顺序：
    /// 硬规则 > 权限约束 > 当前用户指令 > 强规则 > 软规则 > 临时策略
    pub fn resolve_conflicts(rules: Vec<Rule>, context: &RuleContext) -> Vec<Rule> {
        if rules.is_empty() {
            return Vec::new();
        }

        // 按优先级排序
        let mut sorted = rules;
        sorted.sort_by(|a, b| Self::compare_priority(a, b));

        // 检测并处理冲突
        Self::handle_conflicts(sorted, context)
    }

    /// 获取有效规则（已排序、已去重、已解决冲突）
    pub fn get_effective_rules<'a>(all_rules: &'a [Rule], context: &RuleContext) -> Vec<&'a Rule> {
        // 过滤活跃且符合条件的规则
        let mut candidates: Vec<_> = all_rules
            .iter()
            .filter(|rule| {
                matches!(rule.status, RuleStatus::Active)
                    && Self::scope_matches(&rule.scope, context)
                    && Self::conditions_match(rule, context)
            })
            .collect();

        // 按优先级排序
        candidates.sort_by(|a, b| Self::compare_priority(a, b));

        // 去除冲突规则（保留优先级高的）
        Self::remove_conflicting_refs(candidates)
    }

    /// 比较两个规则的优先级
    fn compare_priority(a: &Rule, b: &Rule) -> std::cmp::Ordering {
        // 1. 先按强度比较
        match b.strength.cmp(&a.strength) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }

        // 2. 再按优先级数值比较
        match b.priority.cmp(&a.priority) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }

        // 3. 最后按置信度比较
        b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
    }

    /// 处理冲突规则
    fn handle_conflicts(rules: Vec<Rule>, _context: &RuleContext) -> Vec<Rule> {
        let mut result = Vec::new();
        let mut effect_types_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for rule in rules {
            let effect_key = format!("{:?}", rule.behavior_effect.effect_type);

            // 如果同类型效果已存在且当前规则优先级更低，跳过
            if effect_types_seen.contains(&effect_key) {
                // 检查是否允许多个同类型效果
                if !Self::allows_multiple(&rule.behavior_effect.effect_type) {
                    continue;
                }
            }

            effect_types_seen.insert(effect_key);
            result.push(rule);
        }

        result
    }

    /// 移除冲突的规则引用
    fn remove_conflicting_refs(rules: Vec<&Rule>) -> Vec<&Rule> {
        let mut result = Vec::new();
        let mut effect_types_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for rule in rules {
            let effect_key = format!("{:?}", rule.behavior_effect.effect_type);

            if effect_types_seen.contains(&effect_key) && !Self::allows_multiple(&rule.behavior_effect.effect_type) {
                continue;
            }

            effect_types_seen.insert(effect_key);
            result.push(rule);
        }

        result
    }

    /// 检查是否允许多个同类型效果
    fn allows_multiple(effect_type: &EffectType) -> bool {
        match effect_type {
            EffectType::LogWarning => true,  // 允许多个警告
            EffectType::AddStep => true,     // 允许添加多个步骤
            _ => false,
        }
    }

    /// 检查作用域是否匹配
    fn scope_matches(scope: &RuleScope, context: &RuleContext) -> bool {
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

    /// 检查规则条件是否满足
    fn conditions_match(rule: &Rule, context: &RuleContext) -> bool {
        rule.activation_conditions.iter().all(|cond| cond.evaluate(context))
    }

    /// 合并规则效果
    pub fn merge_effects(rules: &[&Rule]) -> Vec<BehaviorEffect> {
        let mut effects = Vec::new();
        let mut seen_types: std::collections::HashSet<String> = std::collections::HashSet::new();

        for rule in rules {
            let effect_key = format!("{:?}", rule.behavior_effect.effect_type);

            if !seen_types.contains(&effect_key) || Self::allows_multiple(&rule.behavior_effect.effect_type) {
                effects.push(rule.behavior_effect.clone());
                seen_types.insert(effect_key);
            }
        }

        effects
    }

    /// 检测规则冲突
    pub fn detect_conflicts(rules: &[Rule]) -> Vec<RuleConflict> {
        let mut conflicts = Vec::new();

        for i in 0..rules.len() {
            for j in (i + 1)..rules.len() {
                if Self::rules_conflict(&rules[i], &rules[j]) {
                    conflicts.push(RuleConflict {
                        rule_a: rules[i].id.clone(),
                        rule_b: rules[j].id.clone(),
                        conflict_type: ConflictType::SameEffect,
                        resolution: if rules[i].strength > rules[j].strength {
                            format!("保留 {} (强度更高)", rules[i].name)
                        } else if rules[i].priority > rules[j].priority {
                            format!("保留 {} (优先级更高)", rules[i].name)
                        } else {
                            format!("保留 {} (置信度更高)", rules[i].name)
                        },
                    });
                }
            }
        }

        conflicts
    }

    /// 判断两个规则是否冲突
    fn rules_conflict(a: &Rule, b: &Rule) -> bool {
        // 同类型效果且不允许多个
        let same_effect = std::mem::discriminant(&a.behavior_effect.effect_type)
            == std::mem::discriminant(&b.behavior_effect.effect_type);

        if !same_effect {
            return false;
        }

        // 检查是否允许多个
        !Self::allows_multiple(&a.behavior_effect.effect_type)
    }
}

/// 规则冲突
#[derive(Debug, Clone)]
pub struct RuleConflict {
    pub rule_a: String,
    pub rule_b: String,
    pub conflict_type: ConflictType,
    pub resolution: String,
}

/// 冲突类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    SameEffect,
    ContradictoryEffect,
    ScopeOverlap,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_comparison() {
        let rule_hard = {
            let mut r = Rule::new("1".into(), "Hard".into(), RuleType::Risk, RuleStrength::Hard);
            r.priority = 10;
            r
        };

        let rule_soft = {
            let mut r = Rule::new("2".into(), "Soft".into(), RuleType::Preference, RuleStrength::Soft);
            r.priority = 100;  // 更高的优先级数值
            r
        };

        // 硬规则应该优先于软规则，即使软规则优先级数值更高
        assert_eq!(
            RuleResolver::compare_priority(&rule_hard, &rule_soft),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_conflict_detection() {
        let rule1 = {
            let mut r = Rule::new("1".into(), "Rule 1".into(), RuleType::Preference, RuleStrength::Soft);
            r.behavior_effect = BehaviorEffect::require_confirmation("test");
            r
        };

        let rule2 = {
            let mut r = Rule::new("2".into(), "Rule 2".into(), RuleType::Preference, RuleStrength::Soft);
            r.behavior_effect = BehaviorEffect::require_confirmation("test2");
            r
        };

        let conflicts = RuleResolver::detect_conflicts(&[rule1, rule2]);
        assert!(!conflicts.is_empty());
    }
}
