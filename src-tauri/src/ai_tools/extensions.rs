use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toml_edit::{Array, InlineTable, Item, Table, Value as TomlValue};

use super::{
    providers::{json_bytes, json_object, toml, Client},
    storage::{read_config, Change},
};
use crate::companion_sync::digest;

const PROMPT_START: &str = "<!-- penguinpal-ai-tools:begin -->";
const PROMPT_END: &str = "<!-- penguinpal-ai-tools:end -->";
const SKILL_FILE: &str = "SKILL.md";
const DISABLED_FILE: &str = "SKILL.md.penguinpal-disabled";

pub(super) fn supports_mcp(client: Client) -> bool {
    !matches!(client, Client::ClaudeDesktop | Client::Openclaw)
}

pub(super) fn supports_sse(client: Client) -> bool {
    matches!(
        client,
        Client::Claude
            | Client::Gemini
            | Client::Opencode
            | Client::Kimi
            | Client::Workbuddy
            | Client::Copilot
            | Client::Mcode
    )
}

fn toml_mcp(client: Client) -> bool {
    matches!(client, Client::Codex | Client::Grokbuild)
}

fn mcp_key(client: Client) -> &'static str {
    match client {
        Client::Codex | Client::Grokbuild | Client::Hermes => "mcp_servers",
        Client::Opencode => "mcp",
        _ => "mcpServers",
    }
}

pub(super) fn mcp_path(client: Client, user: &Path) -> PathBuf {
    if client == Client::Claude {
        // Claude Code stores the user-scope MCP definitions outside settings.json.
        let home = client.home(user);
        return if home == user.join(".claude") {
            user.join(".claude.json")
        } else {
            home.join(".claude.json")
        };
    }
    match client {
        Client::Copilot => client.home(user).join("mcp-config.json"),
        Client::Workbuddy | Client::Mcode | Client::Kimi | Client::Pi | Client::Omp => {
            client.home(user).join("mcp.json")
        }
        Client::Agy => user.join(".gemini/config/mcp_config.json"),
        _ => client.config(user),
    }
}

fn from_toml(value: &TomlValue) -> Value {
    match value {
        TomlValue::String(v) => json!(v.value()),
        TomlValue::Integer(v) => json!(v.value()),
        TomlValue::Float(v) => json!(v.value()),
        TomlValue::Boolean(v) => json!(v.value()),
        TomlValue::Datetime(v) => json!(v.value().to_string()),
        TomlValue::Array(v) => Value::Array(v.iter().map(from_toml).collect()),
        TomlValue::InlineTable(v) => Value::Object(
            v.iter()
                .map(|(k, v)| (k.to_owned(), from_toml(v)))
                .collect(),
        ),
    }
}

fn from_item(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(value) => from_toml(value),
        Item::Table(table) => Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_owned(), from_item(v)))
                .collect(),
        ),
        Item::ArrayOfTables(tables) => Value::Array(
            tables
                .iter()
                .map(|t| from_item(&Item::Table(t.clone())))
                .collect(),
        ),
    }
}

fn to_toml(value: &Value) -> Result<TomlValue, String> {
    Ok(match value {
        Value::String(s) => TomlValue::from(s.clone()),
        Value::Bool(b) => TomlValue::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TomlValue::from(i)
            } else {
                TomlValue::from(n.as_f64().ok_or("MCP 数值无效。")?)
            }
        }
        Value::Array(a) => {
            let mut array = Array::new();
            for entry in a {
                array.push(to_toml(entry)?);
            }
            TomlValue::Array(array)
        }
        Value::Object(m) => {
            let mut table = InlineTable::new();
            for (k, v) in m {
                table.insert(k, to_toml(v)?);
            }
            TomlValue::InlineTable(table)
        }
        Value::Null => return Err("TOML 格式的 MCP 参数不能为 null。".into()),
    })
}

