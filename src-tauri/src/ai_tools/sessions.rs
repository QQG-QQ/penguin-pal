use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};

use super::{providers::Client, storage::read};
use crate::companion_sync::{digest, scrub};

const FILE_LIMIT: u64 = 32 * 1024 * 1024;
const USAGE_FILE_LIMIT: u64 = 192 * 1024 * 1024;
const USAGE_SCAN_LIMIT: u64 = 512 * 1024 * 1024;
const SCAN_LIMIT: usize = 12_000;
const SHOW_LIMIT: usize = 150;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Session {
    pub id: String,
    pub title: String,
    pub client: Client,
    pub cwd: String,
    pub modified: u64,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    cli_id: String,
}

#[derive(Serialize)]
pub(super) struct Message {
    role: &'static str,
    text: String,
}

#[derive(Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    input: u64,
    output: u64,
    cached: u64,
}

impl Tokens {
    fn delta(&self, before: &Self) -> Self {
        Self {
            input: if self.input < before.input {
                self.input
            } else {
                self.input - before.input
            },
            output: if self.output < before.output {
                self.output
            } else {
                self.output - before.output
            },
            cached: if self.cached < before.cached {
                self.cached
            } else {
                self.cached - before.cached
            },
        }
    }
}

struct Logs {
    files: Vec<(PathBuf, u64)>,
    limited: bool,
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn valid_cli_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}

fn walk(path: &Path, client: Client, budget: &mut usize, depth: u8, logs: &mut Logs) {
    if depth > 7 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if *budget == 0 {
            logs.limited = true;
            return;
        }
        *budget -= 1;
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, client, budget, depth + 1, logs);
        } else if kind.is_file() {
            let ext = path.extension().and_then(|ext| ext.to_str());
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let selected = match client {
                Client::Codex => ext == Some("jsonl") && name.starts_with("rollout-"),
                Client::Claude => ext == Some("jsonl") && !name.starts_with("agent-"),
                Client::Dsh => {
                    name.starts_with("session.")
                        && (name.ends_with(".jsonl.zstd")
                            || (name.ends_with(".jsonl")
                                && !path.with_extension("jsonl.zstd").is_file()))
                }
                Client::Workbuddy => depth == 1 && ext == Some("jsonl"),
                Client::Pi | Client::Omp => ext == Some("jsonl"),
                Client::Grokbuild => depth == 2 && name == "chat_history.jsonl",
                Client::Agy => {
                    depth == 3
                        && name == "transcript_full.jsonl"
                        && path
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|part| part == "logs")
                        && path
                            .ancestors()
                            .nth(2)
                            .and_then(Path::file_name)
                            .is_some_and(|part| part == ".system_generated")
                }
                Client::Hermes => ext == Some("jsonl"),
                Client::Copilot => depth == 1 && name == "events.jsonl",
                Client::Kimi => {
                    depth == 4
                        && name == "wire.jsonl"
                        && path
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|part| part == "main")
                }
                Client::Openclaw => {
                    depth == 2
                        && ext == Some("jsonl")
                        && path
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|part| part == "sessions")
                }
                Client::Gemini => {
                    name.starts_with("session-")
                        && matches!(ext, Some("jsonl" | "json"))
                        && (ext != Some("json") || !path.with_extension("jsonl").exists())
                }
                _ => false,
            };
            if selected {
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(millis)
                    .unwrap_or(0);
                logs.files.push((path, modified));
            }
        }
    }
}

fn log_files(user: &Path, client: Client) -> Logs {
    let home = client.home(user);
    let roots = match client {
        Client::Codex => vec![home.join("sessions"), home.join("archived_sessions")],
        Client::Claude | Client::Workbuddy => vec![home.join("projects")],
        Client::Dsh | Client::Kimi | Client::Hermes => vec![home.join("sessions")],
        Client::Grokbuild => vec![home.join("sessions"), home.join("archived_sessions")],
        Client::Agy => vec![home.join("brain"), user.join(".gemini/antigravity/brain")],
        Client::Pi | Client::Omp => vec![home.join("sessions")],
        Client::Copilot => vec![home.join("session-state")],
        Client::Openclaw => vec![home.join("agents")],
        Client::Gemini => vec![home.join("tmp")],
        _ => Vec::new(),
    };
    let mut logs = Logs {
        files: Vec::new(),
        limited: false,
    };
    let mut budget = SCAN_LIMIT;
    for path in roots {
        walk(&path, client, &mut budget, 0, &mut logs);
    }
    logs.files
        .sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    logs
}

fn each_line<T>(
    path: &Path,
    cap: u64,
    inspect: impl FnMut(Value) -> Option<T>,
) -> Result<Option<T>, String> {
    each_matching_line(path, cap, |_| true, inspect)
}

