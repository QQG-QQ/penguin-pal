use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    providers::{json_bytes, json_object, toml, Client},
    sessions,
    storage::{self, load_private, read_config, save_private, Change},
};

const DSH_GRANT: &str = "deepseek-account-platform/default";
const WB_INFO: &str = "workbuddy-desktop.info";

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    client: Client,
    label: String,
    identity: Option<String>,
    parts: BTreeMap<String, Vec<u8>>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    accounts: Vec<Account>,
}

#[derive(Default)]
struct Live {
    identity: Option<String>,
    parts: BTreeMap<String, Vec<u8>>,
}

fn account_book(root: &Path) -> Result<Book, String> {
    Ok(load_private(&root.join("accounts.bin"))?.unwrap_or_default())
}

pub(super) fn can_capture(client: Client) -> bool {
    matches!(
        client,
        Client::Codex
            | Client::Claude
            | Client::Grokbuild
            | Client::Dsh
            | Client::Workbuddy
            | Client::Kimi
    )
}

pub(super) fn can_launch(client: Client) -> bool {
    matches!(
        client,
        Client::Codex
            | Client::Claude
            | Client::Gemini
            | Client::Dsh
            | Client::Workbuddy
            | Client::Copilot
            | Client::Grokbuild
            | Client::Kimi
            | Client::Agy
            | Client::ClaudeDesktop
            | Client::Opencode
    )
}

fn credential_file(client: Client, user: &Path) -> PathBuf {
    client.home(user).join(match client {
        Client::Claude => ".credentials.json",
        Client::Dsh => ".credentials.yaml",
        _ => "auth.json",
    })
}

fn workbuddy_dir(user: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| user.join("AppData/Local"))
        .join("CodeBuddyExtension/Data/Public/auth")
}

fn json_doc(bytes: &[u8]) -> Result<Value, String> {
    json_object(Some(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)))
}

fn jwt_subject(value: &str) -> Option<String> {
    use base64::Engine;
    let payload = value.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claim: Value = serde_json::from_slice(&bytes).ok()?;
    claim["sub"].as_str().map(str::to_owned)
}

fn token(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| !value.trim().is_empty())
}

fn codex_file_store(config: &toml_edit::DocumentMut) -> bool {
    config
        .get("cli_auth_credentials_store")
        .and_then(toml_edit::Item::as_str)
        .is_none_or(|mode| mode == "file")
}

fn ensure_codex_file_store(config: &toml_edit::DocumentMut) -> Result<(), String> {
    if codex_file_store(config) {
        Ok(())
    } else {
        Err("当前 Codex 使用其他凭据存储，请在 Codex 的账号菜单管理登录。".into())
    }
}

fn codex_live(bytes: Vec<u8>) -> Result<Live, String> {
    let doc = json_doc(&bytes)?;
    let credentials = &doc["tokens"];
    if doc["auth_mode"] == "apikey" || token(&credentials["access_token"]).is_none() {
        return Ok(Live::default());
    }
    let account = token(&credentials["account_id"]);
    let identity = token(&credentials["id_token"])
        .and_then(jwt_subject)
        .map(|subject| format!("{subject}:{}", account.unwrap_or_default()))
        .or_else(|| account.map(str::to_owned));
    Ok(Live {
        identity,
        parts: BTreeMap::from([("primary".into(), bytes)]),
    })
}

