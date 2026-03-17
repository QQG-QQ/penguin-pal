//! Shell Agent - 真正自主的 AI Agent
//!
//! AI 通过 shell 命令完全自主操作电脑：
//! - 无预定义工具列表，AI 自己探索系统能力
//! - 每步执行后观察结果，自主决定下一步
//! - 高风险命令需要用户确认

pub mod executor;
pub mod risk;
pub mod prompt;

pub use executor::{ShellAgentExecutor, AgentLoopResult};
pub use risk::is_high_risk_command;
