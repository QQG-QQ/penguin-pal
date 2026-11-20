use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};

use super::{providers::Client, sessions};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Quota {
    five_hours: Option<Window>,
    weekly: Option<Window>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    remaining_percent: f64,
    resets_at: Option<i64>,
}

fn parse_quota(result: &Value) -> Result<Quota, String> {
    let snapshot = result
        .pointer("/rateLimitsByLimitId/codex")
        .filter(|snapshot| snapshot.is_object())
        .or_else(|| {
            let snapshot = &result["rateLimits"];
            (snapshot["limitId"].is_null() || snapshot["limitId"] == "codex").then_some(snapshot)
        })
        .unwrap_or(&Value::Null);
    let mut quota = Quota {
        five_hours: None,
        weekly: None,
    };
    for window in [&snapshot["primary"], &snapshot["secondary"]] {
        let Some(used) = window["usedPercent"].as_f64().filter(|value| *value >= 0.0) else {
            continue;
        };
        let slot = match window["windowDurationMins"].as_u64() {
            Some(300) => &mut quota.five_hours,
            Some(10_080) => &mut quota.weekly,
            _ => continue,
        };
        *slot = Some(Window {
            remaining_percent: 100.0 - used.min(100.0),
            resets_at: window["resetsAt"]
                .as_i64()
                .filter(|timestamp| *timestamp > 0),
        });
    }
    if quota.five_hours.is_none() && quota.weekly.is_none() {
        return Err("官方账号暂未返回 5 小时或每周额度。".into());
    }
    Ok(quota)
}

struct AppServer(Child);

impl Drop for AppServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn send(input: &mut ChildStdin, request: Value) -> Result<(), String> {
    writeln!(input, "{request}").map_err(|_| "无法向 Codex 发送额度查询。".into())
}

fn receive(
    output: &Receiver<Value>,
    id: u64,
    deadline: Instant,
    error: &str,
) -> Result<Value, String> {
    while Instant::now() < deadline {
        match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(message) if message["id"].as_u64() == Some(id) => {
                return message.get("result").cloned().ok_or_else(|| error.into());
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(error.into()),
            Err(RecvTimeoutError::Timeout) => break,
        }
    }
    Err("查询官方额度超时，请重试。".into())
}

pub(super) fn read(user: &Path) -> Result<Quota, String> {
    let executable = sessions::executable(Client::Codex).ok_or("未找到 Codex，请先安装。")?;
    read_from(&executable, user, &Client::Codex.home(user))
}

fn read_from(executable: &Path, user: &Path, codex_dir: &Path) -> Result<Quota, String> {
    let mut command = Command::new(executable);
    command
        .current_dir(user)
        .env("CODEX_HOME", codex_dir)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            "chatgpt_base_url=\"https://chatgpt.com/backend-api/\"",
            "-c",
            "model_provider=\"openai\"",
            "-c",
            "analytics.enabled=false",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut server = AppServer(command.spawn().map_err(|_| "无法启动 Codex 额度查询。")?);
    let mut input = server.0.stdin.take().ok_or("无法连接 Codex。")?;
    let output = server.0.stdout.take().ok_or("无法连接 Codex。")?;
    let (tx, rx) = mpsc::sync_channel(3);
    std::thread::Builder::new()
        .spawn(move || {
            let mut reader = BufReader::new(output.take(1024 * 1024));
            let mut line = Vec::new();
            loop {
                line.clear();
                if !reader
                    .read_until(b'\n', &mut line)
                    .is_ok_and(|size| size > 0)
                {
                    break;
                }
                if let Ok(message) = serde_json::from_slice::<Value>(&line) {
                    if message.get("method").is_none()
                        && message["id"].as_u64().is_some()
                        && tx.send(message).is_err()
                    {
                        break;
                    }
                }
            }
        })
        .map_err(|_| "无法读取 Codex 额度响应。")?;

    let deadline = Instant::now() + Duration::from_secs(15);
    send(
        &mut input,
        json!({
            "id":1, "method":"initialize",
            "params":{"clientInfo":{"name":"penguinpal","version":env!("CARGO_PKG_VERSION")}}
        }),
    )?;
    receive(&rx, 1, deadline, "无法连接 Codex，请检查客户端版本。")?;
    send(&mut input, json!({"method":"initialized"}))?;
    send(
        &mut input,
        json!({"id":2,"method":"account/read","params":{"refreshToken":false}}),
    )?;
    let account = receive(&rx, 2, deadline, "无法读取 Codex 的当前登录状态。")?;
    if account["account"]["type"] != "chatgpt" {
        return Err("请先登录或切换到一个 ChatGPT 官方账号。".into());
    }
    send(
        &mut input,
        json!({"id":3,"method":"account/rateLimits/read"}),
    )?;
    let result = receive(&rx, 3, deadline, "无法获取官方额度，请检查网络或重新登录。")?;
    parse_quota(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_codex_bucket_and_maps_windows_by_duration() {
        let quota = parse_quota(&json!({
            "rateLimits":{"limitId":"other","primary":{"usedPercent":20,"windowDurationMins":300}},
            "rateLimitsByLimitId":{"codex":{
                "primary":{"usedPercent":103,"windowDurationMins":10080,"resetsAt":1800000000_i64},
                "secondary":{"usedPercent":61.5,"windowDurationMins":300,"resetsAt":null}
            }}
        }))
        .unwrap();
        assert_eq!(quota.five_hours.unwrap().remaining_percent, 38.5);
        let weekly = quota.weekly.unwrap();
        assert_eq!(weekly.remaining_percent, 0.0);
        assert_eq!(weekly.resets_at, Some(1800000000));
    }

    #[test]
    fn handles_old_servers_and_absent_windows_without_guessing() {
        let quota = parse_quota(&json!({"rateLimits":{
            "primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":null}
        }}))
        .unwrap();
        let short = quota.five_hours.unwrap();
        assert_eq!(short.remaining_percent, 100.0);
        assert!(short.resets_at.is_none());
        assert!(quota.weekly.is_none());
        assert!(parse_quota(&json!({"rateLimits":{
            "limitId":"other","primary":{"usedPercent":0,"windowDurationMins":300}
        }}))
        .is_err());
    }
}
