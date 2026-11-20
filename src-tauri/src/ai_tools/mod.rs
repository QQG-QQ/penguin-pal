mod accounts;
mod codex_models;
mod extensions;
mod mcp_yaml;
mod native_models;
mod providers;
mod sessions;
mod storage;

use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

use crate::{companion_chat::ChatService, companion_sync::Service};

use extensions::ExtensionProfile;
use providers::{Client, Provider};
use sessions::Session;
use storage::{load_private, save_private, Change};

const WINDOW: &str = "ai-tools";

#[derive(Clone)]
pub(crate) struct AiTools {
    root: Arc<PathBuf>,
    state: Arc<Mutex<WindowState>>,
}

impl AiTools {
    pub(crate) fn new(companion_root: &Path) -> Self {
        Self {
            root: Arc::new(companion_root.join("ai-tools")),
            state: Arc::default(),
        }
    }
}

#[derive(Default)]
struct WindowState {
    skill_source: Option<PathBuf>,
    sessions: HashMap<String, Session>,
    codex_login: Option<Arc<AtomicBool>>,
    codex_login_note: Option<String>,
}

impl WindowState {
    fn account_status(&self, root: &Path, user: &Path, client: Client) -> Result<Value, String> {
        let mut status = accounts::status(root, user, client)?;
        if client == Client::Codex {
            status["loginPending"] = json!(self.codex_login.is_some());
            if let Some(note) = &self.codex_login_note {
                status["loginNote"] = json!(note);
            }
        }
        Ok(status)
    }
}

impl AiTools {
    pub(crate) fn backup_block(&self, app: &AppHandle) -> Result<Option<String>, String> {
        self.backup_block_for(&user_home(app)?)
    }

    fn backup_block_for(&self, user: &Path) -> Result<Option<String>, String> {
        use base64::Engine;
        let _guard = self.state.lock().map_err(|_| "AI 工具状态忙。")?;
        let mut settings: Settings =
            load_private(&self.root.join("settings.bin"))?.unwrap_or_default();
        for client in Client::ALL {
            if let Some(provider) = providers::original(client, user, &settings.providers) {
                settings.providers.push(provider);
            }
        }
        if settings.providers.is_empty() && settings.profiles.is_empty() {
            return Ok(None);
        }
        let archive = SettingsArchive {
            version: 1,
            settings,
        };
        Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(storage::sealed(&archive)?),
        ))
    }

    pub(crate) fn restore_block(&self, archive: SettingsArchive) -> Result<(), String> {
        let _guard = self.state.lock().map_err(|_| "AI 工具状态忙。")?;
        let path = self.root.join("settings.bin");
        let mut settings: Settings = load_private(&path)?.unwrap_or_default();
        for provider in archive.settings.providers {
            if provider.switchable().is_ok()
                && settings.providers.len() < 501
                && !settings
                    .providers
                    .iter()
                    .any(|p| p.id == provider.id && p.client == provider.client)
            {
                settings.providers.push(provider);
            }
        }
        for profile in archive.settings.profiles {
            if settings.profiles.len() < 60
                && !settings.profiles.iter().any(|p| {
                    p.id == profile.id
                        || (p.provider.client == profile.provider.client && p.name == profile.name)
                })
            {
                settings.profiles.push(profile);
            }
        }
        save_private(&path, &settings)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn encrypted_backup_block_merges_ai_tool_settings() {
        let fixture = tempfile::tempdir().unwrap();
        let source = AiTools::new(&fixture.path().join("source"));
        let target = AiTools::new(&fixture.path().join("target"));
        let user = fixture.path().join("fixture-user");
        let mut original = Settings::default();
        original.providers.push(
            Provider::new_direct(
                Client::Codex,
                "Fixture",
                "https://fixture.example/v1",
                "fixture-key",
                "fixture-model",
            )
            .unwrap(),
        );
        let mut legacy = serde_json::to_value(original).unwrap();
        legacy["prices"] = json!({"codex:fixture":{"input":1.0,"output":2.0,"cached":0.1}});
        save_private(&source.root.join("settings.bin"), &legacy).unwrap();

        let block = source.backup_block_for(&user).unwrap().unwrap();
        assert!(!block.contains("fixture-key"));
        let archive = SettingsArchive::decode_block(&block).unwrap().unwrap();
        target.restore_block(archive).unwrap();
        let restored: Settings = load_private(&target.root.join("settings.bin"))
            .unwrap()
            .unwrap();
        assert!(restored
            .providers
            .iter()
            .any(|provider| provider.name == "Fixture"));
    }
}

