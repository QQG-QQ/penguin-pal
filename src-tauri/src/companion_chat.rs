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
    last_try: Arc<Mutex<Option<Instant>>>,
}
impl ChatService {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            gate: Arc::new(Mutex::new(Gate::default())),
            started: Instant::now(),
            last_learn: Arc::new(Mutex::new(None)),
            last_try: Arc::new(Mutex::new(None)),
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
    ] {
        if fs::read(path).ok().as_deref() != Some(source) {
            fs::write(path, source).map_err(|_| "无法更新气泡运行程序。")?;
        }
    }
    request["command"] = json!(codex()?);
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
fn allowed(hour: u32, idle: u32, gate: &Gate) -> bool {
    (8..22).contains(&hour)
        && (90..1800).contains(&idle)
        && !gate.busy
        && !gate.quiet
        && !gate.fullscreen
        && gate.visible
}
fn environment_ok(chat: &ChatService) -> bool {
    chat.gate.lock().is_ok_and(|g| {
        allowed(
            chrono::Local::now().hour(),
            companion_platform::desktop(false, false).idle_seconds,
            &g,
        )
    })
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
    let report_error = action == "tick";
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
                if social(&service.status()?)?.enabled{learn(&service,&output,&[source],false)?;}
                let reply=output["reply"].as_str().filter(|s|!s.trim().is_empty()).ok_or("回复为空。")?;
                service.record("social",json!({"type":"reply","text":reply}))?;let _=sync_if_server(&service,&view);return result(&service,Some(reply));
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
                    view=service.status()?;
                }
                if chat.started.elapsed()<Duration::from_secs(120)||!environment_ok(&chat){return result(&service,None);}
                if view["mode"]=="server"{service.sync_chat()?;view=service.status()?;if view["active"]!=true{return result(&service,None);}}
                let now=chrono::Utc::now().timestamp_millis().max(0) as u64;
                if !social(&view)?.can_send(now)||!environment_ok(&chat)||chat.last_try.lock().map_err(|_|"陪伴状态忙。")?.is_some_and(|t|t.elapsed()<Duration::from_secs(600)){return result(&service,None);}
                *chat.last_try.lock().map_err(|_|"陪伴状态忙。")?=Some(Instant::now());
                let token=uuid::Uuid::new_v4().to_string();service.record("social",json!({"type":"reserve","token":token}))?;
                sync_if_server(&service,&view)?;view=service.status()?;
                if view["pending"]!=0||view["document"]["social"]["reservation"]!=token{return result(&service,None);}
                let generated=bridge(&service,json!({"action":"opener","document":view["document"],"sources":[],"prompt":"围绕一条共同记忆或近期事情自然说一两句话，只开一个话题。"}));
                let output=match generated{Ok(value)=>value,Err(error)=>{service.record("social",json!({"type":"release","token":token}))?;let _=sync_if_server(&service,&view);return Err(error);}};
                service.history_note("outreachError", "")?;
                sync_if_server(&service,&view)?;view=service.status()?;
                if view["pending"]!=0||!environment_ok(&chat)||view["document"]["social"]["reservation"]!=token||(view["mode"]=="server"&&view["active"]!=true){service.record("social",json!({"type":"release","token":token}))?;let _=sync_if_server(&service,&view);return result(&service,None);}
                let previous=view["document"]["social"]["lastOutreachAt"].as_u64().unwrap_or(0);
                service.record("social",json!({"type":"commit","token":token,"text":output["reply"],"topic":output["topic"]}))?;
                sync_if_server(&service,&view)?;view=service.status()?;
                if environment_ok(&chat)&&view["pending"]==0&&view["document"]["social"]["lastOutreachAt"].as_u64().unwrap_or(0)>previous&&view["document"]["social"]["lastTopic"]==output["topic"]{return result(&service,output["reply"].as_str());}
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