fn reading(client: Client, user: &Path) -> Result<Live, String> {
    let mut live = Live::default();
    if client == Client::Codex {
        let bytes = read_config(&client.config(user))?;
        let config = toml(bytes.as_deref())?;
        if !codex_file_store(&config) {
            return Ok(live);
        }
        return read_config(&credential_file(client, user))?
            .map(codex_live)
            .transpose()
            .map(Option::unwrap_or_default);
    }
    if client == Client::Workbuddy {
        let dir = workbuddy_dir(user);
        if dir.join(format!("{WB_INFO}.logged-out")).exists() {
            return Ok(live);
        }
        let (Some(info), Some(key)) = (
            read_config(&dir.join(WB_INFO))?,
            read_config(&client.home(user).join("keyblob"))?,
        ) else {
            return Ok(live);
        };
        let doc = json_doc(&info)?;
        let Some(uid) = token(&doc["account"]["uid"])
            .map(str::to_owned)
            .or_else(|| doc["account"]["uid"].as_u64().map(|uid| uid.to_string()))
        else {
            return Ok(live);
        };
        if !doc["auth"].is_object() {
            return Ok(live);
        }
        live.identity = Some(uid);
        live.parts.insert("keyblob".into(), key);
        live.parts.insert(WB_INFO.into(), info);
        for entry in std::fs::read_dir(&dir).map_err(|_| "无法读取 WorkBuddy 登录目录。")?
        {
            let entry = entry.map_err(|_| "无法读取 WorkBuddy 登录记录。")?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if valid_observation(&name) {
                if let Some(bytes) = read_config(&entry.path())? {
                    live.parts.insert(name.into_owned(), bytes);
                }
            }
        }
        return Ok(live);
    }
    let path = if client == Client::Kimi {
        let Some(path) = kimi_credential(user)? else {
            return Ok(live);
        };
        path
    } else {
        credential_file(client, user)
    };
    let Some(bytes) = read_config(&path)? else {
        return Ok(live);
    };
    if client == Client::Dsh {
        let mut source: serde_yaml::Value =
            serde_yaml::from_slice(&bytes).map_err(|_| "DeepSeek 本机登录文件无法读取。")?;
        let records = source["records"].as_mapping_mut();
        let Some(grant) =
            records.and_then(|records| records.remove(serde_yaml::Value::from(DSH_GRANT)))
        else {
            return Ok(live);
        };
        let Some(secret) = grant["payload"]["token"]
            .as_str()
            .filter(|secret| !secret.is_empty())
        else {
            return Ok(live);
        };
        live.identity = jwt_subject(secret);
        live.parts.insert(
            "primary".into(),
            serde_yaml::to_string(&grant)
                .map_err(|_| "无法保存 DeepSeek 登录记录。")?
                .into_bytes(),
        );
        return Ok(live);
    }
    let doc = json_doc(&bytes)?;
    let identity = match client {
        Client::Claude => {
            if token(&doc["claudeAiOauth"]["accessToken"]).is_none() {
                return Ok(live);
            }
            let hints = read_config(&claude_settings(user))?
                .map(|bytes| json_doc(&bytes))
                .transpose()?;
            if let Some(oauth) = hints
                .as_ref()
                .and_then(|hint| hint.get("oauthAccount"))
                .filter(|hint| hint.is_object())
            {
                live.parts
                    .insert("claude-account".into(), json_bytes(oauth)?);
            }
            hints
                .as_ref()
                .and_then(|hint| token(&hint["oauthAccount"]["accountUuid"]))
                .map(str::to_owned)
        }
        Client::Grokbuild => {
            let native = &doc["https://accounts.x.ai/sign-in"];
            let access = token(&native["key"])
                .or_else(|| token(&doc["access_token"]))
                .or_else(|| token(&doc["accessToken"]))
                .or_else(|| token(&doc["token"]));
            let Some(access) = access else {
                return Ok(live);
            };
            token(&native["id_token"])
                .or_else(|| token(&doc["id_token"]))
                .or_else(|| token(&doc["idToken"]))
                .and_then(jwt_subject)
                .or_else(|| jwt_subject(access))
        }
        Client::Kimi => {
            let Some(access) = token(&doc["access_token"]) else {
                return Ok(live);
            };
            jwt_subject(access)
        }
        _ => return Err("此客户端通过原生入口管理登录。".into()),
    };
    live.identity = identity;
    live.parts.insert("primary".into(), bytes);
    Ok(live)
}

fn kimi_credential(user: &Path) -> Result<Option<PathBuf>, String> {
    let bytes = read_config(&Client::Kimi.config(user))?;
    let config = toml(bytes.as_deref())?;
    let Some(providers) = config
        .get("providers")
        .and_then(toml_edit::Item::as_table_like)
    else {
        return Ok(None);
    };
    let active = config
        .get("default_model")
        .and_then(toml_edit::Item::as_str)
        .and_then(|model| config.get("models")?.get(model)?.get("provider")?.as_str());
    let oauth = active
        .and_then(|name| providers.get(name)?.get("oauth"))
        .or_else(|| providers.get("kimi-code")?.get("oauth"));
    let Some(oauth) = oauth else {
        return Ok(None);
    };
    if oauth
        .get("storage")
        .and_then(toml_edit::Item::as_str)
        .is_some_and(|mode| mode != "file")
    {
        return Ok(None);
    }
    let Some(name) = oauth.get("key").and_then(toml_edit::Item::as_str) else {
        return Ok(None);
    };
    let name = name.strip_prefix("oauth/").unwrap_or(name);
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("Kimi 官方账号凭据名称无效。".into());
    }
    Ok(Some(
        Client::Kimi
            .home(user)
            .join("credentials")
            .join(format!("{name}.json")),
    ))
}

