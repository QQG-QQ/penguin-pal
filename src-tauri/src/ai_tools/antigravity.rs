use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
        },
    },
};

use super::{
    accounts::{installed_desktop, process_names_running},
    providers::{direct_url, Client},
    sessions, storage,
};

#[path = "antigravity_credentials.rs"]
mod credentials;

use credentials::{
    claims, current_credential, ensure_shared_login_stopped, matches_credential, read_import,
    save_account, switch_account,
};

const APP: &str = "Antigravity.exe";
const GUIDE: &str = "https://antigravity.google/docs/cli/tutorial/";

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum Action {
    Status,
    InstallGuide,
    OpenApp,
    StartLogin,
    Capture {
        label: String,
    },
    Import,
    Switch {
        id: String,
    },
    RemoveAccount {
        id: String,
    },
    PickDirectory,
    SaveInstance {
        id: Option<String>,
        name: String,
        directory: PathBuf,
    },
    StartInstance {
        id: String,
    },
    StopInstance {
        id: String,
    },
    RemoveInstance {
        id: String,
    },
    SaveRelay {
        id: Option<String>,
        name: String,
        endpoint: String,
        key: String,
    },
    RemoveRelay {
        id: String,
    },
    StartRelay {
        id: String,
    },
    StartOfficialCli,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    accounts: Vec<Account>,
    instances: Vec<Instance>,
    relays: Vec<Relay>,
    relay_mode: Option<RelayMode>,
}

#[derive(Serialize, Deserialize)]
struct Account {
    id: Uuid,
    label: String,
    credential: Value,
}

#[derive(Serialize, Deserialize)]
struct Instance {
    id: Uuid,
    name: String,
    directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Relay {
    id: Uuid,
    name: String,
    endpoint: String,
    key: String,
}

#[derive(Serialize, Deserialize)]
struct RelayMode {
    previous: Option<Value>,
    created_settings: bool,
}

#[derive(Default)]
pub(super) struct Runtime {
    instances: HashMap<Uuid, Child>,
}

fn app_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    local
        .map(|path| path.join("Programs").join("Antigravity").join(APP))
        .filter(|path| path.is_file())
        .or_else(|| installed_desktop("Antigravity", APP))
}

fn agy_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("agy/bin/agy.exe"))
        .filter(|path| path.is_file())
        .or_else(|| sessions::executable(Client::Agy))
}

pub(super) fn describe(info: &mut Value) {
    info["label"] = json!("Antigravity");
    info["antigravityManagement"] = json!(true);
    if let Some(path) = app_path().or_else(agy_path) {
        info["executable"] = json!(path);
    }
}

fn open_app(directory: &Path, profile: Option<&Path>) -> Result<Child, String> {
    let path = app_path().ok_or("未找到 Antigravity App。")?;
    let mut command = Command::new(path);
    command.current_dir(directory);
    if let Some(profile) = profile {
        let mut arg = OsString::from("--user-data-dir=");
        arg.push(profile);
        command.arg(arg).arg(directory);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法启动 Antigravity App。".into())
}

fn label(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 120 || value.chars().any(char::is_control) {
        return Err("请输入有效名称，最多 120 个字符。".into());
    }
    Ok(value.to_owned())
}

fn id(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| "项目标识无效。".into())
}

fn launch_cli(
    root: &Path,
    user: &Path,
    path: &Path,
    book: &mut Book,
    relay_id: Option<Uuid>,
) -> Result<(), String> {
    if process_names_running(&["agy.exe"])? {
        return Err("请先退出当前 Antigravity CLI。".into());
    }
    with_cli_settings(root, user, path, book, relay_id, |command| {
        command.spawn().map(|_| ())
    })
}