fn config_mcp(client: Client, bytes: Option<&[u8]>) -> Result<Map<String, Value>, String> {
    let value = if toml_mcp(client) {
        toml(bytes)?.get(mcp_key(client)).map(from_item)
    } else if client == Client::Hermes {
        super::mcp_yaml::hermes_entries(bytes)?
    } else {
        let bytes = bytes
            .filter(|bytes| !bytes.iter().all(u8::is_ascii_whitespace))
            .map(|bytes| bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes));
        json_object(bytes)?.get(mcp_key(client)).cloned()
    };
    match value {
        None => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m),
        _ => Err("现有 MCP 区块格式无效。".into()),
    }
}

fn mcp_entries(client: Client, user: &Path) -> Result<Map<String, Value>, String> {
    if client == Client::Dsh {
        return super::mcp_yaml::dsh_entries(user);
    }
    config_mcp(client, read_config(&mcp_path(client, user))?.as_deref())
}

fn transport(client: Client, definition: &Value) -> &'static str {
    if definition.get("command").is_some() {
        return "stdio";
    }
    if definition["type"] == "sse"
        || definition["transport"] == "sse"
        || (client == Client::Gemini && definition.get("httpUrl").is_none())
    {
        return "sse";
    }
    "http"
}

pub(super) fn mcp_list(client: Client, user: &Path) -> Result<Value, String> {
    let entries = mcp_entries(client, user)?;
    Ok(json!(entries
        .iter()
        .map(|(name, definition)| json!({
            "name": name,
            "transport": transport(client, definition),
            "enabled": definition["enabled"] != false && definition["disabled"] != true,
            "editable": definition.is_object() && definition["managedBy"] != "connector",
        }))
        .collect::<Vec<_>>()))
}

const PRIVATE_MCP_FIELDS: &[&str] = &[
    "env",
    "environment",
    "headers",
    "http_headers",
    "env_http_headers",
    "bearer_token_env_var",
    "auth",
    "oauth",
];

pub(super) fn mcp_editor(client: Client, user: &Path, name: &str) -> Result<Value, String> {
    let entries = mcp_entries(client, user)?;
    let definition = entries.get(name).ok_or("此 MCP 已不存在。")?;
    if definition["managedBy"] == "connector" {
        return Err("这个 MCP 由客户端连接器管理。".into());
    }
    let mut params = definition["args"].as_array().cloned().unwrap_or_default();
    let command = if let Some(parts) = definition["command"].as_array() {
        params = parts.iter().skip(1).cloned().collect();
        parts.first().cloned().unwrap_or_default()
    } else {
        definition["command"].clone()
    };
    let url = ["httpUrl", "url", "serverUrl"]
        .iter()
        .find_map(|key| definition.get(*key));
    Ok(json!({
        "definition": {"type": transport(client, definition), "command": command, "args": params, "cwd": definition["cwd"], "url": url},
        "preservesPrivateFields": PRIVATE_MCP_FIELDS.iter().any(|field| definition.get(*field).is_some())
    }))
}

