//! DSH 工作摘要桥：只读 DSH 会话标题，生成一条本地摘要，供气泡聊天自然关心，
//! 不写入记忆库、不参与同步。
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const FRESH_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_TITLES: usize = 4;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn modified(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn sessions_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| {
        PathBuf::from(home)
            .join(".dsh")
            .join("storages")
            .join("session_projcache")
            .join("sessions")
    })
}


fn node() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|dir| dir.join("node.exe"))
        .find(|path| path.is_file())
}

/// 安装并运行 DSH 会话导入脚本；脚本自身做增量去重，重复运行不重复入库。
pub fn import_once(root: &Path) {
    const SOURCE: &[u8] = include_bytes!("../resources/companion/dsh-import.mjs");
    let script = root.join("dsh-import.mjs");
    if fs::read(&script).ok().as_deref() != Some(SOURCE) {
        let _ = fs::write(&script, SOURCE);
    }
    let Some(node) = node() else { return };
    let mut command = std::process::Command::new(node);
    command
        .arg(&script)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// 读取当前有效的摘要文本（24 小时内），供 bridge 注入请求。
pub fn digest_text(root: &Path, time: u64) -> Option<String> {
    if root.join("dsh-digest.off").exists() {
        return None;
    }
    let data: Value = fs::read(root.join("dsh-digest.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())?;
    let at = data["at"].as_u64()?;
    if time.saturating_sub(at) > FRESH_MS {
        return None;
    }
    let text = data["text"].as_str()?.trim();
    (!text.is_empty()).then(|| text.chars().take(300).collect())
}

/// 从最近的 DSH 会话标题生成摘要；内容不变时只刷新时间戳。
pub fn refresh(root: &Path) {
    if root.join("dsh-digest.off").exists() {
        return;
    }
    let Some(dir) = sessions_dir() else { return };
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let time = now();
    let mut items: Vec<(u64, String)> = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        let touched = modified(&path);
        if time.saturating_sub(touched) > 7 * FRESH_MS {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let title = value["record"]["rows"]["title"]["val"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_owned();
        let chars = title.chars().count();
        if chars < 6 || chars > 60 || title == "新会话" {
            continue;
        }
        let cjk = title.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
        if cjk < 2 {
            continue;
        }
        let lower = title.to_lowercase();
        let blocked = ["只回复", "仅回复", "只回答", "回复两个", "回复内容", "回复", "简单确认", "确认回复", "你是", "请", "帮我", "ok", "ping", "test", "测试", "你好", "hi", "greeting", "reply"];
        if title.ends_with('：') || title.ends_with(':') || title.ends_with('（') { continue; }
        if blocked.iter().any(|word| lower.starts_with(word)) {
            continue;
        }
        if items.iter().any(|(_, seen)| seen == &title) {
            continue;
        }
        items.push((touched, title));
    }
    if items.is_empty() {
        return;
    }
    items.sort_by(|left, right| right.0.cmp(&left.0));
    let titles: Vec<String> = items.into_iter().take(MAX_TITLES).map(|(_, title)| title).collect();
    let text = format!("最近在忙：{}", titles.join("、"));
    let target = root.join("dsh-digest.json");
    let existing = fs::read(&target).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if existing.as_ref().and_then(|value| value["text"].as_str()) == Some(text.as_str()) {
        let _ = fs::write(&target, serde_json::to_vec_pretty(&json!({"text": text, "at": time, "source": "dsh-session-titles"})).unwrap_or_default());
        return;
    }
    let _ = fs::write(&target, serde_json::to_vec_pretty(&json!({"text": text, "at": time, "source": "dsh-session-titles"})).unwrap_or_default());
}
