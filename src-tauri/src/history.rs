use chrono::Local;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

use crate::app_state::now_millis;

const INPUT_HISTORY_LIMIT: usize = 50;
const HISTORY_ROOT: &str = "history";
const INPUT_HISTORY_FILE: &str = "input/history.json";
const DAILY_CURRENT_FILE: &str = "daily/current.json";
const ARCHIVE_DIR: &str = "archive";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyHistoryEntry {
    pub id: String,
    pub timestamp: u64,
    pub user_input: String,
    pub assistant_reply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct InputHistoryFile {
    items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DailyConversationLog {
    date: String,
    entries: Vec<ReplyHistoryEntry>,
}

fn history_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(HISTORY_ROOT);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

fn input_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(INPUT_HISTORY_FILE))
}

fn daily_current_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(DAILY_CURRENT_FILE))
}

fn archive_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(ARCHIVE_DIR))
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent).map_err(|error| error.to_string())
}

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn build_backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("history");
    path.with_file_name(format!("{file_name}.corrupt-{}.bak", now_millis()))
}

fn move_to_backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let backup = build_backup_path(path);
    fs::rename(path, backup).map_err(|error| error.to_string())
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    match serde_json::from_str::<T>(&content) {
        Ok(value) => Ok(Some(value)),
        Err(_) => {
            move_to_backup(path)?;
            Ok(None)
        }
    }
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    ensure_parent(path)?;
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn load_input_file(app: &AppHandle) -> Result<InputHistoryFile, String> {
    let path = input_history_path(app)?;
    Ok(read_json_file::<InputHistoryFile>(&path)?.unwrap_or_default())
}

fn load_today_log(app: &AppHandle) -> Result<Option<DailyConversationLog>, String> {
    let path = daily_current_path(app)?;
    read_json_file::<DailyConversationLog>(&path)
}

fn archive_existing_log(app: &AppHandle, log: &DailyConversationLog) -> Result<(), String> {
    if log.entries.is_empty() || log.date.trim().is_empty() {
        return Ok(());
    }

    let archive_path = archive_dir(app)?.join(format!("{}.json", log.date));
    let mut archive_log = read_json_file::<DailyConversationLog>(&archive_path)?.unwrap_or(
        DailyConversationLog {
            date: log.date.clone(),
            entries: vec![],
        },
    );
    archive_log.entries.extend(log.entries.clone());
    write_json_file(&archive_path, &archive_log)
}

fn rotate_daily_log_if_needed(app: &AppHandle) -> Result<(), String> {
    let path = daily_current_path(app)?;
    let Some(log) = load_today_log(app)? else {
        return Ok(());
    };

    if log.date == today_key() {
        return Ok(());
    }

    archive_existing_log(app, &log)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn build_today_log() -> DailyConversationLog {
    DailyConversationLog {
        date: today_key(),
        entries: vec![],
    }
}

pub fn prepare_storage(app: &AppHandle) -> Result<(), String> {
    rotate_daily_log_if_needed(app)
}

pub fn get_input_history(app: &AppHandle) -> Result<Vec<String>, String> {
    Ok(load_input_file(app)?.items)
}

pub fn record_input_history(app: &AppHandle, content: &str) -> Result<Vec<String>, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return get_input_history(app);
    }

    let path = input_history_path(app)?;
    let mut history = load_input_file(app)?;
    let should_append = history
        .items
        .last()
        .map(|item| item != trimmed)
        .unwrap_or(true);

    if should_append {
        history.items.push(trimmed.to_string());
    }

    if history.items.len() > INPUT_HISTORY_LIMIT {
        let extra = history.items.len() - INPUT_HISTORY_LIMIT;
        history.items.drain(0..extra);
    }

    write_json_file(&path, &history)?;
    Ok(history.items)
}

pub fn get_today_reply_history(app: &AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    rotate_daily_log_if_needed(app)?;
    let Some(log) = load_today_log(app)? else {
        return Ok(vec![]);
    };

    if log.date != today_key() {
        return Ok(vec![]);
    }

    Ok(log.entries)
}

pub fn record_reply_history(
    app: &AppHandle,
    user_input: &str,
    assistant_reply: &str,
) -> Result<Vec<ReplyHistoryEntry>, String> {
    let trimmed_user = user_input.trim();
    let trimmed_reply = assistant_reply.trim();

    if trimmed_user.is_empty() || trimmed_reply.is_empty() {
        return get_today_reply_history(app);
    }

    rotate_daily_log_if_needed(app)?;
    let path = daily_current_path(app)?;
    let mut log = load_today_log(app)?.unwrap_or_else(build_today_log);
    if log.date != today_key() {
        log = build_today_log();
    }

    log.entries.push(ReplyHistoryEntry {
        id: format!("reply-{}", now_millis()),
        timestamp: now_millis(),
        user_input: trimmed_user.to_string(),
        assistant_reply: trimmed_reply.to_string(),
    });

    write_json_file(&path, &log)?;
    Ok(log.entries)
}

pub fn clear_today_reply_history(app: &AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    rotate_daily_log_if_needed(app)?;
    let path = daily_current_path(app)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(vec![]),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.to_string()),
    }
}
