use crate::{
    companion_platform,
    companion_social::{pet_session, Social},
    companion_sync::{scrub, Service},
};
use chrono::Timelike;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::WebviewWindow;
#[path = "companion_scheduler.rs"]
mod scheduler;
use scheduler::{Scheduler, Task};

#[derive(Clone, Default)]
struct Gate {
    busy: bool,
    working: bool,
    quiet: bool,
    fullscreen: bool,
    visible: bool,
}
#[derive(Clone)]
pub struct ChatService {
    scheduler: Scheduler,
    tick_running: Arc<AtomicBool>,
    gate: Arc<Mutex<Gate>>,
    started: Instant,
    last_learn: Arc<Mutex<Option<Instant>>>,
}
impl ChatService {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
            tick_running: Arc::new(AtomicBool::new(false)),
            gate: Arc::new(Mutex::new(Gate::default())),
            started: Instant::now(),
            last_learn: Arc::new(Mutex::new(None)),
        }
    }
}
struct Running(Arc<AtomicBool>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
fn codex() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEX_BIN")
        .map(PathBuf::from)
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "exe"))
    {
        return Ok(path);
    }
    if let Some(path) = executable("codex.exe") {
        return Ok(path);
    }
    if let Some(root) =
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("OpenAI/Codex/bin"))
    {
        let mut paths: Vec<_> = fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path().join("codex.exe"))
            .filter(|p| p.is_file())
            .collect();
        paths.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
        if let Some(path) = paths.pop() {
            return Ok(path);
        }
    }
    Err("没有找到系统 Codex，请安装并登录 Codex 后重试。".into())
}
fn day_key(at: u64) -> String {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(at);
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d")
        .to_string()
}

fn deepseek_sessions(service: &Service) -> Result<Value, String> {
    let view = service.status()?;
    let mut days: Vec<String> = view["document"]["conversations"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["at"].as_u64())
                .map(day_key)
                .collect()
        })
        .unwrap_or_default();
    days.sort();
    days.dedup();
    days.reverse();
    let sessions: Vec<Value> = days
        .iter()
        .take(30)
        .map(|day| json!({"id": format!("local-{day}"), "name": format!("本地对话 {day}")}))
        .collect();
    Ok(json!({"sessions": sessions}))
}