fn claude_settings(user: &Path) -> PathBuf {
    let home = Client::Claude.home(user);
    if home == user.join(".claude") {
        user.join(".claude.json")
    } else {
        home.join(".claude.json")
    }
}

fn valid_observation(name: &str) -> bool {
    name.strip_prefix("workbuddy-desktop.info.")
        .and_then(|rest| rest.strip_suffix(".observation.json"))
        .is_some_and(|hash| !hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn same(account: &Account, live: &Live) -> bool {
    if let (Some(saved), Some(current)) = (&account.identity, &live.identity) {
        return saved == current;
    }
    account
        .parts
        .get("primary")
        .zip(live.parts.get("primary"))
        .is_some_and(|(a, b)| a == b)
}

fn replace_live(
    book: &mut Book,
    client: Client,
    live: Live,
    label: Option<&str>,
) -> Result<(), String> {
    if live.parts.is_empty() {
        return Err("尚未找到官方登录。请在原生登录窗口完成授权，再回桌宠收录。".into());
    }
    if let Some(existing) = book
        .accounts
        .iter_mut()
        .find(|account| account.client == client && same(account, &live))
    {
        existing.parts = live.parts;
        existing.identity = live.identity;
        if let Some(label) = label {
            existing.label = label.into();
        }
        return Ok(());
    }
    if book.accounts.len() >= 100 {
        return Err("已保存的官方账号达到上限。".into());
    }
    book.accounts.push(Account {
        id: uuid::Uuid::new_v4().simple().to_string(),
        client,
        label: label.unwrap_or("切换前的本机账号").into(),
        identity: live.identity,
        parts: live.parts,
    });
    Ok(())
}

pub(super) fn status(root: &Path, user: &Path, client: Client) -> Result<Value, String> {
    let supported = if client == Client::Codex {
        codex_file_store(&toml(read_config(&client.config(user))?.as_deref())?)
    } else {
        can_capture(client)
    };
    let live = if supported {
        Some(reading(client, user)?)
    } else {
        None
    };
    let book = account_book(root)?;
    let accounts: Vec<_> = book.accounts.iter().filter(|entry| entry.client == client)
        .map(|entry| json!({"id":entry.id,"label":entry.label,"active":live.as_ref().is_some_and(|live| same(entry,live))})).collect();
    Ok(
        json!({"canLaunch":can_launch(client),"canCapture":supported,
        "loginNote":match client {
            Client::Codex if !supported => "当前 Codex 使用其他凭据存储，请在 Codex 的账号菜单管理登录。",
            Client::Gemini => "账号在 Gemini CLI 的 /auth 内管理。",
            Client::Copilot => "登录后，在 Copilot CLI 用 /user switch 切换已保存的 GitHub 账号。",
            Client::Kimi => "桌宠发起 Kimi Code 设备码登录。",
            _ => "",
        },
        "hasLive":live.is_some_and(|live| !live.parts.is_empty()),"accounts":accounts}),
    )
}

pub(super) fn capture(root: &Path, user: &Path, client: Client, label: &str) -> Result<(), String> {
    if !can_capture(client) {
        return Err("此客户端通过原生入口管理登录。".into());
    }
    let label = account_label(label)?;
    let mut book = account_book(root)?;
    replace_live(&mut book, client, reading(client, user)?, Some(label))?;
    save_private(&root.join("accounts.bin"), &book)
}

fn account_label(label: &str) -> Result<&str, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
        return Err("请填写 1 至 64 字的账号备注。".into());
    }
    Ok(label)
}

fn same_live_revision(a: &Live, b: &Live) -> bool {
    a.identity == b.identity && a.parts == b.parts
}

pub(super) fn remove(root: &Path, client: Client, id: &str) -> Result<(), String> {
    let mut book = account_book(root)?;
    book.accounts
        .retain(|account| !(account.client == client && account.id == id));
    save_private(&root.join("accounts.bin"), &book)
}

fn change(path: PathBuf, after: Option<Vec<u8>>) -> Result<Change, String> {
    let before = read_config(&path)?;
    Ok(Change {
        path,
        before,
        after,
    })
}

