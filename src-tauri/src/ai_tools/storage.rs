use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{companion_llm::dpapi, companion_sync::digest};

const CONFIG_LIMIT: u64 = 2 * 1024 * 1024;
const BACKUP_LIMIT: u64 = 32 * 1024 * 1024;

pub(super) fn read(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("无法读取 {}", label(path))),
    };
    if !meta.is_file() || meta.len() > limit {
        return Err(format!("{} 不是普通文件或超过容量限制。", label(path)));
    }
    let mut value = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut value))
        .map_err(|_| format!("无法读取 {}", label(path)))?;
    if value.len() as u64 > limit {
        return Err(format!("{} 超过容量限制。", label(path)));
    }
    Ok(Some(value))
}

pub(super) fn read_config(path: &Path) -> Result<Option<Vec<u8>>, String> {
    read(path, CONFIG_LIMIT)
}

pub(super) fn label(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn persist(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("缺少配置目录。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建配置目录。")?;
    let mut file = NamedTempFile::new_in(parent).map_err(|_| "无法创建临时文件。")?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| format!("无法写入 {}", label(path)))?;
    file.persist(path)
        .map_err(|_| format!("无法替换 {}", label(path)))?;
    Ok(())
}

pub(super) fn sealed<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let plain = serde_json::to_vec(value).map_err(|_| "无法编码本机配置。")?;
    let sealed = dpapi::protect(&plain).ok_or("Windows 凭据加密失败，本次修改已停止。")?;
    if sealed.len() as u64 > BACKUP_LIMIT {
        return Err("本机配置或备份超过容量限制。".into());
    }
    Ok(sealed)
}

pub(super) fn save_private<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    persist(path, &sealed(value)?)
}