fn each_matching_line<T>(
    path: &Path,
    cap: u64,
    matches: impl Fn(&[u8]) -> bool,
    mut inspect: impl FnMut(Value) -> Option<T>,
) -> Result<Option<T>, String> {
    if !path.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
        return Err("会话已移动或不是普通文件。".into());
    }
    let file = fs::File::open(path).map_err(|_| "会话已移动或无法读取。")?;
    let source: Box<dyn Read> = if path.extension().is_some_and(|ext| ext == "zstd") {
        let mut reader = zstd::stream::read::Decoder::new(file.take(cap.max(FILE_LIMIT)))
            .map_err(|_| "无法解压会话文件。")?;
        reader
            .window_log_max(24)
            .map_err(|_| "无法限制会话解压大小。")?;
        Box::new(reader)
    } else {
        Box::new(file)
    };
    let mut reader = BufReader::new(source.take(cap));
    let mut line = Vec::new();
    loop {
        line.clear();
        let mut segment = reader.by_ref().take(2 * 1024 * 1024 + 1);
        if segment
            .read_until(b'\n', &mut line)
            .map_err(|_| "读取会话文件失败。")?
            == 0
        {
            break;
        }
        if line.len() > 2 * 1024 * 1024 {
            if line.last() != Some(&b'\n') {
                reader.skip_until(b'\n').map_err(|_| "读取会话文件失败。")?;
            }
            continue;
        }
        if !matches(&line) {
            continue;
        }
        if let Ok(record) = serde_json::from_slice(&line) {
            if let Some(result) = inspect(record) {
                return Ok(Some(result));
            }
        }
    }
    Ok(None)
}

fn text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_owned();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn user_prompt(body: &str) -> &str {
    if body
        .trim_start()
        .starts_with("# Files mentioned by the user:")
    {
        if let Some((_, request)) = body.split_once("## My request:") {
            return request.trim();
        }
    }
    body.trim()
}

fn message(client: Client, entry: &Value) -> Option<Message> {
    let (role, body) = match client {
        Client::Codex
            if entry["type"] == "response_item" && entry["payload"]["type"] == "message" =>
        {
            let p = &entry["payload"];
            (p["role"].as_str()?, text(&p["content"]))
        }
        Client::Codex if entry["type"] == "event_msg" => {
            match entry["payload"]["type"].as_str()? {
                "user_message" => ("user", text(&entry["payload"]["message"])),
                "agent_message" => ("assistant", text(&entry["payload"]["message"])),
                _ => return None,
            }
        }
        Client::Claude if entry["type"] == "user" || entry["type"] == "assistant" => {
            (entry["type"].as_str()?, text(&entry["message"]["content"]))
        }
        Client::Gemini => (entry["type"].as_str()?, text(&entry["content"])),
        Client::Grokbuild => (entry["type"].as_str()?, text(&entry["content"])),
        Client::Hermes => {
            let entry = entry
                .get("message")
                .filter(|message| message.is_object())
                .unwrap_or(entry);
            (entry["role"].as_str()?, text(&entry["content"]))
        }
        Client::Agy => {
            let role = match entry["type"].as_str()? {
                "USER_INPUT" => "user",
                "PLANNER_RESPONSE" => "assistant",
                _ => return None,
            };
            let raw = text(&entry["content"]);
            let content = raw
                .split_once("<USER_REQUEST>")
                .and_then(|(_, request)| request.split_once("</USER_REQUEST>"))
                .map(|(request, _)| request.to_owned())
                .unwrap_or(raw);
            (role, content)
        }
        Client::Dsh => match entry["type"].as_str()? {
            "user/message" => ("user", text(&entry["data"]["content"])),
            "assistant/message" => ("assistant", text(&entry["data"]["message"]["content"])),
            _ => return None,
        },
        Client::Workbuddy if entry["type"] == "message" => {
            (entry["role"].as_str()?, text(&entry["content"]))
        }
        Client::Pi | Client::Omp | Client::Openclaw if entry["type"] == "message" => {
            let message = &entry["message"];
            (message["role"].as_str()?, text(&message["content"]))
        }
        Client::Copilot if entry["data"]["parentToolCallId"].is_null() => {
            let role = match entry["type"].as_str()? {
                "user.message" => "user",
                "assistant.message" => "assistant",
                _ => return None,
            };
            (role, text(&entry["data"]["content"]))
        }
        Client::Kimi if entry["type"] == "agent.message.appended" => {
            let message = &entry["message"]["message"];
            (message["role"].as_str()?, text(&message["content"]))
        }
        _ => return None,
    };
    let role = match role {
        "user" => "user",
        "assistant" | "gemini" => "assistant",
        _ => return None,
    };
    let body = if role == "user" {
        user_prompt(&body)
    } else {
        body.trim()
    };
    if body.is_empty()
        || [
            "# AGENTS.md instructions",
            "# Files mentioned by the user:",
            "<environment_context>",
            "<external_codex_apps_open_page>",
        ]
        .iter()
        .any(|prefix| body.starts_with(prefix))
    {
        return None;
    }
    Some(Message {
        role,
        text: scrub(body, 12_000),
    })
}

