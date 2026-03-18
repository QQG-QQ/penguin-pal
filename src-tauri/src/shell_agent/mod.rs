//! Shell Agent - 真正自主的 AI Agent
//!
//! AI 通过 shell 命令完全自主操作电脑：
//! - 无预定义工具列表，AI 自己探索系统能力
//! - 每步执行后观察结果，自主决定下一步
//! - 高风险命令需要用户确认
//!
//! ## 三层架构集成
//!
//! 1. **记忆层**: 从 MemoryService 检索相关记忆，任务完成后写回经验
//! 2. **规则层**: RuleEngine 应用行为规则，调整 AI 行为
//! 3. **权限层**: PermissionChecker 验证操作权限，AI 不能自主修改权限
//!
//! ## 权限模型
//!
//! 核心原则：AI 可以自主决定记忆与规则，但不能自主决定权限。
//!
//! - `shell:execute` - 基本命令执行
//! - `shell:delete` - 删除文件/目录
//! - `shell:modify` - 修改/移动文件
//! - `shell:network` - 网络请求
//! - `shell:registry` - 注册表操作
//! - `shell:system` - 系统管理操作

#![allow(unused_imports)]

pub mod executor;
pub mod prompt;
pub mod risk;
pub mod state;

pub use executor::{ShellAgentExecutor, grant_basic_shell_permissions, PendingShellConfirmation};
pub use state::{BehaviorState, MaintenanceResult};
