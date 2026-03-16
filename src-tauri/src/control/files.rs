use std::{
    env,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};
use tauri::AppHandle;

use super::errors::{ControlError, ControlResult};

const MAX_LIST_ITEMS: usize = 200;
const MAX_READ_BYTES: u64 = 256 * 1024;

pub fn list_directory(_app: &AppHandle, path: &str) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    let metadata = fs::metadata(&resolved).map_err(|error| map_fs_error("读取目录", &resolved, error))?;
    if !metadata.is_dir() {
        return Err(ControlError::invalid_argument("path 必须指向目录。"));
    }

    let mut items = Vec::new();
    let mut total = 0usize;
    let reader = fs::read_dir(&resolved).map_err(|error| map_fs_error("列出目录", &resolved, error))?;
    for entry in reader {
        let entry = entry.map_err(|error| map_fs_error("读取目录项", &resolved, error))?;
        total += 1;
        if items.len() >= MAX_LIST_ITEMS {
            continue;
        }
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| map_fs_error("读取目录项元数据", &path, error))?;
        items.push(json!({
            "name": entry.file_name().to_string_lossy().to_string(),
            "path": display_path(&path),
            "isDir": metadata.is_dir(),
            "size": if metadata.is_file() { Some(metadata.len()) } else { None::<u64> },
        }));
    }

    items.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(right.get("name").and_then(Value::as_str).unwrap_or_default())
    });

    Ok(json!({
        "path": display_path(&resolved),
        "entryCount": total,
        "items": items,
        "truncated": total > MAX_LIST_ITEMS,
    }))
}

pub fn read_file_text(_app: &AppHandle, path: &str) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    let metadata = fs::metadata(&resolved).map_err(|error| map_fs_error("读取文件", &resolved, error))?;
    if !metadata.is_file() {
        return Err(ControlError::invalid_argument("path 必须指向文件。"));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(ControlError::invalid_argument(format!(
            "文件过大，第一版只允许读取不超过 {} KB 的文本文件。",
            MAX_READ_BYTES / 1024
        )));
    }

    let bytes = fs::read(&resolved).map_err(|error| map_fs_error("读取文件", &resolved, error))?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| {
        ControlError::backend(
            "file_encoding_unsupported",
            "当前只支持 UTF-8 文本读取。",
            Some(display_path(&resolved)),
        )
    })?;
    let line_count = if text.is_empty() { 0 } else { text.lines().count() };
    Ok(json!({
        "path": display_path(&resolved),
        "text": text,
        "byteCount": bytes.len(),
        "lineCount": line_count,
    }))
}

pub fn write_file_text(
    _app: &AppHandle,
    path: &str,
    content: &str,
    overwrite: bool,
    ensure_parent: bool,
) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    let existed_before = resolved.exists();
    if existed_before {
        let metadata = fs::metadata(&resolved)
            .map_err(|error| map_fs_error("检查写入目标", &resolved, error))?;
        if metadata.is_dir() {
            return Err(ControlError::invalid_argument("不能向目录路径写入文本。"));
        }
        if !overwrite {
            return Err(ControlError::invalid_argument("目标文件已存在；如需覆盖，请显式传 overwrite=true。"));
        }
    }

    if let Some(parent) = resolved.parent() {
        if !parent.exists() {
            if ensure_parent {
                fs::create_dir_all(parent)
                    .map_err(|error| map_fs_error("创建父目录", parent, error))?;
            } else {
                return Err(ControlError::not_found(
                    "parent_not_found",
                    format!("父目录不存在：{}", display_path(parent)),
                ));
            }
        }
    }

    fs::write(&resolved, content).map_err(|error| map_fs_error("写入文件", &resolved, error))?;
    Ok(json!({
        "path": display_path(&resolved),
        "byteCount": content.as_bytes().len(),
        "lineCount": if content.is_empty() { 0 } else { content.lines().count() },
        "overwritten": existed_before,
        "created": !existed_before,
    }))
}