fn gemini_record(path: &Path) -> Result<(Value, Vec<Value>), String> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
        let bytes = read(path, FILE_LIMIT)?.ok_or("会话已移动。")?;
        let mut object: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Gemini 会话格式无效。")?;
        let messages = object
            .get_mut("messages")
            .and_then(|v| v.take().as_array().cloned())
            .unwrap_or_default();
        return Ok((object, messages));
    }
    let mut metadata = json!({});
    let mut messages: Vec<Value> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    each_line::<()>(path, FILE_LIMIT, |record| {
        let patch = record
            .get("$set")
            .or_else(|| record.get("sessionId").map(|_| &record));
        if let Some(patch) = patch.and_then(Value::as_object) {
            for (key, value) in patch {
                if key == "messages" {
                    if let Some(entries) = value.as_array() {
                        messages.clone_from(entries);
                        positions = messages
                            .iter()
                            .enumerate()
                            .filter_map(|(index, row)| {
                                row["id"].as_str().map(|id| (id.to_owned(), index))
                            })
                            .collect();
                    }
                } else {
                    metadata[key] = value.clone();
                }
            }
        } else if let Some(id) = record["id"].as_str() {
            if let Some(position) = positions.get(id) {
                messages[*position] = record;
            } else {
                positions.insert(id.to_owned(), messages.len());
                messages.push(record);
            }
        } else if let Some(length) = record["$truncate"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
        {
            messages.truncate(length);
            positions.retain(|_, index| *index < length);
        }
        None
    })?;
    Ok((metadata, messages))
}

fn metadata(client: Client, path: &Path, modified: u64) -> Option<Session> {
    let (mut cli_id, mut cwd, mut title) = (String::new(), String::new(), String::new());
    let mut excluded = false;
    if client == Client::Gemini {
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let (head, messages) = gemini_record(path).ok()?;
            cli_id = head["sessionId"].as_str()?.to_owned();
            title = messages
                .iter()
                .find_map(|row| message(client, row).filter(|m| m.role == "user"))
                .map(|m| m.text)
                .unwrap_or_default();
        } else {
            each_line::<()>(path, 128 * 1024, |line| {
                if let Some(id) = line["sessionId"].as_str() {
                    cli_id = id.to_owned();
                }
                if title.is_empty() {
                    if let Some(first) = message(client, &line).filter(|m| m.role == "user") {
                        title = first.text;
                    }
                }
                (!cli_id.is_empty() && !title.is_empty()).then_some(())
            })
            .ok()?;
        }
        let root_file = path.parent()?.parent()?.join(".project_root");
        if let Ok(Some(path)) = read(&root_file, 4096) {
            cwd = String::from_utf8_lossy(&path).trim().into();
        }
    } else {
        if client == Client::Agy {
            cli_id = path.ancestors().nth(3)?.file_name()?.to_str()?.to_owned();
        } else if client == Client::Grokbuild {
            cli_id = path.parent()?.file_name()?.to_str()?.to_owned();
            let summary = path.with_file_name("summary.json");
            if let Some(bytes) = read(&summary, 256 * 1024).ok().flatten() {
                if let Ok(summary) = serde_json::from_slice::<Value>(&bytes) {
                    cwd = summary["info"]["cwd"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    title = summary["generated_title"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                }
            }
        } else if client == Client::Hermes {
            cli_id = path.file_stem()?.to_str()?.to_owned();
        }
        if client == Client::Kimi {
            let dir = path.ancestors().nth(3)?;
            cli_id = dir.file_name()?.to_str()?.to_owned();
            for meta in [dir.join("state.json"), dir.join("session-meta/state.json")] {
                let Some(bytes) = read(&meta, 256 * 1024).ok().flatten() else {
                    continue;
                };
                let Ok(state) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                cwd = state["cwd"].as_str().unwrap_or_default().to_owned();
                title = state["title"].as_str().unwrap_or_default().to_owned();
                break;
            }
        }
        each_line::<()>(path, 192 * 1024, |line| {
            if client == Client::Codex && line["type"] == "session_meta" {
                let p = &line["payload"];
                cli_id = p["id"].as_str().unwrap_or("").to_owned();
                cwd = p["cwd"].as_str().unwrap_or("").to_owned();
                excluded = p["source"] == "subagent"
                    || p["source"].get("subagent").is_some()
                    || p.get("agent_id").is_some_and(|v| !v.is_null());
            } else if matches!(
                client,
                Client::Dsh | Client::Pi | Client::Omp | Client::Openclaw | Client::Hermes
            ) && line["type"] == "session"
            {
                cli_id = line["id"].as_str().unwrap_or("").to_owned();
                cwd = line["cwd"].as_str().unwrap_or("").to_owned();
            } else if client == Client::Copilot && line["type"] == "session.start" {
                cli_id = line["data"]["sessionId"].as_str().unwrap_or("").to_owned();
                cwd = line["data"]["context"]["cwd"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
            } else if client == Client::Workbuddy {
                if cli_id.is_empty() {
                    cli_id = line["sessionId"].as_str().unwrap_or("").to_owned();
                }
                if cwd.is_empty() {
                    cwd = line["cwd"].as_str().unwrap_or("").to_owned();
                }
            } else if client == Client::Claude && cli_id.is_empty() {
                cli_id = line["sessionId"].as_str().unwrap_or("").to_owned();
                cwd = line["cwd"].as_str().unwrap_or("").to_owned();
                excluded = line["isSidechain"] == true;
            }
            if title.is_empty() {
                if let Some(first) = message(client, &line).filter(|m| m.role == "user") {
                    title = first.text;
                }
            }
            (excluded || (!cli_id.is_empty() && !title.is_empty())).then_some(())
        })
        .ok()?;
    }
    if excluded || !valid_cli_id(&cli_id) {
        return None;
    }
    Some(session_summary(client, path, cli_id, cwd, title, modified))
}

fn session_summary(
    client: Client,
    path: &Path,
    cli_id: String,
    cwd: String,
    title: String,
    modified: u64,
) -> Session {
    let id = digest(format!("{}:{}", client.id(), cli_id).as_bytes());
    let title = if title.trim().is_empty() {
        format!("{} 会话", client.label())
    } else {
        title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(100)
            .collect()
    };
    Session {
        id,
        title,
        client,
        cwd,
        modified,
        path: path.to_owned(),
        cli_id,
    }
}

fn session_db(user: &Path, client: Client) -> Option<PathBuf> {
    let path = match client {
        Client::Hermes => client.home(user).join("state.db"),
        Client::Mcode => client.home(user).join("v2/sqlite/runtime-state.sqlite"),
        Client::Opencode => env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| user.join(".local/share"))
            .join("opencode/opencode.db"),
        _ => return None,
    };
    path.is_file().then_some(path)
}

fn read_db(path: &Path) -> Result<Connection, String> {
    if !path.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
        return Err("会话数据库已移动或无法读取。".into());
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "无法打开会话数据库。".into())
}

