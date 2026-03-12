pub mod executor;
pub mod intent;
pub mod planner;
pub mod prompt;
pub mod router;
pub mod task_store;
pub mod types;

use std::sync::Mutex;

use self::types::AgentTaskRun;

pub struct AgentTaskState {
    active_task: Mutex<Option<AgentTaskRun>>,
}

impl AgentTaskState {
    pub fn new() -> Self {
        Self {
            active_task: Mutex::new(None),
        }
    }

    pub fn active_task(&self) -> Result<std::sync::MutexGuard<'_, Option<AgentTaskRun>>, String> {
        self.active_task
            .lock()
            .map_err(|_| "桌面任务状态锁定失败".to_string())
    }
}
