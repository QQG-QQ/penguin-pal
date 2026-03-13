use chrono::Local;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

use super::types::{
    RecentFailureSummary, TestCase, TestHistoryIndex, TestRunIndexEntry, TestRunReport,
    TestRunStatus,
};

const TEST_HISTORY_ROOT: &str = "test-history";
const RUNS_DIR: &str = "runs";
const INDEX_DIR: &str = "index";
const LATEST_FILE: &str = "latest.json";

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(TEST_HISTORY_ROOT);
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn runs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(root(app)?.join(RUNS_DIR).join(today_key()))
}

fn latest_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(root(app)?.join(LATEST_FILE))
}

fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(root(app)?.join(INDEX_DIR).join(format!("{}.json", today_key())))
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    serde_json::from_str::<T>(&content)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    ensure_parent(path)?;
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

pub fn persist_report(app: &AppHandle, report: &TestRunReport) -> Result<(), String> {
    let run_path = runs_dir(app)?.join(format!("{}.json", report.run_id));
    write_json(&run_path, report)?;
    write_json(&latest_path(app)?, report)?;

    let mut index = read_json::<TestHistoryIndex>(&index_path(app)?)?.unwrap_or(TestHistoryIndex {
        date: today_key(),
        recent_runs: vec![],
        recent_failed_items: vec![],
    });
    index.recent_runs.push(TestRunIndexEntry {
        run_id: report.run_id.clone(),
        title: report.title.clone(),
        started_at: report.started_at,
        status: report.status.clone(),
        summary: report.summary.clone(),
    });
    if index.recent_runs.len() > 20 {
        let drain = index.recent_runs.len() - 20;
        index.recent_runs.drain(0..drain);
    }
    index.recent_failed_items = report
        .failure_items
        .iter()
        .take(8)
        .map(|item| RecentFailureSummary {
            case_id: item.case_id.clone(),
            case_title: item.case_title.clone(),
            failure_stage: item.failure_stage.clone(),
            reason: item.reason.clone(),
        })
        .collect();
    write_json(&index_path(app)?, &index)
}

pub fn load_last_failed_selection(app: &AppHandle) -> Result<(Vec<String>, Vec<TestCase>), String> {
    let Some(report) = read_json::<TestRunReport>(&latest_path(app)?)? else {
        return Ok((vec![], vec![]));
    };

    let mut builtin_ids = Vec::new();
    let mut dynamic_cases = Vec::new();
    for item in report.failure_items {
        if let Some(case) = report.dynamic_cases.iter().find(|case| case.id == item.case_id) {
            if !dynamic_cases.iter().any(|existing: &TestCase| existing.id == case.id) {
                dynamic_cases.push(case.clone());
            }
        } else if !builtin_ids.iter().any(|existing| existing == &item.case_id) {
            builtin_ids.push(item.case_id);
        }
    }

    Ok((builtin_ids, dynamic_cases))
}

pub fn recent_failed_summary(app: &AppHandle) -> Result<Vec<String>, String> {
    let Some(index) = read_json::<TestHistoryIndex>(&index_path(app)?)? else {
        return Ok(vec![]);
    };
    Ok(index
        .recent_failed_items
        .iter()
        .map(|item| {
            format!(
                "{} / {:?}：{}",
                item.case_title, item.failure_stage, item.reason
            )
        })
        .collect())
}

pub fn status_message(report: &TestRunReport) -> &'static str {
    match report.status {
        TestRunStatus::Passed => "测试已通过。",
        TestRunStatus::Failed => "测试存在失败项。",
        TestRunStatus::Blocked => "测试被阻止。",
        TestRunStatus::WaitingConfirmation => "测试等待确认。",
        TestRunStatus::Cancelled => "测试已取消。",
        TestRunStatus::Running => "测试运行中。",
    }
}