fn switch_changes(user: &Path, account: &Account) -> Result<Vec<Change>, String> {
    let client = account.client;
    if client == Client::Workbuddy {
        let dir = workbuddy_dir(user);
        if dir.join(".workbuddy-desktop.info.workbuddy.lock").exists() {
            return Err("WorkBuddy 登录文件正在使用，请退出 WorkBuddy 后再切换。".into());
        }
        let info = account
            .parts
            .get(WB_INFO)
            .ok_or("WorkBuddy 账号副本不完整。")?;
        let key = account
            .parts
            .get("keyblob")
            .ok_or("WorkBuddy 账号密钥包缺失。")?;
        let mut changes = vec![
            change(client.home(user).join("keyblob"), Some(key.clone()))?,
            change(dir.join(WB_INFO), Some(info.clone()))?,
            change(dir.join(format!("{WB_INFO}.logged-out")), None)?,
        ];
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).map_err(|_| "无法读取 WorkBuddy 登录目录。")?
            {
                let entry = entry.map_err(|_| "无法读取 WorkBuddy 登录路径。")?;
                let name = entry.file_name();
                if valid_observation(&name.to_string_lossy())
                    && !account.parts.contains_key(&*name.to_string_lossy())
                {
                    changes.push(change(entry.path(), None)?);
                }
            }
        }
        for (name, bytes) in account
            .parts
            .iter()
            .filter(|(name, _)| valid_observation(name))
        {
            changes.push(change(dir.join(name), Some(bytes.clone()))?);
        }
        return Ok(changes);
    }
    let primary = account.parts.get("primary").ok_or("官方账号副本不完整。")?;
    let path = if client == Client::Kimi {
        kimi_credential(user)?.ok_or("请先在 Kimi Code 中选择官方登录配置。")?
    } else {
        credential_file(client, user)
    };
    if client == Client::Dsh {
        let before = read_config(&path)?;
        let mut document = if let Some(bytes) = before.as_deref() {
            serde_yaml::from_slice(bytes).map_err(|_| "DeepSeek 登录文件格式无效。")?
        } else {
            serde_yaml::to_value(json!({"version":1,"records":{}})).unwrap()
        };
        let records = document["records"]
            .as_mapping_mut()
            .ok_or("DeepSeek 登录记录格式无效。")?;
        records.insert(
            serde_yaml::Value::from(DSH_GRANT),
            serde_yaml::from_slice(primary).map_err(|_| "DeepSeek 账号副本无效。")?,
        );
        let after = serde_yaml::to_string(&document).map_err(|_| "无法生成 DeepSeek 登录配置。")?;
        return Ok(vec![Change::new(path, before, after.into_bytes())]);
    }
    let mut changes = vec![change(path, Some(primary.clone()))?];
    if client == Client::Codex {
        let path = client.config(user);
        let before = read_config(&path)?;
        let mut config = toml(before.as_deref())?;
        ensure_codex_file_store(&config)?;
        config["cli_auth_credentials_store"] = toml_edit::value("file");
        changes.push(Change::new(path, before, config.to_string().into_bytes()));
    } else if client == Client::Claude {
        let path = claude_settings(user);
        let before = read_config(&path)?;
        let mut config = json_object(before.as_deref())?;
        if let Some(identity) = account.parts.get("claude-account") {
            config["oauthAccount"] = json_doc(identity)?;
        } else if let Some(fields) = config.as_object_mut() {
            fields.remove("oauthAccount");
        }
        changes.push(Change::new(path, before, json_bytes(&config)?));
    }
    Ok(changes)
}

pub(super) fn plan(
    root: &Path,
    user: &Path,
    client: Client,
    id: &str,
) -> Result<(String, Vec<Change>), String> {
    let book = account_book(root)?;
    let account = book
        .accounts
        .iter()
        .find(|entry| entry.client == client && entry.id == id)
        .ok_or("此官方账号已不存在。")?;
    if same(account, &reading(client, user)?) {
        return Ok((
            format!("{} · {}", client.label(), account.label),
            Vec::new(),
        ));
    }
    ensure_stopped(client)?;
    Ok((
        format!("切换 {} 账号 · {}", client.label(), account.label),
        storage::changed(switch_changes(user, account)?),
    ))
}

pub(super) fn ensure_stopped(client: Client) -> Result<(), String> {
    if client_running(client)? {
        return Err(format!("请退出 {}，再更改它的登录。", client.label()));
    }
    Ok(())
}

