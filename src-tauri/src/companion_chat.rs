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

#[derive(Clone, Default)]
struct Gate {
    busy: bool,
    quiet: bool,
    fullscreen: bool,
    visible: bool,
}
#[derive(Clone)]
pub struct ChatService {
    running: Arc<AtomicBool>,
    gate: Arc<Mutex<Gate>>,
    started: Instant,
    last_learn: Arc<Mutex<Option<Instant>>>,
}
impl ChatService {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
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

fn acquire(running: &Arc<AtomicBool>) -> Result<Running, String> {
    let started = Instant::now();
    loop {
        if running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(Running(running.clone()));
        }
        if started.elapsed() > Duration::from_secs(95) {
            return Err("消息已保存，企鹅还在思考，请稍后再聊。".into());
        }
        std::thread::sleep(Duration::from_millis(100));
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
fn bridge(service: &Service, mut request: Value) -> Result<Value, String> {
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
    if request["action"] != "recall" {
        request["command"] = json!(codex()?);
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
fn policy_reason(
    g: &Gate,
    cfg: &crate::companion_brain::Config,
    manual: bool,
    hour: u32,
    idle: u32,
) -> Option<&'static str> {
    if g.busy {
        return Some("你正在忙，暂不打扰");
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
        let chance = (cfg.probability * factor
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
        if environment_reason(chat, &current.config, manual).is_none() {
            return result(service, output["reply"].as_str());
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
    if action == "tick" || action == "gate" {
        let mut gate = chat.gate.lock().map_err(|_| "陪伴状态忙。")?;
        *gate = Gate {
            busy: data["busy"].as_bool().unwrap_or(true),
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
    if action != "send" && chat.running.swap(true, Ordering::AcqRel) {
        return if action == "tick" {
            result(&service, None)
        } else {
            Err("企鹅正在想这句话，请稍等。".into())
        };
    }
    let guard = if action == "send" {
        None
    } else {
        Some(Running(chat.running.clone()))
    };
    let report_service = service.clone();
    let report_error = action == "tick" || action == "outreach";
    let outcome = tauri::async_runtime::spawn_blocking(move||{
        // Persist a user's response immediately, even while an opener is being generated.
        // This cancels its reservation before waiting for the single inference slot.
        let mut user_source=None;
        let _guard=if let Some(guard)=guard {guard}else{
            let prompt=data["text"].as_str().ok_or("请输入内容。")?;
            service.record("social",json!({"type":"user","text":prompt}))?;
            let view=service.status()?;
            let session=pet_session(view["document"]["petId"].as_str().ok_or("宠物身份无效。")?);
            user_source=view["document"]["conversations"].as_array().unwrap().iter().rev().find(|m|m["session"]==session&&m["role"]=="user").cloned();
            let _=sync_if_server(&service,&view);
            acquire(&chat.running)?
        };
        let mut view=service.status()?;
        match action.as_str(){
            "settings"=>{service.record("social",json!({"type":"settings","enabled":data["enabled"],"proactive":data["proactive"]}))?;let _=sync_if_server(&service,&view);}
            "sessions"=>return bridge(&service,json!({"action":"sessions"})),
            "read"=>{
                let id=data["threadId"].as_str().ok_or("请选择聊天。")?;let output=bridge(&service,json!({"action":"read","threadId":id}))?;
                for m in output["messages"].as_array().ok_or("聊天历史无效。")?{service.record("conversation",m.clone())?;}
                view=service.status()?;
                if social(&view)?.enabled {
                    let items:Vec<Value>=output["messages"].as_array().unwrap().iter().rev().filter(|m|m["role"]=="user").take(8).cloned().collect();
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
                let brain=crate::companion_brain::load(&view["document"])?;
                let recalled=bridge(&service,json!({"action":"recall","document":view["document"],"prompt":data["text"].as_str().unwrap_or(""),"proactive":action=="rehearse"}))?;
                let mut response=result(&service,None)?;
                response["preview"]=json!({"recall":recalled,"motive":brain.config.motive,"reason":environment_reason(&chat,&brain.config,false).unwrap_or("环境允许；实际发送仍需满足共享预约、冷却和概率")});return Ok(response);
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
                let should_learn=state.enabled&&chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?.is_none_or(|t|Instant::now().checked_duration_since(t).is_some_and(|d|d>Duration::from_secs(60)));
                if should_learn{
                    *chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?=Some(Instant::now());
                    if learn_batch(&service,&view).is_err() {
                        // Persist a visible error and back off inference failures for five minutes.
                        *chat.last_learn.lock().map_err(|_|"陪伴状态忙。")?=Some(Instant::now()+Duration::from_secs(240));
                    }
                }
                return outreach(&service,&chat,false);
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
