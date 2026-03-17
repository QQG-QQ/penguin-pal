//! Rule Engine Module - 规则引擎
//!
//! 从记忆中提取、生成、管理规则。
//!
//! ## 规则类型
//! - Preference: 用户偏好
//! - Workflow: 流程规则
//! - Risk: 风险规避
//! - Strategy: 任务策略
//! - Reflection: 反思规则
//!
//! ## 规则强度（优先级从低到高）
//! - Suggestion: 建议级
//! - Soft: 软规则
//! - Strong: 强规则
//! - Hard: 硬规则（只能系统设定）
//!
//! ## 规则生命周期
//! Candidate -> Active -> Suspended -> Deprecated

#![allow(unused)]

pub mod engine;
pub mod generator;
pub mod resolver;
pub mod store;
pub mod types;

// 核心类型导出
pub use types::{
    // 规则核心
    Rule,
    RuleType,
    RuleScope,
    RuleStatus,
    RuleStrength,
    // 条件和效果
    Condition,
    ConditionOp,
    BehaviorEffect,
    EffectType,
    // 上下文和结果
    RuleContext,
    RuleApplicationResult,
    AppliedEffect,
};

// 引擎导出
pub use engine::RuleEngine;

// 生成器导出
pub use generator::RuleGenerator;

// 解决器导出
pub use resolver::{RuleResolver, RuleConflict, ConflictType};

// 存储导出
pub use store::RuleStore;