pub(super) fn credential_client(path: &Path) -> Option<Client> {
    let name = path.file_name()?.to_str()?;
    match name {
        name if name == WB_INFO
            || name.starts_with("workbuddy-desktop.info.")
            || name == "keyblob" =>
        {
            Some(Client::Workbuddy)
        }
        ".credentials.yaml" => Some(Client::Dsh),
        ".credentials.json" => Some(Client::Claude),
        "auth.json" => {
            let path = path.parent()?;
            let user = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(PathBuf::from)?;
            [Client::Codex, Client::Grokbuild]
                .into_iter()
                .find(|client| client.home(&user) == path)
        }
        _ => None,
    }
}

pub(super) fn preserve_current(root: &Path, user: &Path, client: Client) -> Result<(), String> {
    let live = reading(client, user)?;
    if !live.parts.is_empty() {
        if !same_live_revision(&live, &reading(client, user)?) {
            return Err("客户端正在更新登录，请稍后再试。".into());
        }
        let mut book = account_book(root)?;
        replace_live(&mut book, client, live, None)?;
        save_private(&root.join("accounts.bin"), &book)?;
    }
    Ok(())
}

pub(super) struct CodexLogin {
    child: Child,
    home: tempfile::TempDir,
    label: String,
}

impl CodexLogin {
    pub(super) fn start(
        root: &Path,
        user: &Path,
        label: &str,
        device: bool,
    ) -> Result<Self, String> {
        let label = account_label(label)?.to_owned();
        let config = toml(read_config(&Client::Codex.config(user))?.as_deref())?;
        ensure_codex_file_store(&config)?;
        if config
            .get("forced_login_method")
            .and_then(toml_edit::Item::as_str)
            == Some("api")
        {
            return Err("当前 Codex 配置只允许 API 登录。".into());
        }
        let executable =
            sessions::executable(Client::Codex).ok_or("未找到 Codex，请先安装并刷新。")?;
        std::fs::create_dir_all(root).map_err(|_| "无法准备官方登录目录。")?;
        let home = tempfile::Builder::new()
            .prefix("codex-login-")
            .tempdir_in(root)
            .map_err(|_| "无法准备官方登录目录。")?;
        let mut policy = toml_edit::DocumentMut::new();
        if let Some(workspaces) = config.get("forced_chatgpt_workspace_id") {
            policy["forced_chatgpt_workspace_id"] = workspaces.clone();
        }
        std::fs::write(home.path().join("config.toml"), policy.to_string())
            .map_err(|_| "无法准备官方登录配置。")?;
        preserve_current(root, user, Client::Codex)?;
        let mut command = Command::new(executable);
        command
            .current_dir(home.path())
            .env("CODEX_HOME", home.path())
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .args([
                "login",
                "-c",
                "chatgpt_base_url=\"https://chatgpt.com/backend-api/\"",
                "-c",
                "cli_auth_credentials_store=\"file\"",
            ]);
        if device {
            command.arg("--device-auth");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x10);
        }
        let child = command.spawn().map_err(|_| "无法启动 Codex 官方登录。")?;
        Ok(Self { child, home, label })
    }

    pub(super) fn wait(&mut self, canceled: &AtomicBool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(15 * 60);
        while !canceled.load(Ordering::Relaxed) && Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|_| "无法读取 Codex 登录结果。")?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err("Codex 授权未完成，请查看登录窗口并重试。".into())
                };
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        Err(if canceled.load(Ordering::Relaxed) {
            "已取消添加账号。"
        } else {
            "Codex 授权已超时，请重试。"
        }
        .into())
    }

    pub(super) fn save(&self, root: &Path) -> Result<bool, String> {
        let bytes = read_config(&self.home.path().join("auth.json"))?
            .ok_or("Codex 未生成官方登录记录，请重试。")?;
        let mut book = account_book(root)?;
        let live = codex_live(bytes)?;
        let existing = book
            .accounts
            .iter()
            .any(|account| account.client == Client::Codex && same(account, &live));
        let label = (!existing).then_some(self.label.as_str());
        replace_live(&mut book, Client::Codex, live, label)?;
        save_private(&root.join("accounts.bin"), &book)?;
        Ok(!existing)
    }
}

impl Drop for CodexLogin {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() && self.child.kill().is_ok() {
            let _ = self.child.wait();
        }
    }
}