fn database_sessions(client: Client, path: &Path) -> Result<Vec<Session>, String> {
    let db = read_db(path)?;
    let sql = match client {
        Client::Hermes => "SELECT id, cwd, title, CAST(COALESCE(last_activity_at, ended_at, started_at) * 1000 AS INTEGER) FROM sessions ORDER BY 4 DESC LIMIT 900",
        Client::Mcode => "SELECT session_id, workspace_dir, title, COALESCE(updated_at_ms, created_at_ms) FROM local_runtime_sessions
            WHERE visibility <> 'hidden' AND archived = 0 AND parent_session_id IS NULL AND session_kind NOT IN ('peek','channel','cron')
            ORDER BY 4 DESC LIMIT 900",
        Client::Opencode => "SELECT id, directory, title, COALESCE(time_updated, time_created)
            FROM session_v2 ORDER BY 4 DESC LIMIT 900",
        _ => return Ok(Vec::new()),
    };
    let mut query = db.prepare(sql).or_else(|err| match client {
        Client::Hermes => db.prepare("SELECT id, cwd, title, CAST(started_at * 1000 AS INTEGER) FROM sessions ORDER BY 4 DESC LIMIT 900"),
        Client::Opencode => db.prepare("SELECT id, directory, title, COALESCE(time_updated, time_created) FROM session ORDER BY 4 DESC LIMIT 900"),
        _ => Err(err),
    }).map_err(|_| "无法读取此版本的会话索引。")?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        })
        .map_err(|_| "无法加载会话索引。")?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|(id, _, _, _)| valid_cli_id(id))
        .map(|(id, cwd, title, ms)| {
            session_summary(client, path, id, cwd, scrub(&title, 120), ms.max(0) as u64)
        })
        .collect())
}

