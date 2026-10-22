//! Explicit local legacy-data access. The old assistant and its memory writer stay off.
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
const FILE_LIMIT: u64 = 2 * 1024 * 1024;
const TOTAL_LIMIT: usize = 20 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFile {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub bytes: u64,
}
fn candidates(root: &Path) -> Vec<(String, &'static str, PathBuf)> {
    let mut files = vec![];
    for name in [
        "profile",
        "episodic",
        "procedural",
        "policy",
        "semantic",
        "meta",
    ] {
        files.push((
            format!("memory/{name}.json"),
            "旧记忆",
            root.join("memory").join(format!("{name}.json")),
        ));
    }
    for relative in ["history/input/history.json", "history/daily/current.json"] {
        files.push((relative.into(), "旧聊天", root.join(relative)));
    }
    if let Ok(entries) = fs::read_dir(root.join("history/archive")) {
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries.into_iter().rev().take(100) {
            if path.extension().and_then(|p| p.to_str()) == Some("json") {
                if let Some(name) = path.file_name().and_then(|p| p.to_str()) {
                    files.push((format!("history/archive/{name}"), "旧归档", path.clone()));
                }
            }
        }
    }
    files
}
fn permitted(root: &Path, path: &Path) -> bool {
    let Ok(base) = root.canonicalize() else {
        return false;
    };
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    resolved.starts_with(base)
        && fs::symlink_metadata(path)
            .map(|m| m.is_file() && !m.file_type().is_symlink() && m.len() <= FILE_LIMIT)
            .unwrap_or(false)
}
pub fn list(root: &Path) -> Vec<LegacyFile> {
    candidates(root)
        .into_iter()
        .filter(|(_, _, p)| permitted(root, p))
        .filter_map(|(id, kind, path)| {
            let bytes = fs::metadata(&path).ok()?.len();
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(LegacyFile {
                id,
                name,
                kind,
                bytes,
            })
        })
        .collect()
}
pub fn read(root: &Path, id: &str) -> Result<String, String> {
    let (_, _, path) = candidates(root)
        .into_iter()
        .find(|(key, _, _)| key == id)
        .ok_or("未找到允许查看的旧数据文件")?;
    if !permitted(root, &path) {
        return Err("文件超出旧数据目录、过大或不可读取".into());
    }
    let data = fs::read(&path).map_err(|_| "读取旧数据失败")?;
    if data.len() > FILE_LIMIT as usize {
        return Err("文件超过查看上限".into());
    }
    String::from_utf8(data).map_err(|_| "旧数据不是 UTF-8 文本".into())
}
pub fn export(root: &Path, destination: &Path, ids: &[String], at: u64) -> Result<PathBuf, String> {
    if ids.is_empty() || ids.len() > 20 {
        return Err("每次请选择 1 至 20 个文件".into());
    }
    let mut total = 0;
    let mut records = vec![];
    for id in ids {
        let text = read(root, id)?;
        total += text.len();
        if total > TOTAL_LIMIT {
            return Err("本次导出超过 20 MB 上限".into());
        }
        records.push(serde_json::json!({"id":id,"content":text}));
    }
    fs::create_dir_all(destination).map_err(|_| "无法创建本地导出目录")?;
    let path = destination.join(format!("penguinpal-legacy-{at}.json"));
    let bytes = serde_json::to_vec_pretty(
        &serde_json::json!({"schema":1,"exportedAt":at,"records":records}),
    )
    .map_err(|_| "导出序列化失败")?;
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|_| "无法写入导出文件；请稍后重试")?;
    let result = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&path);
        return Err("写入导出文件失败，已清理不完整文件".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_data_is_viewable_and_export_does_not_modify_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("memory")).unwrap();
        fs::write(root.join("memory/profile.json"), br#"{"name":"test"}"#).unwrap();
        fs::write(root.join("settings.json"), b"do not export credentials").unwrap();
        assert_eq!(list(root).len(), 1);
        assert!(read(root, "../settings.json").is_err());
        assert!(read(root, "settings.json").is_err());
        let before = fs::read(root.join("memory/profile.json")).unwrap();
        let output = export(
            root,
            &root.join("exports"),
            &["memory/profile.json".into()],
            123,
        )
        .unwrap();
        assert!(output.exists());
        assert_eq!(before, fs::read(root.join("memory/profile.json")).unwrap());
        assert!(export(root, &root.join("exports"), &[], 124).is_err());
    }
    #[test]
    fn oversized_and_unknown_files_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("memory")).unwrap();
        fs::write(
            root.join("memory/profile.json"),
            vec![b'x'; FILE_LIMIT as usize + 1],
        )
        .unwrap();
        assert!(list(root).is_empty());
        assert!(read(root, "memory/profile.json").is_err());
    }
}