pub fn create_directory(_app: &AppHandle, path: &str, recursive: bool) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    let existed_before = resolved.exists();
    if existed_before {
        let metadata = fs::metadata(&resolved)
            .map_err(|error| map_fs_error("检查目录", &resolved, error))?;
        if !metadata.is_dir() {
            return Err(ControlError::invalid_argument("目标路径已存在且不是目录。"));
        }
        return Ok(json!({
            "path": display_path(&resolved),
            "created": false,
            "alreadyExists": true,
        }));
    }

    if recursive {
        fs::create_dir_all(&resolved)
            .map_err(|error| map_fs_error("创建目录", &resolved, error))?;
    } else {
        fs::create_dir(&resolved).map_err(|error| map_fs_error("创建目录", &resolved, error))?;
    }

    Ok(json!({
        "path": display_path(&resolved),
        "created": true,
        "alreadyExists": false,
    }))
}

pub fn move_path(
    _app: &AppHandle,
    from_path: &str,
    to_path: &str,
    overwrite: bool,
) -> ControlResult<Value> {
    let source = resolve_path(from_path)?;
    let target = resolve_path(to_path)?;
    if !source.exists() {
        return Err(ControlError::not_found(
            "file_not_found",
            format!("源路径不存在：{}", display_path(&source)),
        ));
    }

    let source_meta = fs::metadata(&source).map_err(|error| map_fs_error("检查源路径", &source, error))?;
    let target_exists = target.exists();
    if target_exists {
        if !overwrite {
            return Err(ControlError::invalid_argument("目标路径已存在；如需覆盖，请显式传 overwrite=true。"));
        }
        let target_meta = fs::metadata(&target)
            .map_err(|error| map_fs_error("检查目标路径", &target, error))?;
        if source_meta.is_dir() || target_meta.is_dir() {
            return Err(ControlError::invalid_argument(
                "第一版 move_path 只允许覆盖已有文件，不支持目录覆盖。",
            ));
        }
        fs::remove_file(&target).map_err(|error| map_fs_error("移除目标文件", &target, error))?;
    } else if let Some(parent) = target.parent() {
        if !parent.exists() {
            return Err(ControlError::not_found(
                "parent_not_found",
                format!("目标父目录不存在：{}", display_path(parent)),
            ));
        }
    }

    fs::rename(&source, &target).map_err(|error| map_fs_error("移动路径", &source, error))?;
    Ok(json!({
        "fromPath": display_path(&source),
        "toPath": display_path(&target),
        "overwritten": target_exists,
    }))
}

pub fn delete_path(_app: &AppHandle, path: &str, recursive: bool) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    let metadata = fs::metadata(&resolved).map_err(|error| map_fs_error("删除路径", &resolved, error))?;
    let kind = if metadata.is_dir() { "directory" } else { "file" };
    if metadata.is_dir() {
        if recursive {
            fs::remove_dir_all(&resolved)
                .map_err(|error| map_fs_error("递归删除目录", &resolved, error))?;
        } else {
            fs::remove_dir(&resolved).map_err(|error| map_fs_error("删除目录", &resolved, error))?;
        }
    } else {
        fs::remove_file(&resolved).map_err(|error| map_fs_error("删除文件", &resolved, error))?;
    }

    Ok(json!({
        "path": display_path(&resolved),
        "kind": kind,
        "recursive": recursive,
        "deleted": true,
    }))
}

fn resolve_path(input: &str) -> ControlResult<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ControlError::invalid_argument("path 不能为空。"));
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|_| ControlError::internal("解析当前工作目录失败。"))
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn map_fs_error(action: &str, path: &Path, error: std::io::Error) -> ControlError {
    match error.kind() {
        ErrorKind::NotFound => ControlError::not_found(
            "file_not_found",
            format!("{action}失败：路径不存在：{}", display_path(path)),
        ),
        ErrorKind::PermissionDenied => ControlError::permission_denied(format!(
            "{action}失败：没有权限访问 {}",
            display_path(path)
        )),
        _ => ControlError::backend(
            "filesystem_error",
            format!("{action}失败。"),
            Some(format!("path={} error={error}", display_path(path))),
        ),
    }
}