fn database_messages(session: &Session) -> Result<(Vec<Message>, bool), String> {
    let db = read_db(&session.path)?;
    let opencode_v2 = session.client == Client::Opencode
        && db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM session_v2 WHERE id = ?1)",
                [&session.cli_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
    let sql = match session.client {
        Client::Hermes => "SELECT role, substr(content, 1, 12000) FROM messages
             WHERE session_id = ?1 AND role IN ('user','assistant') ORDER BY id LIMIT 451",
        Client::Mcode => "WITH migrated AS (
                SELECT 1 FROM local_runtime_message_row_migrations WHERE session_id = ?1
             ), items AS (
                SELECT role, data_json, id AS ordinal FROM local_runtime_message_rows
                  WHERE session_id = ?1 AND EXISTS (SELECT 1 FROM migrated)
                UNION ALL
                SELECT json_extract(j.value, '$.role'), j.value, j.key FROM local_runtime_messages,
                  json_each(display_messages_json) AS j WHERE session_id = ?1 AND NOT EXISTS (SELECT 1 FROM migrated)
             ) SELECT role, substr(data_json, 1, 1000000) FROM items
               WHERE role IN ('user','assistant') ORDER BY ordinal LIMIT 451",
        Client::Opencode if opencode_v2 => "SELECT type, substr(data, 1, 1000000) FROM session_message
            WHERE session_id = ?1 AND type IN ('user','assistant') ORDER BY seq, rowid LIMIT 451",
        Client::Opencode => "SELECT json_extract(m.data, '$.role'), substr(json_extract(p.data, '$.text'), 1, 12000)
             FROM message m JOIN part p ON p.message_id = m.id
             WHERE m.session_id = ?1 AND json_extract(p.data, '$.type') = 'text'
             ORDER BY m.time_created, p.time_created LIMIT 451",
        _ => return Err("无法读取此客户端的会话数据库。".into()),
    };
    let mut query = db
        .prepare(sql)
        .map_err(|_| "此客户端的会话正文格式暂不可读。")?;
    let rows = query
        .query_map([&session.cli_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .map_err(|_| "无法加载数据库会话。")?;
    let mut messages: Vec<_> = rows
        .filter_map(Result::ok)
        .filter_map(|(role, content)| {
            let role = match role.as_str() {
                "user" => "user",
                "assistant" => "assistant",
                _ => return None,
            };
            let body = if session.client == Client::Mcode {
                text(&serde_json::from_str::<Value>(&content).ok()?["msg_content"])
            } else if opencode_v2 {
                let record = serde_json::from_str::<Value>(&content).ok()?;
                record["text"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| text(&record["content"]))
            } else {
                content
            };
            (!body.trim().is_empty()).then(|| Message {
                role,
                text: scrub(&body, 12_000),
            })
        })
        .collect();
    let limited = messages.len() > 450;
    messages.truncate(450);
    Ok((messages, limited))
}

pub(super) fn list(user: &Path, client: Client, query: &str) -> (Vec<Session>, bool) {
    let logs = log_files(user, client);
    let query: String = query.to_lowercase().chars().take(160).collect();
    let mut seen = HashSet::new();
    let db = session_db(user, client).map(|path| database_sessions(client, &path));
    let db_error = db.as_ref().is_some_and(Result::is_err);
    let mut candidates = db.and_then(Result::ok).unwrap_or_default();
    let db_limited = candidates.len() == 900;
    candidates.extend(
        logs.files
            .iter()
            .take(900)
            .filter_map(|(path, modified)| metadata(client, path, *modified)),
    );
    candidates.sort_by_key(|session| std::cmp::Reverse(session.modified));
    let mut sessions = Vec::new();
    for session in candidates {
        if !(seen.insert(session.id.clone())) {
            continue;
        }
        if query.is_empty()
            || [&session.title, &session.cwd, &session.cli_id]
                .iter()
                .any(|field| field.to_lowercase().contains(&query))
        {
            sessions.push(session);
        }
        if sessions.len() == SHOW_LIMIT {
            break;
        }
    }
    let limited = db_error
        || db_limited
        || logs.limited
        || logs.files.len() > 900
        || sessions.len() == SHOW_LIMIT;
    (sessions, limited)
}

pub(super) fn messages(session: &Session) -> Result<(Vec<Message>, bool), String> {
    if session
        .path
        .extension()
        .is_some_and(|ext| ext == "db" || ext == "sqlite")
    {
        return database_messages(session);
    }
    let size = fs::metadata(&session.path)
        .map_err(|_| "会话已移动或无法读取。")?
        .len();
    let mut result: Vec<Message> = Vec::new();
    let mut take_message = |value: &Value| {
        if let Some(entry) = message(session.client, value) {
            if !result
                .last()
                .is_some_and(|prev| prev.role == entry.role && prev.text == entry.text)
            {
                result.push(entry);
            }
        }
        result.len() >= 450
    };
    if session.client == Client::Gemini {
        for entry in gemini_record(&session.path)?.1 {
            if take_message(&entry) {
                break;
            }
        }
    } else {
        each_line::<()>(&session.path, FILE_LIMIT, |record| {
            take_message(&record).then_some(())
        })?;
    }
    let limited = result.len() >= 450 || size > FILE_LIMIT;
    Ok((result, limited))
}

pub(super) fn markdown(session: &Session) -> Result<String, String> {
    let (messages, limited) = messages(session)?;
    let mut output = format!("# {}\n\n", session.title);
    if limited {
        output.push_str("> 此导出包含会话的可读预览。\n\n");
    }
    for m in messages {
        output.push_str(&format!(
            "## {}\n\n{}\n\n",
            if m.role == "user" { "用户" } else { "助手" },
            m.text
        ));
    }
    Ok(output)
}

pub(super) fn memory_sources(session: &Session) -> Result<Vec<Value>, String> {
    let (messages, _) = messages(session)?;
    let sources: Vec<_> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user")
        .rev()
        .take(8)
        .map(|(index, m)| {
            let text = scrub(&m.text, 1700);
            json!({"id": digest(format!("{}:{index}:{text}", session.id).as_bytes()),
                "session": session.id, "role": "user", "text": text, "at": session.modified})
        })
        .collect();
    if sources.is_empty() {
        return Err("这个会话没有可整理的用户消息。".into());
    }
    Ok(sources.into_iter().rev().collect())
}

pub(super) fn executable(client: Client) -> Option<PathBuf> {
    executables(client).into_iter().next()
}

pub(super) fn executables(client: Client) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if client == Client::Codex {
        if let Ok(path) = crate::companion_chat::codex() {
            results.push(path);
        }
    }
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{}.exe", client.cli()),
            format!("{}.cmd", client.cli()),
        ]
    } else {
        vec![client.cli().to_owned()]
    };
    for path in env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|path| path.is_absolute())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .filter(|path| path.is_file())
    {
        if !results.contains(&path) {
            results.push(path);
        }
        if results.len() >= 12 {
            break;
        }
    }
    results
}

pub(super) fn cli_version(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| "无法启动 CLI 版本检查。")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("CLI 版本检查失败。".into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("CLI 版本检查超时。".into());
            }
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()
        .ok_or("CLI 没有返回版本。")?
        .take(1024)
        .read_to_string(&mut output)
        .map_err(|_| "无法读取 CLI 版本。")?;
    let line = output.lines().next().ok_or("CLI 没有返回版本。")?;
    Ok(scrub(line, 120))
}

