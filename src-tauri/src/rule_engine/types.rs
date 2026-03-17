//! Rule Engine Types - 规则类型定义

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// 规则核心类型
// ============================================================================

/// 规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub derived_from_memories: Vec<String>,  // 来源记忆 ID
    pub rule_type: RuleType,
    pub scope: RuleScope,
    pub priority: u32,           // 数值越大优先级越高
    pub confidence: f64,         // 0.0-1.0
    pub activation_conditions: Vec<Condition>,
    pub behavior_effect: BehaviorEffect,
    pub exceptions: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: RuleStatus,
    pub review_required: bool,   // 是否需要人工审核
    pub strength: RuleStrength,
    pub success_count: u32,
    pub failure_count: u32,
}

impl Rule {
    pub fn new(
        id: String,
        name: String,
        rule_type: RuleType,
        strength: RuleStrength,
    ) -> Self {
        let now = crate::memory::now_millis();
        Self {
            id,
            name,
            description: String::new(),
            derived_from_memories: Vec::new(),
            rule_type,
            scope: RuleScope::Global,
            priority: 50,
            confidence: 0.5,
            activation_conditions: Vec::new(),
            behavior_effect: BehaviorEffect::default(),
            exceptions: Vec::new(),
            created_at: now,
            updated_at: now,
            status: RuleStatus::Candidate,
            review_required: false,
            strength,
            success_count: 0,
            failure_count: 0,
        }
    }

    /// 计算规则成功率
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.5
        } else {
            self.success_count as f64 / total as f64
        }
    }

    /// 更新置信度
    pub fn update_confidence(&mut self, success: bool) {
        if success {
            self.success_count += 1;
            self.confidence = (self.confidence * 0.9 + 0.1).min(1.0);
        } else {
            self.failure_count += 1;
            self.confidence = (self.confidence * 0.9).max(0.0);
        }
        self.updated_at = crate::memory::now_millis();
    }
}

/// 规则类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    Preference,   // 用户偏好
    Workflow,     // 流程规则
    Risk,         // 风险规避
    Strategy,     // 任务策略
    Reflection,   // 反思规则
}

/// 规则作用域
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    Global,
    App(String),
    Window(String),
    Task(String),
}

impl Default for RuleScope {
    fn default() -> Self {
        Self::Global
    }
}

/// 规则状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Candidate,    // 候选
    Active,       // 激活
    Suspended,    // 暂停
    Deprecated,   // 废弃
}

impl Default for RuleStatus {
    fn default() -> Self {
        Self::Candidate
    }
}

/// 规则强度
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStrength {
    Suggestion = 1,   // 建议级
    Soft = 2,         // 软规则
    Strong = 3,       // 强规则
    Hard = 4,         // 硬规则 (只能系统设定)
}

impl Default for RuleStrength {
    fn default() -> Self {
        Self::Suggestion
    }
}

// ============================================================================
// 条件和效果
// ============================================================================

/// 激活条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub operator: ConditionOp,
    pub value: Value,
}

impl Condition {
    pub fn new(field: &str, operator: ConditionOp, value: Value) -> Self {
        Self {
            field: field.to_string(),
            operator,
            value,
        }
    }