fn native_mcp(client: Client, input: &Value, previous: Option<&Value>) -> Result<Value, String> {
    let transport = input["type"].as_str().ok_or("请选择 MCP 连接方式。")?;
    if !matches!(transport, "stdio" | "http" | "sse") {
        return Err("MCP 连接方式无效。".into());
    }
    if transport == "sse" && !supports_sse(client) {
        return Err(format!("{} 请使用 HTTP 或 stdio。", client.label()));
    }
    let mut output = previous
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let same_connection = previous.is_some_and(|entry| {
        if self::transport(client, entry) != transport {
            return false;
        }
        if transport == "stdio" {
            let original = entry["command"]
                .as_str()
                .or_else(|| entry["command"].get(0).and_then(Value::as_str));
            return original == input["command"].as_str();
        }
        ["httpUrl", "url", "serverUrl"]
            .iter()
            .find_map(|key| entry[*key].as_str())
            == input["url"].as_str()
    });
    for field in [
        "type",
        "transport",
        "url",
        "httpUrl",
        "serverUrl",
        "command",
        "args",
        "cwd",
    ] {
        output.remove(field);
    }
    if !same_connection {
        for field in PRIVATE_MCP_FIELDS {
            output.remove(*field);
        }
    }
    let is_stdio = transport == "stdio";
    for field in if is_stdio {
        &["command", "args", "cwd", "env"][..]
    } else {
        &["url", "headers"][..]
    } {
        if let Some(value) = input.get(*field) {
            if matches!(*field, "env" | "headers") {
                let incoming = value
                    .as_object()
                    .ok_or("MCP 环境变量和请求头必须是对象。")?;
                if !incoming.values().all(Value::is_string) {
                    return Err("MCP 环境变量和请求头必须填写文本。".into());
                }
                let key = match (client, *field) {
                    (Client::Codex, "headers") => "http_headers",
                    (Client::Opencode, "env") => "environment",
                    _ => field,
                };
                let mut merged = output
                    .get(key)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                merged.extend(incoming.clone());
                output.insert(key.to_string(), json!(merged));
            } else {
                output.insert(field.to_string(), value.clone());
            }
        }
    }
    if client == Client::Opencode {
        if output.remove("cwd").is_some() {
            return Err("OpenCode 的 MCP 暂不能设置工作目录。".into());
        }
        if is_stdio {
            let mut command = vec![output.remove("command").unwrap_or_default()];
            if let Some(Value::Array(args)) = output.remove("args") {
                command.extend(args);
            }
            output.insert("command".into(), json!(command));
        }
        output.insert(
            "type".into(),
            json!(if is_stdio { "local" } else { "remote" }),
        );
    } else if client == Client::Gemini {
        if transport == "http" {
            let url = output.remove("url").unwrap_or_default();
            output.insert("httpUrl".into(), url);
        }
    } else if client == Client::Kimi || client == Client::Dsh {
        output.insert(
            "transport".into(),
            json!(if transport == "http" {
                "streamable-http"
            } else {
                transport
            }),
        );
    } else if !toml_mcp(client) && !matches!(client, Client::Agy | Client::Hermes) {
        output.insert(
            "type".into(),
            json!(if is_stdio && client == Client::Copilot {
                "local"
            } else {
                transport
            }),
        );
    }
    if client == Client::Copilot {
        output.entry("tools").or_insert_with(|| json!(["*"]));
    }
    Ok(json!(output))
}

fn validate_mcp(name: &str, definition: &Value) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err("MCP 名称只能包含字母、数字、下划线和连字符。".into());
    }
    if !definition.is_object() || definition.to_string().len() > 32 * 1024 {
        return Err("请输入有效的 MCP JSON 对象。".into());
    }
    let command = definition["command"]
        .as_str()
        .filter(|s| !s.trim().is_empty());
    let endpoint = definition["url"]
        .as_str()
        .or_else(|| definition["httpUrl"].as_str());
    match (command, endpoint) {
        (Some(_), None) => {
            if definition
                .get("args")
                .is_some_and(|v| !v.as_array().is_some_and(|a| a.iter().all(Value::is_string)))
            {
                return Err("MCP 的 args 必须是字符串数组。".into());
            }
        }
        (None, Some(endpoint)) => {
            let url = reqwest::Url::parse(endpoint).map_err(|_| "MCP URL 无效。")?;
            if !matches!(url.scheme(), "https" | "http")
                || url.host().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("MCP URL 必须是无嵌入凭据的 HTTP(S) 地址。".into());
            }
        }
        _ => return Err("请选择 command 或 url/httpUrl 中的一种传输方式。".into()),
    }
    Ok(())
}

