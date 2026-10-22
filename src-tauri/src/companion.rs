use crate::companion_media;
use crate::companion_platform;
use crate::{companion_data, companion_runtime, companion_updates};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub wander: bool,
    pub gravity: bool,
    pub surfaces: bool,
    pub awareness: bool,
    pub reduced_motion: bool,
    pub quiet: bool,
    pub codex_events: bool,
    pub movement: Movement,
    pub size: f64,
    pub rhythm: Rhythm,
    pub walk_speed: f64,
    pub hover_pause: bool,
    pub topmost: bool,
    pub fullscreen_hide: bool,
    pub passthrough: bool,
    pub autostart: bool,
    pub personality: Personality,
    pub edge_dock: bool,
    pub auto_hide: bool,
    pub notifications: bool,
    pub animation_pack: String,
    pub video_animations: bool,
    pub spring_drag: bool,
    pub status_animations: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Movement {
    Still,
    Wander,
    Follow,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rhythm {
    Calm,
    Balanced,
    Lively,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Personality {
    Gentle,
    Playful,
    Quiet,
    Warm,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            wander: false,
            gravity: true,
            surfaces: true,
            awareness: false,
            reduced_motion: false,
            quiet: false,
            codex_events: true,
            movement: Movement::Still,
            size: 1.0,
            rhythm: Rhythm::Balanced,
            walk_speed: 55.0,
            hover_pause: true,
            topmost: true,
            fullscreen_hide: true,
            passthrough: false,
            autostart: false,
            personality: Personality::Gentle,
            edge_dock: true,
            auto_hide: true,
            notifications: true,
            animation_pack: "builtin".into(),
            video_animations: true,
            spring_drag: true,
            status_animations: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PetState {
    pub schema: u32,
    pub energy: f64,
    pub hunger: f64,
    pub affection: f64,
    pub saved_at: u64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub preferences: Preferences,
    pub dock: Dock,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dock {
    None,
    Left,
    Right,
    Top,
}
impl Default for PetState {
    fn default() -> Self {
        Self {
            schema: 1,
            energy: 80.0,
            hunger: 20.0,
            affection: 65.0,
            saved_at: now(),
            x: None,
            y: None,
            preferences: Preferences::default(),
            dock: Dock::None,
        }
    }
}
impl PetState {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !companion_media::valid_id(&self.preferences.animation_pack)
            || ![0.8, 1.0, 1.2].contains(&self.preferences.size)
            || ![35.0, 55.0, 80.0].contains(&self.preferences.walk_speed)
            || [self.energy, self.hunger, self.affection]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=100.0).contains(v))
        {
            return Err("桌宠状态无效；未覆盖已有状态。".into());
        }
        Ok(())
    }
}

fn read_state(path: &Path) -> Option<PetState> {
    let state: PetState = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    state.validate().ok()?;
    Some(state)
}
fn load_state(root: &Path) -> PetState {
    read_state(&root.join("state.json"))
        .or_else(|| read_state(&root.join("state.json.bak")))
        .unwrap_or_default()
}
fn preserve_pre_media_state(root: &Path) -> Result<(), String> {
    let checkpoint = root.join("state.pre-0.4.json");
    if checkpoint.exists() {
        return Ok(());
    }
    for name in ["state.json", "state.json.bak"] {
        let path = root.join(name);
        if read_state(&path).is_none() {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if value["preferences"].get("animationPack").is_some() {
            return Ok(());
        }
        atomic_write(&checkpoint, &bytes)?;
        return Ok(());
    }
    Ok(())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("路径缺少父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    {
        use std::io::Write;
        let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    fs::rename(temporary, path).map_err(|e| e.to_string())
}
fn persist(root: &Path, state: &PetState) -> Result<(), String> {
    state.validate()?;
    let path = root.join("state.json");
    if let Some(previous) = read_state(&path) {
        atomic_write(
            &root.join("state.json.bak"),
            &serde_json::to_vec(&previous).unwrap(),
        )?;
    }
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?,
    )?;
    atomic_write(
        &root.join("bridge-enabled.json"),
        if state.preferences.codex_events {
            b"true"
        } else {
            b"false"
        },
    )
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookEvent {
    event: EventKind,
    session: String,
    turn: String,
    at: u64,
    #[serde(default)]
    issue: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum EventKind {
    UserPromptSubmit,
    PermissionRequest,
    PostToolUse,
    Stop,
    Interrupt,
    SessionEnd,
}
impl EventKind {
    fn status(self) -> &'static str {
        match self {
            Self::PermissionRequest => "waiting",
            Self::Stop => "ended",
            Self::Interrupt => "interrupted",
            Self::SessionEnd => "idle",
            _ => "working",
        }
    }
    fn terminal(self) -> bool {
        matches!(self, Self::Stop | Self::Interrupt | Self::SessionEnd)
    }
}
#[derive(Default)]
struct Bridge {
    sessions: HashMap<String, HookEvent>,
    retired: HashMap<String, Vec<String>>,
    last_received: u64,
    rejected: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeSummary {
    status: &'static str,
    active_sessions: usize,
    last_received: u64,
}
impl Default for BridgeSummary {
    fn default() -> Self {
        Self {
            status: "idle",
            active_sessions: 0,
            last_received: 0,
        }
    }
}
impl Bridge {
    fn accept(&mut self, event: HookEvent, time: u64) {
        let valid_key =
            |s: &str| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
        if !valid_key(&event.session)
            || !valid_key(&event.turn)
            || event.at > time + 5000
            || time.saturating_sub(event.at) > 900_000
            || (event.issue && event.event != EventKind::PostToolUse)
        {
            self.rejected += 1;
            return;
        }
        if let Some(old) = self.sessions.get(&event.session) {
            if event.at <= old.at {
                return;
            }
            if event.turn == old.turn && old.event.terminal() && !event.event.terminal() {
                return;
            }
            if event.turn != old.turn && event.event != EventKind::SessionEnd {
                let history = self.retired.entry(event.session.clone()).or_default();
                if history.contains(&event.turn) {
                    return;
                }
                history.push(old.turn.clone());
                if history.len() > 32 {
                    history.remove(0);
                }
            }
        }
        if self.sessions.len() >= 32 && !self.sessions.contains_key(&event.session) {
            if let Some(key) = self
                .sessions
                .iter()
                .min_by_key(|(_, e)| e.at)
                .map(|(k, _)| k.clone())
            {
                self.sessions.remove(&key);
                self.retired.remove(&key);
            }
        }
        self.last_received = time;
        self.sessions.insert(event.session.clone(), event);
    }
    fn poll(&mut self, root: &Path, enabled: bool) -> BridgeSummary {
        self.poll_at(root, enabled, now())
    }
    fn poll_at(&mut self, root: &Path, enabled: bool, time: u64) -> BridgeSummary {
        let mut events = vec![];
        if let Ok(entries) = fs::read_dir(root.join("events")) {
            for entry in entries
                .flatten()
                .filter(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("json"))
                .take(256)
            {
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                if enabled {
                    if let Ok(file) = fs::File::open(&path) {
                        let mut bytes = vec![];
                        if file.take(2049).read_to_end(&mut bytes).is_ok() && bytes.len() <= 2048 {
                            match serde_json::from_slice::<HookEvent>(&bytes) {
                                Ok(event) => events.push(event),
                                Err(_) => self.rejected += 1,
                            }
                        } else {
                            self.rejected += 1;
                        }
                    }
                }
                let _ = fs::remove_file(path);
            }
        }
        events.sort_by_key(|event| event.at);
        for event in events {
            self.accept(event, time);
        }
        if !enabled {
            self.sessions.clear();
            self.retired.clear();
            self.last_received = 0;
        }
        // Keep terminal tombstones for the queue's whole lifetime: delayed tool hooks must
        // not revive a turn after the short on-screen completion message has faded.
        self.sessions
            .retain(|_, e| time.saturating_sub(e.at) < 900_000);
        self.retired
            .retain(|key, _| self.sessions.contains_key(key));
        let status = self
            .sessions
            .values()
            .filter(|e| !e.event.terminal() || time.saturating_sub(e.at) < 12_000)
            .max_by_key(|e| {
                match if e.issue && time.saturating_sub(e.at) < 10000 {
                    "toolError"
                } else {
                    e.event.status()
                } {
                    "waiting" => 6,
                    "toolError" => 5,
                    "working" => 3,
                    "ended" => 2,
                    "interrupted" => 1,
                    _ => 0,
                }
            })
            .map(|e| {
                if e.issue && time.saturating_sub(e.at) < 10000 {
                    "toolError"
                } else {
                    e.event.status()
                }
            })
            .unwrap_or("idle");
        BridgeSummary {
            status,
            active_sessions: self
                .sessions
                .values()
                .filter(|e| !e.event.terminal())
                .count(),
            last_received: self.last_received,
        }
    }
}

struct Store {
    root: PathBuf,
    state: PetState,
    bridge: Bridge,
    last_bridge: BridgeSummary,
    runtime: companion_runtime::Runtime,
    restored_backup: bool,
    update: Option<companion_updates::Package>,
}
fn main_only(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("仅允许桌宠主窗口调用。".into())
    }
}
#[tauri::command]
fn companion_load(window: WebviewWindow, store: State<Mutex<Store>>) -> Result<PetState, String> {
    main_only(&window)?;
    Ok(store.lock().map_err(|_| "状态忙")?.state.clone())
}
#[tauri::command]
fn companion_save(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
    mut state: PetState,
) -> Result<(), String> {
    main_only(&window)?;
    state.validate()?;
    state.saved_at = now();
    let mut store = store.lock().map_err(|_| "状态忙")?;
    if state.preferences.autostart != store.state.preferences.autostart {
        let manager = window.app_handle().autolaunch();
        if state.preferences.autostart {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|e| e.to_string())?;
    }
    if let Err(error) = persist(&store.root, &state) {
        companion_runtime::record(&store.root, "state.save_error", now());
        return Err(error);
    }
    store.runtime.pulse(now())?;
    store.state = state;
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    bridge: BridgeSummary,
    desktop: companion_platform::Desktop,
    safe_mode: bool,
}
#[tauri::command]
fn companion_poll(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
) -> Result<Observation, String> {
    main_only(&window)?;
    let mut store = store.lock().map_err(|_| "状态忙")?;
    let root = store.root.clone();
    let prefs = store.state.preferences.clone();
    let bridge = store.bridge.poll(&root, prefs.codex_events);
    store.last_bridge = bridge.clone();
    let safe = store.runtime.safe_mode;
    Ok(Observation {
        bridge,
        desktop: companion_platform::desktop(prefs.surfaces && !safe, prefs.awareness && !safe),
        safe_mode: safe,
    })
}
#[tauri::command]
fn companion_open_codex(window: WebviewWindow) -> Result<(), String> {
    main_only(&window)?;
    companion_platform::open_codex()
}
#[tauri::command]
fn companion_quit(window: WebviewWindow) -> Result<(), String> {
    main_only(&window)?;
    window.app_handle().exit(0);
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum VideoIssue {
    Blocked,
    Play,
    Timeout,
    Metadata,
    Aborted,
    Network,
    Decode,
    Format,
    Load,
}
#[tauri::command]
fn companion_video_issue(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
    reason: VideoIssue,
) -> Result<(), String> {
    main_only(&window)?;
    let label = match reason {
        VideoIssue::Blocked => "media.video_blocked",
        VideoIssue::Play => "media.video_play",
        VideoIssue::Timeout => "media.video_timeout",
        VideoIssue::Metadata => "media.video_metadata",
        VideoIssue::Aborted => "media.video_aborted",
        VideoIssue::Network => "media.video_network",
        VideoIssue::Decode => "media.video_decode",
        VideoIssue::Format => "media.video_format",
        VideoIssue::Load => "media.video_load",
    };
    companion_runtime::record(&store.lock().map_err(|_| "状态忙")?.root, label, now());
    Ok(())
}
const PREVIEW_ACTIONS: &[&str] = &[
    "greet",
    "happy",
    "pat",
    "poke",
    "tail",
    "eat",
    "rest",
    "wake",
    "sweep",
    "think",
    "look",
    "blink",
    "talk",
    "annoyed",
    "idle",
    "land",
    "dizzy",
    "walk-left",
    "walk-right",
];
#[tauri::command]
fn companion_preview_plan(window: WebviewWindow) -> Result<Vec<&'static str>, String> {
    main_only(&window)?;
    if std::env::args().any(|a| a == "--smoke")
        && std::env::args().any(|a| a == "--smoke-preview")
        && std::env::args().any(|a| a == "--smoke-actions")
    {
        Ok(PREVIEW_ACTIONS.to_vec())
    } else {
        Ok(vec![])
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreviewProbe {
    width: u16,
    height: u16,
    corner: u8,
    center: u8,
    panel: bool,
    #[serde(default)]
    reduced: bool,
    #[serde(default)]
    selected: bool,
    #[serde(default)]
    ready: u8,
    #[serde(default)]
    network: u8,
    #[serde(default)]
    media_error: u8,
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    playing: bool,
    action: Option<String>,
}
#[tauri::command]
fn companion_preview_mode(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
) -> Result<bool, String> {
    main_only(&window)?;
    let enabled = std::env::args().any(|a| a == "--smoke")
        && std::env::args().any(|a| a == "--smoke-preview");
    if enabled {
        let store = store.lock().map_err(|_| "状态忙")?;
        atomic_write(&store.root.join("preview-start.json"), b"true")?;
    }
    Ok(enabled)
}
#[tauri::command]
fn companion_preview_report(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
    probe: PreviewProbe,
) -> Result<(), String> {
    main_only(&window)?;
    if !std::env::args().any(|a| a == "--smoke")
        || !std::env::args().any(|a| a == "--smoke-preview")
    {
        return Err("仅供隔离预览".into());
    }
    let store = store.lock().map_err(|_| "状态忙")?;
    let file = match &probe.action {
        Some(action) if PREVIEW_ACTIONS.contains(&action.as_str()) => {
            format!("preview-{action}.json")
        }
        Some(_) => return Err("未知测试动作".into()),
        None => "preview.json".into(),
    };
    atomic_write(
        &store.root.join(file),
        &serde_json::to_vec(&probe).map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
fn companion_media_list(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
) -> Result<Vec<companion_media::InstalledPack>, String> {
    main_only(&window)?;
    let root = store.lock().map_err(|_| "状态忙")?.root.join("packs");
    Ok(companion_media::list(&root))
}
#[tauri::command]
async fn companion_media_import(
    window: WebviewWindow,
    store: State<'_, Mutex<Store>>,
) -> Result<Option<companion_media::InstalledPack>, String> {
    main_only(&window)?;
    let data_root = store.lock().map_err(|_| "状态忙")?.root.clone();
    let root = data_root.join("packs");
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    let callback_root = data_root.clone();
    companion_runtime::record(&data_root, "media.selection_opened", now());
    window
        .app_handle()
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("选择 PenguinPal 动画包 manifest.json")
        .add_filter("动画清单", &["json"])
        .pick_file(move |file| {
            companion_runtime::record(
                &callback_root,
                if file.is_some() {
                    "media.selection_chosen"
                } else {
                    "media.selection_cancelled"
                },
                now(),
            );
            let _ = sender.try_send(file);
        });
    let Some(file) = receiver.recv().await.ok_or("文件选择窗口未返回结果")? else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        let path = file.into_path().map_err(|_| "请选择本地清单文件")?;
        let result = companion_media::import(&path, &root).map(Some);
        companion_runtime::record(
            &data_root,
            if result.is_ok() {
                "media.imported"
            } else {
                "media.import_error"
            },
            now(),
        );
        result
    })
    .await
    .map_err(|_| "动画导入任务失败")?
}
#[tauri::command]
fn companion_media_folder(window: WebviewWindow, store: State<Mutex<Store>>) -> Result<(), String> {
    main_only(&window)?;
    let root = store.lock().map_err(|_| "状态忙")?.root.join("packs");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    companion_platform::open_folder(&root)
}

fn codex_home() -> Result<PathBuf, String> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
        .ok_or_else(|| "无法定位 Codex 配置目录".into())
}
fn node_path() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .map(|p| p.join("node.exe"))
        .find(|p| p.is_file())
}

#[tauri::command]
fn companion_health(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
) -> Result<serde_json::Value, String> {
    main_only(&window)?;
    let store = store.lock().map_err(|_| "状态忙")?;
    let configured = codex_home()
        .ok()
        .and_then(|h| fs::read(h.join("hooks.json")).ok())
        .filter(|b| b.len() <= 1024 * 1024)
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("hooks").and_then(|h| h.as_object()).cloned())
        .map(|hooks| {
            hooks
                .values()
                .filter_map(|v| v.as_array())
                .flatten()
                .filter_map(|g| g["hooks"].as_array())
                .flatten()
                .filter(|h| h["statusMessage"] == "PenguinPal 本地状态")
                .count()
        })
        .unwrap_or(0);
    let script_current = fs::read(store.root.join("codex-events.mjs"))
        .map(|b| b == include_bytes!("../resources/companion/codex-events.mjs"))
        .unwrap_or(false);
    let legacy_root = window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位旧数据目录")?;
    Ok(
        serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"safeMode":store.runtime.safe_mode,"crashStreak":store.runtime.crash_streak,"startedAt":store.runtime.started_at,
        "restoredBackup":store.restored_backup,"nodeAvailable":node_path().is_some(),"configuredHooks":configured,"scriptCurrent":script_current,"bridge":store.last_bridge,"rejectedEvents":store.bridge.rejected,
        "dataDirectory":store.root,"legacyFileCount":companion_data::list(&legacy_root).len(),"metrics":companion_platform::metrics(),"logs":companion_runtime::recent_logs(&store.root)}),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum SupportAction {
    ResetRecovery,
    OpenLogs,
    OpenExports,
    OpenUpdates,
    OpenReleases,
}
#[tauri::command]
fn companion_support(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
    action: SupportAction,
) -> Result<(), String> {
    main_only(&window)?;
    let mut store = store.lock().map_err(|_| "状态忙")?;
    let folder = match action {
        SupportAction::ResetRecovery => return store.runtime.leave_safe_mode(now()),
        SupportAction::OpenReleases => return companion_platform::open_releases(),
        SupportAction::OpenLogs => "logs",
        SupportAction::OpenExports => "exports",
        SupportAction::OpenUpdates => "updates",
    };
    let path = store.root.join(folder);
    fs::create_dir_all(&path).map_err(|_| "无法创建文件夹")?;
    companion_platform::open_folder(&path)
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum DataAction {
    List,
    Read,
    Export,
}
#[tauri::command]
fn companion_legacy(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
    action: DataAction,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    main_only(&window)?;
    let root = window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位旧资料目录")?;
    match action {
        DataAction::List => Ok(serde_json::json!({"files":companion_data::list(&root)})),
        DataAction::Read => {
            if ids.len() != 1 {
                return Err("请选择一个文件查看".into());
            }
            Ok(serde_json::json!({"content":companion_data::read(&root,&ids[0])?}))
        }
        DataAction::Export => {
            let store = store.lock().map_err(|_| "状态忙")?;
            let path = companion_data::export(&root, &store.root.join("exports"), &ids, now())?;
            companion_runtime::record(&store.root, "legacy.exported", now());
            Ok(serde_json::json!({"path":path}))
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum UpdateAction {
    Check,
    Download,
}
#[tauri::command]
async fn companion_update(
    window: WebviewWindow,
    store: State<'_, Mutex<Store>>,
    action: UpdateAction,
) -> Result<serde_json::Value, String> {
    main_only(&window)?;
    match action {
        UpdateAction::Check => {
            let (info, package) = companion_updates::check().await?;
            let mut store = store.lock().map_err(|_| "状态忙")?;
            store.update = package;
            companion_runtime::record(&store.root, "update.checked", now());
            Ok(serde_json::to_value(info).unwrap())
        }
        UpdateAction::Download => {
            let (root, package) = {
                let store = store.lock().map_err(|_| "状态忙")?;
                (
                    store.root.clone(),
                    store.update.clone().ok_or("请先检查 Companion 更新")?,
                )
            };
            let path = companion_updates::download(&root, &package).await?;
            companion_runtime::record(&root, "update.verified", now());
            Ok(serde_json::json!({"path":path}))
        }
    }
}

fn merge_hooks(mut config: serde_json::Value, command: &str) -> Result<serde_json::Value, String> {
    let object = config
        .as_object_mut()
        .ok_or("hooks.json 必须是 JSON 对象；未覆盖")?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("hooks 格式无效；未覆盖")?;
    for event in [
        "UserPromptSubmit",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
        "Interrupt",
        "SessionEnd",
    ] {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or("hooks 事件格式无效；未覆盖")?;
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(|v| v.as_array_mut()) {
                handlers.retain(|h| {
                    h.get("statusMessage").and_then(|v| v.as_str()) != Some("PenguinPal 本地状态")
                });
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(true)
        });
        groups.push(
            serde_json::json!({"hooks":[{"type":"command", "command":command,
            "commandWindows":command, "async":event == "PostToolUse", "timeout":3, "statusMessage":"PenguinPal 本地状态"}]}),
        );
    }
    Ok(config)
}

fn install_hooks(root: &Path) -> Result<String, String> {
    let profile = std::env::var_os("USERPROFILE").ok_or("无法定位用户目录")?;
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(profile).join(".codex"));
    let node = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .map(|p| p.join("node.exe"))
        .find(|p| p.is_file())
        .ok_or("安装联动需要 Node.js；当前 PATH 中未找到 node.exe。")?;
    let script = root.join("codex-events.mjs");
    // All paths are quoted as PowerShell literals, then encoded; event input never becomes code.
    let literal = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    let ps = format!(
        "& {} {} {} 'companion-v{}'",
        literal(&node),
        literal(&script),
        literal(root),
        env!("CARGO_PKG_VERSION")
    );
    let encoded = STANDARD.encode(
        ps.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let command = format!("powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand {encoded}");
    let path = codex_home.join("hooks.json");
    let previous = match fs::read(&path) {
        Ok(v) => Some(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let config = match &previous {
        Some(v) => {
            serde_json::from_slice(v).map_err(|_| "已有 hooks.json 不是有效 JSON；未覆盖")?
        }
        None => serde_json::json!({}),
    };
    let merged = merge_hooks(config, &command)?;
    let current = fs::read(&path).ok();
    if current.as_ref() != previous.as_ref() {
        return Err("hooks.json 在安装期间发生变化，请重新安装联动".into());
    }
    atomic_write(
        &script,
        include_bytes!("../resources/companion/codex-events.mjs"),
    )?;
    if let Some(bytes) = previous {
        atomic_write(
            &codex_home.join(format!("hooks.penguinpal-{}.bak", now())),
            &bytes,
        )?;
    }
    atomic_write(&path, &serde_json::to_vec_pretty(&merged).unwrap())?;
    companion_runtime::record(root, "hooks.installed", now());
    Ok("联动已安装。请在 Codex 的 /hooks 中查看并信任 PenguinPal 本地状态 hooks；需要时重新打开会话。".into())
}
#[tauri::command]
fn companion_install_hooks(
    window: WebviewWindow,
    store: State<Mutex<Store>>,
) -> Result<String, String> {
    main_only(&window)?;
    install_hooks(&store.lock().map_err(|_| "状态忙")?.root)
}

pub fn run() {
    if std::env::args().any(|arg| arg == "--install-codex-hooks") {
        let result = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "缺少 LOCALAPPDATA".to_string())
            .and_then(|dir| {
                install_hooks(&PathBuf::from(dir).join("com.penguinpal.app/companion"))
            });
        match result {
            Ok(message) => println!("{message}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let smoke_preview = std::env::args().any(|a| a == "--smoke")
        && std::env::args().any(|a| a == "--smoke-preview");
    // A bounded, explicit preview gets its own state and never contends with the user's pet.
    let _instance = if smoke_preview {
        None
    } else {
        Some(match companion_platform::claim_instance() {
            Ok(Some(guard)) => guard,
            Ok(None) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        })
    };
    let mut context = tauri::generate_context!();
    if smoke_preview {
        context.config_mut().app.windows[0].data_directory =
            Some(format!("smoke-preview-{}", std::process::id()).into());
    }
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let root = app.path().app_local_data_dir()?.join("companion");
            let root = if smoke_preview {
                root.join(format!("smoke-preview-{}", std::process::id()))
            } else {
                root
            };
            fs::create_dir_all(&root)?;
            preserve_pre_media_state(&root).map_err(std::io::Error::other)?;
            let restored_backup = read_state(&root.join("state.json")).is_none()
                && read_state(&root.join("state.json.bak")).is_some();
            let mut state = load_state(&root);
            if let Ok(enabled) = app.autolaunch().is_enabled() {
                state.preferences.autostart = enabled;
            }
            persist(&root, &state).map_err(std::io::Error::other)?;
            let runtime =
                companion_runtime::Runtime::start(&root, now()).map_err(std::io::Error::other)?;
            if restored_backup {
                companion_runtime::record(&root, "state.recovered_backup", now());
            }
            let crash_root = root.clone();
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                companion_runtime::record(&crash_root, "panic", now());
                previous(info);
            }));
            app.manage(Mutex::new(Store {
                root,
                state,
                bridge: Bridge::default(),
                last_bridge: BridgeSummary::default(),
                runtime,
                restored_backup,
                update: None,
            }));
            let show = MenuItem::with_id(app, "show", "显示/隐藏企鹅", true, None::<&str>)?;
            let controls = MenuItem::with_id(app, "controls", "快速控制", true, None::<&str>)?;
            let codex = MenuItem::with_id(app, "codex", "打开 Codex", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出企鹅", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &controls, &codex, &quit])?;
            let mut tray = TrayIconBuilder::with_id("penguinpal-companion")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    "controls" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.emit("companion-panel", ());
                        }
                    }
                    "codex" => {
                        let _ = companion_platform::open_codex();
                    }
                    "quit" => app.exit(0),
                    _ => (),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick { .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if std::env::args().any(|arg| arg == "--smoke") {
                let handle = app.handle().clone();
                let seconds = std::env::var("PENGUINPAL_SMOKE_SECONDS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(3)
                    .clamp(1, 600);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(seconds));
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            companion_load,
            companion_save,
            companion_poll,
            companion_open_codex,
            companion_quit,
            companion_install_hooks,
            companion_health,
            companion_support,
            companion_legacy,
            companion_update,
            companion_media_list,
            companion_media_import,
            companion_media_folder,
            companion_preview_report,
            companion_preview_mode,
            companion_preview_plan,
            companion_video_issue
        ])
        .build(context)
        .expect("PenguinPal companion failed");
    application.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(store) = app.try_state::<Mutex<Store>>() {
                if let Ok(store) = store.lock() {
                    store.runtime.finish(now());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_diagnostics_reject_arbitrary_payloads() {
        for label in [
            "blocked", "play", "timeout", "metadata", "aborted", "network", "decode", "format",
            "load",
        ] {
            assert!(serde_json::from_value::<VideoIssue>(serde_json::json!(label)).is_ok());
        }
        assert!(serde_json::from_value::<VideoIssue>(serde_json::json!(
            "file:///private/video.webm"
        ))
        .is_err());
        assert!(
            serde_json::from_value::<VideoIssue>(serde_json::json!({"path":"private"})).is_err()
        );
    }
    #[test]
    fn pre_media_checkpoint_preserves_old_bytes_and_never_overwrites() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let old = br#"{"schema":1,"energy":62,"preferences":{"quiet":true}}"#;
        fs::write(root.join("state.json"), old).unwrap();
        preserve_pre_media_state(root).unwrap();
        assert_eq!(fs::read(root.join("state.pre-0.4.json")).unwrap(), old);
        persist(root, &PetState::default()).unwrap();
        preserve_pre_media_state(root).unwrap();
        assert_eq!(fs::read(root.join("state.pre-0.4.json")).unwrap(), old);
        assert!(serde_json::from_slice::<serde_json::Value>(
            &fs::read(root.join("state.pre-0.4.json")).unwrap()
        )
        .unwrap()["preferences"]
            .get("animationPack")
            .is_none());
    }
    #[test]
    fn corrupt_primary_recovers_backup_and_rejects_invalid_state() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let mut state = PetState::default();
        state.energy = 62.0;
        persist(root, &state).unwrap();
        state.energy = 44.0;
        persist(root, &state).unwrap();
        fs::write(root.join("state.json"), b"{broken").unwrap();
        assert_eq!(load_state(root).energy, 62.0);
        state.energy = f64::NAN;
        assert!(persist(root, &state).is_err());
    }
    fn event(kind: EventKind, turn: &str, at: u64) -> HookEvent {
        HookEvent {
            event: kind,
            session: "abc123".into(),
            turn: turn.into(),
            at,
            issue: false,
        }
    }
    #[test]
    fn bridge_rejects_stale_future_and_late_tool_events() {
        let mut bridge = Bridge::default();
        bridge.accept(event(EventKind::UserPromptSubmit, "a", 100), 200);
        bridge.accept(event(EventKind::Stop, "a", 200), 300);
        bridge.accept(event(EventKind::PostToolUse, "a", 300), 400);
        assert_eq!(bridge.sessions["abc123"].event, EventKind::Stop);
        bridge.accept(event(EventKind::UserPromptSubmit, "b", 400), 500);
        bridge.accept(event(EventKind::Stop, "a", 500), 600);
        assert_eq!(bridge.sessions["abc123"].turn, "b");
        bridge.accept(event(EventKind::PermissionRequest, "c", 999999), 600);
        assert_eq!(bridge.sessions["abc123"].turn, "b");
    }
    #[test]
    fn bridge_limits_sessions_and_drops_private_fields() {
        let json = br#"{"event":"Stop","session":"abc","turn":"def","at":1,"prompt":"private"}"#;
        assert!(serde_json::from_slice::<HookEvent>(json).is_err());
        let mut bridge = Bridge::default();
        for index in 0..100 {
            let mut e = event(EventKind::UserPromptSubmit, "a", index + 1);
            e.session = format!("{index:x}");
            bridge.accept(e, 101);
        }
        assert_eq!(bridge.sessions.len(), 32);
    }
    #[test]
    fn install_is_idempotent_and_preserves_other_hooks() {
        let old = serde_json::json!({"description":"mine","hooks":{"Stop":[{"hooks":[{"type":"command","command":"keep"}]}]}});
        let once = merge_hooks(old, "new").unwrap();
        let twice = merge_hooks(once.clone(), "new").unwrap();
        assert_eq!(once, twice);
        assert_eq!(twice["hooks"]["Stop"][0]["hooks"][0]["command"], "keep");
        assert_eq!(twice["hooks"]["PostToolUse"][0]["hooks"][0]["async"], true);
        assert_eq!(twice["hooks"]["SessionEnd"][0]["hooks"][0]["async"], false);
        assert!(merge_hooks(serde_json::json!([]), "new").is_err());
    }
    #[test]
    fn tool_issue_expires_and_waiting_has_priority_without_claiming_success() {
        let folder = tempfile::tempdir().unwrap();
        let mut bridge = Bridge::default();
        let mut issue = event(EventKind::PostToolUse, "a", 1000);
        issue.issue = true;
        bridge.accept(issue, 1000);
        assert_eq!(
            bridge.poll_at(folder.path(), true, 2000).status,
            "toolError"
        );
        let mut waiting = event(EventKind::PermissionRequest, "b", 2000);
        waiting.session = "def456".into();
        bridge.accept(waiting, 2000);
        assert_eq!(bridge.poll_at(folder.path(), true, 2000).status, "waiting");
        bridge.accept(event(EventKind::Stop, "a", 3000), 3000);
        assert_eq!(bridge.sessions["abc123"].event.status(), "ended");
        let mut invalid = event(EventKind::Stop, "a", 4000);
        invalid.issue = true;
        bridge.accept(invalid, 4000);
        assert_eq!(bridge.rejected, 1);
        let mut second = Bridge::default();
        let mut issue = event(EventKind::PostToolUse, "c", 1000);
        issue.issue = true;
        second.accept(issue, 1000);
        assert_eq!(second.poll_at(folder.path(), true, 11000).status, "working");
    }
    #[test]
    fn preference_migration_and_bounds_are_enforced() {
        let state: PetState =
            serde_json::from_str(r#"{"schema":1,"preferences":{"wander":false}}"#).unwrap();
        assert_eq!(state.preferences.size, 1.0);
        assert!(state.preferences.hover_pause);
        let mut invalid = state.clone();
        invalid.preferences.size = 9.0;
        assert!(invalid.validate().is_err());
        invalid = state;
        invalid.preferences.walk_speed = f64::NAN;
        assert!(invalid.validate().is_err());
        assert!(
            serde_json::from_str::<PetState>(r#"{"preferences":{"movement":"execute"}}"#).is_err()
        );
    }
    #[test]
    fn queue_handles_corruption_restart_disable_and_late_events() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        fs::create_dir(root.join("events")).unwrap();
        fs::write(root.join("events/1.json"), b"{invalid").unwrap();
        fs::write(root.join("events/2.json"), vec![b'x'; 3000]).unwrap();
        fs::write(
            root.join("events/3.json"),
            br#"{"event":"Stop","session":"abc123","turn":"a","at":1000}"#,
        )
        .unwrap();
        let mut bridge = Bridge::default();
        assert_eq!(bridge.poll_at(root, true, 2000).status, "ended");
        assert_eq!(fs::read_dir(root.join("events")).unwrap().count(), 0);
        assert_eq!(bridge.poll_at(root, true, 20000).status, "idle");
        bridge.accept(event(EventKind::PostToolUse, "a", 21000), 22000);
        assert_eq!(bridge.poll_at(root, true, 22000).status, "idle");
        bridge.accept(event(EventKind::UserPromptSubmit, "b", 23000), 24000);
        assert_eq!(bridge.poll_at(root, true, 24000).status, "working");
        let disabled = bridge.poll_at(root, false, 25000);
        assert_eq!(disabled.status, "idle");
        assert_eq!(disabled.last_received, 0);
        // A restarted pet consumes only metadata still inside the bounded TTL.
        fs::write(
            root.join("events/4.json"),
            br#"{"event":"UserPromptSubmit","session":"abc123","turn":"b","at":1}"#,
        )
        .unwrap();
        assert_eq!(
            Bridge::default().poll_at(root, true, 1000000).status,
            "idle"
        );
    }
}