pub(super) fn launch(root: &Path, user: &Path, client: Client) -> Result<String, String> {
    let desktop_name = match client {
        Client::Dsh => Some(("DeepSeek Harness", "DeepSeek Harness.exe")),
        Client::Workbuddy => Some(("WorkBuddy", "WorkBuddy.exe")),
        Client::ClaudeDesktop => Some(("Claude", "Claude.exe")),
        _ => None,
    };
    let desktop = desktop_name.and_then(|(label, exe)| installed_desktop(label, exe));
    let is_desktop = desktop.is_some();
    let path = desktop
        .or_else(|| sessions::executable(client))
        .ok_or("未找到客户端，请先安装并刷新。")?;
    let mut command = Command::new(path);
    let login_dir = root.join("login");
    std::fs::create_dir_all(&login_dir).map_err(|_| "无法准备官方登录目录。")?;
    command.current_dir(&login_dir);
    let message = match client {
        Client::Claude => {
            command
                .args(["auth", "login"])
                .env_remove("ANTHROPIC_API_KEY")
                .env_remove("ANTHROPIC_AUTH_TOKEN")
                .env_remove("ANTHROPIC_BASE_URL");
            "已发起 Claude 登录。完成后刷新，便可收录账号。"
        }
        Client::Grokbuild => {
            command.arg("login");
            "已发起 xAI 登录。完成后刷新，便可收录账号。"
        }
        Client::Copilot => {
            command.arg("login");
            "已发起 GitHub 登录。账号切换由 Copilot 的 /user switch 完成。"
        }
        Client::Dsh if !is_desktop => {
            command.arg("web");
            "已打开 DeepSeek Harness。请在客户端中登录，再回来收录账号。"
        }
        Client::Gemini => "已打开 Gemini CLI。请用 /auth 选择 Google 登录。",
        Client::Kimi => {
            command.arg("login");
            "已发起 Kimi Code 设备码登录，请按终端提示完成授权。"
        }
        Client::Dsh | Client::Workbuddy | Client::ClaudeDesktop => {
            "已打开官方客户端。请在客户端完成登录。"
        }
        Client::Agy => "已打开 Antigravity CLI，请按终端提示完成登录。",
        Client::Opencode => "已打开 OpenCode，请使用 /connect 选择账号或服务。",
        _ => return Err("此客户端没有桌宠可发起的官方登录流程。".into()),
    };
    if can_capture(client) {
        preserve_current(root, user, client)?;
    }
    #[cfg(windows)]
    if !is_desktop {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x10);
    }
    if is_desktop {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    command.spawn().map_err(|_| "无法启动官方客户端。")?;
    Ok(message.into())
}

#[cfg(windows)]
pub(super) fn installed_desktop(label: &str, exe: &str) -> Option<PathBuf> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
        RegKey,
    };
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let hive = RegKey::predef(hive);
        for base in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(branch) = hive.open_subkey(base) else {
                continue;
            };
            for key in branch.enum_keys().flatten() {
                let Ok(entry) = branch.open_subkey(key) else {
                    continue;
                };
                let Ok(name) = entry.get_value::<String, _>("DisplayName") else {
                    continue;
                };
                if name != label && !name.starts_with(&format!("{label} ")) {
                    continue;
                }
                let location = entry
                    .get_value::<String, _>("InstallLocation")
                    .ok()
                    .map(|p| PathBuf::from(p).join(exe));
                let icon = entry.get_value::<String, _>("DisplayIcon").ok().map(|p| {
                    PathBuf::from(
                        p.rsplit_once(',')
                            .map_or(p.as_str(), |(path, _)| path)
                            .trim()
                            .trim_matches('"'),
                    )
                });
                if let Some(path) = [location, icon].into_iter().flatten().find(|p| {
                    p.is_absolute()
                        && p.file_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case(exe))
                        && p.is_file()
                }) {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn installed_desktop(_: &str, _: &str) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn client_running(client: Client) -> Result<bool, String> {
    let names: &[&str] = match client {
        Client::Codex => &["codex.exe"],
        Client::Claude => &["claude.exe"],
        Client::Grokbuild => &["grok.exe"],
        Client::Kimi => &["kimi.exe"],
        Client::Dsh => &["dsh.exe", "DeepSeek Harness.exe"],
        Client::Workbuddy => &["WorkBuddy.exe"],
        Client::Agy => &["agy.exe"],
        _ => &[],
    };
    process_names_running(names)
}

#[cfg(windows)]
pub(super) fn process_names_running(names: &[&str]) -> Result<bool, String> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|_| "无法确认客户端是否已退出。")?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut result = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if names.iter().any(|name| exe.eq_ignore_ascii_case(name)) {
                    result = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        Ok(result)
    }
}

