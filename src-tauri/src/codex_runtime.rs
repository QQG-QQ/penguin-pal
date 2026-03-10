use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use tauri::AppHandle;

const ENV_SYSTEM_CODEX_BIN: &str = "CODEX_BIN";

#[derive(Debug, Clone)]
pub struct CodexRuntimeInfo {
    pub command: Option<PathBuf>,
    pub source: &'static str,
    pub home_root: PathBuf,
}

#[cfg(target_os = "windows")]
const CODEX_EXECUTABLE: &str = "codex.cmd";
#[cfg(not(target_os = "windows"))]
const CODEX_EXECUTABLE: &str = "codex";

fn private_home_root(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("codex-runtime");
    std::fs::create_dir_all(dir.join(".codex")).map_err(|error| error.to_string())?;
    Ok(dir)
}

pub fn private_auth_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(private_home_root(app)?.join(".codex").join("auth.json"))
}

#[cfg(target_os = "windows")]
fn platform_dir() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "windows-arm64"
    } else {
        "windows-x64"
    }
}

#[cfg(not(target_os = "windows"))]
fn platform_dir() -> &'static str {
    "unix"
}

fn local_runtime_candidate(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("codex")
        .join(platform_dir())
        .join("node_modules")
        .join(".bin")
        .join(CODEX_EXECUTABLE))
}

fn bundled_runtime_candidate(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?
        .join("codex")
        .join(platform_dir())
        .join("node_modules")
        .join(".bin")
        .join(CODEX_EXECUTABLE))
}

fn dev_runtime_candidate() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".codex-runtime")
        .join(platform_dir())
        .join("node_modules")
        .join(".bin")
        .join(CODEX_EXECUTABLE)
}

fn dev_resources_candidate() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("codex")
        .join(platform_dir())
        .join("node_modules")
        .join(".bin")
        .join(CODEX_EXECUTABLE)
}

#[cfg(target_os = "windows")]
fn resolve_codex_from_where(command: &str) -> Option<PathBuf> {
    let output = Command::new("cmd")
        .args(["/C", "where", command])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(target_os = "windows"))]
fn resolve_codex_from_where(_command: &str) -> Option<PathBuf> {
    None
}

fn file_if_exists(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

pub fn resolve_for_app(app: &AppHandle) -> Result<CodexRuntimeInfo, String> {
    let home_root = private_home_root(app)?;

    let command = file_if_exists(local_runtime_candidate(app)?)
        .map(|path| (path, "应用私有运行时"))
        .or_else(|| file_if_exists(bundled_runtime_candidate(app)?).map(|path| (path, "应用内置运行时")))
        .or_else(|| file_if_exists(dev_runtime_candidate()).map(|path| (path, "开发目录私有运行时")))
        .or_else(|| file_if_exists(dev_resources_candidate()).map(|path| (path, "开发目录资源运行时")))
        .or_else(|| {
            env::var_os(ENV_SYSTEM_CODEX_BIN)
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .map(|path| (path, "显式环境变量"))
        })
        .or_else(|| resolve_codex_from_where("codex").map(|path| (path, "系统安装")))
        .or_else(|| resolve_codex_from_where("codex.cmd").map(|path| (path, "系统安装")));

    Ok(match command {
        Some((path, source)) => CodexRuntimeInfo {
            command: Some(path),
            source,
            home_root,
        },
        None => CodexRuntimeInfo {
            command: None,
            source: "未找到",
            home_root,
        },
    })
}

pub fn apply_private_env(command: &mut Command, home_root: &Path) {
    let codex_home = home_root.join(".codex");
    command.env("CODEX_HOME", &codex_home);
    command.env("HOME", home_root);
    command.env("USERPROFILE", home_root);
}