    /// 评估条件是否满足
    pub fn evaluate(&self, context: &RuleContext) -> bool {
        let actual = match self.field.as_str() {
            "goal" => context.goal.as_ref().map(|s| Value::String(s.clone())),
            "intent" => context.intent.as_ref().map(|s| Value::String(s.clone())),
            "app" => context.app.as_ref().map(|s| Value::String(s.clone())),
            "window" => context.window.as_ref().map(|s| Value::String(s.clone())),
            "tool" => context.current_tool.as_ref().map(|s| Value::String(s.clone())),
            "step" => Some(Value::Number(context.step.into())),
            _ => context.custom.get(&self.field).cloned(),
        };

        let Some(actual) = actual else {
            return false;
        };

        match self.operator {
            ConditionOp::Equals => actual == self.value,
            ConditionOp::NotEquals => actual != self.value,
            ConditionOp::Contains => {
                actual.as_str().map(|s| {
                    self.value.as_str().map(|v| s.contains(v)).unwrap_or(false)
                }).unwrap_or(false)
            }
            ConditionOp::StartsWith => {
                actual.as_str().map(|s| {
                    self.value.as_str().map(|v| s.starts_with(v)).unwrap_or(false)
                }).unwrap_or(false)
            }
            ConditionOp::EndsWith => {
                actual.as_str().map(|s| {
                    self.value.as_str().map(|v| s.ends_with(v)).unwrap_or(false)
                }).unwrap_or(false)
            }
            ConditionOp::GreaterThan => {
                match (actual.as_f64(), self.value.as_f64()) {
                    (Some(a), Some(v)) => a > v,
                    _ => false,
                }
            }
            ConditionOp::LessThan => {
                match (actual.as_f64(), self.value.as_f64()) {
                    (Some(a), Some(v)) => a < v,
                    _ => false,
                }
            }
            ConditionOp::Matches => {
                // 简化的模式匹配，不使用完整 regex
                actual.as_str().map(|s| {
                    self.value.as_str().map(|pattern| {
                        s.contains(pattern) || pattern == "*"
                    }).unwrap_or(false)
                }).unwrap_or(false)
            }
        }
    }
}

/// 条件操作符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    Matches,  // 模式匹配
}

/// 行为效果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorEffect {
    pub effect_type: EffectType,
    pub parameters: Value,
}

impl Default for BehaviorEffect {
    fn default() -> Self {
        Self {
            effect_type: EffectType::LogWarning,
            parameters: Value::Null,
        }
    }
}

impl BehaviorEffect {
    pub fn set_preference(key: &str, value: Value) -> Self {
        Self {
            effect_type: EffectType::SetPreference,
            parameters: serde_json::json!({
                "key": key,
                "value": value,
            }),
        }
    }

    pub fn require_confirmation(message: &str) -> Self {
        Self {
            effect_type: EffectType::RequireConfirmation,
            parameters: Value::String(message.to_string()),
        }
    }

    pub fn log_warning(message: &str) -> Self {
        Self {
            effect_type: EffectType::LogWarning,
            parameters: Value::String(message.to_string()),
        }
    }
}

/// 效果类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectType {
    SetPreference,
    RequireConfirmation,
    SkipStep,
    AddStep,
    ModifyOrder,
    LogWarning,
    Abort,
}

// ============================================================================
// 规则上下文
// ============================================================================

/// 规则评估上下文
#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    pub goal: Option<String>,
    pub intent: Option<String>,
    pub app: Option<String>,
    pub window: Option<String>,
    pub current_tool: Option<String>,
    pub step: u32,
    pub custom: std::collections::HashMap<String, Value>,
}

impl RuleContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_goal(mut self, goal: &str) -> Self {
        self.goal = Some(goal.to_string());
        self
    }

    pub fn with_intent(mut self, intent: &str) -> Self {
        self.intent = Some(intent.to_string());
        self
    }

    pub fn with_app(mut self, app: &str) -> Self {
        self.app = Some(app.to_string());
        self
    }

    pub fn with_window(mut self, window: &str) -> Self {
        self.window = Some(window.to_string());
        self
    }

    pub fn with_tool(mut self, tool: &str) -> Self {
        self.current_tool = Some(tool.to_string());
        self
    }

    pub fn with_step(mut self, step: u32) -> Self {
        self.step = step;
        self
    }
}

// ============================================================================
// 规则应用结果
// ============================================================================

/// 规则应用结果
#[derive(Debug, Clone)]
pub struct RuleApplicationResult {
    pub applied_rules: Vec<String>,  // 应用的规则 ID
    pub effects: Vec<AppliedEffect>,
    pub blocked: bool,
    pub block_reason: Option<String>,
}

impl RuleApplicationResult {
    pub fn empty() -> Self {
        Self {
            applied_rules: Vec::new(),
            effects: Vec::new(),
            blocked: false,
            block_reason: None,
        }
    }
}

/// 已应用的效果
#[derive(Debug, Clone)]
pub struct AppliedEffect {
    pub rule_id: String,
    pub effect: BehaviorEffect,
}