fn put_mcp(
    client: Client,
    bytes: Option<&[u8]>,
    names: Option<&str>,
    entries: Map<String, Value>,
) -> Result<Vec<u8>, String> {
    if client == Client::Hermes {
        return super::mcp_yaml::put_hermes(bytes, names, entries);
    }
    if toml_mcp(client) {
        let mut doc = toml(bytes)?;
        let item = doc
            .entry(mcp_key(client))
            .or_insert(Item::Table(Table::new()));
        let inline = item.is_inline_table();
        let table = item
            .as_table_like_mut()
            .ok_or("mcp_servers 必须是 TOML 表。")?;
        if let Some(name) = names {
            table.remove(name);
        } else {
            table.clear();
        }
        for (name, definition) in entries {
            let value = to_toml(&definition)?;
            let item = match value {
                TomlValue::InlineTable(table) if !inline => Item::Table(table.into_table()),
                value => Item::Value(value),
            };
            table.insert(&name, item);
        }
        return Ok(doc.to_string().into_bytes());
    }
    let bytes = bytes
        .filter(|bytes| !bytes.iter().all(u8::is_ascii_whitespace))
        .map(|bytes| bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes));
    let mut doc = json_object(bytes)?;
    if let Some(name) = names {
        let target = doc
            .as_object_mut()
            .unwrap()
            .entry(mcp_key(client))
            .or_insert_with(|| json!({}));
        let table = target
            .as_object_mut()
            .ok_or("mcpServers 必须是 JSON 对象。")?;
        table.remove(name);
        table.extend(entries);
    } else {
        doc[mcp_key(client)] = Value::Object(entries);
    }
    json_bytes(&doc)
}

pub(super) fn mcp_plan(
    client: Client,
    user: &Path,
    name: &str,
    definition: Option<&Value>,
    replace: bool,
) -> Result<Vec<Change>, String> {
    if !supports_mcp(client) {
        return Err("此客户端的 MCP 由客户端自身管理。".into());
    }
    if let Some(definition) = definition {
        validate_mcp(name, definition)?;
    }
    let existing = mcp_entries(client, user)?;
    let original = existing.get(name);
    if matches!(client, Client::Pi | Client::Omp)
        && existing
            .keys()
            .any(|id| id != name && id.replace('-', "_") == name.replace('-', "_"))
    {
        return Err("目标客户端已有名称仅在连字符和下划线处不同的 MCP。".into());
    }
    if original.is_some_and(|entry| entry["managedBy"] == "connector") {
        return Err(format!("{} 的同名 MCP 由连接器管理。", client.label()));
    }
    if !replace && original.is_some() {
        return Err(format!(
            "{} 已有同名 MCP，请先在该客户端页编辑。",
            client.label()
        ));
    }
    if definition.is_none() && original.is_none() {
        return Err("此 MCP 配置已不存在。".into());
    }
    let next = definition
        .map(|definition| native_mcp(client, definition, original))
        .transpose()?;
    if client == Client::Dsh {
        return super::mcp_yaml::plan_dsh(user, name, original, next.as_ref());
    }
    let path = mcp_path(client, user);
    let before = read_config(&path)?;
    if config_mcp(client, before.as_deref())?.get(name) != original {
        return Err("MCP 配置刚刚发生变化，请刷新后重试。".into());
    }
    let entries = next
        .into_iter()
        .map(|definition| (name.to_owned(), definition))
        .collect();
    let after = put_mcp(client, before.as_deref(), Some(name), entries)?;
    Ok(vec![Change::new(path, before, after)])
}

fn prompt_parts(text: &str) -> Result<(&str, &str, &str), String> {
    let start = text.match_indices(PROMPT_START).collect::<Vec<_>>();
    let end = text.match_indices(PROMPT_END).collect::<Vec<_>>();
    match (start.as_slice(), end.as_slice()) {
        ([], []) => Ok((text, "", "")),
        ([(start, _)], [(end, _)]) if start + PROMPT_START.len() <= *end => Ok((
            &text[..*start],
            text[start + PROMPT_START.len()..*end].trim_matches(['\r', '\n']),
            &text[end + PROMPT_END.len()..],
        )),
        _ => Err("现有提示词的桌宠标记不完整，请先检查原文件。".into()),
    }
}

fn prompt_content(bytes: Option<&[u8]>) -> Result<&str, String> {
    let text =
        std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| "提示词文件不是 UTF-8。")?;
    Ok(prompt_parts(text)?.1)
}

