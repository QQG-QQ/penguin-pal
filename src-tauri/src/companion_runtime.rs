//! Bounded local diagnostics and crash recovery. No chat payloads or credentials.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

static LOG_LOCK: Mutex<()> = Mutex::new(());
const LOG_BYTES: u64 = 256 * 1024;

pub fn record(root: &Path, event: &str, at: u64) {
    if event.is_empty()
        || event.len() > 64
        || !event
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
    {
        return;
    }
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let folder = root.join("logs");
    if fs::create_dir_all(&folder).is_err() {
        return;
    }
    let path = folder.join("app.jsonl");
    if fs::metadata(&path)
        .map(|m| m.len() >= LOG_BYTES)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(folder.join("app.3.jsonl"));
        for i in (1..3).rev() {
            let _ = fs::rename(
                folder.join(format!("app.{i}.jsonl")),
                folder.join(format!("app.{}.jsonl", i + 1)),
            );
        }
        let _ = fs::rename(&path, folder.join("app.1.jsonl"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", serde_json::json!({"at":at,"event":event}));
    }
}

pub fn recent_logs(root: &Path) -> Vec<serde_json::Value> {
    let mut result = vec![];
    for name in ["app.1.jsonl", "app.jsonl"] {
        if let Ok(bytes) = fs::read(root.join("logs").join(name)) {
            if bytes.len() > LOG_BYTES as usize + 256 {
                continue;
            }
            if let Ok(text) = std::str::from_utf8(&bytes) {
                for line in text.lines() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        if value["at"].is_u64()
                            && value["event"].as_str().is_some_and(|s| s.len() <= 64)
                        {
                            result.push(value);
                        }
                    }
                }
            }
        }
    }
    result.into_iter().rev().take(100).collect()
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Session {
    clean: bool,
    crash_streak: u32,
    last_seen: u64,
}

pub struct Runtime {
    root: PathBuf,
    pub safe_mode: bool,
    pub crash_streak: u32,
    pub started_at: u64,
}
impl Runtime {
    pub fn start(root: &Path, at: u64) -> Result<Self, String> {
        let session_path = root.join("session.json");
        let prior: Option<Session> = fs::metadata(&session_path)
            .ok()
            .filter(|m| m.len() <= 4096)
            .and_then(|_| fs::read(&session_path).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        if session_path.exists() && prior.is_none() {
            record(root, "session.invalid", at);
        }
        let streak = prior
            .filter(|s| !s.clean && at.saturating_sub(s.last_seen) < 86_400_000)
            .map(|s| s.crash_streak.saturating_add(1))
            .unwrap_or(0);
        let runtime = Self {
            root: root.into(),
            safe_mode: streak >= 2,
            crash_streak: streak,
            started_at: at,
        };
        runtime.pulse(at)?;
        record(
            root,
            if runtime.safe_mode {
                "startup.safe_mode"
            } else {
                "startup"
            },
            at,
        );
        Ok(runtime)
    }
    fn write_session(&self, clean: bool, at: u64) -> Result<(), String> {
        let bytes = serde_json::to_vec(&Session {
            clean,
            crash_streak: if clean { 0 } else { self.crash_streak },
            last_seen: at,
        })
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let temp = self
            .root
            .join(format!("session.{}.tmp", std::process::id()));
        fs::write(&temp, bytes)
            .and_then(|_| fs::rename(temp, self.root.join("session.json")))
            .map_err(|e| e.to_string())
    }
    pub fn pulse(&self, at: u64) -> Result<(), String> {
        self.write_session(false, at)
    }
    pub fn finish(&self, at: u64) {
        record(&self.root, "exit.clean", at);
        let _ = self.write_session(true, at);
    }
    pub fn leave_safe_mode(&mut self, at: u64) -> Result<(), String> {
        self.safe_mode = false;
        self.crash_streak = 0;
        self.pulse(at)?;
        record(&self.root, "recovery.reset", at);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_restarts_enter_safe_mode_and_clean_exit_resets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let first = Runtime::start(root, 100).unwrap();
        assert!(!first.safe_mode);
        let second = Runtime::start(root, 200).unwrap();
        assert_eq!(second.crash_streak, 1);
        let mut third = Runtime::start(root, 300).unwrap();
        assert!(third.safe_mode);
        third.leave_safe_mode(400).unwrap();
        assert!(!third.safe_mode);
        third.finish(500);
        assert_eq!(Runtime::start(root, 600).unwrap().crash_streak, 0);
    }
    #[test]
    fn logs_are_bounded_and_reject_payload_strings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        record(root, "prompt: private content", 1);
        assert!(recent_logs(root).is_empty());
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/app.jsonl"), vec![b'x'; LOG_BYTES as usize]).unwrap();
        record(root, "state.saved", 2);
        assert!(root.join("logs/app.1.jsonl").exists());
        assert_eq!(recent_logs(root)[0]["event"], "state.saved");
    }
}