#[cfg(not(windows))]
fn client_running(_: Client) -> Result<bool, String> {
    Ok(false)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn codex_adds_another_official_account_before_switching() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path().join("user");
        let archive = fixture.path().join("ai-tools");
        let file = credential_file(Client::Codex, &user);
        assert!(file.starts_with(&user), "run with fixture CODEX_HOME unset");
        let credentials = |subject| {
            let claims = URL_SAFE_NO_PAD.encode(json!({"sub":subject}).to_string());
            json_bytes(&json!({"auth_mode":"chatgpt","tokens":{
                "id_token":format!("fixture.{claims}.unused"),
                "access_token":format!("fixture-access-{subject}"),
                "account_id":"fixture-shared-workspace"
            }}))
            .unwrap()
        };
        let first = credentials("first");
        let second = credentials("second");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &first).unwrap();
        capture(&archive, &user, Client::Codex, "账号甲").unwrap();

        let home = tempfile::tempdir().unwrap();
        let mut login = CodexLogin {
            child: Command::new("cmd.exe")
                .args(["/d", "/c", "exit", "0"])
                .spawn()
                .unwrap(),
            home,
            label: "账号乙".into(),
        };
        std::fs::write(login.home.path().join("auth.json"), &second).unwrap();
        login.wait(&AtomicBool::new(false)).unwrap();
        assert!(login.save(&archive).unwrap());
        login.label = "重复授权备注".into();
        assert!(!login.save(&archive).unwrap());
        let pending_home = login.home.path().to_owned();
        drop(login);

        assert!(!pending_home.exists());
        assert_eq!(std::fs::read(&file).unwrap(), first);
        let visible = status(&archive, &user, Client::Codex).unwrap();
        assert_eq!(visible["accounts"][0]["active"], true);
        assert_eq!(visible["accounts"][1]["active"], false);
        assert!(!visible.to_string().contains("fixture-access"));
        let book = account_book(&archive).unwrap();
        assert_eq!(book.accounts.len(), 2);
        assert_eq!(book.accounts[1].label, "账号乙");
        storage::commit(
            &archive,
            "fixture switch",
            switch_changes(&user, &book.accounts[1]).unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), second);
        assert_eq!(
            status(&archive, &user, Client::Codex).unwrap()["accounts"][1]["active"],
            true
        );
    }

    #[test]
    fn account_switch_changes_only_deepseek_account_grant() {
        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path().join("user");
        let archive = fixture.path().join("ai-tools");
        let file = credential_file(Client::Dsh, &user);
        assert!(file.starts_with(&user));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let write_grant = |token: &str, native_ref: &str| {
            let value = json!({"version":1,"records":{(DSH_GRANT):{"kind":"grant","payload":{"token":token}}},
                "refs":{"EXISTING":native_ref}});
            std::fs::write(&file, serde_yaml::to_string(&value).unwrap()).unwrap();
        };
        write_grant("fixture-grant-a", "old");
        capture(&archive, &user, Client::Dsh, "账号甲").unwrap();
        write_grant("fixture-grant-b", "kept");
        capture(&archive, &user, Client::Dsh, "账号乙").unwrap();
        let book = account_book(&archive).unwrap();
        assert_eq!(book.accounts.len(), 2);
        let first = book
            .accounts
            .iter()
            .find(|account| account.label == "账号甲")
            .unwrap();
        let changes = switch_changes(&user, first).unwrap();
        let backup = storage::commit(&archive, "fixture account switch", changes)
            .unwrap()
            .unwrap();
        let current: serde_yaml::Value =
            serde_yaml::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            current["records"][DSH_GRANT]["payload"]["token"].as_str(),
            Some("fixture-grant-a")
        );
        assert_eq!(current["refs"]["EXISTING"].as_str(), Some("kept"));
        let visible = status(&archive, &user, Client::Dsh).unwrap();
        assert_eq!(visible["accounts"][0]["active"], true);
        assert!(!visible.to_string().contains("fixture-grant-a"));
        storage::rollback(&archive, &backup, &user).unwrap();
        assert_eq!(
            status(&archive, &user, Client::Dsh).unwrap()["accounts"][1]["active"],
            true
        );
    }
}