pub(super) fn prompt_view(client: Client, user: &Path) -> Result<Value, String> {
    let path = client.prompt(user);
    let content = read_config(&path)?;
    Ok(json!({"text": prompt_content(content.as_deref())?, "path": path}))
}

pub(super) fn prompt_plan(client: Client, user: &Path, text: &str) -> Result<Vec<Change>, String> {
    if text.len() > 48 * 1024 || text.contains(PROMPT_START) || text.contains(PROMPT_END) {
        return Err("提示词过长或含保留标记。".into());
    }
    let path = client.prompt(user);
    let before = read_config(&path)?;
    let source = std::str::from_utf8(before.as_deref().unwrap_or_default())
        .map_err(|_| "提示词文件不是 UTF-8。")?;
    let (prefix, _, suffix) = prompt_parts(source)?;
    let mut next = prefix.to_owned();
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if !text.trim().is_empty() {
        if !next.is_empty() && !next.ends_with('\n') {
            next.push_str(newline);
        }
        next.push_str(&format!(
            "{PROMPT_START}{newline}{}{newline}{PROMPT_END}",
            text.trim_matches(['\r', '\n'])
        ));
        if suffix.is_empty() {
            next.push_str(newline);
        }
    }
    next.push_str(suffix);
    if before.is_none() && next.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Change::new(path, before, next.into_bytes())])
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Skill {
    id: String,
    name: String,
    source: String,
    enabled: bool,
    #[serde(skip)]
    path: PathBuf,
}

fn folder_entries(path: &Path) -> Vec<fs::DirEntry> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .collect()
}

fn normal_dirs(path: &Path) -> impl Iterator<Item = fs::DirEntry> {
    folder_entries(path).into_iter().filter(|entry| {
        entry
            .path()
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir() && !is_link(&meta))
    })
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn skill_at(root: &Path, source: &str) -> Vec<Skill> {
    normal_dirs(root)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            let present = |file| {
                path.join(file)
                    .symlink_metadata()
                    .is_ok_and(|m| m.is_file())
            };
            let enabled = present(SKILL_FILE);
            if !enabled && !present(DISABLED_FILE) {
                return None;
            }
            Some(Skill {
                id: digest(path.to_string_lossy().as_bytes()),
                name,
                source: source.to_owned(),
                enabled,
                path,
            })
        })
        .collect()
}

fn skill_root(client: Client, user: &Path) -> PathBuf {
    match client {
        Client::Agy => user.join(".gemini/config/skills"),
        Client::ClaudeDesktop => user.join(".claude-desktop/skills"),
        _ => client.home(user).join("skills"),
    }
}

fn local_skills(user: &Path, library: &Path) -> Vec<Skill> {
    let mut skills = skill_at(library, "penguinpal");
    skills.extend(skill_at(&user.join(".agents/skills"), "agents"));
    for app in Client::ALL {
        skills.extend(skill_at(&skill_root(app, user), app.id()));
    }
    skills.retain(|skill| skill.enabled);
    skills.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.source.cmp(&b.source))
    });
    skills
}

pub(super) fn skills(client: Client, user: &Path, library: &Path) -> Value {
    json!({
        "installed": skill_at(&skill_root(client, user), client.id()),
        "library": local_skills(user, library)
    })
}

fn copy_skill(
    source: &Path,
    target: &Path,
    changes: &mut Vec<Change>,
    bytes: &mut usize,
    depth: u8,
) -> Result<(), String> {
    if depth > 6 {
        return Err("Skill 子目录超过支持深度。".into());
    }
    for entry in fs::read_dir(source).map_err(|_| "无法读取 Skill 目录。")? {
        let entry = entry.map_err(|_| "无法读取 Skill 文件。")?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let meta = path
            .symlink_metadata()
            .map_err(|_| "无法读取 Skill 文件类型。")?;
        if is_link(&meta) {
            return Err("Skill 包含链接或特殊文件，无法导入。".into());
        }
        if meta.is_dir() {
            copy_skill(&path, &target.join(name), changes, bytes, depth + 1)?;
        } else if meta.is_file() {
            let value = read_config(&path)?.ok_or("Skill 在复制过程中发生变化。")?;
            *bytes += value.len();
            if changes.len() >= 100 || *bytes > 2 * 1024 * 1024 {
                return Err("Skill 超过本地导入限制。".into());
            }
            changes.push(Change::new(target.join(name), None, value));
        } else {
            return Err("Skill 包含链接或特殊文件，无法导入。".into());
        }
    }
    Ok(())
}