pub(super) fn resume(session: &Session) -> Result<(), String> {
    if !session.client.manages_config() {
        return Err("请在原客户端继续此会话。".into());
    }
    if !valid_cli_id(&session.cli_id) {
        return Err("无效的会话编号。".into());
    }
    let executable = executable(session.client).ok_or("该工具未安装或没有加入 PATH。")?;
    let mut command = Command::new(executable);
    match session.client {
        Client::Codex => {
            command.arg("resume");
        }
        Client::Claude | Client::Gemini => {
            command.arg("--resume");
        }
        _ => return Err("请在原客户端继续此会话。".into()),
    }
    command.arg(&session.cli_id);
    let cwd = Path::new(&session.cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("会话的原项目目录不可用。".into());
    }
    command.current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    command.spawn().map_err(|_| "无法打开客户端继续会话。")?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn run_installer(client: Client) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let package = match client {
        Client::Codex => "@openai/codex",
        Client::Claude => "@anthropic-ai/claude-code",
        Client::Gemini => "@google/gemini-cli",
        _ => return Err("请通过客户端的官方安装入口升级。".into()),
    };
    let npm = env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|path| path.is_absolute())
        .map(|dir| dir.join("npm.cmd"))
        .find(|path| path.is_file())
        .ok_or("找不到 npm，请先安装 Node.js，或查看官方安装说明。")?;
    Command::new(npm)
        .args(["install", "--global", package])
        .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
        .spawn()
        .map_err(|_| "无法启动 CLI 安装。")?;
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn run_installer(_: Client) -> Result<(), String> {
    Err("此安装入口仅支持 Windows。".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageEntry {
    pub model: String,
    pub provider: Option<String>,
    pub time: Option<u64>,
    pub request_count: u64,
    #[serde(flatten)]
    tokens: Tokens,
}

fn timestamp(value: &Value) -> Option<u64> {
    value.as_u64().filter(|ms| *ms > 0).or_else(|| {
        let date = chrono::DateTime::parse_from_rfc3339(value.as_str()?).ok()?;
        u64::try_from(date.timestamp_millis()).ok()
    })
}

fn nonempty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.trim().is_empty())
}

fn grok_usage(path: &Path) -> Result<Vec<UsageEntry>, String> {
    let mut result = Vec::new();
    let mut turns = HashSet::new();
    each_line::<()>(path, FILE_LIMIT, |record| {
        let update = &record["params"]["update"];
        let usage = &update["usage"];
        if update["sessionUpdate"]
            .as_str()
            .is_some_and(|kind| kind != "turn_completed")
            || !usage.is_object()
        {
            return None;
        }
        if nonempty(&update["prompt_id"]).is_some_and(|id| !turns.insert(id.to_owned())) {
            return None;
        }
        let time = timestamp(&record["timestamp"]);
        let mut append = |name: &str, counters: &Value| {
            let input = counters["inputTokens"].as_u64().unwrap_or(0);
            let output = counters["outputTokens"].as_u64().unwrap_or(0);
            if input > 0 || output > 0 {
                result.push(UsageEntry {
                    model: name.chars().take(120).collect(),
                    provider: None,
                    time,
                    request_count: counters["modelCalls"].as_u64().unwrap_or(1).clamp(1, 1000),
                    tokens: Tokens {
                        input,
                        output,
                        cached: counters["cachedReadTokens"]
                            .as_u64()
                            .unwrap_or(0)
                            .min(input),
                    },
                });
            }
        };
        if let Some(models) = usage["modelUsage"]
            .as_object()
            .filter(|models| !models.is_empty())
        {
            for (name, counters) in models {
                append(name, counters);
            }
        } else {
            append("未知模型", usage);
        }
        None
    })?;
    Ok(result)
}

