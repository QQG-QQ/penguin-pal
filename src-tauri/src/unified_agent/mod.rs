//! 统一智能 Agent 模块
//!
//! 移除意图分类层，让 AI 自主决定如何响应用户输入。
//! AI 可以选择：
//! - 直接文本回复（普通对话）
//! - 调用工具（桌面操作、文件操作等）
//! - 查询内部状态（记忆系统）

pub mod executor;
pub mod response;
pub mod prompt;

// 保留模块但不再导出，已被 shell_agent 替代
#[allow(unused_imports)]
pub use executor::UnifiedAgentExecutor;
#[allow(unused_imports)]
pub use response::parse_response as response;
