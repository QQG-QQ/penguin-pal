use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use serde_json::{json, Value};
use tauri::AppHandle;

use super::errors::{ControlError, ControlResult};

#[derive(Clone, Copy)]
enum ShellProfile {
    ReadOnly,
    Build,
}

pub fn run_shell_command(
    _app: &AppHandle,
    command: &str,
    args: &[String],
    workdir: Option<&str>,
    timeout_ms: i64,
) -> ControlResult<Value> {
    let spec = resolve_allowed_command(command, args)?;
    let cwd = resolve_workdir(workdir)?;

    let mut cmd = spec.build_command(&cwd);
    let output = run_with_timeout(&mut cmd, Duration::from_millis(timeout_ms as u64))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(ControlError::backend(
            "shell_command_failed",
            format!("shell 命令执行失败：{}", spec.display_name()),
            Some(format!(
                "cwd={} exitCode={:?} stderr={}",
                cwd.display(),
                output.status.code(),
                stderr
            )),
        ));
    }

    Ok(json!({
        "command": command,
        "args": args,
        "profile": match spec.profile {
            ShellProfile::ReadOnly => "read_only",
            ShellProfile::Build => "build",
        },
        "workdir": cwd.to_string_lossy().to_string(),
        "displayName": spec.display_name(),
        "stdout": stdout,
        "stderr": stderr,
        "exitCode": output.status.code().unwrap_or_default(),
    }))
}

struct AllowedCommand {
    profile: ShellProfile,
    display: String,
    program: String,
    args: Vec<String>,
}

impl AllowedCommand {
    fn build_command(&self, cwd: &PathBuf) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn display_name(&self) -> String {
        self.display.clone()
    }
}

fn resolve_allowed_command(command: &str, args: &[String]) -> ControlResult<AllowedCommand> {
    match command {
        "pwd" if args.is_empty() => Ok(AllowedCommand {
            profile: ShellProfile::ReadOnly,
            display: "pwd".to_string(),
            program: "cmd".to_string(),
            args: vec!["/C".to_string(), "cd".to_string()],
        }),
        "dir" if args.len() <= 1 => {
            let mut command_args = vec!["/C".to_string(), "dir".to_string()];
            if let Some(path) = args.first() {
                command_args.push(path.clone());
            }
            Ok(AllowedCommand {
                profile: ShellProfile::ReadOnly,
                display: format!("dir {}", args.join(" ")).trim().to_string(),
                program: "cmd".to_string(),
                args: command_args,
            })
        }
        "type" if args.len() == 1 => Ok(AllowedCommand {
            profile: ShellProfile::ReadOnly,
            display: format!("type {}", args[0]),
            program: "cmd".to_string(),
            args: vec!["/C".to_string(), "type".to_string(), args[0].clone()],
        }),
        "where" if args.len() == 1 => Ok(AllowedCommand {
            profile: ShellProfile::ReadOnly,
            display: format!("where {}", args[0]),
            program: "where".to_string(),
            args: vec![args[0].clone()],
        }),
        "node" if args_match(args, &["--version"]) => Ok(version_command("node", "node --version")),
        "npm" if args_match(args, &["--version"]) => Ok(version_command("npm", "npm --version")),
        "cargo" if args_match(args, &["--version"]) => Ok(version_command("cargo", "cargo --version")),
        "git" if args_match(args, &["status"]) => Ok(simple_command(
            ShellProfile::ReadOnly,
            "git status",
            "git",
            args,
        )),
        "git" if args_match(args, &["branch", "--show-current"]) => Ok(simple_command(
            ShellProfile::ReadOnly,
            "git branch --show-current",
            "git",
            args,
        )),
        "git" if args_match(args, &["rev-parse", "--short", "HEAD"]) => Ok(simple_command(
            ShellProfile::ReadOnly,
            "git rev-parse --short HEAD",
            "git",
            args,
        )),
        "npm" if args_match(args, &["run", "build"]) => Ok(simple_command(
            ShellProfile::Build,
            "npm run build",
            "npm",
            args,
        )),
        "npm" if args_match(args, &["run", "test"]) => Ok(simple_command(
            ShellProfile::Build,
            "npm run test",
            "npm",
            args,
        )),
        "npm" if args_match(args, &["run", "lint"]) => Ok(simple_command(
            ShellProfile::Build,
            "npm run lint",
            "npm",
            args,
        )),
        "cargo" if args_match(args, &["build"]) => Ok(simple_command(
            ShellProfile::Build,
            "cargo build",
            "cargo",
            args,
        )),
        "cargo" if args_match(args, &["test"]) => Ok(simple_command(
            ShellProfile::Build,
            "cargo test",
            "cargo",
            args,
        )),
        _ => Err(ControlError::invalid_argument(
            "run_shell_command 只允许受控白名单命令：pwd/dir/type/where/git/npm/cargo 的有限子集。",
        )),
    }
}

fn simple_command(
    profile: ShellProfile,
    display: &str,
    program: &str,
    args: &[String],
) -> AllowedCommand {
    AllowedCommand {
        profile,
        display: display.to_string(),
        program: program.to_string(),
        args: args.to_vec(),
    }
}

fn version_command(program: &str, display: &str) -> AllowedCommand {
    AllowedCommand {
        profile: ShellProfile::ReadOnly,
        display: display.to_string(),
        program: program.to_string(),
        args: vec!["--version".to_string()],
    }
}

fn args_match(args: &[String], expected: &[&str]) -> bool {
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected.iter())
            .all(|(left, right)| left == right)
}

fn resolve_workdir(input: Option<&str>) -> ControlResult<PathBuf> {
    match input.map(str::trim).filter(|value| !value.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            let resolved = if path.is_absolute() {
                path
            } else {
                env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .map_err(|_| ControlError::internal("解析当前工作目录失败。"))?
            };
            if !resolved.exists() || !resolved.is_dir() {
                return Err(ControlError::invalid_argument("workdir 必须是已存在目录。"));
            }
            Ok(resolved)
        }
        None => env::current_dir().map_err(|_| ControlError::internal("解析当前工作目录失败。")),
    }
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> ControlResult<std::process::Output> {
    let mut child = command
        .spawn()
        .map_err(|error| ControlError::backend("shell_spawn_failed", "启动 shell 命令失败。", Some(error.to_string())))?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| ControlError::backend("shell_wait_failed", "等待 shell 命令失败。", Some(error.to_string())))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| ControlError::backend("shell_output_failed", "读取 shell 命令输出失败。", Some(error.to_string())))?;
            let mut next_output = output;
            next_output.status = status;
            return Ok(next_output);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ControlError::timeout("shell 命令执行超时。"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