fn with_cli_settings(
    root: &Path,
    user: &Path,
    path: &Path,
    book: &mut Book,
    relay_id: Option<Uuid>,
    launch: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Result<(), String> {
    let state_path = root.join("antigravity.bin");
    let state_before = storage::read_config(&state_path)?;
    let config_path = user.join(".gemini/antigravity-cli/settings.json");
    let before = storage::read_config(&config_path)?;
    let config: Value = before
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes))
        .transpose()
        .map_err(|_| "Antigravity CLI 设置不是有效的 JSON。")?
        .unwrap_or_else(|| json!({}));
    let mut after = config.clone();
    let fields = after
        .as_object_mut()
        .ok_or("Antigravity CLI 设置格式无效。")?;
    let mut command = Command::new(path);
    let remove_empty_settings = relay_id.is_none()
        && book
            .relay_mode
            .as_ref()
            .is_some_and(|mode| mode.created_settings);
    command
        .current_dir(user)
        .creation_flags(0x10)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_GEMINI_BASE_URL");
    if let Some(id) = relay_id {
        let relay = book
            .relays
            .iter()
            .find(|relay| relay.id == id)
            .ok_or("中转已不存在。")?;
        if book.relay_mode.is_none() && fields.get("modelProvider") != Some(&json!("gemini")) {
            book.relay_mode = Some(RelayMode {
                previous: fields.get("modelProvider").cloned(),
                created_settings: before.is_none(),
            });
        } else if fields.get("modelProvider") != Some(&json!("gemini")) {
            return Err("CLI 模式已被其他程序调整，请检查 settings.json。".into());
        }
        fields.insert("modelProvider".into(), json!("gemini"));
        command
            .env_remove("AGY_ADC_AUTH")
            .env("GEMINI_API_KEY", &relay.key)
            .env("GOOGLE_GEMINI_BASE_URL", &relay.endpoint);
    } else if let Some(mode) = book.relay_mode.take() {
        if fields.get("modelProvider") != Some(&json!("gemini")) {
            return Err("CLI 模式已被其他程序调整，请检查 settings.json。".into());
        }
        match mode.previous {
            Some(previous) => {
                fields.insert("modelProvider".into(), previous);
            }
            _ => {
                fields.remove("modelProvider");
            }
        }
    } else if fields.get("modelProvider") == Some(&json!("gemini")) {
        return Err("CLI 在桌宠之外启用了 Gemini API Key 模式，请先用客户端恢复账号登录。".into());
    }
    if after == config {
        return launch(&mut command).map_err(|_| "无法启动 Antigravity CLI。".into());
    }
    let config_after = if remove_empty_settings && after == json!({}) {
        None
    } else {
        Some(serde_json::to_vec_pretty(&after).map_err(|_| "无法编码 CLI 设置。")?)
    };
    let state_after = storage::sealed(book)?;
    storage::commit(
        root,
        "切换 Antigravity CLI 登录方式",
        vec![
            storage::Change {
                path: config_path.clone(),
                before: before.clone(),
                after: config_after.clone(),
            },
            storage::Change::new(
                state_path.clone(),
                state_before.clone(),
                state_after.clone(),
            ),
        ],
    )?;
    if launch(&mut command).is_err() {
        storage::commit(
            root,
            "还原 Antigravity CLI 设置",
            vec![
                storage::Change {
                    path: config_path,
                    before: config_after,
                    after: before,
                },
                storage::Change {
                    path: state_path,
                    before: Some(state_after),
                    after: state_before,
                },
            ],
        )
        .map_err(|_| "CLI 启动失败，配置恢复失败，请查看配置回滚。")?;
        return Err("无法启动 Antigravity CLI。".into());
    }
    Ok(())
}

