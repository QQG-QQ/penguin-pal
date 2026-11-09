//! Optional local drawing and owner-only WeChat transport. No DSH runtime or tool executor.
use crate::{
    companion_chat::{self, ChatService},
    companion_sync::{scrub, Service},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

static FILES: Mutex<()> = Mutex::new(());
static STATE: Mutex<()> = Mutex::new(());
static DRAWING: AtomicBool = AtomicBool::new(false);
static POLLING: AtomicBool = AtomicBool::new(false);
static LOGIN_POLLING: AtomicBool = AtomicBool::new(false);
static JOB: OnceLock<Mutex<Value>> = OnceLock::new();
static WECHAT_ERROR: OnceLock<Mutex<String>> = OnceLock::new();
static CHAT: OnceLock<Mutex<Option<Value>>> = OnceLock::new();
const MAX_STORE_BYTES: u64 = 24 * 1024 * 1024;
fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
fn root(service: &Service) -> Result<PathBuf, String> {
    Ok(service.chat_root()?.join("visuals"))
}
fn job(value: Value) {
    if let Ok(mut state) = JOB.get_or_init(|| Mutex::new(json!({}))).lock() {
        *state = value;
    }
}
fn read(path: &Path, default: Value) -> Result<Value, String> {
    if !path.exists() {
        return Ok(default);
    }
    if fs::metadata(path).map_err(|_| "无法读取图片设置。")?.len() > MAX_STORE_BYTES {
        return Err("图片设置超过大小限制。".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| "无法读取图片设置。")?)
        .map_err(|_| "图片设置损坏，请先备份并检查。".into())
}
fn write(path: &Path, value: &Value) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("设置目录无效。")?).map_err(|_| "无法建立图片目录。")?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec(value).map_err(|_| "图片数据无法编码。")?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("本机图片或微信记录达到大小限制，请检查连接状态。".into());
    }
    fs::write(&temp, bytes).map_err(|_| "无法保存图片设置。")?;
    fs::rename(&temp, path).map_err(|_| "无法替换图片设置。".into())
}
fn defaults() -> Value {
    json!({"enabled":false,"automatic":true,"managedRuntime":false,"baseUrl":"http://127.0.0.1:8188","workflow":"","character":"2D chibi anime reaction sticker, crisp dark contour lines, simple flat color fills, cel animation frame, oversized head and tiny body, exaggerated readable comic expression, one animal or cartoon subject, plain white background, no collage, no text or watermark",
        "referenceDenoise":0.2,"referenceStyle":"","referenceNegative":"","cooldownMinutes":30,"dailyLimit":4,"timeoutSeconds":180,"wechatEnabled":false,"epoch":0})
}
fn settings(path: &Path) -> Result<Value, String> {
    read(&path.join("settings.json"), defaults())
}
fn gallery(path: &Path) -> Result<Value, String> {
    read(
        &path.join("gallery.json"),
        json!({"schema":1,"entries":[],"error":""}),
    )
}
fn seal(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "微信凭据无法编码。")?;
    let protected = crate::companion_llm::dpapi::protect(&bytes)
        .ok_or("系统无法保护微信凭据，本次连接未保存。")?;
    write(path, &json!({"dpapi":STANDARD.encode(protected)}))
}
fn unseal(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let value = read(path, Value::Null)?;
    let encoded = value["dpapi"].as_str().ok_or("微信凭据格式无效。")?;
    let protected = STANDARD.decode(encoded).map_err(|_| "微信凭据格式无效。")?;
    let bytes =
        crate::companion_llm::dpapi::unprotect(&protected).ok_or("无法解锁当前用户的微信凭据。")?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "微信凭据损坏。".into())
}
fn auth(path: &Path) -> Result<Option<Value>, String> {
    unseal(&path.join("wechat-auth.json"))
}
fn image_path(path: &Path, file: &str) -> Result<PathBuf, String> {
    let (id, ext) = file.rsplit_once('.').ok_or("图片标识无效。")?;
    if uuid::Uuid::parse_str(id).is_err() || !["png", "jpeg", "webp"].contains(&ext) {
        return Err("图片标识无效。".into());
    }
    Ok(path.join("gallery").join(file))
}
fn run_node(path: &Path, wechat: bool, request: Value) -> Result<Value, String> {
    {
        let _lock = FILES.lock().map_err(|_| "图片组件忙。")?;
        fs::create_dir_all(path).map_err(|_| "无法建立图片组件目录。")?;
        for (name, source) in [
            (
                "visual-engine.mjs",
                include_bytes!("../resources/companion/visual-engine.mjs").as_slice(),
            ),
            (
                "wechat-engine.mjs",
                include_bytes!("../resources/companion/wechat-engine.mjs").as_slice(),
            ),
            (
                "wechat-LICENSE.txt",
                include_bytes!("../resources/companion/wechat-LICENSE.txt").as_slice(),
            ),
        ] {
            let file = path.join(name);
            if fs::read(&file).ok().as_deref() != Some(source) {
                fs::write(file, source).map_err(|_| "无法更新图片组件。")?;
            }
        }
    }
    let node = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .map(|p| p.join(if cfg!(windows) { "node.exe" } else { "node" }))
        .find(|p| p.is_file())
        .ok_or("图片与微信功能需要 Node.js。")?;
    let mut command = Command::new(node);
    command
        .arg(path.join(if wechat {
            "wechat-engine.mjs"
        } else {
            "visual-engine.mjs"
        }))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "无法启动图片组件。")?;
    let stdout = child.stdout.take().ok_or("图片返回通道未就绪。")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(20 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let mut input = child.stdin.take().ok_or("图片通道未就绪。")?;
    if input
        .write_all(&serde_json::to_vec(&request).map_err(|_| "图片请求无法编码。")?)
        .is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err("图片通道中断。".into());
    }
    drop(input);
    let seconds = match request["action"].as_str() {
        Some("generate") => {
            request["settings"]["timeoutSeconds"]
                .as_u64()
                .unwrap_or(180)
                .min(600)
                + 15
        }
        Some("image") => 95,
        Some("poll" | "loginStatus") => 45,
        Some("text") => 40,
        _ => 20,
    };
    let start = Instant::now();
    let bytes = loop {
        match rx.try_recv() {
            Ok(value) => break value.map_err(|_| "图片返回通道中断。")?,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("图片返回通道中断。".into());
            }
            Err(_) => {}
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("后台请求超时；发送结果可能未确认，不会自动重发。".into());
        }
        if wechat
            && request["auth"]["session"].is_string()
            && !same_auth(path, &request["auth"]["session"])
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("微信连接已断开，已停止后台请求。".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if bytes.len() > 20 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("图片返回超过大小限制。".into());
    }
    let status = child.wait().map_err(|_| "图片组件中断。")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "图片组件未返回有效数据。")?;
    if let Some(error) = value["error"].as_str() {
        return Err(scrub(error, 240));
    }
    if !status.success() {
        return Err("图片组件未完成请求。".into());
    }
    Ok(value)
}
pub fn context(service: &Service) -> Value {
    let Ok(path) = root(service) else {
        return json!({"enabled":false});
    };
    let Ok(cfg) = settings(&path) else {
        return json!({"enabled":false});
    };
    let items = gallery(&path).unwrap_or_else(|_| json!({}));
    let catalog:Vec<Value>=items["entries"].as_array().into_iter().flatten().rev().take(20).map(|x|json!({"id":x["id"],"kind":x["kind"],"caption":x["caption"],"emotion":x["emotion"],"useReference":x["useReference"]})).collect();
    json!({"enabled":cfg["enabled"],"automatic":cfg["automatic"],"style":cfg["character"].as_str().unwrap_or("").chars().take(600).collect::<String>(),"hasReference":path.join("reference.image").is_file(),"catalog":catalog})
}
fn snapshot(service: &Service, full: bool) -> Result<Value, String> {
    let path = root(service)?;
    let _lock = STATE.lock().map_err(|_| "图片资料忙。")?;
    let mut cfg = settings(&path)?;
    cfg["hasWorkflow"] = json!(!cfg["workflow"].as_str().unwrap_or("").trim().is_empty());
    if !full {
        cfg["workflow"] = json!("");
    }
    cfg["hasReference"] = json!(path.join("reference.image").exists());
    let mut media = gallery(&path)?;
    if let Some(items) = media["entries"].as_array_mut() {
        for item in items {
            if let Some(file) = item["file"].as_str() {
                if let Ok(file_path) = image_path(&path, file) {
                    item["path"] = json!(file_path);
                }
            }
        }
    }
    let mut connection = match auth(&path) {
        Ok(Some(value)) => json!({"connected":true,"account":value["account"]}),
        Ok(None) => json!({"connected":false}),
        Err(error) => json!({"connected":false,"error":error}),
    };
    if let Ok(Some(inbox)) = unseal(&path.join("wechat-inbox.json")) {
        connection["receive"] = inbox["receive"].clone();
        for phase in ["queued", "processing", "sending", "failed", "uncertain"] {
            connection[phase] = json!(inbox["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|x| x["phase"] == phase)
                .count());
        }
    }
    if let Ok(error) = WECHAT_ERROR.get_or_init(|| Mutex::new(String::new())).lock() {
        if !error.is_empty() { connection["error"] = json!(*error); }
    }
    connection["imagesUncertain"] = json!(media["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| matches!(x["wechatDelivery"].as_str(), Some("sending" | "uncertain")))
        .count());
    let state = JOB
        .get_or_init(|| Mutex::new(json!({})))
        .lock()
        .map_err(|_| "绘图状态忙。")?
        .clone();
    Ok(
        json!({"settings":cfg,"entries":media["entries"],"error":media["error"],"wechat":connection,"job":state,"runtime":crate::companion_comfy::status(DRAWING.load(Ordering::Acquire))}),
    )
}
fn note(path: &Path, error: &str) {
    if let Ok(_lock) = STATE.lock() {
        if let Ok(mut value) = gallery(path) {
            value["error"] = json!(scrub(error, 240));
            let _ = write(&path.join("gallery.json"), &value);
        }
    }
}
fn image_delivery(path: &Path, id: &str, status: &str) -> Result<(), String> {
    let _lock = STATE.lock().map_err(|_| "图库忙。")?;
    let mut value = gallery(path)?;
    if let Some(entry) = value["entries"]
        .as_array_mut()
        .and_then(|items| items.iter_mut().find(|x| x["id"] == id))
    {
        entry["wechatDelivery"] = json!(status);
        write(&path.join("gallery.json"), &value)?;
    }
    Ok(())
}
struct Flag(&'static AtomicBool);
impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct RemoveFile(PathBuf);
impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn same_auth(path: &Path, session: &Value) -> bool {
    settings(path).is_ok_and(|x| x["wechatEnabled"] == true)
        && auth(path)
            .ok()
            .flatten()
            .is_some_and(|x| x["session"] == *session)
}
fn save_inbox(path: &Path, inbox: &Value) -> Result<(), String> {
    let _lock = STATE.lock().map_err(|_| "微信收件记录忙。")?;
    if !same_auth(path, &inbox["session"]) {
        return Err("微信已断开，未保存旧连接状态。".into());
    }
    seal(&path.join("wechat-inbox.json"), inbox)
}
fn trim_gallery(path: &Path, value: &mut Value) -> Result<(), String> {
    let entries = value["entries"].as_array_mut().ok_or("图库索引无效。")?;
    while entries.len() > 40 {
        entries.remove(0);
    }
    let directory = path.join("gallery");
    let mut files: Vec<_> = fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .collect();
    files.sort_by_key(|x| x.metadata().and_then(|x| x.modified()).ok());
    let mut total: u64 = files
        .iter()
        .filter_map(|x| x.metadata().ok().map(|m| m.len()))
        .sum();
    for file in files {
        let name = file.file_name().to_string_lossy().to_string();
        if image_path(path, &name).is_err() {
            continue;
        }
        let referenced = entries.iter().any(|x| x["file"] == name);
        if !referenced || total > 100 * 1024 * 1024 {
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);
            if fs::remove_file(file.path()).is_ok() {
                total = total.saturating_sub(size);
                entries.retain(|x| x["file"] != name);
            }
        }
    }
    Ok(())
}
pub fn schedule(service: &Service, prompt: &str, output: &Value, target: Option<Value>) {
    let visual = output.get("visual").cloned().unwrap_or(Value::Null);
    let explicit = [
        "给我画",
        "帮我画",
        "画一张",
        "画张",
        "来张",
        "表情包",
        "心情卡",
        "小剧场",
        "生成",
        "画桌宠",
        "画你",
        "画自己",
        "参考图",
    ]
    .iter()
    .any(|x| prompt.contains(x));
    if visual["kind"].as_str().is_none_or(|x| x == "none") && !explicit {
        return;
    }
    let Some(message_id) = output["messageId"]
        .as_str()
        .map(str::to_string)
        .filter(|x| !x.is_empty())
    else {
        return;
    };
    let document = output["visualDocument"].clone();
    if !document.is_object() {
        return;
    }
    if DRAWING.swap(true, Ordering::AcqRel) {
        if explicit {
            if let Ok(path) = root(service) {
                note(
                    &path,
                    "正在画上一张图，这次绘图没有排队；完成后请再叫我画。",
                )
            }
        }
        return;
    }
    let service = service.clone();
    let prompt = prompt.to_string();
    std::thread::spawn(move || {
        let _guard = Flag(&DRAWING);
        let Ok(path) = root(&service) else { return };
        let outcome = (|| -> Result<(), String> {
            let cfg = settings(&path)?;
            let reference = if path.join("reference.image").is_file() {
                let bytes = fs::read(path.join("reference.image")).map_err(|_| "无法读取参考图。")?;
                if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 { return Err("参考图大小无效。".into()); }
                Some(bytes)
            } else { None };
            let reference_key = reference.as_ref().map(|bytes| format!("{:x}", Sha256::digest(bytes))).unwrap_or_default();
            let items = gallery(&path)?;
            let plan = run_node(
                &path,
                false,
                json!({"action":"plan","settings":cfg,"document":document,"visual":visual,"prompt":prompt,"catalog":items["entries"],"referenceKey":reference_key,"now":now()}),
            )?;
            if plan.is_null() {
                return Ok(());
            }
            let generation = uuid::Uuid::new_v4().to_string();
            let cancel = path.join(format!("cancel-{generation}"));
            let _cleanup = RemoveFile(cancel.clone());
            job(
                json!({"state":"drawing","id":generation,"kind":plan["kind"],"message":"正在结合心情和性格准备图片…"}),
            );
            let reused = plan["reuseId"].as_str().is_some_and(|x| !x.is_empty());
            let (file, bytes) = if reused {
                let cached = items["entries"]
                    .as_array()
                    .and_then(|x| x.iter().find(|x| x["id"] == plan["reuseId"]))
                    .ok_or("缓存表情不存在。")?;
                let name = cached["file"].as_str().ok_or("缓存表情无效。")?.to_string();
                let bytes = fs::read(image_path(&path, &name)?).map_err(|_| "缓存表情已清理。")?;
                (name, bytes)
            } else {
                if cfg["managedRuntime"] == true {
                    crate::companion_comfy::ensure(Some(&cancel))?;
                }
                let reference = reference.as_ref().map(|bytes| STANDARD.encode(bytes));
                let result = run_node(
                    &path,
                    false,
                    json!({"action":"generate","settings":cfg,"prompt":plan["prompt"],"useReference":plan["useReference"],"reference":reference,"cancelFile":cancel}),
                )?;
                let bytes = STANDARD
                    .decode(result["bytes"].as_str().ok_or("绘图没有返回图片。")?)
                    .map_err(|_| "图片编码无效。")?;
                if bytes.len() > 8 * 1024 * 1024 {
                    return Err("图片超过 8 MiB。".into());
                }
                let extension = match result["mediaType"].as_str() {
                    Some("image/png") => "png",
                    Some("image/jpeg") => "jpeg",
                    Some("image/webp") => "webp",
                    _ => return Err("绘图返回了不支持的格式。".into()),
                };
                (format!("{}.{extension}", uuid::Uuid::new_v4()), bytes)
            };
            let attachment_id = uuid::Uuid::new_v4().to_string();
            {
                let _lock = STATE.lock().map_err(|_| "图库忙。")?;
                let current = settings(&path)?;
                if cancel.exists() || current["enabled"] != true || current["epoch"] != cfg["epoch"]
                {
                    let _ = fs::remove_file(&cancel);
                    return Err("绘图已取消，未附加图片。".into());
                }
                if let Some(ref channel) = target {
                    if !same_auth(&path, &channel["session"]) {
                        return Err("微信连接已变化，未发送旧图片。".into());
                    }
                }
                fs::create_dir_all(path.join("gallery")).map_err(|_| "无法建立图库。")?;
                if !reused {
                    fs::write(image_path(&path, &file)?, &bytes)
                        .map_err(|_| "无法保存生成图片。")?;
                }
                let mut current_gallery = gallery(&path)?;
                let mut entry = plan.clone();
                entry["id"] = json!(attachment_id);
                entry["file"] = json!(file);
                entry["at"] = json!(now());
                entry["messageId"] = json!(message_id);
                entry["reused"] = json!(reused);
                if target.is_some() {
                    entry["wechatDelivery"] = json!("sending");
                }
                entry
                    .as_object_mut()
                    .ok_or("图片意图无效。")?
                    .remove("prompt");
                current_gallery["entries"]
                    .as_array_mut()
                    .ok_or("图库索引无效。")?
                    .push(entry);
                current_gallery["error"] = json!("");
                trim_gallery(&path, &mut current_gallery)?;
                write(&path.join("gallery.json"), &current_gallery)?;
            }
            if let Some(channel) = target {
                let account = auth(&path)?.ok_or("微信已断开。")?;
                if account["session"] != channel["session"]
                    || !same_auth(&path, &channel["session"])
                {
                    return Err("微信连接已变化，未发送旧图片。".into());
                }
                let delivery = run_node(
                    &path,
                    true,
                    json!({"action":"image","auth":account,"bytes":STANDARD.encode(bytes),"contextToken":channel["contextToken"]}),
                );
                image_delivery(
                    &path,
                    &attachment_id,
                    if delivery.is_ok() {
                        "sent"
                    } else {
                        "uncertain"
                    },
                )?;
                delivery?;
            }
            let _ = fs::remove_file(cancel);
            Ok(())
        })();
        match outcome {
            Ok(()) => job(json!({"state":"idle"})),
            Err(error) => {
                note(&path, &error);
                job(json!({"state":"error","message":error}));
            }
        }
    });
}
fn record_receive(inbox: &mut Value, receipt: &Value) {
    if !receipt.is_object() { return; }
    for key in ["received", "accepted", "ignored"] {
        inbox["receive"][key] = json!(inbox["receive"][key].as_u64().unwrap_or(0)
            .saturating_add(receipt[key].as_u64().unwrap_or(0)));
    }
    if let Some(reasons) = receipt["rejected"].as_object() {
        for (key, count) in reasons {
            inbox["receive"]["rejected"][key] = json!(inbox["receive"]["rejected"][key].as_u64().unwrap_or(0)
                .saturating_add(count.as_u64().unwrap_or(0)));
        }
    }
    inbox["receive"]["polledAt"] = json!(now());
}
fn wechat_tick(service: &Service, chat: &ChatService) {
    if POLLING.swap(true, Ordering::AcqRel) {
        return;
    }
    let service = service.clone();
    let chat = chat.clone();
    std::thread::spawn(move || {
        let _guard = Flag(&POLLING);
        let Ok(path) = root(&service) else { return };
        let outcome = (|| -> Result<(), String> {
            if settings(&path)?["wechatEnabled"] != true {
                return Ok(());
            }
            let Some(account) = auth(&path)? else {
                return Ok(());
            };
            let inbox_path = path.join("wechat-inbox.json");
            let mut inbox = unseal(&inbox_path)?
                .unwrap_or_else(|| json!({"session":account["session"],"cursor":"","messages":[]}));
            if inbox["session"] != account["session"] {
                inbox = json!({"session":account["session"],"cursor":"","messages":[]});
            }
            let queued = inbox["messages"]
                .as_array()
                .is_some_and(|q| q.iter().any(|x| x["phase"] == "queued"));
            let response = if queued {
                json!({"messages":[],"cursor":inbox["cursor"]})
            } else {
                run_node(
                    &path,
                    true,
                    json!({"action":"poll","auth":account,"cursor":inbox["cursor"]}),
                )?
            };
            if !same_auth(&path, &account["session"]) {
                return Ok(());
            }
            let queue = inbox["messages"]
                .as_array_mut()
                .ok_or("微信收件记录无效。")?;
            for incoming in response["messages"].as_array().into_iter().flatten() {
                if queue.iter().any(|x| x["id"] == incoming["id"]) {
                    continue;
                }
                if queue.len() >= 500 {
                    if let Some(index) = queue.iter().position(|x| x["phase"] == "done") {
                        queue.remove(index);
                    } else {
                        return Err("微信待处理记录已满，请检查连接状态。".into());
                    }
                }
                let mut item = incoming.clone();
                item["phase"] = json!("queued");
                queue.push(item);
            }
            record_receive(&mut inbox, &response["receive"]);
            if let Ok(mut error) = WECHAT_ERROR.get_or_init(|| Mutex::new(String::new())).lock() { error.clear(); }
            inbox["cursor"] = response["cursor"].clone();
            save_inbox(&path, &inbox)?;
            let next = inbox["messages"]
                .as_array()
                .and_then(|q| q.iter().position(|x| x["phase"] == "queued"));
            let Some(index) = next else { return Ok(()) };
            let incoming = inbox["messages"][index].clone();
            inbox["messages"][index]["phase"] = json!("processing");
            save_inbox(&path, &inbox)?;
            // A crash after this durable claim is never automatically replayed into chat.
            let response = companion_chat::reply_text(
                &service,
                &chat,
                incoming["text"].as_str().ok_or("微信正文无效。")?,
                false,
            );
            match response {
                Ok(mut reply) => {
                    inbox["messages"][index]["phase"] = json!("sending");
                    inbox["messages"][index]["reply"] = reply["reply"].clone();
                    save_inbox(&path, &inbox)?;
                    if !same_auth(&path, &account["session"]) {
                        return Ok(());
                    }
                    let delivery = run_node(
                        &path,
                        true,
                        json!({"action":"text","auth":account,"text":reply["reply"],"contextToken":incoming["contextToken"]}),
                    );
                    if let Err(error) = delivery {
                        inbox["messages"][index]["phase"] = json!("uncertain");
                        save_inbox(&path, &inbox)?;
                        return Err(error);
                    }
                    inbox["messages"][index]["phase"] = json!("done");
                    save_inbox(&path, &inbox)?;
                    let intent = reply["visualIntent"].clone();
                    reply
                        .as_object_mut()
                        .ok_or("微信回复无效。")?
                        .remove("visualIntent");
                    if let Ok(mut pending) = CHAT.get_or_init(|| Mutex::new(None)).lock() {
                        *pending = Some(reply);
                    }
                    schedule(
                        &service,
                        incoming["text"].as_str().unwrap_or(""),
                        &intent,
                        Some(
                            json!({"session":account["session"],"contextToken":incoming["contextToken"]}),
                        ),
                    );
                }
                Err(error) => {
                    inbox["messages"][index]["phase"] = json!("failed");
                    save_inbox(&path, &inbox)?;
                    return Err(error);
                }
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            if let Ok(mut state) = WECHAT_ERROR.get_or_init(|| Mutex::new(String::new())).lock() { *state = scrub(&error, 240); }
            note(&path, &error);
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
    });
}
pub async fn dispatch(
    service: Service,
    chat: ChatService,
    action: String,
    data: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move||{
        let path=root(&service)?;
        match action.as_str(){
            "visualStatus"=>snapshot(&service,true),
            "visualRuntimeInstall"=>{if DRAWING.load(Ordering::Acquire){return Err("请先取消或等待当前绘图。".into());}crate::companion_comfy::install()?;snapshot(&service,true)},
            "visualRuntimeStart"=>{crate::companion_comfy::start()?;snapshot(&service,true)},
            "visualRuntimeStop"=>{if DRAWING.load(Ordering::Acquire){return Err("请先取消或等待当前绘图。".into());}crate::companion_comfy::stop()?;snapshot(&service,true)},
            "visualTick"=>{
                wechat_tick(&service,&chat);
                let mut value=json!({"visual":snapshot(&service,false)?});
                if let Ok(mut pending)=CHAT.get_or_init(||Mutex::new(None)).lock(){if let Some(reply)=pending.take(){value["chat"]=reply;}}
                Ok(value)
            },
            "visualSave"=>{
                let mut value=run_node(&path,false,json!({"action":"normalize","settings":data}))?;
                if value["managedRuntime"]==true {value["baseUrl"]=json!(crate::companion_comfy::BASE_URL);}
                let _lock=STATE.lock().map_err(|_|"图片设置忙。")?;
                let current=settings(&path)?;
                value["epoch"]=json!(current["epoch"].as_u64().unwrap_or(0).saturating_add(1));
                value["wechatEnabled"]=current["wechatEnabled"].clone();
                write(&path.join("settings.json"),&value)?;drop(_lock);snapshot(&service,true)
            },
            "visualReference"=>{
                let bytes=STANDARD.decode(data["bytes"].as_str().ok_or("请选择参考图。")?).map_err(|_|"参考图编码无效。")?;
                if bytes.is_empty()||bytes.len()>8*1024*1024||!(bytes.starts_with(&[137,80,78,71,13,10,26,10])||bytes.starts_with(&[255,216,255])||(bytes.starts_with(b"RIFF")&&bytes.get(8..12)==Some(b"WEBP".as_slice()))){return Err("请选择 8 MiB 内的 PNG、JPEG 或 WebP。".into());}
                let lock=STATE.lock().map_err(|_|"参考图设置忙。")?;
                fs::create_dir_all(&path).map_err(|_|"无法建立参考图目录。")?;fs::write(path.join("reference.image"),bytes).map_err(|_|"无法保存参考图。")?;
                let mut cfg=settings(&path)?;cfg["epoch"]=json!(cfg["epoch"].as_u64().unwrap_or(0).saturating_add(1));write(&path.join("settings.json"),&cfg)?;drop(lock);snapshot(&service,true)
            },
            "visualCheck"=>{let cfg=settings(&path)?;if cfg["managedRuntime"]==true{crate::companion_comfy::ensure(None)?;}run_node(&path,false,json!({"action":"check","settings":cfg}))},
            "visualCancel"=>{
                let current=JOB.get_or_init(||Mutex::new(json!({}))).lock().map_err(|_|"绘图状态忙。")?.clone();
                if let Some(id)=current["id"].as_str().filter(|id|uuid::Uuid::parse_str(id).is_ok()){fs::write(path.join(format!("cancel-{id}")),b"cancel").map_err(|_|"无法取消绘图。")?;}
                snapshot(&service,false)
            },
            "visualLogin"=>{
                let login=run_node(&path,true,json!({"action":"login"}))?;
                write(&path.join("wechat-login.json"),&login)?;Ok(json!({"url":login["url"],"status":"wait"}))
            },
            "visualLoginPoll"=>{
                if LOGIN_POLLING.swap(true,Ordering::AcqRel){return Err("正在等待微信确认。".into())}
                let _guard=Flag(&LOGIN_POLLING);
                let login=read(&path.join("wechat-login.json"),Value::Null)?;
                let mut response=run_node(&path,true,json!({"action":"loginStatus","login":login,"verifyCode":data["verifyCode"].as_str().unwrap_or("")}))?;
                let _lock=STATE.lock().map_err(|_|"微信设置忙。")?;
                if read(&path.join("wechat-login.json"),Value::Null)?["qrcode"]!=login["qrcode"]{return Err("二维码已更新，请使用新的二维码。".into());}
                if response["status"]=="confirmed"{
                    seal(&path.join("wechat-auth.json"),&response["auth"])?;
                    let mut cfg=settings(&path)?;cfg["wechatEnabled"]=json!(true);write(&path.join("settings.json"),&cfg)?;
                    let _=fs::remove_file(path.join("wechat-inbox.json"));let _=fs::remove_file(path.join("wechat-login.json"));
                    response=json!({"status":"confirmed"});
                }else if response["baseUrl"].is_string(){let mut changed=login;changed["baseUrl"]=response["baseUrl"].clone();write(&path.join("wechat-login.json"),&changed)?;}
                Ok(response)
            },
            "visualDisconnect"=>{
                let _lock=STATE.lock().map_err(|_|"微信设置忙。")?;
                let mut cfg=settings(&path)?;cfg["wechatEnabled"]=json!(false);write(&path.join("settings.json"),&cfg)?;
                for name in ["wechat-auth.json","wechat-inbox.json","wechat-login.json"]{let file=path.join(name);if file.exists(){fs::remove_file(file).map_err(|_|"无法清除微信授权。")?;}}
                drop(_lock);snapshot(&service,true)
            },
            "visualOpenFolder"=>{fs::create_dir_all(path.join("gallery")).map_err(|_|"无法建立图库。")?;crate::companion_platform::open_folder(&path.join("gallery"))?;Ok(json!({"ok":true}))},
            _=>Err("未知图片或微信操作。".into())
        }
    }).await.map_err(|_|"图片或微信后台中断。".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "real CPU inference: requires installed model and explicit evidence/workflow paths"]
    fn installed_model_generates_and_attaches_to_the_original_reply() {
        struct Cleanup;
        impl Drop for Cleanup { fn drop(&mut self) { crate::companion_comfy::shutdown(); } }
        let _cleanup = Cleanup;
        let evidence = PathBuf::from(std::env::var_os("PENGUINPAL_COMFY_EVIDENCE").expect("explicit evidence directory"));
        let workflow = fs::read_to_string(std::env::var_os("PENGUINPAL_COMFY_WORKFLOW").expect("frontend managed workflow file")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path(), json!({})).unwrap();
        let path = root(&service).unwrap();
        let mut cfg = defaults();
        cfg["enabled"] = json!(true); cfg["managedRuntime"] = json!(true);
        cfg["baseUrl"] = json!(crate::companion_comfy::BASE_URL);
        cfg["workflow"] = json!(workflow); cfg["timeoutSeconds"] = json!(600);
        if let Ok(style) = std::env::var("PENGUINPAL_COMFY_STYLE") { cfg["character"] = json!(style.chars().take(600).collect::<String>()); }
        if let Some(file) = std::env::var_os("PENGUINPAL_COMFY_REFERENCE_PROFILE") {
            let profile: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
            for key in ["referenceDenoise", "referenceStyle", "referenceNegative"] { cfg[key] = profile[key].clone(); }
        }
        write(&path.join("settings.json"), &cfg).unwrap();
        let reference = std::env::var_os("PENGUINPAL_COMFY_REFERENCE");
        if let Some(file) = &reference { fs::write(path.join("reference.image"), fs::read(file).unwrap()).unwrap(); }
        let start = Instant::now();
        let prompt = if reference.is_some() { "按参考图给我画张桌宠开心的表情包" } else { "给我画张开心的表情包" };
        let description = if reference.is_some() { "same 2D anime chibi character as reference, smiling softly, keep original line art, flat colors, hairstyle, blue eyes, penguin costume and cyan headset" } else { "a single orange and white chibi tabby cat, oversized head, tiny body, very wide round eyes and a small open mouth in shock, one paw lifted, crisp ink contours and simple flat colors, 2D anime sticker" };
        schedule(&service, prompt, &json!({
            "messageId":"local-live-test-reply", "visualDocument":{"pet":{"mood":86,"personality":"playful"}},
            "visual":{"kind":"sticker","description":description,"caption":"今天也要开心呀","reuseId":"","useReference":reference.is_some()}
        }), None);
        while DRAWING.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(720) {
            std::thread::sleep(Duration::from_millis(500));
        }
        assert!(!DRAWING.load(Ordering::Acquire), "real image generation exceeded bound");
        let value = snapshot(&service, true).unwrap();
        assert_eq!(value["error"], "", "{}", value);
        let entries = value["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["messageId"], "local-live-test-reply");
        assert_eq!(entries[0]["emotion"], "happy");
        assert_eq!(entries[0]["personality"], "playful");
        assert_eq!(entries[0]["useReference"], reference.is_some());
        if reference.is_some() { assert_eq!(entries[0]["referenceKey"].as_str().unwrap().len(), 64); }
        let bytes = fs::read(image_path(&path, entries[0]["file"].as_str().unwrap()).unwrap()).unwrap();
        assert!(bytes.starts_with(&[137,80,78,71,13,10,26,10]));
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 512);
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 512);
        fs::create_dir_all(&evidence).unwrap();
        fs::write(evidence.join(if reference.is_some() { "reference-sticker.png" } else { "happy-sticker.png" }), &bytes).unwrap();
        fs::write(evidence.join("inference.json"), serde_json::to_vec_pretty(&json!({
            "seconds":start.elapsed().as_secs_f64(),"width":512,"height":512,"bytes":bytes.len(),
            "runtime":crate::companion_comfy::status(false),"attachment":entries[0],"scope":"real native scheduler, managed lifecycle, frontend workflow and Node adapter; synthetic text reply, no cloud chat or WeChat"
        })).unwrap()).unwrap();
        println!("real CPU inference and original reply attachment passed in {:.1}s", start.elapsed().as_secs_f64());
    }
    #[test]
    fn wechat_receive_diagnostics_preserve_counts_across_empty_polls() {
        let mut inbox = json!({"messages":[]});
        record_receive(&mut inbox, &json!({"received":2,"accepted":1,"ignored":1,"rejected":{"id":1}}));
        record_receive(&mut inbox, &json!({"received":0,"accepted":0,"ignored":0,"rejected":{}}));
        record_receive(&mut inbox, &Value::Null);
        assert_eq!(inbox["receive"]["received"], 2);
        assert_eq!(inbox["receive"]["accepted"], 1);
        assert_eq!(inbox["receive"]["rejected"]["id"], 1);
        assert!(inbox["receive"]["polledAt"].as_u64().unwrap() > 0);
        assert_eq!(inbox["messages"], json!([]));
    }
    #[test]
    fn gallery_paths_never_accept_arbitrary_files() {
        let root = Path::new("gallery-test");
        for bad in [
            "../secret.png",
            "secret.svg",
            "C:\\private.png",
            "not-a-uuid.png",
        ] {
            assert!(image_path(root, bad).is_err());
        }
        assert!(image_path(root, &format!("{}.png", uuid::Uuid::new_v4())).is_ok());
    }
    #[test]
    fn settings_default_to_no_connections() {
        let value = defaults();
        assert_eq!(value["enabled"], false);
        assert_eq!(value["wechatEnabled"], false);
    }
    #[test]
    fn image_delivery_updates_only_the_recorded_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        write(&path.join("gallery.json"),&json!({"entries":[{"id":"first","messageId":"reply-1","wechatDelivery":"sending"},{"id":"second","messageId":"reply-2","wechatDelivery":"sending"}]})).unwrap();
        image_delivery(path, "first", "uncertain").unwrap();
        let value = gallery(path).unwrap();
        assert_eq!(value["entries"][0]["wechatDelivery"], "uncertain");
        assert_eq!(value["entries"][1]["wechatDelivery"], "sending");
        assert_eq!(value["entries"][0]["messageId"], "reply-1");
    }
    #[cfg(windows)]
    #[test]
    fn sealed_inbox_above_three_megabytes_remains_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inbox.json");
        let value = json!({"synthetic":"字".repeat(1_100_000)});
        seal(&path, &value).unwrap();
        assert!(fs::metadata(&path).unwrap().len() > 3 * 1024 * 1024);
        assert_eq!(unseal(&path).unwrap().unwrap(), value);
    }
    #[test]
    fn embedded_node_bridge_runs_locally_and_preserves_chinese() {
        let dir = tempfile::tempdir().unwrap();
        let value = run_node(
            dir.path(),
            false,
            json!({"action":"normalize","settings":{"character":"企鹅的心情"}}),
        )
        .unwrap();
        assert_eq!(value["character"], "企鹅的心情");
        assert_eq!(value["enabled"], false);
        assert!(run_node(
            dir.path(),
            false,
            json!({"action":"normalize","settings":{"baseUrl":"https://not-local.example/"}})
        )
        .is_err());
    }
    #[cfg(windows)]
    #[test]
    fn wechat_credentials_are_dpapi_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret.json");
        seal(
            &file,
            &json!({"token":"synthetic-secret","owner":"synthetic-owner"}),
        )
        .unwrap();
        assert!(!fs::read_to_string(&file)
            .unwrap()
            .contains("synthetic-secret"));
        assert_eq!(unseal(&file).unwrap().unwrap()["token"], "synthetic-secret");
    }
    #[cfg(windows)]
    #[test]
    fn stale_workers_cannot_restore_disconnected_or_replaced_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut cfg = defaults();
        cfg["wechatEnabled"] = json!(true);
        write(&path.join("settings.json"), &cfg).unwrap();
        seal(&path.join("wechat-auth.json"), &json!({"session":"first"})).unwrap();
        let inbox = json!({"session":"first","cursor":"durable","messages":[{"id":"1","phase":"processing"}]});
        save_inbox(path, &inbox).unwrap();
        assert_eq!(
            unseal(&path.join("wechat-inbox.json")).unwrap().unwrap()["messages"][0]["phase"],
            "processing"
        );
        fs::remove_file(path.join("wechat-auth.json")).unwrap();
        fs::remove_file(path.join("wechat-inbox.json")).unwrap();
        assert!(save_inbox(path, &inbox).is_err());
        assert!(!path.join("wechat-inbox.json").exists());
        seal(&path.join("wechat-auth.json"), &json!({"session":"second"})).unwrap();
        assert!(save_inbox(path, &inbox).is_err());
        assert!(!path.join("wechat-inbox.json").exists());
    }
    #[test]
    fn gallery_keeps_shared_cache_files_and_removes_unreferenced_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        fs::create_dir(path.join("gallery")).unwrap();
        let kept = format!("{}.png", uuid::Uuid::new_v4());
        let discarded = format!("{}.png", uuid::Uuid::new_v4());
        fs::write(image_path(path, &kept).unwrap(), b"image").unwrap();
        fs::write(image_path(path, &discarded).unwrap(), b"orphan").unwrap();
        let mut value =
            json!({"entries":(0..42).map(|n|json!({"id":n,"file":kept})).collect::<Vec<_>>()});
        trim_gallery(path, &mut value).unwrap();
        assert_eq!(value["entries"].as_array().unwrap().len(), 40);
        assert!(image_path(path, &kept).unwrap().exists());
        assert!(!image_path(path, &discarded).unwrap().exists());
    }
}
