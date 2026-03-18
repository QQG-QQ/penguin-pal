#![allow(dead_code)]

use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tauri::{AppHandle, Manager};

use crate::codex_config::{CodexConfig, RuleSet, SkillSet};

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

/// 获取 Codex 配置目录路径
pub fn codex_config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(private_home_root(app)?.join(".codex"))
}

/// 初始化 Codex 配置目录结构
/// 创建必要的子目录和默认配置文件
pub fn initialize_codex_config(app: &AppHandle) -> Result<(), String> {
    let codex_home = codex_config_dir(app)?;

    // 创建必要的目录结构
    let dirs = [
        codex_home.join("sessions"),
        codex_home.join("skills"),
        codex_home.join("rules"),
        codex_home.join("memories"),
    ];

    for dir in &dirs {
        fs::create_dir_all(dir).map_err(|e| format!("创建目录失败 {:?}: {}", dir, e))?;
    }

    // 创建默认配置文件（如果不存在）
    let config_path = codex_home.join("config.toml");
    if !config_path.exists() {
        let default_config = CodexConfig::default();
        default_config.save(&config_path)?;
    }

    // 创建默认规则文件（如果不存在）
    let rules_path = codex_home.join("rules").join("default.rules");
    if !rules_path.exists() {
        let default_rules = r#"# Codex 自动批准规则
# 格式: rule_type(pattern=[...], decision="...")
#
# rule_type:
#   - prefix_rule: 前缀匹配
#   - exact_rule: 精确匹配
#   - regex_rule: 正则匹配
#
# decision:
#   - allow: 自动允许执行
#   - deny: 自动拒绝
#   - confirm: 需要用户确认（默认）

# 示例规则（取消注释以启用）
# prefix_rule(pattern=["git", "status"], decision="allow")
# prefix_rule(pattern=["git", "diff"], decision="allow")
# prefix_rule(pattern=["npm", "run", "build"], decision="allow")
# prefix_rule(pattern=["cargo", "check"], decision="allow")
"#;
        fs::write(&rules_path, default_rules)
            .map_err(|e| format!("创建默认规则文件失败: {}", e))?;
    }

    // 创建示例技能目录结构
    let example_skill_dir = codex_home.join("skills").join(".example");
    if !example_skill_dir.exists() {
        fs::create_dir_all(&example_skill_dir)
            .map_err(|e| format!("创建示例技能目录失败: {}", e))?;

        let skill_json = r#"{
    "name": "example-skill",
    "description": "这是一个示例技能，展示技能文件结构",
    "instructions": "这里是技能指令，会被注入到 system prompt 中。",
    "triggers": ["示例", "example"],
    "enabled": false,
    "priority": 0
}"#;
        fs::write(example_skill_dir.join("skill.json"), skill_json)
            .map_err(|e| format!("创建示例技能配置失败: {}", e))?;
    }

    Ok(())
}

/// 加载 Codex 配置
pub fn load_codex_config(app: &AppHandle) -> Result<CodexConfig, String> {
    let codex_home = codex_config_dir(app)?;
    CodexConfig::load_from_home(&codex_home)
}

/// 保存 Codex 配置
pub fn save_codex_config(app: &AppHandle, config: &CodexConfig) -> Result<(), String> {
    let codex_home = codex_config_dir(app)?;
    config.save_to_home(&codex_home)
}

/// 加载规则集
pub fn load_codex_rules(app: &AppHandle) -> Result<RuleSet, String> {
    let codex_home = codex_config_dir(app)?;
    let mut rules = RuleSet::new(&codex_home);
    rules.load()?;
    Ok(rules)
}

/// 加载技能集
pub fn load_codex_skills(app: &AppHandle) -> Result<SkillSet, String> {
    let codex_home = codex_config_dir(app)?;
    let mut skills = SkillSet::new(&codex_home);
    skills.load()?;
    Ok(skills)
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
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let output = Command::new("cmd")
        .args(["/C", "where", command])
        .creation_flags(CREATE_NO_WINDOW)
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
    let local_candidate = local_runtime_candidate(app)?;
    let bundled_candidate = bundled_runtime_candidate(app)?;

    let command = file_if_exists(local_candidate)
        .map(|path| (path, "应用私有运行时"))
        .or_else(|| file_if_exists(bundled_candidate).map(|path| (path, "应用内置运行时")))
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