pub(super) fn install_skill(
    client: Client,
    user: &Path,
    source: &Path,
) -> Result<Vec<Change>, String> {
    if !source.symlink_metadata().is_ok_and(|m| m.is_dir())
        || !source
            .join(SKILL_FILE)
            .symlink_metadata()
            .is_ok_and(|m| m.is_file())
    {
        return Err("请选择包含 SKILL.md 的普通目录。".into());
    }
    let name = source.file_name().ok_or("Skill 目录名称无效。")?;
    if name.to_string_lossy().starts_with('.') {
        return Err("Skill 目录名称无效。".into());
    }
    plan_skill_copy(source, &skill_root(client, user).join(name))
}

fn plan_skill_copy(source: &Path, target: &Path) -> Result<Vec<Change>, String> {
    if !source
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_dir() && !is_link(&meta))
        || !source
            .join(SKILL_FILE)
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_file())
    {
        return Err("此 Skill 已移动或缺少 SKILL.md。".into());
    }
    for ancestor in target.ancestors() {
        match ancestor.symlink_metadata() {
            Ok(meta) if meta.is_dir() && !is_link(&meta) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("目标 Skill 路径包含链接或无法访问的目录。".into()),
        }
    }
    if let Ok(meta) = target.symlink_metadata() {
        if target
            .canonicalize()
            .ok()
            .is_some_and(|resolved| source.canonicalize().ok() == Some(resolved))
        {
            return Ok(Vec::new());
        }
        let empty = meta.is_dir()
            && fs::read_dir(&target).is_ok_and(|mut entries| entries.next().is_none());
        if !empty {
            let mut expected = Vec::new();
            copy_skill(source, target, &mut expected, &mut 0, 0)?;
            if !expected.is_empty()
                && expected.iter().all(|change| {
                    change
                        .path
                        .ancestors()
                        .take_while(|path| *path != target)
                        .all(|path| path.symlink_metadata().is_ok_and(|meta| !is_link(&meta)))
                        && read_config(&change.path).ok().flatten() == change.after
                })
            {
                return Ok(Vec::new());
            }
            return Err(format!("{} 已有内容不同的同名 Skill。", target.display()));
        }
    }
    let mut changes = Vec::new();
    copy_skill(source, target, &mut changes, &mut 0, 0)?;
    if changes.is_empty() {
        return Err("Skill 文件为空。".into());
    }
    // Create the entry file last so readers cannot discover a partially installed skill.
    changes.sort_by_key(|c| c.path.file_name().is_some_and(|name| name == SKILL_FILE));
    Ok(changes)
}

pub(super) fn library_skills(
    user: &Path,
    library: &Path,
    ids: &[String],
    targets: &[Client],
) -> Result<Vec<Change>, String> {
    if ids.is_empty() || ids.len() > 50 || targets.len() > Client::ALL.len() {
        return Err("请选择 1 至 50 个本机 Skill。".into());
    }
    let candidates = local_skills(user, library);
    let mut names = HashSet::new();
    let mut directories = HashSet::new();
    let mut changes = Vec::new();
    let mut bytes = 0;
    for id in ids {
        let skill = candidates
            .iter()
            .find(|skill| skill.id == *id)
            .ok_or("所选 Skill 已移动，请重新扫描。")?;
        if !names.insert(skill.name.to_lowercase()) {
            return Err("多个来源包含同名 Skill，每个名称请选择一个来源。".into());
        }
        for root in std::iter::once(library.to_path_buf())
            .chain(targets.iter().map(|client| skill_root(*client, user)))
        {
            let target = root.join(&skill.name);
            let identity = root
                .canonicalize()
                .unwrap_or(root)
                .join(skill.name.to_lowercase());
            if !directories.insert(identity) {
                continue;
            }
            for change in plan_skill_copy(&skill.path, &target)? {
                bytes += change.after.as_ref().map_or(0, Vec::len);
                changes.push(change);
                if bytes > 4 * 1024 * 1024 || changes.len() > 500 {
                    return Err("本批 Skill 文件超过容量限制，请分批导入。".into());
                }
            }
        }
    }
    // Publish each Skill entry only after all its support files have been copied.
    changes.sort_by_key(|change| {
        change
            .path
            .file_name()
            .is_some_and(|name| name == SKILL_FILE)
    });
    Ok(changes)
}