fn deepseek_read(service: &Service, thread_id: &str) -> Result<Value, String> {
    let day = thread_id
        .strip_prefix("local-")
        .filter(|value| value.len() == 10)
        .ok_or("聊天标识无效。")?;
    let view = service.status()?;
    let messages: Vec<Value> = view["document"]["conversations"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item["at"].as_u64().map(|at| day_key(at) == day).unwrap_or(false))
                .filter_map(|item| {
                    Some(json!({
                        "id": item["id"],
                        "session": thread_id,
                        "role": item["role"],
                        "text": item["text"],
                        "at": item["at"],
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({"messages": messages, "name": format!("本地对话 {day}")}))
}

async fn deepseek_models(llm: &crate::companion_llm::LlmConfig) -> Result<Value, String> {
    let fallback = json!({
        "models": [
            {"id":"deepseek-chat","model":"deepseek-chat","displayName":"DeepSeek Chat（对话 · 兼容别名）","description":"公开 API 通用别名","isDefault":true,"supportedReasoningEfforts":[]},
            {"id":"deepseek-reasoner","model":"deepseek-reasoner","displayName":"DeepSeek Reasoner（推理 · 兼容别名）","description":"公开 API 推理别名","isDefault":false,"supportedReasoningEfforts":[]},
            {"id":"deepseek-flash","model":"deepseek-flash","displayName":"DeepSeek V4.1 Flash","description":"新一代，1M 上下文，需网关支持","isDefault":false,"supportedReasoningEfforts":[]},
            {"id":"deepseek-v4-pro","model":"deepseek-v4-pro","displayName":"DeepSeek V4 Pro","description":"新一代，纯文本，需网关支持","isDefault":false,"supportedReasoningEfforts":[]}
        ],
        "defaultModel": "deepseek-chat",
        "source": "fallback"
    });
    if llm.api_key.trim().is_empty() {
        return Ok(fallback);
    }
    let base = llm.base_url.trim_end_matches('/');
    let response = reqwest::Client::new()
        .get(format!("{base}/models"))
        .bearer_auth(llm.api_key.trim())
        .send()
        .await;
    let Ok(response) = response else {
        return Ok(fallback);
    };
    if !response.status().is_success() {
        return Ok(fallback);
    }
    let value: Value = response.json().await.unwrap_or(Value::Null);
    let models: Vec<Value> = value["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["id"].as_str())
                .map(|id| {
                    json!({
                        "id": id,
                        "model": id,
                        "displayName": id,
                        "description": if id.contains("reasoner") { "推理模型" } else { "DeepSeek 模型" },
                        "isDefault": id == "deepseek-chat",
                        "supportedReasoningEfforts": [],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Ok(fallback);
    }
    let default = models
        .iter()
        .find(|model| model["isDefault"] == json!(true))
        .and_then(|model| model["model"].as_str())
        .unwrap_or("deepseek-chat");
    Ok(json!({"models": models, "defaultModel": default, "source": "api"}))
}

fn bridge(service: &Service, mut request: Value) -> Result<Value, String> {
    if matches!(request["action"].as_str(), Some("reply") | Some("opener")) {
        if let Ok(root) = service.chat_root() {
            if let Some(text) = crate::companion_dsh::digest_text(&root, chrono::Utc::now().timestamp_millis() as u64) {
                request["workDigest"] = json!(text);
            }
        }
    }
    if matches!(request["action"].as_str(), Some("reply") | Some("opener")) {
        if let Ok(brain) = crate::companion_brain::load(&request["document"]) {
            if let Ok(value) = serde_json::to_value(crate::companion_brain::tendency(&request["document"], &brain)) {
                request["tendency"] = value;
            }
        }
    }
    // Recall may run alongside inference. Serialize only module installation.
    static FILES: Mutex<()> = Mutex::new(());
    let files = FILES.lock().map_err(|_| "气泡程序暂不可用。")?;
    let root = service.chat_root()?.join("bubble-engine");
    fs::create_dir_all(&root).map_err(|_| "无法建立气泡运行目录。")?;
    let engine = root.join("chat-engine.mjs");
    let helper = root.join("conversation-events.mjs");
    for (path, source) in [
        (
            &engine,
            include_bytes!("../resources/companion/chat-engine.mjs").as_slice(),
        ),
        (
            &helper,
            include_bytes!("../resources/companion/conversation-events.mjs").as_slice(),
        ),
        (
            &root.join("memory-engine.mjs"),
            include_bytes!("../resources/companion/memory-engine.mjs").as_slice(),
        ),
        (
            &root.join("memory-words.txt"),
            include_bytes!("../resources/companion/memory-words.txt").as_slice(),
        ),
        (
            &root.join("memory-words-LICENSE.txt"),
            include_bytes!("../resources/companion/memory-words-LICENSE.txt").as_slice(),
        ),
    ] {
        if fs::read(path).ok().as_deref() != Some(source) {
            fs::write(path, source).map_err(|_| "无法更新气泡运行程序。")?;
        }
    }
    drop(files);
    if request["action"] != "recall" {
        let llm = crate::companion_llm::load(&service.chat_root()?);
        if llm.is_deepseek() {
            request["provider"] = crate::companion_llm::bridge_value(&llm);
        } else {
            request["command"] = json!(codex()?);
        }
    }
    request["cwd"] = json!(root);
    let node = executable("node.exe").ok_or("气泡聊天需要 PATH 中的 Node.js。")?;
    let mut command = Command::new(node);
    command
        .arg(engine)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "无法启动气泡聊天。")?;
    let mut input = child.stdin.take().ok_or("聊天通道未就绪。")?;
    if input
        .write_all(&serde_json::to_vec(&request).map_err(|_| "聊天数据无法编码。")?)
        .is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err("聊天发送失败。".into());
    }
    drop(input);
    let output = child.wait_with_output().map_err(|_| "聊天后台中断。")?;
    if output.stdout.len() > 1024 * 1024 {
        return Err("聊天返回超过容量上限。".into());
    }
    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "聊天后台没有返回有效数据。")?;
    if let Some(error) = value["error"].as_str() {
        return Err(scrub(error, 200));
    }
    if !output.status.success() {
        return Err("Codex 未完成本轮回复，请检查登录和连接。".into());
    }
    Ok(value)
}
fn social(view: &Value) -> Result<Social, String> {
    serde_json::from_value(
        view["document"]
            .get("social")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|_| "陪伴状态无效。".into())
}
fn result(service: &Service, reply: Option<&str>) -> Result<Value, String> {
    let view = service.status()?;
    let session = pet_session(view["document"]["petId"].as_str().ok_or("宠物身份无效。")?);
    Ok(json!({"status":view,"session":session,"reply":reply}))
}
fn learn(
    service: &Service,
    output: &Value,
    sources: &[Value],
    historical: bool,
) -> Result<(), String> {
    let analyses = output["analyses"].as_array().ok_or("学习结果无效。")?;
    if historical
        && (analyses.len() != sources.len()
            || sources
                .iter()
                .any(|s| !analyses.iter().any(|a| a["sourceId"] == s["id"])))
    {
        return Err("这批对话未完整整理，已保留待重试。".into());
    }
    for item in analyses {
        let source = sources
            .iter()
            .find(|s| s["id"] == item["sourceId"])
            .ok_or("学习来源无效。")?;
        let mut item = item.clone();
        if !historical && !social(&service.status()?)?.enabled {
            item["memories"] = json!([]);
        }
        item["type"] = json!(if historical { "history" } else { "analysis" });
        if historical {
            item["source"] = source.clone();
        }
        // Empty historical analyses need only a local durable acknowledgement.
        if !historical || item["memories"].as_array().is_none_or(|m| !m.is_empty()) {
            service.record("social", item)?;
        }
        service.history_done(source["id"].as_str().ok_or("学习来源无效。")?)?;
    }
    Ok(())
}
fn learn_batch(service: &Service, view: &Value) -> Result<(), String> {
    let work = || {
        let items = service.history_sources()?;
        if !items.is_empty() {
            let output = bridge(
                service,
                json!({"action":"analysis","document":view["document"],"sources":items}),
            )?;
            if social(&service.status()?)?.enabled {
                learn(service, &output, &items, true)?;
            }
            let _ = sync_if_server(service, view);
        }
        Ok(())
    };
    let result: Result<(), String> = work();
    service.history_note(
        "learningError",
        result.as_ref().err().map(String::as_str).unwrap_or(""),
    )?;
    if result.is_ok() {
        service.history_note(
            "lastLearn",
            &chrono::Utc::now().timestamp_millis().to_string(),
        )?;
    }
    result
}
fn scan(service: &Service) -> Result<(), String> {
    let result = service.scan_history();
    service.history_note(
        "scanError",
        result.as_ref().err().map(String::as_str).unwrap_or(""),
    )?;
    result
}
fn sync_if_server(service: &Service, view: &Value) -> Result<(), String> {
    if view["mode"] == "server" {
        service.sync_chat()?;
    }
    Ok(())
}
fn remember_output(service: &Service, output: &Value, consume: bool) -> Result<(), String> {
    if !output["recallReport"].is_null() {
        service.history_note(
            "recallReport",
            &serde_json::to_string(&output["recallReport"]).map_err(|_| "召回解释无效。")?,
        )?;
    }
    if consume
        && output["usedMoodEventId"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        service.record(
            "brain",
            json!({"type":"consume","eventId":output["usedMoodEventId"],"text":output["moodText"]}),
        )?;
    }
    Ok(())
}
fn recall_preview(service: &Service, chat: &ChatService, data: &Value, proactive: bool) -> Result<Value, String> {
    let view = service.status()?;
    let brain = crate::companion_brain::load(&view["document"])?;
    let recalled = bridge(service, json!({"action":"recall","document":view["document"],"prompt":data["text"].as_str().unwrap_or(""),"proactive":proactive}))?;
    let mut response = result(service, None)?;
    response["preview"] = json!({"recall":recalled,"motive":brain.config.motive,"reason":environment_reason(chat,&brain.config,false).unwrap_or("环境允许；实际发送仍需满足共享预约、冷却和概率")});
    Ok(response)
}
fn policy_reason(
    g: &Gate,
    cfg: &crate::companion_brain::Config,
    manual: bool,
    hour: u32,
    idle: u32,
) -> Option<&'static str> {
    if g.busy {
        return Some("桌宠正在被拖动，暂不发话");
    }
    if g.fullscreen {
        return Some("正在全屏，暂不打扰");
    }
    if !g.visible {
        return Some("桌宠暂未显示");
    }
    if manual {
        return None;
    }
    if g.working {
        return Some("Codex 正在工作，已开启工作免打扰");
    }
    if g.quiet {
        return Some("安静模式、拖动或面板打开中");
    }
    let quiet = if cfg.quiet_start == cfg.quiet_end {
        false
    } else if cfg.quiet_start < cfg.quiet_end {
        (cfg.quiet_start..cfg.quiet_end).contains(&hour)
    } else {
        hour >= cfg.quiet_start || hour < cfg.quiet_end
    };
    if quiet {
        return Some("当前是安静时段");
    }
    if idle < 90 {
        return Some("正在使用电脑，等待空闲90秒");
    }
    if idle >= 1800 {
        return Some("离开较久，等你回来再聊");
    }
    None
}
fn environment_reason(
    chat: &ChatService,
    cfg: &crate::companion_brain::Config,
    manual: bool,
) -> Option<&'static str> {
    let Ok(g) = chat.gate.lock() else {
        return Some("环境状态暂不可用");
    };
    policy_reason(
        &g,
        cfg,
        manual,
        chrono::Local::now().hour(),
        companion_platform::desktop(false, false).idle_seconds,
    )
}
fn outreach(service: &Service, chat: &ChatService, manual: bool) -> Result<Value, String> {
    let mut view = service.status()?;
    let initial = crate::companion_brain::load(&view["document"])?;
    let status = |reason: &str| -> Result<Value, String> {
        service.history_note("outreachStatus", reason)?;
        result(service, None)
    };
    if manual && chat.scheduler.manual_cancelled() { return status("手动搭话已取消"); }
    if !manual && !social(&view)?.proactive {
        return status("主动搭话已关闭；仍可手动真发一次");
    }
    if !manual && chat.started.elapsed() < Duration::from_secs(120) {
        return status("启动后先等待两分钟");
    }
    if let Some(reason) = environment_reason(chat, &initial.config, manual) {
        return status(reason);
    }
    if view["mode"] == "server" {
        service.sync_chat()?;
        view = service.status()?;
        if !manual && view["active"] != true {
            return status("另一台设备负责主动陪伴");
        }
    }
    if view["pending"] != 0 {
        return status("等待积压数据同步确认");
    }
    let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let brain = crate::companion_brain::load(&view["document"])?;
    let state = social(&view)?;
    let cfg = &brain.config;
    if !manual {
        if state.day == at / 86400000 && state.sent_today >= cfg.daily_limit {
            return status("今天的自动搭话次数已用完");
        }
        if state.last_outreach_at > 0
            && at.saturating_sub(state.last_outreach_at) < cfg.cooldown_minutes as u64 * 60000
        {
            return status("搭话冷却中；可在记录中查看上次时间");
        }
        if state.reserved_until > at {
            return status("已有一轮搭话正在准备");
        }
        if brain.next_check_at > at {
            return status("等待下一次随机机会");
        }
        let elapsed = at.saturating_sub(state.last_response_at.max(state.last_outreach_at));
        let factor = if state.last_response_at.max(state.last_outreach_at) == 0 {
            1.0
        } else if elapsed < cfg.recent_minutes as u64 * 60000 {
            0.15
        } else if elapsed > cfg.distant_minutes as u64 * 60000 {
            1.5
        } else {
            1.0
        };
        let tendency = crate::companion_brain::tendency(&view["document"], &brain);
        let chance = (cfg.probability * factor * tendency.proactive
            + (brain.misses as f64 * cfg.miss_bonus).min(cfg.bonus_cap))
        .min(100.0);
        let random = uuid::Uuid::new_v4();
        let bytes = random.as_bytes();
        let roll = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            / (u32::MAX as f64 + 1.0)
            * 100.0;
        let reason = if roll >= chance {
            "随机未命中，下次机会按设置增加概率"
        } else if cfg.dry_run {
            "试运行命中：只记记录，不调用模型、不发气泡"
        } else {
            "随机命中，正在准备话题"
        };
        service.record(
            "brain",
            json!({"type":"decision","chance":chance,"roll":roll,"reason":reason}),
        )?;
        sync_if_server(service, &view)?;
        if roll >= chance || cfg.dry_run {
            return status(reason);
        }
    }
    let token = uuid::Uuid::new_v4().to_string();
    if manual && chat.scheduler.manual_cancelled() { return status("手动搭话已取消"); }
    service.record(
        "social",
        json!({"type":"reserve","token":token,"manual":manual}),
    )?;
    sync_if_server(service, &view)?;
    view = service.status()?;
    if view["pending"] != 0 || view["document"]["social"]["reservation"] != token {
        return status("共享预约未取得，本轮未生成");
    }
    let generated = bridge(
        service,
        json!({"action":"opener","document":view["document"],"sources":[],"prompt":cfg.motive}),
    );
    let output = match generated {
        Ok(v) => v,
        Err(e) => {
            service.record("social", json!({"type":"release","token":token}))?;
            let _ = sync_if_server(service, &view);
            return Err(e);
        }
    };
    sync_if_server(service, &view)?;
    view = service.status()?;
    let current = crate::companion_brain::load(&view["document"])?;
    if view["pending"] != 0
        || (manual && chat.scheduler.manual_cancelled())
        || environment_reason(chat, &current.config, manual).is_some()
        || (!manual && current.config.dry_run)
        || view["document"]["social"]["reservation"] != token
        || (view["mode"] == "server" && view["active"] != true)
    {
        service.record("social", json!({"type":"release","token":token}))?;
        let _ = sync_if_server(service, &view);
        return status("环境或共享状态已变化，本轮气泡已取消");
    }
    let previous = view["document"]["social"]["lastOutreachAt"]
        .as_u64()
        .unwrap_or(0);
    service.record("social",json!({"type":"commit","token":token,"text":output["reply"],"topic":output["topic"],"selfMood":output["selfMood"]}))?;
    sync_if_server(service, &view)?;
    view = service.status()?;
    if view["pending"] == 0
        && view["document"]["social"]["lastOutreachAt"]
            .as_u64()
            .unwrap_or(0)
            > previous
        && view["document"]["social"]["lastTopic"] == output["topic"]
    {
        remember_output(service, &output, true)?;
        service.record(
            "brain",
            json!({"type":"log","reason":if manual{"手动搭话已发出"}else{"主动气泡已发出"}}),
        )?;
        let _ = sync_if_server(service, &view);
        service.history_note("outreachError", "")?;
        service.history_note("outreachStatus", "本轮气泡已发出")?;
        if !(manual && chat.scheduler.manual_cancelled()) && environment_reason(chat, &current.config, manual).is_none() {
            let mut response = result(service, output["reply"].as_str())?;
            response["manual"] = json!(manual);
            return Ok(response);
        }
    }
    status("本轮未显示新气泡；可能话题重复或预约已取消")
}
#[tauri::command]
pub async fn companion_chat(
    window: WebviewWindow,
    chat: tauri::State<'_, ChatService>,
    service: tauri::State<'_, Service>,
    action: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("仅允许桌宠主窗口调用。".into());
    }
    let chat = chat.inner().clone();
    let service = service.inner().clone();
    let data = payload.unwrap_or(Value::Null);
    if action == "llmGet" {
        let llm = crate::companion_llm::load(&service.chat_root()?);
        return Ok(crate::companion_llm::public_view(&llm));
    }
    if action == "llmSet" {
        let mut llm = crate::companion_llm::load(&service.chat_root()?);
        if let Some(value) = data["provider"].as_str() { llm.provider = value.to_string(); }
        if let Some(value) = data["model"].as_str() { llm.model = value.to_string(); }
        if let Some(value) = data["baseUrl"].as_str() { llm.base_url = value.to_string(); }
        if let Some(value) = data["apiKey"].as_str() {
            if !value.trim().is_empty() { llm.api_key = value.to_string(); }
        }
        if data["clearKey"].as_bool() == Some(true) { llm.api_key = String::new(); }
        let saved = crate::companion_llm::save(&service.chat_root()?, &llm)?;
        service.history_note("learningError", "")?;
        return Ok(crate::companion_llm::public_view(&saved));
    }
    if action == "models" {
        let llm = crate::companion_llm::load(&service.chat_root()?);
        if llm.is_deepseek() {
            return deepseek_models(&llm).await;
        }
    }
    if action == "sessions" {
        let llm = crate::companion_llm::load(&service.chat_root()?);
        if llm.is_deepseek() {
            return deepseek_sessions(&service);
        }
    }
    if action == "runtime" { return serde_json::to_value(chat.scheduler.status()?).map_err(|_| "调度状态无效。".into()); }
    if action == "cancelOutreach" { chat.scheduler.cancel_manual(); return Ok(json!({"ok":true})); }
    if action == "tick" || action == "gate" {
        let mut gate = chat.gate.lock().map_err(|_| "陪伴状态忙。")?;
        *gate = Gate {
            busy: data["busy"].as_bool().unwrap_or(true),
            working: data["working"].as_bool().unwrap_or(true),
            quiet: data["quiet"].as_bool().unwrap_or(true),
            fullscreen: data["fullscreen"].as_bool().unwrap_or(true),
            visible: data["visible"].as_bool().unwrap_or(false),
        };
    }
    if action == "gate" {
        return Ok(json!({"ok":true}));
    }
    if action == "status" {
        return result(&service, None);
    }
    if action == "send" {
        chat.gate.lock().map_err(|_| "陪伴状态忙。")?.quiet = true;
    }
    // Only one scan/tick runs, but manual actions and local previews stay available.
    let tick_guard = if action == "tick" {
        if chat.tick_running.swap(true, Ordering::AcqRel) { return result(&service, None); }
        Some(Running(chat.tick_running.clone()))
    } else { None };
    // Register before spawning so even immediate cancellation sees this request.
    let manual_request = if action == "outreach" { Some(chat.scheduler.reserve(Task::Manual)?) } else { None };
    let report_service = service.clone();
    let report_error = action == "tick" || action == "outreach";
    let outcome = tauri::async_runtime::spawn_blocking(move||{
        let _tick_guard = tick_guard;
        // Persist a user's response immediately, even while an opener is being generated.
        // This cancels its reservation before waiting for the single inference slot.
        let mut user_source=None;
        if action == "send" {
            let prompt=data["text"].as_str().ok_or("请输入内容。")?;
            service.record("social",json!({"type":"user","text":prompt}))?;
            let view=service.status()?;
            let session=pet_session(view["document"]["petId"].as_str().ok_or("宠物身份无效。")?);
            user_source=view["document"]["conversations"].as_array().unwrap().iter().rev().find(|m|m["session"]==session&&m["role"]=="user").cloned();
        }
        let task = match action.as_str() { "send" => Some(Task::Reply), "sessions"|"models"|"read"|"learn"|"previewMemories" => Some(Task::Interactive), _ => None };
        let _inference = if let Some(request)=manual_request { Some(request.wait(Duration::from_secs(150))?) } else { task.map(|task| chat.scheduler.acquire(task, Duration::from_secs(150))).transpose()? };
        let mut view=service.status()?;
        match action.as_str(){
            "settings"=>{service.record("social",json!({"type":"settings","enabled":data["enabled"],"proactive":data["proactive"]}))?;let _=sync_if_server(&service,&view);}
            "sessions"=>return bridge(&service,json!({"action":"sessions"})),
            "models"=>return bridge(&service,json!({"action":"models"})),
            "read"=>{
                let id=data["threadId"].as_str().ok_or("请选择聊天。")?;
                let llm = crate::companion_llm::load(&service.chat_root()?);
                let output=if llm.is_deepseek(){deepseek_read(&service,id)?}else{bridge(&service,json!({"action":"read","threadId":id}))?};
                let known: std::collections::HashSet<String> = view["document"]["conversations"].as_array().map(|items| items.iter().filter_map(|m| m["id"].as_str().map(|id| id.to_string())).collect()).unwrap_or_default();
                for m in output["messages"].as_array().ok_or("聊天历史无效。")?{service.record("conversation",m.clone())?;}
                view=service.status()?;
                if social(&view)?.enabled {
                    let items:Vec<Value>=output["messages"].as_array().unwrap().iter().rev().filter(|m|m["role"]=="user"&&!known.contains(m["id"].as_str().unwrap_or(""))).take(8).cloned().collect();
                    if !items.is_empty(){let generated=bridge(&service,json!({"action":"analysis","document":view["document"],"sources":items}))?;if social(&service.status()?)?.enabled{learn(&service,&generated,&items,true)?;}}
                }
                let _=sync_if_server(&service,&view);
            }
            "send"=>{
                let prompt=data["text"].as_str().ok_or("请输入内容。")?;
                let _=sync_if_server(&service,&view);
                view=service.status()?;
                let source=user_source.ok_or("用户消息未保存。")?;
                let output=bridge(&service,json!({"action":"reply","document":view["document"],"prompt":scrub(prompt,3000),"sources":[source.clone()]}))?;
                learn(&service,&output,&[source],false)?;
                let reply=output["reply"].as_str().filter(|s|!s.trim().is_empty()).ok_or("回复为空。")?;
                service.record("social",json!({"type":"reply","text":reply}))?;remember_output(&service,&output,true)?;let _=sync_if_server(&service,&view);return result(&service,Some(reply));
            }
            "brain"=>{service.record("brain",data)?;let _=sync_if_server(&service,&view);}
            "outreach"=>return outreach(&service,&chat,true),
            "recall"|"rehearse"=>{
                return recall_preview(&service,&chat,&data,action=="rehearse");
            }
            "previewMemories"=>{
                let mut sources:Vec<Value>=serde_json::from_value(data["sources"].clone()).map_err(|_|"资料格式无效。")?;
                if sources.is_empty()||sources.len()>8||sources.iter().any(|m|m["text"].as_str().is_none_or(|s|s.chars().count()>6000)||m["role"]!="user"){return Err("每批请选择1至8段资料，每段最多6000字。".into())}
                for source in &mut sources {source["text"]=json!(scrub(source["text"].as_str().unwrap_or(""),6000));}
                let generated=bridge(&service,json!({"action":"analysis","document":view["document"],"sources":sources}))?;
                return Ok(json!({"sources":sources,"analyses":generated["analyses"]}));
            }
            "importMemories"=>{
                let sources:Vec<Value>=serde_json::from_value(data["sources"].clone()).map_err(|_|"资料格式无效。")?;
                if sources.is_empty()||sources.len()>8{return Err("一次最多导入八段资料。".into())}
                learn(&service,&data,&sources,true)?;let _=sync_if_server(&service,&view);
            }
            "history"=>return service.history_search(&data),
            "learn"=>{
                scan(&service)?;
                view=service.status()?;
                if !social(&view)?.enabled { return Err("请先开启自动学习。".into()); }
                learn_batch(&service,&view)?;
            }
            "tick"=>{
                // Archive and learning are independent of the gate for visible proactive bubbles.
                // A server outage leaves extracted memories in the existing durable sync queue.
                let _=scan(&service);
                view=service.status()?;
                let state=social(&view)?;
                let mut outreach_error=None;
                // Give a due opener the next free slot before starting more learning.
                if let Some(_slot)=chat.scheduler.try_acquire(Task::Automatic) {
                    match outreach(&service,&chat,false) {
                        Ok(response) if response["reply"].as_str().is_some_and(|s|!s.is_empty()) => return Ok(response),
                        Ok(_) => (),
                        Err(error) => { service.history_note("outreachError",&error)?; outreach_error=Some(error); }
                    }
                }
                let should_learn=state.enabled&&chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?.is_none_or(|t|Instant::now().checked_duration_since(t).is_some_and(|d|d>Duration::from_secs(60)));
                if let Some(_slot)=should_learn.then(||chat.scheduler.try_acquire(Task::Learning)).flatten(){
                    *chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?=Some(Instant::now());
                    if learn_batch(&service,&view).is_err() {
                        // Persist a visible error and back off inference failures for five minutes.
                        *chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?=Some(Instant::now()+Duration::from_secs(240));
                    }
                }
                // Recheck after a long learning batch; queued manual work goes first.
                if let Some(error)=outreach_error { return Err(error); }
                if let Some(_slot)=chat.scheduler.try_acquire(Task::Automatic) { return outreach(&service,&chat,false); }
                return result(&service,None);
            }
            _=>return Err("不支持的气泡操作。".into()),
        }
        result(&service,None)
    }).await.map_err(|_|"气泡任务中断，已保存的数据仍保留。".to_string())?;
    if report_error {
        if let Err(error) = &outcome {
            report_service.history_note("outreachError", error)?;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_manual_opener_bypasses_work_policy_but_not_hidden_or_fullscreen() {
        let config = crate::companion_brain::Config::default();
        let gate = Gate { working: true, quiet: true, visible: true, ..Default::default() };
        assert!(policy_reason(&gate, &config, true, 23, 0).is_none());
        assert_eq!(policy_reason(&gate, &config, false, 12, 120), Some("Codex 正在工作，已开启工作免打扰"));
        assert!(policy_reason(&Gate { visible: false, ..gate.clone() }, &config, true, 12, 120).is_some());
        assert!(policy_reason(&Gate { fullscreen: true, ..gate }, &config, true, 12, 120).is_some());
    }
    #[test]
    #[cfg(windows)]
    fn local_rehearsal_succeeds_during_learning_without_reservation_or_inference() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path(), json!({})).unwrap();
        service.record("memory", json!({"id":"local-recall","text":"喜欢看星星","at":1,"deleted":false})).unwrap();
        let chat = ChatService::new();
        let _learning = chat.scheduler.try_acquire(Task::Learning).unwrap();
        let before = service.status().unwrap();
        let preview = recall_preview(&service, &chat, &Value::Null, true).unwrap();
        assert_eq!(preview["preview"]["recall"]["memories"].as_array().unwrap().len(), 1);
        assert!(preview["reply"].is_null());
        assert_eq!(preview["status"]["document"]["social"], before["document"]["social"]);
        assert_eq!(preview["status"]["document"]["brain"], before["document"]["brain"]);
        assert_eq!(serde_json::to_value(chat.scheduler.status().unwrap()).unwrap()["active"], "learning");
    }
    fn allowed(hour: u32, idle: u32, gate: &Gate) -> bool {
        policy_reason(
            gate,
            &crate::companion_brain::Config::default(),
            false,
            hour,
            idle,
        )
        .is_none()
    }
    #[test]
    fn configurable_quiet_hours_and_manual_action_use_actual_policy() {
        let gate = Gate {
            visible: true,
            quiet: true,
            ..Default::default()
        };
        let mut cfg = crate::companion_brain::Config::default();
        assert!(policy_reason(&gate, &cfg, true, 2, 0).is_none());
        assert!(policy_reason(
            &Gate {
                busy: true,
                ..gate.clone()
            },
            &cfg,
            true,
            2,
            0
        )
        .is_some());
        assert!(policy_reason(
            &Gate {
                fullscreen: true,
                ..gate.clone()
            },
            &cfg,
            true,
            2,
            0
        )
        .is_some());
        cfg.quiet_start = 10;
        cfg.quiet_end = 12;
        let gate = Gate {
            quiet: false,
            ..gate
        };
        assert_eq!(
            policy_reason(&gate, &cfg, false, 10, 120),
            Some("当前是安静时段")
        );
        assert!(policy_reason(&gate, &cfg, false, 12, 120).is_none());
        cfg.quiet_start = 0;
        cfg.quiet_end = 0;
        assert!(policy_reason(&gate, &cfg, false, 2, 120).is_none());
    }
    #[test]
    fn realtime_mood_and_automatic_memory_can_be_controlled_independently() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path(), json!({})).unwrap();
        service
            .record(
                "social",
                json!({"type":"settings","enabled":false,"proactive":false}),
            )
            .unwrap();
        service
            .record(
                "social",
                json!({"type":"user","text":"我喜欢小鱼，谢谢你陪我"}),
            )
            .unwrap();
        let source = service.status().unwrap()["document"]["conversations"][0].clone();
        let analysis = json!({"sourceId":source["id"],"memories":[{"kind":"preference","key":"food","text":"喜欢小鱼","quote":"我喜欢小鱼"}],"delta":2,"dims":{"happy":2},"confidence":80,"reason":"被感谢","quote":"谢谢你陪我"});
        learn(&service, &json!({"analyses":[analysis]}), &[source], false).unwrap();
        let view = service.status().unwrap();
        assert!(view["document"]["memories"].as_array().unwrap().is_empty());
        assert_eq!(view["document"]["pet"]["mood"], 72.0);
    }
    #[test]
    fn idle_policy_covers_work_night_away_and_quiet() {
        let gate = Gate {
            visible: true,
            ..Default::default()
        };
        assert!(allowed(12, 120, &gate));
        assert!(!allowed(12, 89, &gate));
        assert!(allowed(8, 90, &gate));
        assert!(allowed(21, 1799, &gate));
        assert!(!allowed(22, 120, &gate));
        assert!(!allowed(12, 1800, &gate));
        assert!(!allowed(23, 120, &gate));
        assert!(!allowed(7, 120, &gate));
        assert!(!allowed(12, 10, &gate));
        assert!(!allowed(12, 2000, &gate));
        assert!(!allowed(
            12,
            120,
            &Gate {
                busy: true,
                ..gate.clone()
            }
        ));
        assert!(!allowed(
            12,
            120,
            &Gate {
                quiet: true,
                ..gate.clone()
            }
        ));
        assert!(!allowed(
            12,
            120,
            &Gate {
                fullscreen: true,
                ..gate.clone()
            }
        ));
    }
    #[test]
    fn backlog_is_learned_after_recent_eviction_and_acknowledged_only_after_success() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path(), json!({})).unwrap();
        for n in 0..510 {
            service.record("conversation",json!({"id":format!("u-{n}"),"session":"project-or-outside","role":"user","text":format!("我喜欢清淡小鱼 {n}"),"at":n+1})).unwrap();
        }
        let view = service.status().unwrap();
        assert_eq!(
            view["document"]["conversations"].as_array().unwrap().len(),
            500
        );
        assert_eq!(view["history"]["pending"], 510);
        let items = service.history_sources().unwrap();
        assert_eq!(items[0]["id"], "u-0");
        assert!(learn(&service, &json!({"analyses":[]}), &items, true).is_err());
        assert_eq!(service.status().unwrap()["history"]["pending"], 510);
        let analyses:Vec<Value>=items.iter().enumerate().map(|(n,m)|json!({"sourceId":m["id"],"memories":if n==0 {vec![json!({"kind":"preference","key":"饮食","text":"喜欢清淡小鱼","quote":"我喜欢清淡小鱼"})]}else{vec![]},"delta":3,"reason":"旧日互动","quote":"我喜欢清淡小鱼"})).collect();
        learn(&service, &json!({"analyses":analyses}), &items, true).unwrap();
        let view = service.status().unwrap();
        assert_eq!(view["history"]["pending"], 502);
        assert_eq!(view["document"]["memories"][0]["at"], 1);
        assert!(view["document"]["memories"][0]["text"]
            .as_str()
            .unwrap()
            .contains("来源：u-0"));
        assert_eq!(view["document"]["pet"]["mood"], 70.0);
        drop(service);
        let restarted = Service::new(root.path(), json!({})).unwrap();
        assert_eq!(restarted.status().unwrap()["history"]["pending"], 502);
        assert_eq!(restarted.history_sources().unwrap()[0]["id"], "u-8");
    }
}