fn usage_file(client: Client, path: &Path, is_child: bool) -> Result<Vec<UsageEntry>, String> {
    if client == Client::Grokbuild {
        return grok_usage(&path.with_file_name("updates.jsonl"));
    }
    let mut model = "未知模型".to_owned();
    let mut provider = None;
    let mut previous: Option<Tokens> = None;
    let mut entries = Vec::new();
    let mut assistant: HashMap<String, usize> = HashMap::new();
    let number = |v: &Value| v.as_u64().unwrap_or(0);
    let codex_tokens = |t: &Value| Tokens {
        input: number(&t["input_tokens"]),
        output: number(&t["output_tokens"]),
        cached: number(&t["cached_input_tokens"]),
    };
    let mut collect = |record: &Value| {
        let (id, model, provider, time, mut tokens) = match client {
            Client::Codex => {
                let p = &record["payload"];
                if let Some(name) = nonempty(&p["model"]) {
                    model = name.chars().take(120).collect();
                }
                if let Some(name) = nonempty(&p["model_provider"]) {
                    provider = Some(name.chars().take(120).collect::<String>());
                }
                if p["type"] != "token_count" {
                    return;
                }
                let info = &p["info"];
                let tokens = if info["total_token_usage"].is_object() {
                    let total = codex_tokens(&info["total_token_usage"]);
                    let baseline = previous.replace(total.clone());
                    if baseline.as_ref() == Some(&total) {
                        return;
                    }
                    let last = codex_tokens(&info["last_token_usage"]);
                    if let Some(before) = baseline {
                        total.delta(&before)
                    } else if is_child && last != Tokens::default() {
                        last
                    } else {
                        total
                    }
                } else {
                    codex_tokens(&info["last_token_usage"])
                };
                (
                    None,
                    Some(model.as_str()),
                    provider.as_deref(),
                    timestamp(&record["timestamp"]),
                    tokens,
                )
            }
            Client::Claude if record["type"] == "assistant" => {
                let msg = &record["message"];
                let u = &msg["usage"];
                let cached = number(&u["cache_read_input_tokens"]);
                (
                    msg["id"].as_str(),
                    msg["model"].as_str(),
                    None,
                    timestamp(&record["timestamp"]),
                    Tokens {
                        input: number(&u["input_tokens"])
                            .saturating_add(cached)
                            .saturating_add(number(&u["cache_creation_input_tokens"])),
                        output: number(&u["output_tokens"]),
                        cached,
                    },
                )
            }
            Client::Gemini if record["tokens"].is_object() => {
                let u = &record["tokens"];
                (
                    record["id"].as_str(),
                    record["model"].as_str(),
                    None,
                    timestamp(&record["timestamp"]),
                    Tokens {
                        input: number(&u["input"]),
                        output: number(&u["output"]),
                        cached: number(&u["cached"]),
                    },
                )
            }
            Client::Dsh if record["type"] == "assistant/message" => {
                let data = &record["data"];
                let msg = &data["message"];
                let u = &data["usage"];
                let cached = number(&u["cacheReadTokens"]);
                (
                    msg["id"].as_str(),
                    msg["source"]["model"].as_str(),
                    msg["source"]["provider"].as_str(),
                    timestamp(&record["time"]),
                    Tokens {
                        input: number(&u["inputTokens"])
                            .saturating_add(cached)
                            .saturating_add(number(&u["cacheWriteTokens"])),
                        output: number(&u["outputTokens"]),
                        cached,
                    },
                )
            }
            Client::Workbuddy if record["type"] == "message" && record["role"] == "assistant" => {
                let data = &record["providerData"];
                let u = &data["rawUsage"];
                let cached = number(&u["prompt_cache_hit_tokens"])
                    .max(number(&u["prompt_tokens_details"]["cached_tokens"]))
                    .max(number(&u["cached_tokens"]))
                    .max(number(&u["cache_read_input_tokens"]));
                let model = nonempty(&data["model"]).or_else(|| nonempty(&data["requestModelId"]));
                (
                    record["id"].as_str(),
                    model,
                    None,
                    timestamp(&record["timestamp"]),
                    Tokens {
                        input: number(&u["prompt_tokens"]),
                        output: number(&u["completion_tokens"]),
                        cached,
                    },
                )
            }
            Client::Pi | Client::Omp | Client::Openclaw
                if record["type"] == "message" && record["message"]["role"] == "assistant" =>
            {
                let message = &record["message"];
                let usage = &message["usage"];
                let cached = number(&usage["cacheRead"]);
                (
                    record["id"].as_str(),
                    message["model"].as_str(),
                    message["provider"].as_str(),
                    timestamp(&record["timestamp"]).or_else(|| timestamp(&message["timestamp"])),
                    Tokens {
                        input: number(&usage["input"])
                            .saturating_add(cached)
                            .saturating_add(number(&usage["cacheWrite"])),
                        output: number(&usage["output"]),
                        cached,
                    },
                )
            }
            Client::Copilot if record["type"] == "session.usage_record" => {
                let usage = &record["data"]["usage"];
                (
                    record["id"].as_str(),
                    usage["model"].as_str(),
                    None,
                    timestamp(&record["timestamp"]),
                    Tokens {
                        input: number(&usage["inputTokens"]),
                        output: number(&usage["outputTokens"]),
                        cached: number(&usage["cacheReadTokens"]),
                    },
                )
            }
            Client::Kimi
                if record["type"] == "agent.message.appended"
                    && record["message"]["message"]["role"] == "assistant" =>
            {
                let meta = &record["message"]["meta"];
                let usage = &meta["usage"];
                let cached = number(&usage["inputCacheRead"]);
                (
                    meta["messageId"].as_str(),
                    meta["model"]["model"].as_str(),
                    meta["model"]["provider"].as_str(),
                    timestamp(&record["timestamp"]).or_else(|| timestamp(&record["time"])),
                    Tokens {
                        input: number(&usage["inputOther"])
                            .saturating_add(cached)
                            .saturating_add(number(&usage["inputCacheCreation"])),
                        output: number(&usage["output"]),
                        cached,
                    },
                )
            }
            _ => return,
        };
        if tokens == Tokens::default() {
            return;
        }
        tokens.cached = tokens.cached.min(tokens.input);
        if let Some(index) = id.and_then(|id| assistant.get(id).copied()) {
            let entry: &mut UsageEntry = &mut entries[index];
            entry.tokens.input = entry.tokens.input.max(tokens.input);
            entry.tokens.output = entry.tokens.output.max(tokens.output);
            entry.tokens.cached = entry.tokens.cached.max(tokens.cached);
        } else {
            if let Some(id) = id {
                assistant.insert(id.to_owned(), entries.len());
            }
            entries.push(UsageEntry {
                model: model.unwrap_or("未知模型").chars().take(120).collect(),
                provider: provider.map(|name| name.chars().take(120).collect()),
                time,
                request_count: 1,
                tokens,
            });
        }
    };
    if client == Client::Gemini {
        for record in gemini_record(path)?.1 {
            collect(&record);
        }
    } else {
        let cap = if client == Client::Codex {
            USAGE_FILE_LIMIT
        } else {
            FILE_LIMIT
        };
        each_matching_line::<()>(
            path,
            cap,
            |line| {
                client != Client::Codex
                    || [
                        b"\"token_count\"".as_slice(),
                        b"\"model\"",
                        b"\"model_provider\"",
                    ]
                    .iter()
                    .any(|field| line.windows(field.len()).any(|part| part == *field))
            },
            |record| {
                collect(&record);
                None
            },
        )?;
    }
    Ok(entries)
}

