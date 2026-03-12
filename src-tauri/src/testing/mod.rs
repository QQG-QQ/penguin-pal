pub mod assertions;
pub mod harness;
pub mod history;
pub mod registry;
pub mod retry;
pub mod types;

use std::sync::Mutex;

use self::types::TestRunState;

pub struct TestingState {
    active_run: Mutex<Option<TestRunState>>,
}

impl TestingState {
    pub fn new() -> Self {
        Self {
            active_run: Mutex::new(None),
        }
    }

    pub fn active_run(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<TestRunState>>, String> {
        self.active_run
            .lock()
            .map_err(|_| "测试运行状态锁定失败".to_string())
    }
}