fn close_instance(pid: u32) -> Result<(), String> {
    struct Close {
        pid: u32,
        sent: bool,
    }
    unsafe extern "system" fn close(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = &mut *(data.0 as *mut Close);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.pid
            && IsWindowVisible(hwnd).as_bool()
            && PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok()
        {
            state.sent = true;
        }
        BOOL(1)
    }
    let mut state = Close { pid, sent: false };
    unsafe { EnumWindows(Some(close), LPARAM(&mut state as *mut Close as isize)) }
        .map_err(|_| "无法定位 Antigravity 实例窗口。")?;
    state
        .sent
        .then_some(())
        .ok_or_else(|| "没有找到实例窗口，请在 Antigravity 中退出。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_launch_scopes_key_to_child_and_restores_official_mode() {
        let user = tempfile::tempdir().unwrap();
        let user = user.path();
        let root = user.join("book");
        let config = user.join(".gemini/antigravity-cli/settings.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, br#"{"theme":"Dark"}"#).unwrap();
        let id = Uuid::new_v4();
        let mut book = Book::default();
        book.relays.push(Relay {
            id,
            name: "test".into(),
            endpoint: "https://example.test/".into(),
            key: "synthetic-key".into(),
        });
        let cli = Path::new("agy.exe");
        with_cli_settings(&root, user, cli, &mut book, Some(id), |command| {
            assert!(command.get_args().next().is_none());
            assert!(command
                .get_envs()
                .any(|(name, value)| name == "GEMINI_API_KEY"
                    && value.is_some_and(|value| value == "synthetic-key")));
            let data = fs::read_to_string(&config).unwrap();
            assert!(!data.contains("synthetic-key"));
            assert_eq!(
                serde_json::from_str::<Value>(&data).unwrap(),
                json!({"modelProvider":"gemini","theme":"Dark"})
            );
            Ok(())
        })
        .unwrap();
        let mut book: Book = storage::load_private(&root.join("antigravity.bin"))
            .unwrap()
            .unwrap();
        with_cli_settings(&root, user, cli, &mut book, None, |command| {
            assert!(command
                .get_envs()
                .any(|(name, value)| name == "GEMINI_API_KEY" && value.is_none()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&config).unwrap()).unwrap(),
            json!({"theme":"Dark"})
        );
        assert!(book.relay_mode.is_none());
    }

    #[test]
    fn failed_cli_launch_restores_settings_and_book() {
        let user = tempfile::tempdir().unwrap();
        let user = user.path();
        let root = user.join("book");
        let id = Uuid::new_v4();
        let mut book = Book::default();
        book.relays.push(Relay {
            id,
            name: "test".into(),
            endpoint: "https://example.test/".into(),
            key: "synthetic-key".into(),
        });
        let state = root.join("antigravity.bin");
        storage::save_private(&state, &book).unwrap();
        let before = fs::read(&state).unwrap();
        let result = with_cli_settings(
            &root,
            user,
            Path::new("agy.exe"),
            &mut book,
            Some(id),
            |_| Err(std::io::ErrorKind::NotFound.into()),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&state).unwrap(), before);
        assert!(!user.join(".gemini/antigravity-cli/settings.json").exists());
    }
}

impl Runtime {
    pub(super) fn perform(
        &mut self,
        app: &AppHandle,
        root: &Path,
        user: &Path,
        action: Action,
    ) -> Result<Value, String> {
        let path = root.join("antigravity.bin");
        let mut book: Book = storage::load_private(&path)?.unwrap_or_default();
        self.instances
            .retain(|_, process| !matches!(process.try_wait(), Ok(Some(_))));
        let message = match action {
            Action::Status => {
                let current = current_credential(user);
                return Ok(json!({
                    "app": app_path(), "cli": agy_path(),
                    "cliRelayMode": book.relay_mode.is_some(),
                    "signedIn": current.as_ref().is_ok_and(|value| value.is_some()),
                    "credentialError": current.as_ref().err(),
                    "accounts": book.accounts.iter().map(|account| json!({
                        "id": account.id, "label": account.label,
                        "active": current.as_ref().ok().and_then(Option::as_ref)
                            .is_some_and(|credential| matches_credential(&account.credential, credential)),
                    })).collect::<Vec<_>>(),
                    "instances": book.instances.iter().map(|instance| json!({
                        "id": instance.id, "name": instance.name, "directory": instance.directory,
                        "running": self.instances.contains_key(&instance.id),
                    })).collect::<Vec<_>>(),
                    "relays": book.relays.iter().map(|relay| json!({
                        "id": relay.id, "name": relay.name, "endpoint": relay.endpoint,
                    })).collect::<Vec<_>>(),
                }));
            }
            Action::InstallGuide => {
                crate::companion_platform::open_page(GUIDE)?;
                return Ok(json!({}));
            }
            Action::PickDirectory => {
                let directory = app
                    .dialog()
                    .file()
                    .blocking_pick_folder()
                    .and_then(|path| path.into_path().ok());
                return Ok(json!({"directory": directory}));
            }
            Action::OpenApp => {
                open_app(user, None)?;
                return Ok(json!({"message":"已请求打开 Antigravity App。"}));
            }
            Action::StartLogin => {
                open_app(user, None)?;
                return Ok(
                    json!({"message":"请在 App 账号菜单登录 Google。添加另一个账号前，请先收录现有账号。登录完成后返回收录。"}),
                );
            }
            Action::Capture {
                label: account_label,
            } => {
                let credential =
                    current_credential(user)?.ok_or("请先在 Antigravity App 登录 Google。")?;
                save_account(&mut book, credential, label(&account_label)?)?;
                "已收录当前 Google 账号。"
            }
            Action::Import => {
                let file = app
                    .dialog()
                    .file()
                    .add_filter("JSON", &["json"])
                    .blocking_pick_file();
                let Some(file) = file else {
                    return Ok(json!({}));
                };
                let file = file.into_path().map_err(|_| "无法打开导入文件。")?;
                let bytes = storage::read_config(&file)?.ok_or("导入文件不存在。")?;
                let accounts = read_import(
                    serde_json::from_slice(&bytes).map_err(|_| "账号 JSON 格式无效。")?,
                )?;
                for (credential, name) in accounts {
                    save_account(&mut book, credential, name)?;
                }
                "账号已导入。"
            }
            Action::Switch { id: account_id } => {
                let account_id = id(&account_id)?;
                if !book.accounts.iter().any(|account| account.id == account_id) {
                    return Err("账号已不存在。".into());
                }
                ensure_shared_login_stopped()?;
                let current = current_credential(user)?;
                if let Some(current) = current.as_ref() {
                    let name = book
                        .accounts
                        .iter()
                        .find(|saved| matches_credential(&saved.credential, current))
                        .map(|saved| saved.label.clone())
                        .or_else(|| {
                            claims(current).and_then(|value| {
                                value["email"].as_str().and_then(|email| label(email).ok())
                            })
                        })
                        .unwrap_or_else(|| "切换前的 Google 账号".to_owned());
                    save_account(&mut book, current.clone(), name)?;
                    storage::save_private(&path, &book)?;
                }
                let account = book
                    .accounts
                    .iter()
                    .find(|item| item.id == account_id)
                    .ok_or("账号已不存在。")?;
                switch_account(user, current.as_ref(), &account.credential)?;
                return Ok(
                    json!({"message":"共享凭据已写入并读回验证。打开 App 后可核对实际登录账号。"}),
                );
            }
            Action::RemoveAccount { id: account_id } => {
                let account_id = id(&account_id)?;
                book.accounts.retain(|account| account.id != account_id);
                "已移除保存的账号。"
            }
            Action::SaveInstance {
                id: instance_id,
                name,
                directory,
            } => {
                let instance_id = instance_id.as_deref().map(id).transpose()?;
                if !directory.is_absolute() || !directory.is_dir() {
                    return Err("请选择有效的工作目录。".into());
                }
                let name = label(&name)?;
                let instance_id = instance_id.unwrap_or_else(Uuid::new_v4);
                if self.instances.contains_key(&instance_id) {
                    return Err("请先退出此实例。".into());
                }
                book.instances.retain(|item| item.id != instance_id);
                book.instances.push(Instance {
                    id: instance_id,
                    name,
                    directory,
                });
                "已保存实例。"
            }
            Action::StartInstance { id: instance_id } => {
                let instance_id = id(&instance_id)?;
                if self.instances.contains_key(&instance_id) {
                    return Err("此实例正在运行。".into());
                }
                let instance = book
                    .instances
                    .iter()
                    .find(|item| item.id == instance_id)
                    .ok_or("实例已不存在。")?;
                let profile = root
                    .join("antigravity-instances")
                    .join(instance_id.to_string());
                fs::create_dir_all(&profile).map_err(|_| "无法创建实例目录。")?;
                self.instances
                    .insert(instance_id, open_app(&instance.directory, Some(&profile))?);
                return Ok(json!({"message":"已请求启动 Antigravity 实例。"}));
            }
            Action::StopInstance { id: instance_id } => {
                let process = self
                    .instances
                    .get(&id(&instance_id)?)
                    .ok_or("此实例没有桌宠跟踪的运行进程。")?;
                close_instance(process.id())?;
                return Ok(
                    json!({"message":"已请求关闭实例。请处理 Antigravity 中可能出现的保存提示。"}),
                );
            }
            Action::RemoveInstance { id: instance_id } => {
                let instance_id = id(&instance_id)?;
                if self.instances.contains_key(&instance_id) {
                    return Err("请先退出此实例。".into());
                }
                book.instances.retain(|item| item.id != instance_id);
                "已移除实例登记，项目和实例文件可继续访问。"
            }
            Action::SaveRelay {
                id: relay_id,
                name,
                endpoint,
                key,
            } => {
                let name = label(&name)?;
                let endpoint = direct_url(endpoint.trim())?.ok_or("请输入 Gemini 兼容地址。")?;
                if endpoint.scheme() != "https"
                    && !matches!(
                        endpoint.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]")
                    )
                {
                    return Err("远程中转需要 HTTPS 地址。".into());
                }
                let existing = relay_id.as_deref().map(id).transpose()?;
                let previous =
                    existing.and_then(|id| book.relays.iter().find(|relay| relay.id == id));
                let key = if key.trim().is_empty() && previous.is_some() {
                    let previous = previous.ok_or("中转已不存在。")?;
                    let original = reqwest::Url::parse(&previous.endpoint)
                        .map_err(|_| "已保存的中转地址格式无效。")?;
                    if original.origin() != endpoint.origin() {
                        return Err("更换中转服务器时，请重新输入 API Key。".into());
                    }
                    previous.key.clone()
                } else {
                    key.trim().to_owned()
                };
                if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
                    return Err("请输入有效的中转 API Key。".into());
                }
                let id = existing.unwrap_or_else(Uuid::new_v4);
                book.relays.retain(|relay| relay.id != id);
                book.relays.push(Relay {
                    id,
                    name,
                    endpoint: endpoint.to_string(),
                    key,
                });
                "已保存 Gemini 中转。"
            }
            Action::RemoveRelay { id: relay_id } => {
                let relay_id = id(&relay_id)?;
                book.relays.retain(|relay| relay.id != relay_id);
                "已移除中转。"
            }
            Action::StartRelay { id: relay_id } => {
                let cli = agy_path().ok_or("请先安装 Antigravity CLI。")?;
                launch_cli(root, user, &cli, &mut book, Some(id(&relay_id)?))?;
                return Ok(
                    json!({"message":"已启动使用中转的 Antigravity CLI。请在终端选择模型并验证连接。"}),
                );
            }
            Action::StartOfficialCli => {
                let cli = agy_path().ok_or("请先安装 Antigravity CLI。")?;
                launch_cli(root, user, &cli, &mut book, None)?;
                return Ok(json!({"message":"已请求启动使用官方登录的 Antigravity CLI。"}));
            }
        };
        storage::save_private(&path, &book)?;
        Ok(json!({"message": message}))
    }
}