pub(super) fn load_private<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let Some(sealed) = read(path, BACKUP_LIMIT)? else {
        return Ok(None);
    };
    let plain = dpapi::unprotect(&sealed).ok_or("无法使用当前 Windows 用户解密文件。")?;
    serde_json::from_slice(&plain)
        .map(Some)
        .map_err(|_| "本机配置格式无效。".into())
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Change {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

impl Change {
    pub fn new(path: PathBuf, before: Option<Vec<u8>>, after: Vec<u8>) -> Self {
        Self {
            path,
            before,
            after: Some(after),
        }
    }

    fn verify(&self) -> Result<(), String> {
        if read_config(&self.path)? != self.before {
            return Err(format!(
                "{} 已被其他程序修改，请刷新后重试。",
                label(&self.path)
            ));
        }
        Ok(())
    }

    pub(super) fn apply(&self) -> Result<(), String> {
        self.verify()?;
        match &self.after {
            Some(bytes) => persist(&self.path, bytes),
            None if self.before.is_some() => {
                fs::remove_file(&self.path).map_err(|_| format!("无法移除 {}", label(&self.path)))
            }
            None => Ok(()),
        }
    }

    pub(super) fn reversed(&self) -> Self {
        Self {
            path: self.path.clone(),
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub(super) struct Backup {
    title: String,
    pub changes: Vec<Change>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BackupInfo {
    id: String,
    title: String,
}

pub(super) fn changed(mut changes: Vec<Change>) -> Vec<Change> {
    changes.retain(|change| change.before != change.after);
    changes
}

pub(super) fn revision(changes: &[Change]) -> String {
    let parts: Vec<_> = changes
        .iter()
        .map(|change| {
            (
                &change.path,
                change.before.as_deref().map(digest),
                change.after.as_deref().map(digest),
            )
        })
        .collect();
    digest(&serde_json::to_vec(&parts).unwrap_or_default())
}

pub(super) fn commit(
    root: &Path,
    title: &str,
    changes: Vec<Change>,
) -> Result<Option<String>, String> {
    let changes = changed(changes);
    if changes.is_empty() {
        return Ok(None);
    }
    for change in &changes {
        change.verify()?;
        if change
            .after
            .as_ref()
            .is_some_and(|bytes| bytes.len() as u64 > CONFIG_LIMIT)
        {
            return Err(format!("{} 超过配置容量限制。", label(&change.path)));
        }
    }

    let backup = Backup {
        title: title.to_owned(),
        changes,
    };
    let id = format!(
        "{:014}-{}",
        Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4()
    );
    save_private(&root.join("backups").join(format!("{id}.bin")), &backup)?;

    for (index, change) in backup.changes.iter().enumerate() {
        if let Err(error) = change.apply() {
            let mut rollback_failed = false;
            for applied in backup.changes[..index].iter().rev() {
                rollback_failed |= applied.reversed().apply().is_err();
            }
            return Err(if rollback_failed {
                format!("{error} 部分配置需要核对。备份编号 {id}。")
            } else {
                error
            });
        }
    }
    Ok(Some(id))
}

fn valid_backup_id(id: &str) -> bool {
    id.len() == 51 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
}

pub(super) fn backup(root: &Path, id: &str) -> Result<Backup, String> {
    if !valid_backup_id(id) {
        return Err("备份编号无效。".into());
    }
    load_private(&root.join("backups").join(format!("{id}.bin")))?
        .ok_or_else(|| "备份不存在。".into())
}

pub(super) fn backup_ids(root: &Path) -> Result<Vec<String>, String> {
    let entries = match fs::read_dir(root.join("backups")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("无法读取备份目录。".into()),
    };
    let mut ids: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".bin").map(str::to_owned))
        .filter(|id| valid_backup_id(id))
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    Ok(ids)
}

pub(super) fn backups(root: &Path) -> Result<Vec<BackupInfo>, String> {
    Ok(backup_ids(root)?
        .into_iter()
        .take(40)
        .map(|id| {
            let title = backup(root, &id).map_or_else(|_| "无法读取的备份".into(), |b| b.title);
            BackupInfo { id, title }
        })
        .collect())
}

pub(super) fn rollback(root: &Path, id: &str, user: &Path) -> Result<Option<String>, String> {
    let backup = backup(root, id)?;
    let codex_switch = backup
        .title
        .trim_start_matches("恢复 · ")
        .starts_with("Codex · 切换到 ");
    let codex_config = super::providers::Client::Codex.config(user);
    let mut changes = Vec::new();
    for change in backup.changes.iter().rev() {
        #[cfg(windows)]
        if change.path == user.join(".gemini/antigravity-cli/settings.json") {
            super::accounts::ensure_stopped(super::providers::Client::Agy)?;
        }
        if let Some(client) = super::accounts::credential_client(&change.path) {
            super::accounts::ensure_stopped(client)?;
        }
        let current = read_config(&change.path)?;
        if current == change.before {
            continue;
        }
        if codex_switch && change.path == codex_config {
            changes.push(super::codex_models::undo_config(change, current, true)?);
            continue;
        }
        if current != change.after {
            return Err(format!(
                "{} 已被其他程序修改，请先核对当前内容。",
                label(&change.path)
            ));
        }
        if change.path == codex_config {
            changes.push(super::codex_models::undo_config(change, current, false)?);
        } else {
            changes.push(change.reversed());
        }
    }
    commit(root, &format!("恢复 · {}", backup.title), changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_rejects_edits_after_preview() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "original").unwrap();
        let plan = Change::new(path.clone(), read_config(&path).unwrap(), b"next".to_vec());
        fs::write(&path, "external").unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "external");
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_backup_can_restore_created_and_updated_files() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("config.toml");
        let created = temp.path().join("instructions.md");
        fs::write(&original, "old value").unwrap();
        let id = commit(
            temp.path(),
            "test",
            vec![
                Change::new(
                    original.clone(),
                    Some(b"old value".to_vec()),
                    b"new value".to_vec(),
                ),
                Change::new(created.clone(), None, b"new file".to_vec()),
            ],
        )
        .unwrap()
        .unwrap();
        let encrypted = fs::read(temp.path().join("backups").join(format!("{id}.bin"))).unwrap();
        assert!(encrypted.get(..1) != Some(b"{"));
        rollback(temp.path(), &id, temp.path()).unwrap();
        assert_eq!(fs::read_to_string(original).unwrap(), "old value");
        assert!(!created.exists());
    }
}