impl SettingsArchive {
    pub(crate) fn decode_block(block: &str) -> Result<Option<Self>, String> {
        use base64::Engine;
        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(block)
            .map_err(|_| "AI 工具备份字段无效。")?;
        Self::decode_encrypted(&encrypted)
    }

    pub(crate) fn decode_encrypted(encrypted: &[u8]) -> Result<Option<Self>, String> {
        let Some(plain) = crate::companion_llm::dpapi::unprotect(encrypted) else {
            return Ok(None);
        };
        let archive: Self = serde_json::from_slice(&plain).map_err(|_| "AI 工具备份内容无效。")?;
        if archive.version != 1 {
            return Err("AI 工具的备份版本不受支持。".into());
        }
        Ok(Some(archive))
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    providers: Vec<Provider>,
    profiles: Vec<Profile>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SettingsArchive {
    version: u32,
    settings: Settings,
}

#[derive(Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
    provider: Provider,
    extensions: ExtensionProfile,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Operation {
    NativeModel {
        id: String,
    },
    CodexHistory {
        enabled: bool,
    },
    Provider {
        id: String,
    },
    Mcp {
        name: String,
        definition: Option<Value>,
        #[serde(default)]
        targets: Vec<Client>,
        #[serde(default)]
        replace: bool,
    },
    Prompt {
        text: String,
    },
    ImportSkills {
        ids: Vec<String>,
        targets: Vec<Client>,
    },
    LocalSkill,
    ToggleSkill {
        id: String,
        enabled: bool,
    },
    Profile {
        id: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum Request {
    Overview,
    QuickProviders,
    Switch {
        client: Client,
        operation: Operation,
    },
    RefreshCodexModels,
    AccountStatus {
        client: Client,
    },
    StartLogin {
        client: Client,
        #[serde(default)]
        device: bool,
        label: Option<String>,
    },
    CancelCodexLogin,
    CaptureAccount {
        client: Client,
        label: String,
    },
    PreviewAccount {
        client: Client,
        id: String,
    },
    ApplyAccount {
        client: Client,
        id: String,
        revision: String,
    },
    RemoveAccount {
        client: Client,
        id: String,
    },
    SaveDirect {
        client: Client,
        name: String,
        endpoint: String,
        key: String,
        #[serde(default)]
        model: String,
    },
    SetDirectModel {
        client: Client,
        id: String,
        model: String,
    },
    Inspect {
        client: Client,
    },
    ReadMcp {
        client: Client,
        name: String,
    },
    Preview {
        client: Client,
        operation: Operation,
    },
    Apply {
        client: Client,
        operation: Operation,
        revision: String,
    },
    TestProvider {
        client: Client,
        id: String,
    },
    PickSkill,
    SaveProfile {
        client: Client,
        name: String,
    },
    RemoveProfile {
        client: Client,
        id: String,
    },
    ListSessions {
        client: Client,
        query: String,
    },
    ReadSession {
        id: String,
    },
    ResumeSession {
        id: String,
    },
    ExportSession {
        id: String,
    },
    LearnSession {
        id: String,
    },
    Usage {
        client: Client,
        since: Option<u64>,
    },
    ListBackups,
    Rollback {
        id: String,
    },
    CheckCli {
        client: Client,
    },
    RunInstaller {
        client: Client,
    },
    OpenInstallGuide {
        client: Client,
    },
}

fn user_home(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .home_dir()
        .map_err(|_| "找不到当前用户目录。".into())
}

pub(crate) fn show(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW) {
        window.show().map_err(|_| "无法显示 AI 工具。")?;
        return window.set_focus().map_err(|_| "无法聚焦 AI 工具。".into());
    }
    WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("ai-tools.html".into()))
        .title("PenguinPal · AI 工具")
        .inner_size(1080.0, 745.0)
        .min_inner_size(740.0, 530.0)
        .center()
        .build()
        .map(|_| ())
        .map_err(|_| "无法打开 AI 工具窗口。".into())
}

#[tauri::command]
pub(crate) async fn companion_open_ai_tools(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("仅允许桌宠主窗口打开 AI 工具。".into());
    }
    show(window.app_handle())
}

fn plan(
    operation: &Operation,
    client: Client,
    user: &Path,
    settings: &Settings,
    state: &WindowState,
    root: &Path,
) -> Result<(String, Vec<Change>), String> {
    if let Operation::NativeModel { id } = operation {
        let changes = native_models::select(client, user, id)?;
        return Ok((
            format!("{} · 选择模型 {}", client.label(), id),
            storage::changed(changes),
        ));
    }
    if (matches!(
        operation,
        Operation::Profile { .. } | Operation::Prompt { .. }
    ) || (matches!(operation, Operation::Provider { .. })
        && !native_models::supports_direct(client)))
        && !client.manages_config()
    {
        return Err("该客户端的配置需在客户端内管理。".into());
    }
    let (title, mut changes) = match operation {
        Operation::NativeModel { .. } => return Err("此客户端不支持该模型选择方式。".into()),
        Operation::CodexHistory { enabled } if client == Client::Codex => (
            if *enabled {
                "开启统一 Codex 会话历史"
            } else {
                "关闭统一 Codex 会话历史"
            }
            .into(),
            codex_models::history_plan(user, *enabled)?
                .into_iter()
                .collect(),
        ),
        Operation::CodexHistory { .. } => return Err("只有 Codex 可以统一会话历史。".into()),
        Operation::Provider { id } => {
            let provider = providers::find(id, client, &settings.providers)?;
            (
                format!("{} · 切换到 {}", client.label(), provider.name),
                if client == Client::Codex {
                    codex_models::switch_plan(user, &provider)?
                } else {
                    provider.plan(user)?
                },
            )
        }
        Operation::Mcp {
            name,
            definition,
            targets,
            replace,
        } => {
            let apps = if targets.is_empty() {
                std::slice::from_ref(&client)
            } else {
                targets
            };
            if apps.len() > Client::ALL.len()
                || (*replace && apps != [client])
                || (definition.is_none() && apps != [client])
            {
                return Err("编辑和移除 MCP 只能针对当前客户端。".into());
            }
            let mut changes = Vec::new();
            for (i, app) in apps.iter().enumerate() {
                if apps[..i].contains(app) {
                    return Err("客户端目标重复。".into());
                }
                changes.extend(extensions::mcp_plan(
                    *app,
                    user,
                    name,
                    definition.as_ref(),
                    *replace || definition.is_none(),
                )?);
            }
            (
                format!(
                    "{} MCP {name}",
                    if definition.is_some() {
                        "保存"
                    } else {
                        "移除"
                    }
                ),
                changes,
            )
        }
        Operation::Prompt { text } => (
            format!("{} · 更新提示词", client.label()),
            extensions::prompt_plan(client, user, text)?,
        ),
        Operation::ImportSkills { ids, targets } => (
            format!("导入 {} 个本机 Skill", ids.len()),
            extensions::library_skills(user, &root.join("skills"), ids, targets)?,
        ),
        Operation::LocalSkill => (
            format!("{} · 安装本地 Skill", client.label()),
            extensions::install_skill(
                client,
                user,
                state
                    .skill_source
                    .as_deref()
                    .ok_or("请先选择 Skill 目录。")?,
            )?,
        ),
        Operation::ToggleSkill { id, enabled } => (
            format!(
                "{} · {} Skill",
                client.label(),
                if *enabled { "启用" } else { "停用" }
            ),
            extensions::toggle_skill(client, user, id, *enabled)?,
        ),
        Operation::Profile { id } => {
            let profile = settings
                .profiles
                .iter()
                .find(|p| p.id == *id && p.provider.client == client)
                .ok_or("项目配置已不存在。")?;
            let mut changes = profile.provider.plan_profile(user)?;
            profile.extensions.plan(client, user, &mut changes)?;
            (
                format!("{} · 应用项目 {}", client.label(), profile.name),
                changes,
            )
        }
    };
    if client == Client::Codex
        && matches!(
            operation,
            Operation::Provider { .. } | Operation::Profile { .. }
        )
    {
        let path = client.config(user);
        if let Some(config) = changes.iter_mut().find(|change| change.path == path) {
            codex_models::keep_unified_selection(config)?;
            codex_models::restore_legacy_definition(root, config)?;
        }
    }
    Ok((title, storage::changed(changes)))
}

fn summary(
    operation: &Operation,
    client: Client,
    user: &Path,
    settings: &Settings,
    changes: &[Change],
) -> Vec<String> {
    let target = match operation {
        Operation::Provider { id } => providers::find(id, client, &settings.providers).ok(),
        Operation::Profile { id } => settings
            .profiles
            .iter()
            .find(|profile| profile.id == *id && profile.provider.client == client)
            .map(|profile| profile.provider.clone()),
        _ => None,
    };
    let mut lines = Vec::new();
    match operation {
        Operation::CodexHistory { enabled } => {
            lines.push(if *enabled {
                "开启后新聊天共用一个供应商标识。切换后续聊可能把聊天上下文发给当前供应商，部分加密内容可能无法续用。"
            } else {
                "新聊天恢复使用各自的供应商标识。已建立的统一会话保留最近一次使用的供应商定义。"
            }.into());
            lines.push(
                "既有会话文件和历史数据库保持原样。桌宠的「会话」可查看各供应商的本机历史。".into(),
            );
        }
        Operation::Mcp { targets, .. } if !targets.is_empty() => {
            lines.push(format!(
                "写入客户端 · {}",
                targets
                    .iter()
                    .map(|client| client.label())
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
        Operation::ImportSkills { targets, .. } => {
            let places: Vec<_> = std::iter::once("桌宠技能库")
                .chain(targets.iter().map(|client| client.label()))
                .collect();
            lines.push(format!("保存位置 · {}", places.join("、")));
        }
        _ => {}
    }
    if let Some(target) = target {
        let before = providers::current(client, user)
            .ok()
            .map(|provider| provider.view());
        let mut after = target.view();
        if client == Client::Codex {
            let config = client.config(user);
            if let Some(doc) = changes
                .iter()
                .find(|c| c.path == config)
                .and_then(|change| providers::toml(change.after.as_deref()).ok())
            {
                after["model"] = json!(providers::item_text(&doc, "model"));
            }
        }
        let provider_switch = matches!(operation, Operation::Provider { .. });
        if provider_switch
            && client != Client::Codex
            && !matches!(client, Client::Workbuddy | Client::Omp)
        {
            lines.push("切换时沿用客户端当前模型。未选定模型时使用中转的默认模型。".into());
        }
        for (key, label, default) in [
            ("model", "模型", "客户端默认"),
            ("host", "地址", "客户端默认地址"),
        ] {
            if key == "model" && provider_switch && client != Client::Codex {
                continue;
            }

            let from = before
                .as_ref()
                .and_then(|v| v[key].as_str())
                .filter(|v| !v.is_empty())
                .unwrap_or(default);
            let to = after[key]
                .as_str()
                .filter(|v| !v.is_empty())
                .unwrap_or(default);
            if from != to {
                lines.push(format!("{label}  {from} → {to}"));
            }
        }
    }
    if matches!(operation, Operation::Profile { .. }) {
        lines.push("同时应用项目快照中的 MCP、托管提示词和已知 Skills 状态。".into());
    }
    lines
}

fn session(state: &AiTools, id: &str) -> Result<Session, String> {
    state
        .state
        .lock()
        .map_err(|_| "AI 工具状态忙。")?
        .sessions
        .get(id)
        .cloned()
        .ok_or_else(|| "请重新打开会话列表后选择会话。".into())
}

fn pick_file(app: &AppHandle, session: &Session) -> Result<Value, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md"])
        .set_file_name(&format!("{}-session.md", session.client.id()))
        .blocking_save_file();
    let Some(file) = file else {
        return Ok(json!({"cancelled": true}));
    };
    let path = file.into_path().map_err(|_| "文件路径无效。")?;
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return Err("请选择 Markdown 文件。".into());
    }
    let markdown = sessions::markdown(session)?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(markdown.as_bytes()))
        .map_err(|_| "导出失败，请选择可写目录和新文件名。")?;
    Ok(json!({"ok":true}))
}

fn perform(
    app: &AppHandle,
    state: &AiTools,
    request: Request,
    service: &Service,
    chat: &ChatService,
) -> Result<Value, String> {
    let user = user_home(app)?;
    let root = &*state.root;
    let settings_path = root.join("settings.bin");
    match request {
        Request::PickSkill => {
            let choice = app.dialog().file().blocking_pick_folder();
            let Some(choice) = choice else {
                return Ok(json!({"cancelled": true}));
            };
            let path = choice.into_path().map_err(|_| "目录路径无效。")?;
            let name = path
                .file_name()
                .ok_or("目录名称无效。")?
                .to_string_lossy()
                .into_owned();
            state
                .state
                .lock()
                .map_err(|_| "AI 工具状态忙。")?
                .skill_source = Some(path);
            return Ok(json!({"name": name}));
        }
        Request::ReadSession { id } => {
            let session = session(state, &id)?;
            let (messages, limited) = sessions::messages(&session)?;
            return Ok(json!({"session":session, "messages":messages, "limited":limited}));
        }
        Request::ResumeSession { id } => {
            sessions::resume(&session(state, &id)?)?;
            return Ok(json!({"ok":true}));
        }
        Request::ExportSession { id } => return pick_file(app, &session(state, &id)?),
        Request::LearnSession { id } => {
            let sources = sessions::memory_sources(&session(state, &id)?)?;
            crate::companion_chat::learn_selected_session(service, chat, sources)?;
            return Ok(json!({"ok":true}));
        }
        Request::OpenInstallGuide { client } => {
            let url = match client {
                Client::Codex => "https://developers.openai.com/codex/cli/",
                Client::Claude => "https://code.claude.com/docs/en/setup",
                Client::Gemini => "https://github.com/google-gemini/gemini-cli#installation",
                _ => return Err("请使用客户端自身的安装入口。".into()),
            };
            crate::companion_platform::open_page(url)?;
            return Ok(json!({"ok":true}));
        }
        Request::CheckCli { client } => {
            let files = sessions::executables(client);
            let version = files
                .first()
                .map(|path| sessions::cli_version(path))
                .transpose()?;
            return Ok(json!({"executables":files, "version":version}));
        }
        Request::RunInstaller { client } => {
            sessions::run_installer(client)?;
            return Ok(json!({"ok":true}));
        }
        _ => {}
    }

    let window = &state.state;
    let mut state = window.lock().map_err(|_| "AI 工具状态忙。")?;
    let mut settings: Settings = load_private(&settings_path)?.unwrap_or_default();
    match request {
        Request::Overview => {
            let clients: Vec<_> = Client::ALL
                .into_iter()
                .map(|client| {
                    let mut info = providers::list_for(client, &user, &settings.providers);
                    info["mcpSupported"] = json!(extensions::supports_mcp(client));
                    info["mcpSse"] = json!(extensions::supports_sse(client));
                    info["supportsDirect"] =
                        json!(client.manages_config() || native_models::supports_direct(client));
                    info["canLaunchAuth"] = json!(accounts::can_launch(client));
                    info["executable"] = json!(sessions::executable(client));
                    info
                })
                .collect();
            Ok(json!({"clients":clients}))
        }
        Request::QuickProviders => Ok(providers::list_for(
            Client::Codex,
            &user,
            &settings.providers,
        )),
        Request::AccountStatus { client } => state.account_status(root, &user, client),
        Request::StartLogin {
            client: Client::Codex,
            device,
            label,
        } => {
            if state.codex_login.is_some() {
                return Err("已有 Codex 登录正在进行。".into());
            }
            let mut login = accounts::CodexLogin::start(
                root,
                &user,
                label.as_deref().unwrap_or_default(),
                device,
            )?;
            let canceled = Arc::new(AtomicBool::new(false));
            let worker_cancel = Arc::clone(&canceled);
            let window = Arc::clone(window);
            let root = root.to_owned();
            std::thread::Builder::new()
                .spawn(move || {
                    let completed = login.wait(&worker_cancel);
                    if let Ok(mut state) = window.lock() {
                        let result = if worker_cancel.load(Ordering::Relaxed) {
                            Err("已取消添加账号。".into())
                        } else {
                            completed.and_then(|()| login.save(&root))
                        };
                        drop(login);
                        state.codex_login = None;
                        state.codex_login_note = Some(match result {
                            Ok(true) => "官方账号已保存，可以切换。".into(),
                            Ok(false) => {
                                "这个账号已保存。若要添加另一个，请在授权页面切换账号。".into()
                            }
                            Err(error) => error,
                        });
                    }
                })
                .map_err(|_| "无法启动 Codex 登录后台任务。")?;
            state.codex_login = Some(canceled);
            let message = if device {
                "请按登录窗口提示输入设备码，授权成功后会自动保存账号。"
            } else {
                "请在浏览器选择要添加的账号，授权成功后会自动保存。"
            };
            state.codex_login_note = Some(message.into());
            Ok(json!({"message":message}))
        }
        Request::StartLogin { client, .. } => {
            Ok(json!({"message":accounts::launch(root, &user, client)?}))
        }
        Request::CancelCodexLogin => {
            if let Some(canceled) = &state.codex_login {
                canceled.store(true, Ordering::Relaxed);
                state.codex_login_note = Some("正在取消登录。".into());
            }
            state.account_status(root, &user, Client::Codex)
        }
        Request::CaptureAccount { client, label } => {
            accounts::capture(root, &user, client, &label)?;
            state.account_status(root, &user, client)
        }
        Request::RemoveAccount { client, id } => {
            accounts::remove(root, client, &id)?;
            state.account_status(root, &user, client)
        }
        Request::PreviewAccount { client, id } => {
            let (title, changes) = accounts::plan(root, &user, client, &id)?;
            Ok(
                json!({"title":title,"changed":!changes.is_empty(), "revision":storage::revision(&changes)}),
            )
        }
        Request::ApplyAccount {
            client,
            id,
            revision,
        } => {
            let (title, changes) = accounts::plan(root, &user, client, &id)?;
            if storage::revision(&changes) != revision {
                return Err("本机登录自预览后已变化，请重新预览。".into());
            }
            if !changes.is_empty() {
                accounts::preserve_current(root, &user, client)?;
            }
            storage::commit(root, &title, changes)?;
            state.account_status(root, &user, client)
        }
        Request::SaveDirect {
            client,
            name,
            endpoint,
            key,
            model,
        } => {
            let provider = Provider::new_direct(client, &name, &endpoint, &key, &model)?;
            if settings.providers.iter().any(|entry| {
                entry.client == client && entry.name.eq_ignore_ascii_case(&provider.name)
            }) || provider.name == Provider::official(client).name
            {
                return Err("这个客户端已有同名供应商。".into());
            }
            if settings.providers.len() >= 500 {
                return Err("已保存的供应商达到上限。".into());
            }
            let view = provider.view();
            settings.providers.push(provider);
            save_private(&settings_path, &settings)?;
            Ok(view)
        }
        Request::SetDirectModel { client, id, model } => {
            let id = id
                .split_once(':')
                .filter(|(prefix, _)| *prefix == client.id())
                .map(|(_, id)| id)
                .ok_or("中转编号无效。")?;
            let provider = settings
                .providers
                .iter_mut()
                .find(|provider| provider.client == client && provider.id == id)
                .ok_or("找不到这条中转配置。")?;
            provider.set_native_relay_model(&model)?;
            save_private(&settings_path, &settings)?;
            Ok(json!({"ok":true}))
        }
        Request::Inspect { client } => {
            let result = providers::list_for(client, &user, &settings.providers);
            let mcp = extensions::supports_mcp(client).then(|| extensions::mcp_list(client, &user));
            let prompt = client
                .manages_config()
                .then(|| extensions::prompt_view(client, &user));
            Ok(json!({"provider":result,
                "mcp":mcp.as_ref().and_then(|result| result.as_ref().ok()), "mcpError":mcp.as_ref().and_then(|result| result.as_ref().err()),
                "skills":extensions::skills(client, &user, &root.join("skills")),
                "prompt":prompt.as_ref().and_then(|result| result.as_ref().ok()), "promptError":prompt.as_ref().and_then(|result| result.as_ref().err()),
                "profiles":settings.profiles.iter().filter(|p|p.provider.client == client)
                    .map(|p|json!({"id":p.id,"name":p.name,"provider":p.provider.name})).collect::<Vec<_>>()
            }))
        }
        Request::ReadMcp { client, name } if extensions::supports_mcp(client) => {
            extensions::mcp_editor(client, &user, &name)
        }
        Request::ReadMcp { .. } => Err("请在客户端内管理 MCP。".into()),
        Request::Preview { client, operation } => {
            let (title, changes) = plan(&operation, client, &user, &settings, &state, root)?;
            Ok(
                json!({"title":title, "files":changes.iter().map(|c| &c.path).collect::<Vec<_>>(),
                "revision":storage::revision(&changes), "summary":summary(&operation, client, &user, &settings, &changes)}),
            )
        }
        Request::Apply { .. } | Request::Switch { .. } => {
            let (client, operation, revision) = match request {
                Request::Apply {
                    client,
                    operation,
                    revision,
                } => (client, operation, Some(revision)),
                Request::Switch { client, operation }
                    if matches!(
                        operation,
                        Operation::Provider { .. } | Operation::NativeModel { .. }
                    ) =>
                {
                    (client, operation, None)
                }
                _ => return Err("此操作需要预览。".into()),
            };
            let (title, changes) = plan(&operation, client, &user, &settings, &state, root)?;
            if revision.is_some_and(|revision| revision != storage::revision(&changes)) {
                return Err("配置自预览后发生变化，请重新预览。".into());
            }
            if !changes.is_empty()
                && matches!(
                    operation,
                    Operation::Provider { .. } | Operation::Profile { .. }
                )
            {
                if let Some(original) = providers::original(client, &user, &settings.providers) {
                    settings.providers.push(original);
                    save_private(&settings_path, &settings)?;
                }
            }
            let backup = storage::commit(root, &title, changes)?;
            Ok(json!({"changed": backup.is_some(), "backup": backup}))
        }
        Request::SaveProfile { client, name } => {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 80 {
                return Err("请输入最多 80 字的项目名称。".into());
            }
            if settings
                .profiles
                .iter()
                .any(|p| p.provider.client == client && p.name == name)
            {
                return Err("该客户端已存在同名项目配置。".into());
            }
            if settings.profiles.len() >= 60 {
                return Err("项目快照已达到 60 个。".into());
            }
            let provider = providers::current(client, &user)?;
            provider.switchable()?;
            settings.profiles.push(Profile {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.to_owned(),
                provider,
                extensions: ExtensionProfile::snapshot(client, &user)?,
            });
            save_private(&settings_path, &settings)?;
            Ok(json!({"ok":true}))
        }
        Request::RemoveProfile { client, id } => {
            settings
                .profiles
                .retain(|p| p.id != id || p.provider.client != client);
            save_private(&settings_path, &settings)?;
            Ok(json!({"ok":true}))
        }
        Request::ListSessions { client, query } => {
            let (results, limited) = sessions::list(&user, client, &query);
            state.sessions.retain(|_, s| s.client != client);
            state
                .sessions
                .extend(results.iter().map(|s| (s.id.clone(), s.clone())));
            Ok(json!({"sessions":results, "limited":limited}))
        }
        Request::Usage { client, since } => {
            Ok(json!({"report":sessions::usage(&user, client, since)}))
        }
        Request::ListBackups => Ok(json!(storage::backups(root)?)),
        Request::Rollback { id } => Ok(json!({"backup":storage::rollback(root, &id, &user)?})),
        _ => Err("请求状态无效。".into()),
    }
}

#[tauri::command]
pub(crate) async fn companion_ai_tools(
    window: WebviewWindow,
    manager: State<'_, AiTools>,
    service: State<'_, Service>,
    chat: State<'_, ChatService>,
    request: Value,
) -> Result<Value, String> {
    let request = serde_json::from_value(request).map_err(|_| "AI 工具请求格式无效。")?;
    let quick = window.label() == "main"
        && matches!(
            &request,
            Request::QuickProviders
                | Request::Switch {
                    client: Client::Codex,
                    operation: Operation::Provider { .. }
                }
        );
    if window.label() != WINDOW && !quick {
        return Err("仅 AI 工具窗口可以执行该管理命令。".into());
    }
    let app = window.app_handle().clone();
    let manager = manager.inner().clone();
    let service = service.inner().clone();
    let chat = chat.inner().clone();

    if matches!(&request, Request::RefreshCodexModels) {
        let user = user_home(&app)?;
        let config = Client::Codex.config(&user);
        let before = storage::read_config(&config)?;
        let provider = providers::current(Client::Codex, &user)?;
        let discovered = codex_models::discover(&provider).await?;
        return tauri::async_runtime::spawn_blocking(move || {
            let _guard = manager.state.lock().map_err(|_| "AI 工具状态忙。")?;
            let (usable, changes) = codex_models::refresh_plan(&user, before, &discovered)?;
            let backup = storage::commit(&manager.root, "刷新 Codex 模型目录", changes)?;
            Ok(json!({"available":discovered.count(),"usable":usable,"backup":backup}))
        })
        .await
        .map_err(|_| "模型刷新中断。")?;
    }
    if let Request::TestProvider { client, id } = request {
        let provider = tauri::async_runtime::spawn_blocking(move || {
            let user = user_home(&app)?;
            let _guard = manager.state.lock().map_err(|_| "AI 工具状态忙。")?;
            let settings: Settings =
                load_private(&manager.root.join("settings.bin"))?.unwrap_or_default();
            if id == format!("{}:live", client.id()) {
                providers::current(client, &user)
            } else {
                providers::find(&id, client, &settings.providers)
            }
        })
        .await
        .map_err(|_| "连接测试中断。")??;
        return provider.probe().await;
    }
    tauri::async_runtime::spawn_blocking(move || perform(&app, &manager, request, &service, &chat))
        .await
        .map_err(|_| "AI 工具后台中断。")?
}