fn skill_state(skill: &Skill, enabled: bool) -> Result<Vec<Change>, String> {
    if skill.enabled == enabled {
        return Ok(Vec::new());
    }
    let (read, write) = if enabled {
        (DISABLED_FILE, SKILL_FILE)
    } else {
        (SKILL_FILE, DISABLED_FILE)
    };
    let from = skill.path.join(read);
    let to = skill.path.join(write);
    let bytes = read_config(&from)?.ok_or("Skill 状态已改变，请刷新。")?;
    if read_config(&to)?.is_some() {
        return Err("发现两个同名入口文件，已停止操作。".into());
    }
    Ok(vec![
        Change::new(to, None, bytes.clone()),
        Change {
            path: from,
            before: Some(bytes),
            after: None,
        },
    ])
}

pub(super) fn toggle_skill(
    client: Client,
    user: &Path,
    id: &str,
    enabled: bool,
) -> Result<Vec<Change>, String> {
    let skill = skill_at(&skill_root(client, user), client.id())
        .into_iter()
        .find(|skill| skill.id == id)
        .ok_or("Skill 已从本机移除。")?;
    skill_state(&skill, enabled)
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct ExtensionProfile {
    mcp: Map<String, Value>,
    prompt: String,
    skills: BTreeMap<String, bool>,
}

impl ExtensionProfile {
    pub fn snapshot(client: Client, user: &Path) -> Result<Self, String> {
        let mcp = config_mcp(client, read_config(&mcp_path(client, user))?.as_deref())?;
        let prompt = prompt_content(read_config(&client.prompt(user))?.as_deref())?.to_owned();
        let skills = skill_at(&skill_root(client, user), client.id())
            .into_iter()
            .map(|skill| (skill.name, skill.enabled))
            .collect();
        Ok(Self {
            mcp,
            prompt,
            skills,
        })
    }

    pub fn plan(
        &self,
        client: Client,
        user: &Path,
        changes: &mut Vec<Change>,
    ) -> Result<(), String> {
        let path = mcp_path(client, user);
        let before = read_config(&path)?;
        changes.extend(prompt_plan(client, user, &self.prompt)?);
        if let Some(change) = changes.iter_mut().find(|change| change.path == path) {
            if config_mcp(client, change.after.as_deref())? != self.mcp {
                change.after = Some(put_mcp(
                    client,
                    change.after.as_deref(),
                    None,
                    self.mcp.clone(),
                )?);
            }
        } else if config_mcp(client, before.as_deref())? != self.mcp {
            changes.push(Change::new(
                path,
                before.clone(),
                put_mcp(client, before.as_deref(), None, self.mcp.clone())?,
            ));
        }
        for skill in skill_at(&skill_root(client, user), client.id()) {
            if let Some(enabled) = self.skills.get(&skill.name) {
                changes.extend(skill_state(&skill, *enabled)?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_edits_only_the_managed_block() {
        let text = format!("existing rule\r\n{PROMPT_START}\r\nmine\r\n{PROMPT_END}\r\nmore rules");
        assert_eq!(
            prompt_parts(&text).unwrap(),
            ("existing rule\r\n", "mine", "\r\nmore rules")
        );
        assert!(prompt_parts(PROMPT_START).is_err());
    }

    #[test]
    fn adding_mcp_keeps_provider_and_user_tables() {
        let existing = b"model = 'chosen'\n[mcp_servers.other]\ncommand = 'my-command'\n";
        let new = json!({"new": {"command":"node", "args":["server.js"]}});
        let bytes = put_mcp(
            Client::Codex,
            Some(existing),
            Some("new"),
            new.as_object().unwrap().clone(),
        )
        .unwrap();
        let result = String::from_utf8(bytes).unwrap();
        assert!(result.contains("model = 'chosen'"));
        assert!(result.contains("[mcp_servers.other]"));
        assert!(result.contains("[mcp_servers.new]"));
        assert!(config_mcp(Client::Codex, Some(&result.into_bytes()))
            .unwrap()
            .contains_key("new"));
    }

    #[test]
    fn native_mcp_transports_use_client_fields() {
        let http = json!({"type":"http","url":"https://fixture.example/mcp","headers":{"Fixture":"public"}});
        let codex = native_mcp(Client::Codex, &http, None).unwrap();
        assert_eq!(codex["http_headers"]["Fixture"], "public");
        assert!(codex.get("type").is_none());
        let gemini = native_mcp(Client::Gemini, &http, None).unwrap();
        assert_eq!(gemini["httpUrl"], http["url"]);
        assert!(gemini.get("url").is_none());
        assert_eq!(
            native_mcp(Client::Dsh, &http, None).unwrap()["transport"],
            "streamable-http"
        );
        assert_eq!(
            native_mcp(Client::Workbuddy, &http, None).unwrap()["type"],
            "http"
        );
        assert!(native_mcp(
            Client::Dsh,
            &json!({"type":"sse","url":"https://fixture.example/sse"}),
            None
        )
        .is_err());
        let local = json!({"type":"stdio","command":"fixture","args":["--mcp"],"env":{"Example":"synthetic"}});
        let opencode = native_mcp(Client::Opencode, &local, None).unwrap();
        assert_eq!(opencode["command"], json!(["fixture", "--mcp"]));
        assert_eq!(opencode["environment"]["Example"], "synthetic");
    }

    #[test]
    fn moving_mcp_endpoint_does_not_forward_existing_credentials() {
        let prior = json!({"type":"http","url":"https://first.example/mcp","headers":{"Authorization":"fixture"},"timeout":1000});
        let same = json!({"type":"http","url":"https://first.example/mcp"});
        let different = json!({"type":"http","url":"https://second.example/mcp"});
        assert_eq!(
            native_mcp(Client::Workbuddy, &same, Some(&prior)).unwrap()["headers"],
            prior["headers"]
        );
        let moved = native_mcp(Client::Workbuddy, &different, Some(&prior)).unwrap();
        assert!(moved.get("headers").is_none());
        assert_eq!(moved["timeout"], 1000);
    }

    #[test]
    fn batch_skill_import_keeps_source_and_checks_name_conflicts() {
        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path().join("user");
        let library = fixture.path().join("library");
        let shared = user.join(".agents/skills/example");
        let target = skill_root(Client::Dsh, &user).join("example");
        assert!(target.starts_with(&user));
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join(SKILL_FILE), b"synthetic skill").unwrap();
        let id = local_skills(&user, &library)
            .into_iter()
            .find(|s| s.name == "example")
            .unwrap()
            .id;
        let plan =
            library_skills(&user, &library, std::slice::from_ref(&id), &[Client::Dsh]).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(
            fs::read(shared.join(SKILL_FILE)).unwrap(),
            b"synthetic skill"
        );
        assert!(plan
            .iter()
            .any(|change| change.path == library.join("example").join(SKILL_FILE)));
        assert!(plan
            .iter()
            .any(|change| change.path == target.join(SKILL_FILE)));
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join(SKILL_FILE), b"existing skill").unwrap();
        assert!(library_skills(&user, &library, &[id], &[Client::Dsh]).is_err());
        assert_eq!(
            fs::read(target.join(SKILL_FILE)).unwrap(),
            b"existing skill"
        );
    }
}
