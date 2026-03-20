//! Agent Module - 桌面控制代理
//!
//! 保留用于 control service API 和 vision 缓存管理。
//! 主聊天路径已迁移到 shell_agent 模块。
#![allow(unused)]

pub mod executor;
pub mod agent_turn;
pub mod domain_loop_planner;
pub mod intent;
pub mod intent_classifier;
pub mod loop_planner;
pub mod loop_prompt;
pub mod model_adapter;
pub mod planner;
pub mod prompt;
pub mod runtime_binding;
pub mod runtime_context;
pub mod router;
pub mod screen_context;
pub mod session_turn;
pub mod workspace_context;
pub mod workspace_loop_planner;
pub mod workspace_loop_prompt;
// NOTE: screen_planner 已弃用，主链已迁移到 loop_planner / test_loop_planner
// 保留文件以备回退参考，但不再导出
// pub mod screen_planner;
pub mod screen_reconciler;
pub mod task_store;
pub mod test_assertions;
pub mod test_loop_planner;
pub mod test_loop_prompt;
pub mod types;
pub mod vision_analyzer;
pub mod vision_context;
pub mod vision_types;

use std::sync::Mutex;

use self::{types::AgentTaskRun, vision_types::CachedVisionContext};

pub struct AgentTaskState {
    active_task: Mutex<Option<AgentTaskRun>>,
    vision_cache: Mutex<Option<CachedVisionContext>>,
}

impl AgentTaskState {
    pub fn new() -> Self {
        Self {
            active_task: Mutex::new(None),
            vision_cache: Mutex::new(None),
        }
    }

    pub fn active_task(&self) -> Result<std::sync::MutexGuard<'_, Option<AgentTaskRun>>, String> {
        self.active_task
            .lock()
            .map_err(|_| "桌面任务状态锁定失败".to_string())
    }

    pub fn vision_cache(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<CachedVisionContext>>, String> {
        self.vision_cache
            .lock()
            .map_err(|_| "视觉上下文缓存锁定失败".to_string())
    }
}
