//! One inference at a time; local diagnostics never enter this queue.
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Task {
    Reply,
    Manual,
    Interactive,
    Automatic,
    Learning,
}
impl Task {
    fn priority(self) -> u8 {
        match self {
            Self::Reply | Self::Manual | Self::Interactive => 0,
            Self::Automatic => 1,
            Self::Learning => 2,
        }
    }
}
#[derive(Clone)]
struct Entry {
    id: u64,
    task: Task,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
struct State {
    sequence: u64,
    active: Option<Entry>,
    waiting: VecDeque<Entry>,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
#[derive(Clone, Default)]
pub struct Scheduler(Arc<Shared>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    active: Option<Task>,
    queued_manual: bool,
    queued: usize,
    cancelling: bool,
}
pub struct Running {
    scheduler: Scheduler,
    entry: Entry,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut state) = self.scheduler.0.state.lock() {
            if state.active.as_ref().is_some_and(|a| a.id == self.entry.id) {
                state.active = None;
            }
            state.waiting.retain(|e| e.id != self.entry.id);
            self.scheduler.0.changed.notify_all();
        }
    }
}
impl Scheduler {
    fn entry(state: &mut State, task: Task) -> Entry {
        state.sequence += 1;
        Entry {
            id: state.sequence,
            task,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn reserve(&self, task: Task) -> Result<Running, String> {
        let mut state = self.0.state.lock().map_err(|_| "陪伴调度暂不可用。")?;
        if task == Task::Manual
            && state
                .active
                .iter()
                .chain(state.waiting.iter())
                .any(|e| e.task == task && !e.cancelled.load(Ordering::Acquire))
        {
            return Err("已有一轮手动搭话正在等待或生成，请勿重复点击。".into());
        }
        if state.waiting.len() >= 32 {
            return Err("等待的请求较多，请稍后再试；已保存的消息保留。".into());
        }
        let entry = Self::entry(&mut state, task);
        state.waiting.push_back(entry.clone());
        self.0.changed.notify_all();
        Ok(Running {
            scheduler: self.clone(),
            entry,
        })
    }
    pub fn acquire(&self, task: Task, timeout: Duration) -> Result<Running, String> {
        self.reserve(task)?.wait(timeout)
    }
    /// Background checks do not accumulate stale requests or overtake manual work.
    pub fn try_acquire(&self, task: Task) -> Option<Running> {
        let mut state = self.0.state.lock().ok()?;
        if state.active.is_some() || !state.waiting.is_empty() {
            return None;
        }
        let entry = Self::entry(&mut state, task);
        state.active = Some(entry.clone());
        Some(Running {
            scheduler: self.clone(),
            entry,
        })
    }
    pub fn cancel_manual(&self) {
        if let Ok(state) = self.0.state.lock() {
            for e in state
                .active
                .iter()
                .chain(state.waiting.iter())
                .filter(|e| e.task == Task::Manual)
            {
                e.cancelled.store(true, Ordering::Release);
            }
            self.0.changed.notify_all();
        }
    }
    pub fn manual_cancelled(&self) -> bool {
        self.0
            .state
            .lock()
            .map(|s| {
                s.active
                    .as_ref()
                    .is_some_and(|e| e.task == Task::Manual && e.cancelled.load(Ordering::Acquire))
            })
            .unwrap_or(true)
    }
    pub fn status(&self) -> Result<Status, String> {
        let state = self.0.state.lock().map_err(|_| "陪伴调度暂不可用。")?;
        Ok(Status {
            active: state.active.as_ref().map(|e| e.task),
            queued_manual: state
                .waiting
                .iter()
                .any(|e| e.task == Task::Manual && !e.cancelled.load(Ordering::Acquire)),
            queued: state.waiting.len(),
            cancelling: state
                .active
                .as_ref()
                .is_some_and(|e| e.task == Task::Manual && e.cancelled.load(Ordering::Acquire)),
        })
    }
}
impl Running {
    pub fn wait(self, timeout: Duration) -> Result<Self, String> {
        let mut state = self
            .scheduler
            .0
            .state
            .lock()
            .map_err(|_| "陪伴调度暂不可用。")?;
        let entry = &self.entry;
        let started = Instant::now();
        loop {
            let failure = if entry.cancelled.load(Ordering::Acquire) {
                Some("手动搭话已取消。")
            } else if started.elapsed() >= timeout {
                Some("等待超时，请重试；已保存的消息保留。")
            } else {
                None
            };
            if let Some(error) = failure {
                state.waiting.retain(|e| e.id != entry.id);
                self.scheduler.0.changed.notify_all();
                drop(state);
                return Err(error.into());
            }
            let next = state
                .waiting
                .iter()
                .filter(|e| !e.cancelled.load(Ordering::Acquire))
                .min_by_key(|e| (e.task.priority(), e.id));
            if state.active.is_none() && next.is_some_and(|e| e.id == entry.id) {
                state.waiting.retain(|e| e.id != entry.id);
                state.active = Some(entry.clone());
                drop(state);
                return Ok(self);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            state = self
                .scheduler
                .0
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "陪伴调度中断。")?
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn wait_for_queue(scheduler: &Scheduler) {
        let start = Instant::now();
        while scheduler.status().unwrap().queued == 0 {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::yield_now();
        }
    }
    #[test]
    fn manual_waits_for_current_learning_and_prevents_next_batch() {
        let scheduler = Scheduler::default();
        let current = scheduler.try_acquire(Task::Learning).unwrap();
        let worker = scheduler.clone();
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let guard = worker
                .acquire(Task::Manual, Duration::from_secs(3))
                .unwrap();
            tx.send(()).unwrap();
            guard
        });
        wait_for_queue(&scheduler);
        assert!(scheduler.try_acquire(Task::Learning).is_none());
        assert!(scheduler.try_acquire(Task::Automatic).is_none());
        assert!(scheduler
            .acquire(Task::Manual, Duration::from_millis(1))
            .is_err());
        drop(current);
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let manual = thread.join().unwrap();
        assert!(scheduler.try_acquire(Task::Learning).is_none());
        drop(manual);
        assert!(scheduler.try_acquire(Task::Automatic).is_some());
    }
    #[test]
    fn waiting_cancellation_and_timeout_leave_no_stuck_slot() {
        let scheduler = Scheduler::default();
        let current = scheduler.try_acquire(Task::Learning).unwrap();
        let worker = scheduler.clone();
        let thread = std::thread::spawn(move || {
            worker
                .acquire(Task::Manual, Duration::from_secs(3))
                .err()
                .unwrap()
        });
        wait_for_queue(&scheduler);
        scheduler.cancel_manual();
        assert!(thread.join().unwrap().contains("取消"));
        assert!(scheduler
            .acquire(Task::Reply, Duration::from_millis(5))
            .is_err());
        assert_eq!(scheduler.status().unwrap().queued, 0);
        drop(current);
        assert!(scheduler.try_acquire(Task::Learning).is_some());
    }
    #[test]
    fn active_cancellation_keeps_inference_serial_until_cleanup() {
        let scheduler = Scheduler::default();
        let manual = scheduler
            .acquire(Task::Manual, Duration::from_secs(1))
            .unwrap();
        scheduler.cancel_manual();
        assert!(scheduler.manual_cancelled());
        assert!(scheduler.try_acquire(Task::Learning).is_none());
        drop(manual);
        assert!(!scheduler.manual_cancelled());
        assert!(scheduler.try_acquire(Task::Learning).is_some());
    }
    #[test]
    fn immediate_cancellation_and_abandoned_requests_release_queue() {
        let scheduler = Scheduler::default();
        let request = scheduler.reserve(Task::Manual).unwrap();
        scheduler.cancel_manual();
        assert!(request
            .wait(Duration::from_secs(1))
            .err()
            .unwrap()
            .contains("取消"));
        let abandoned = scheduler.reserve(Task::Manual).unwrap();
        drop(abandoned);
        assert_eq!(scheduler.status().unwrap().queued, 0);
        let result = std::panic::catch_unwind(|| {
            let _slot = scheduler.try_acquire(Task::Learning).unwrap();
            panic!("fixture failure");
        });
        assert!(result.is_err());
        assert!(scheduler.try_acquire(Task::Automatic).is_some());
    }
    #[test]
    fn manual_takes_priority_over_queued_automatic_and_learning() {
        let scheduler = Scheduler::default();
        let current = scheduler.try_acquire(Task::Learning).unwrap();
        let automatic = scheduler.reserve(Task::Automatic).unwrap();
        let learning = scheduler.reserve(Task::Learning).unwrap();
        let manual = scheduler.reserve(Task::Manual).unwrap();
        drop(current);
        let manual = manual.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(scheduler.status().unwrap().active, Some(Task::Manual));
        drop(manual);
        let automatic = automatic.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(scheduler.status().unwrap().active, Some(Task::Automatic));
        drop(automatic);
        assert!(learning.wait(Duration::from_secs(1)).is_ok());
    }
}