#[derive(Serialize)]
pub(super) struct UsageReport {
    pub entries: Vec<UsageEntry>,
    files: usize,
    errors: usize,
    limited: bool,
}

pub(super) fn usage(user: &Path, client: Client, since: Option<u64>) -> UsageReport {
    let logs = log_files(user, client);
    // File modification is a traversal hint. Individual requests are filtered by their logged time.
    let eligible = logs
        .files
        .iter()
        .filter(|(_, modified)| since.is_none_or(|since| *modified >= since));
    let mut seen = HashSet::new();
    let mut scanned_bytes = 0_u64;
    let mut report = UsageReport {
        entries: Vec::new(),
        files: 0,
        errors: 0,
        limited: logs.limited,
    };
    for (file, _) in eligible {
        if report.files == 180
            || report.entries.len() >= 40_000
            || scanned_bytes >= USAGE_SCAN_LIMIT
        {
            report.limited = true;
            break;
        }
        let path = file.canonicalize().unwrap_or_else(|_| file.clone());
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let key = if client == Client::Codex {
            name.to_string()
        } else {
            path.to_string_lossy().into_owned()
        };
        if !seen.insert(key) {
            continue;
        }
        let child = client == Client::Codex && name.contains('_');
        report.files += 1;
        if let Ok(metadata) = file.metadata() {
            let cap = if client == Client::Codex {
                USAGE_FILE_LIMIT
            } else {
                FILE_LIMIT
            };
            scanned_bytes = scanned_bytes.saturating_add(metadata.len().min(cap));
            report.limited |= metadata.len() > cap;
        }
        match usage_file(client, file, child) {
            Ok(entries) => report.entries.extend(entries.into_iter().filter(|entry| {
                since.is_none_or(|since| entry.time.is_some_and(|time| time >= since))
            })),
            Err(_) => report.errors += 1,
        }
    }
    report
        .entries
        .sort_unstable_by_key(|entry| std::cmp::Reverse(entry.time));
    if report.entries.len() > 40_000 {
        report.entries.truncate(40_000);
        report.limited = true;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_counts_request_deltas_once_and_preserves_time_and_model() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout-test.jsonl");
        let usage = |input, output, cached| {
            json!({
                "input_tokens":input, "output_tokens":output, "cached_input_tokens":cached
            })
        };
        let request = |timestamp: &str, total: Value, last: Value| {
            json!({
                "timestamp":timestamp, "type":"event_msg", "payload":{"type":"token_count",
                    "info":{"total_token_usage":total, "last_token_usage":last}}
            })
        };
        let first = request(
            "2026-11-19T00:00:00Z",
            usage(100, 10, 80),
            usage(100, 10, 80),
        );
        let second = request(
            "2026-11-19T01:00:00Z",
            usage(150, 15, 120),
            usage(50, 5, 40),
        );
        let lines = [
            json!({"type":"session_meta","payload":{"model_provider":"example"}}),
            json!({"type":"turn_context","payload":{"model":"alpha"}}),
            first.clone(),
            first,
            json!({"type":"turn_context","payload":{"model":"beta"}}),
            second,
        ];
        fs::write(&path, lines.map(|row| row.to_string()).join("\n")).unwrap();
        let rows = usage_file(Client::Codex, &path, false).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].model, "alpha");
        assert_eq!(rows[1].model, "beta");
        assert_eq!(rows[1].provider.as_deref(), Some("example"));
        assert_eq!(rows[1].tokens.input - rows[1].tokens.cached, 10);
        assert_eq!(rows[1].tokens.output, 5);
        assert_eq!(rows[1].time.unwrap() - rows[0].time.unwrap(), 3_600_000);
    }

    #[test]
    fn deepseek_harness_reads_concatenated_compressed_frames() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.v4.jsonl.zstd");
        let records = [
            json!({"type":"session","id":"fixture-session","cwd":"/fixture"}),
            json!({"type":"user/message","data":{"content":[{"text":"Inspect the fixture"}]}}),
            json!({"type":"assistant/message","time":1000,"data":{
                "message":{"id":"first","source":{"model":"fixture-model"},"content":[{"text":"Done."}]},
                "usage":{"inputTokens":10,"outputTokens":20,"cacheReadTokens":60,"cacheWriteTokens":2}
            }}),
        ];
        let bytes: Vec<_> = records
            .iter()
            .flat_map(|row| zstd::stream::encode_all(format!("{row}\n").as_bytes(), 1).unwrap())
            .collect();
        fs::write(&path, bytes).unwrap();
        let meta = metadata(Client::Dsh, &path, 1000).unwrap();
        assert_eq!(meta.title, "Inspect the fixture");
        assert_eq!(messages(&meta).unwrap().0.len(), 2);
        let rows = usage_file(Client::Dsh, &path, false).unwrap();
        assert_eq!(rows[0].tokens.input, 72);
        assert_eq!(rows[0].tokens.cached, 60);
    }
}
