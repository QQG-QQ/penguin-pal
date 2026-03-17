//! Permission Module - 独立权限系统
//!
//! 核心原则：AI 可以自主决定记忆与规则，但不能自主决定权限。
//!
//! ## 设计原则
//!
//! 1. **权限独立性**: 权限系统独立于 AI 决策，AI 只能请求权限，不能授予或修改权限。
//! 2. **用户主权**: 所有权限的最终决定权在用户手中。
//! 3. **最小权限**: 默认拒绝，需要明确授权。
//! 4. **审计追踪**: 所有权限操作都有完整的审计日志。
//!
//! ## 权限类别
//!
//! - FileSystem: 文件系统操作
//! - Network: 网络访问
//! - Process: 进程管理
//! - Shell: Shell 命令执行
//! - System: 系统设置
//! - Privacy: 隐私数据
//! - Desktop: 桌面控制
//! - Memory: 记忆系统
//! - Rule: 规则系统
//!
//! ## 使用示例
//!
//! ```ignore
//! let mut checker = PermissionChecker::new();
//!
//! // AI 请求权限（返回检查结果，可能需要用户确认）
//! let result = checker.check("shell:execute", "ai_agent");
//!
//! if result.allowed {
//!     // 执行操作
//! } else if result.requires_confirmation {
//!     // 等待用户确认
//! } else {
//!     // 操作被拒绝
//! }
//!
//! // 用户授予权限
//! checker.grant("shell:execute", GrantSource::User, PermissionScope::Session, Some(3600000))?;
//! ```

#![allow(unused)]

pub mod checker;
pub mod store;
pub mod types;

// 类型导出
pub use types::{
    // 核心类型
    Permission,
    PermissionCategory,
    PermissionLevel,
    PermissionScope,
    GrantSource,
    // 请求相关
    PermissionRequest,
    RequestStatus,
    RequestResponse,
    // 检查结果
    PermissionCheckResult,
    // 策略
    PermissionPolicy,
    PolicyRule,
    PolicyAction,
    PolicyCondition,
    // 审计
    PermissionAuditEntry,
    AuditAction,
};

// 检查器导出
pub use checker::PermissionChecker;

// 存储导出
pub use store::{PermissionStore, PermissionState};
