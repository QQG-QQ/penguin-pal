use crate::companion_sync_transport::{self as transport, Server};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}
pub(crate) fn scrub(text: &str, limit: usize) -> String {
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns=PATTERNS.get_or_init(||[
        r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?(?:-----END [^-]*PRIVATE KEY-----|$)",
        r"\bsk-(?:proj-)?[A-Za-z0-9_-]{16,}",
        r"(?i)bearer\s+[A-Za-z0-9._~+/-]{12,}",
        r#"(?i)(?:password|passwd|api[_-]?key|access[_-]?token|密码)["']?\s*[:=：]\s*["']?[^\s,，;；"']+"#,
        r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:[^@\s/]+@",
    ].iter().map(|pattern|regex::Regex::new(pattern).expect("fixed regex")).collect());
    let mut result = text.to_owned();
    for pattern in patterns {
        result = pattern.replace_all(&result, "[凭据已移除]").into_owned();
    }
    result.chars().take(limit).collect()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pet {
    energy: f64,
    hunger: f64,
    affection: f64,
    personality: String,
    mood: f64,
    mood_reason: String,
    mood_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Memory {
    id: String,
    text: String,
    at: u64,
    deleted: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    id: String,
    session: String,
    role: String,
    text: String,
    at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mood {
    id: String,
    value: f64,
    reason: String,
    at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema: u32,
    pet_id: String,
    revision: u64,
    updated_at: u64,
    pet: Pet,
    memories: Vec<Memory>,
    conversations: Vec<Message>,
    mood_history: Vec<Mood>,
    last_feed_at: u64,
    last_pat_at: u64,
    #[serde(
        default,
        skip_serializing_if = "crate::companion_social::Social::is_default"
    )]
    social: crate::companion_social::Social,
    #[serde(
        default,
        skip_serializing_if = "crate::companion_brain::Brain::is_default"
    )]
    brain: crate::companion_brain::Brain,
}
impl Document {
    fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.schema != 1
            || !valid_id(&self.pet_id)
            || [
                self.pet.energy,
                self.pet.hunger,
                self.pet.affection,
                self.pet.mood,
            ]
            .iter()
            .any(|x| !x.is_finite() || !(0.0..=100.0).contains(x))
            || !["gentle", "playful", "quiet", "warm"].contains(&self.pet.personality.as_str())
            || self.pet.mood_reason.chars().count() > 400
            || self.memories.len() > 2000
            || self.conversations.len() > 500
            || self.mood_history.len() > 200
            || !self.social.valid()
            || !self.brain.valid()
        {
            return Err("数据版本或宠物状态无效；未覆盖已有数据。".into());
        }
        for m in &self.memories {
            if !valid_id(&m.id) || !ids.insert(m.id.clone()) || m.text.chars().count() > 2000 {
                return Err("记忆数据无效。".into());
            }
        }
        ids.clear();
        for m in &self.conversations {
            if !valid_id(&m.id)
                || !valid_id(&m.session)
                || !ids.insert(m.id.clone())
                || !["user", "assistant"].contains(&m.role.as_str())
                || m.text.chars().count() > 6000
            {
                return Err("近期对话数据无效。".into());
            }
        }
        ids.clear();
        for m in &self.mood_history {
            if !valid_id(&m.id)
                || !ids.insert(m.id.clone())
                || !m.value.is_finite()
                || !(0.0..=100.0).contains(&m.value)
                || m.reason.chars().count() > 400
            {
                return Err("心情记录无效。".into());
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "数据无法编码。")?
            .len()
            > 4 * 1024 * 1024
        {
            return Err("陪伴数据超过 4 MB，请先导出整理记忆。".into());
        }
        Ok(())
    }
    fn sanitize(&mut self) {
        self.brain.sanitize();
        self.pet.mood_reason = scrub(&self.pet.mood_reason, 400);
        for m in &mut self.memories {
            m.text = if m.deleted {
                String::new()
            } else {
                scrub(&m.text, 2000)
            };
        }
        for m in &mut self.conversations {
            m.text = scrub(&m.text, 6000);
        }
        for m in &mut self.mood_history {
            m.reason = scrub(&m.reason, 400);
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    id: String,
    kind: String,
    data: Value,
}
fn pending_batch(pending: &[Operation]) -> Result<Vec<Operation>, String> {
    // Leave room for the protocol envelope under the transport's 8 MiB limit.
    // Several offline imports may each contain a complete 4 MiB document.
    let mut batch = Vec::new();
    let mut bytes = 0;
    for op in pending.iter().take(250) {
        let size = serde_json::to_vec(op)
            .map_err(|_| "同步数据无法编码。")?
            .len()
            + 1;
        if size > 7 * 1024 * 1024 {
            return Err("单条待同步记录超过容量限制，请导出备份。".into());
        }
        if bytes + size > 7 * 1024 * 1024 {
            break;
        }
        bytes += size;
        batch.push(op.clone());
    }
    Ok(batch)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disk {
    schema: u32,
    device: String,
    server: Option<Server>,
    capture: bool,
    document: Document,
    pending: Vec<Operation>,
}
struct Runtime {
    disk: Disk,
    status: String,
    last_sync: u64,
    active: bool,
    import: Option<Document>,
    failures: u32,
    next_attempt: u64,
}
struct Running(Arc<AtomicBool>);
fn acquire_sync(running: &Arc<AtomicBool>, timeout: Duration) -> Result<Running, String> {
    let started = Instant::now();
    loop {
        if running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(Running(running.clone()));
        }
        if started.elapsed() >= timeout {
            return Err("等待服务器同步超时，请在同步设置中查看。".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Clone)]
pub struct Service {
    root: PathBuf,
    inner: Arc<Mutex<Runtime>>,
    running: Arc<AtomicBool>,
    history: crate::companion_history::History,
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", id()));
    fs::write(&temporary, bytes).map_err(|_| "无法写入桌宠数据。")?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|f| f.sync_all())
        .map_err(|_| "无法确认桌宠数据写入。")?;
    fs::rename(&temporary, path).map_err(|_| "无法原子保存桌宠数据。".into())
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "无法读取数据文件。")?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("数据文件类型或大小无效。".into());
    }
    fs::read(path).map_err(|_| "无法读取数据文件。".into())
}
fn backup(document: &Document) -> Value {
    json!({"format":"penguinpal-companion","version":1,"sha256":digest(&serde_json::to_vec(document).unwrap()),"data":document})
}
fn decode_backup(bytes: &[u8]) -> Result<Document, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("备份超过大小上限。".into());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "备份不是有效的 JSON。")?;
    if value["format"] != "penguinpal-companion" || value["version"] != 1 {
        return Err("不是支持的企鹅备份格式。".into());
    }
    let document: Document =
        serde_json::from_value(value["data"].clone()).map_err(|_| "备份数据无效。")?;
    document.validate()?;
    if value["sha256"] != digest(&serde_json::to_vec(&document).unwrap()) {
        return Err("备份校验失败；文件可能不完整。".into());
    }
    Ok(document)
}
impl Service {
    pub fn new(root: &Path, seed: Value) -> Result<Self, String> {
        let folder = root.join("sync");
        if folder.exists()
            && fs::symlink_metadata(&folder)
                .map_err(|_| "无法检查存储目录。")?
                .file_type()
                .is_symlink()
        {
            return Err("同步目录不能是链接。".into());
        }
        fs::create_dir_all(&folder).map_err(|_| "无法创建同步目录。")?;
        let path = folder.join("local.json");
        let disk: Disk = if path.exists() {
            let parse = |p: &Path| -> Result<Disk, String> {
                serde_json::from_slice(&read(p)?).map_err(|_| "同步存档损坏。".into())
            };
            parse(&path).or_else(|_| parse(&folder.join("local.bak.json")))?
        } else {
            Disk {
                schema: 1,
                device: id(),
                server: None,
                capture: true,
                pending: vec![],
                document: Document {
                    schema: 1,
                    pet_id: id(),
                    revision: 0,
                    updated_at: now(),
                    pet: Pet {
                        energy: seed["energy"].as_f64().unwrap_or(80.0),
                        hunger: seed["hunger"].as_f64().unwrap_or(20.0),
                        affection: seed["affection"].as_f64().unwrap_or(65.0),
                        personality: seed["preferences"]["personality"]
                            .as_str()
                            .unwrap_or("gentle")
                            .into(),
                        mood: 70.0,
                        mood_reason: String::new(),
                        mood_at: now(),
                    },
                    memories: vec![],
                    conversations: vec![],
                    mood_history: vec![],
                    last_feed_at: 0,
                    last_pat_at: 0,
                    social: Default::default(),
                    brain: Default::default(),
                },
            }
        };
        disk.document.validate()?;
        if disk.schema != 1 || !valid_id(&disk.device) || disk.pending.len() > 10000 {
            return Err("同步配置无效。".into());
        }
        if let Some(server) = &disk.server {
            server.validate()?;
        }
        let service = Self {
            history: crate::companion_history::History::open(&folder.join("history.sqlite3"))?,
            root: folder,
            inner: Arc::new(Mutex::new(Runtime {
                disk,
                status: String::new(),
                last_sync: 0,
                active: false,
                import: None,
                failures: 0,
                next_attempt: 0,
            })),
            running: Arc::new(AtomicBool::new(false)),
        };
        service.save(&service.inner.lock().map_err(|_| "同步状态忙。")?.disk)?;
        Ok(service)
    }
    fn save(&self, disk: &Disk) -> Result<(), String> {
        disk.document.validate()?;
        let bytes = serde_json::to_vec(disk).map_err(|_| "数据无法编码。")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("同步缓存已满，请连接服务器或导出备份。".into());
        }
        let target = self.root.join("local.json");
        if target.exists() {
            let previous = read(&target)?;
            if serde_json::from_slice::<Disk>(&previous)
                .is_ok_and(|d| d.document.validate().is_ok())
            {
                write(&self.root.join("local.bak.json"), &previous)?;
            }
        }
        write(&target, &bytes)
    }
    pub fn status(&self) -> Result<Value, String> {
        self.ingest()?;
        let state = self.inner.lock().map_err(|_| "同步状态忙。")?;
        self.archive_document(&state.disk.document)?;
        Ok(
            json!({"mode":if state.disk.server.is_some() {"server"}else{"local"}, "server":state.disk.server,
            "capture":state.disk.capture, "document":state.disk.document, "pending":state.disk.pending.len(), "history":self.history.status()?,
            "message":state.status, "lastSync":state.last_sync, "captureWarning":self.root.parent().is_some_and(|p|p.join("conversation-overflow.json").exists()), "active":state.active&&now().saturating_sub(state.last_sync)<45000, "running":self.running.load(Ordering::Relaxed)}),
        )
    }
    pub fn local_pet(&self, state: &Value) -> Result<(), String> {
        let mut data = self.inner.lock().map_err(|_| "同步状态忙。")?;
        if data.disk.server.is_none() {
            let mut next = data.disk.clone();
            recover_mood(&mut next.document);
            next.document.pet.energy = state["energy"].as_f64().ok_or("体力无效。")?;
            next.document.pet.hunger = state["hunger"].as_f64().ok_or("饥饿无效。")?;
            next.document.pet.affection = state["affection"].as_f64().ok_or("亲近度无效。")?;
            next.document.updated_at = now();
            self.save(&next)?;
            data.disk = next;
        }
        Ok(())
    }
    pub fn record(&self, kind: &str, data: Value) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "同步状态忙。")?;
        let mut next = state.disk.clone();
        if next.server.is_none() {
            recover_mood(&mut next.document);
        }
        if next.pending.len() >= 10000 {
            return Err("待同步队列已满；请先联网同步或导出。".into());
        }
        let mut op = Operation {
            id: id(),
            kind: kind.into(),
            data,
        };
        // The durable queue must contain the same sanitized content as the document.
        match kind {
            "memory" => {
                let mut item: Memory =
                    serde_json::from_value(op.data).map_err(|_| "记忆格式无效。")?;
                item.text = if item.deleted {
                    String::new()
                } else {
                    scrub(&item.text, 2000)
                };
                op.data = serde_json::to_value(item).unwrap();
            }
            "conversation" => {
                let mut item: Message =
                    serde_json::from_value(op.data).map_err(|_| "对话格式无效。")?;
                item.text = scrub(&item.text, 6000);
                op.data = serde_json::to_value(item).unwrap();
            }
            "mood" => {
                op.data = json!({"delta":op.data["delta"], "reason":scrub(op.data["reason"].as_str().unwrap_or(""),400)});
            }
            "restore" => {
                let mut source: Document = serde_json::from_value(op.data["document"].clone())
                    .map_err(|_| "导入数据无效。")?;
                source.validate()?;
                source.sanitize();
                op.data = json!({"document":source});
            }
            "social" | "brain" => sanitize_value(&mut op.data),
            _ => (),
        }
        let imported_identity = if kind == "restore" && next.server.is_none() {
            op.data["document"]["petId"].as_str().map(str::to_owned)
        } else {
            None
        };
        apply(&mut next.document, &op)?;
        // Archive before the recent window can discard an older incoming message.
        match kind {
            "conversation" => self.history.store(&[op.data.clone()], &[])?,
            "restore" => self.archive_document(
                &serde_json::from_value(op.data["document"].clone())
                    .map_err(|_| "导入数据无效。")?,
            )?,
            "social" => {
                if let Some(message) = next.document.conversations.iter().find(|m| m.id == op.id) {
                    self.history
                        .store(&[serde_json::to_value(message).unwrap()], &[])?;
                }
            }
            _ => (),
        }
        if let Some(identity) = imported_identity {
            next.document.pet_id = identity;
        }
        if next.server.is_some() {
            next.pending.push(op);
        } else {
            next.document.revision += 1;
        }
        self.save(&next)?;
        state.disk = next;
        Ok(())
    }
    fn ingest(&self) -> Result<(), String> {
        let queue = self
            .root
            .parent()
            .ok_or("同步目录无效。")?
            .join("conversations");
        if !queue.exists() {
            return Ok(());
        }
        if fs::symlink_metadata(&queue)
            .map_err(|_| "无法读取对话队列。")?
            .file_type()
            .is_symlink()
        {
            return Err("对话队列路径无效。".into());
        }
        let mut files: Vec<_> = fs::read_dir(&queue)
            .map_err(|_| "无法读取对话队列。")?
            .filter_map(Result::ok)
            .filter(|f| f.file_name().to_string_lossy().ends_with(".json"))
            .collect();
        files.sort_by_key(|f| f.file_name());
        for file in files.into_iter().take(250) {
            let message: Message =
                serde_json::from_slice(&read(&file.path())?).map_err(|_| "对话队列格式无效。")?;
            let seen = {
                let s = self.inner.lock().map_err(|_| "同步状态忙。")?;
                s.disk
                    .document
                    .conversations
                    .iter()
                    .any(|m| m.id == message.id)
            };
            if !seen {
                self.record("conversation", serde_json::to_value(message).unwrap())?;
            }
            fs::remove_file(file.path()).map_err(|_| "对话已保存，但无法清理收件文件。")?;
        }
        Ok(())
    }
    fn synchronize(&self, manual: bool) -> Result<(), String> {
        self.ingest()?;
        if !manual && self.inner.lock().map_err(|_| "同步状态忙。")?.next_attempt > now() {
            return Ok(());
        }
        let disk = self.inner.lock().map_err(|_| "同步状态忙。")?.disk.clone();
        let Some(server) = disk.server else {
            return Ok(());
        };
        self.archive_document(&disk.document)?;
        let operations = pending_batch(&disk.pending)?;
        let response = transport::exchange(
            &server,
            None,
            &json!({"protocol":1,"namespace":server.namespace,
            "device":disk.device,"claim":true,"seed":{},"operations":operations}),
        );
        let mut state = self.inner.lock().map_err(|_| "同步状态忙。")?;
        match response {
            Ok(response) => {
                let mut document: Document = serde_json::from_value(response["document"].clone())
                    .map_err(|_| "服务器存档格式无效。")?;
                document.validate()?;
                self.archive_document(&document)?;
                if document.pet_id != disk.document.pet_id
                    || document.revision < disk.document.revision
                {
                    return Err("服务器身份或版本发生回退，已停止覆盖本机缓存。".into());
                }
                let ack: Vec<String> = serde_json::from_value(response["acknowledged"].clone())
                    .map_err(|_| "服务器确认格式无效。")?;
                let sent: HashSet<_> = operations.iter().map(|x| x.id.clone()).collect();
                if ack.iter().cloned().collect::<HashSet<_>>() != sent {
                    return Err("服务器没有完整确认本次提交。".into());
                }
                let ack: HashSet<_> = ack.into_iter().collect();
                let mut next = state.disk.clone();
                next.pending.retain(|op| !ack.contains(&op.id));
                for op in &next.pending {
                    apply(&mut document, op)?;
                }
                next.document = document;
                self.save(&next)?;
                state.disk = next;
                state.status = "已与服务器同步".into();
                state.last_sync = now();
                state.active = response["active"].as_bool().unwrap_or(false);
                state.failures = 0;
                state.next_attempt = 0;
                Ok(())
            }
            Err(error) => {
                state.status = format!("离线，待同步：{error}");
                state.active = false;
                Err(error)
            }
        }
    }
    fn configure(&self, mut server: Server, password: &str) -> Result<(), String> {
        server.validate()?;
        let (session, fingerprint, known) = transport::session(&server)?;
        drop(session);
        if server.fingerprint != fingerprint && !known {
            return Err("请先检查并确认服务器指纹。".into());
        }
        server.fingerprint = fingerprint;
        let disk = self.inner.lock().map_err(|_| "同步状态忙。")?.disk.clone();
        if !disk.pending.is_empty() && disk.server.as_ref() != Some(&server) {
            return Err("旧服务器仍有待同步数据，请先完成同步或切换本地并导出。".into());
        }
        let response = transport::exchange(
            &server,
            Some(password),
            &json!({"protocol":1,"namespace":server.namespace,"device":disk.device,
            "claim":true,"seed":disk.document,"operations":[]}),
        );
        let response = response?;
        let mut remote: Document =
            serde_json::from_value(response["document"].clone()).map_err(|_| "服务器存档无效。")?;
        remote.validate()?;
        self.archive_document(&remote)?;
        if disk.server.as_ref() == Some(&server)
            && (remote.pet_id != disk.document.pet_id || remote.revision < disk.document.revision)
        {
            return Err("服务器身份或版本变化，请先核对存储和备份。".into());
        }
        if !password.is_empty() {
            transport::credential_write(&server, password)?;
        }
        let mut state = self.inner.lock().map_err(|_| "同步状态忙。")?;
        let mut next = state.disk.clone();
        // Existing shared identity wins; local memories and recent messages are merged.
        for m in &next.document.memories {
            if !remote
                .memories
                .iter()
                .any(|x| x.id == m.id && x.deleted == m.deleted && x.text == m.text)
            {
                next.pending.push(Operation {
                    id: id(),
                    kind: "memory".into(),
                    data: serde_json::to_value(m).unwrap(),
                });
            }
            if let Some(meta) = next.document.brain.memories.get(&m.id) {
                if !remote.brain.memories.contains_key(&m.id) {
                    next.pending.push(Operation {
                        id: id(),
                        kind: "brain".into(),
                        data: json!({"type":"mergeMeta","id":m.id,"expected":m.text,"meta":meta}),
                    });
                }
            }
        }
        for m in &next.document.conversations {
            if !remote.conversations.iter().any(|x| x.id == m.id) {
                next.pending.push(Operation {
                    id: id(),
                    kind: "conversation".into(),
                    data: serde_json::to_value(m).unwrap(),
                });
            }
        }
        for op in &next.pending {
            apply(&mut remote, op)?;
        }
        next.document = remote;
        next.server = Some(server);
        self.save(&next)?;
        state.disk = next;
        state.status = "已连接；自动同步已启用".into();
        state.last_sync = now();
        state.active = response["active"].as_bool().unwrap_or(false);
        state.failures = 0;
        state.next_attempt = 0;
        Ok(())
    }
}

fn apply(doc: &mut Document, op: &Operation) -> Result<(), String> {
    let at = now();
    let p = &mut doc.pet;
    match op.kind.as_str() {
        "brain" => {
            let mut value = serde_json::to_value(&*doc).map_err(|_| "陪伴数据无法编码。")?;
            crate::companion_brain::apply(&mut value, &op.data, &op.id, at)?;
            *doc = serde_json::from_value(value).map_err(|_| "陪伴数据无效。")?;
        }
        "social" => {
            let mut value = serde_json::to_value(&*doc).map_err(|_| "陪伴数据无法编码。")?;
            crate::companion_social::apply(&mut value, &op.data, &op.id, at)?;
            *doc = serde_json::from_value(value).map_err(|_| "陪伴数据无效。")?;
        }
        "care" => match op.data["action"].as_str().ok_or("互动无效。")? {
            "feed" if p.hunger >= 8.0 && at.saturating_sub(doc.last_feed_at) >= 5000 => {
                p.hunger = (p.hunger - 18.0).max(0.0);
                p.energy = (p.energy + 5.0).min(100.0);
                p.affection = (p.affection + 2.0).min(100.0);
                doc.last_feed_at = at;
            }
            "pat" if at.saturating_sub(doc.last_pat_at) >= 3000 => {
                p.affection = (p.affection + 1.0).min(100.0);
                doc.last_pat_at = at;
            }
            "rest" => p.energy = (p.energy + 3.0).min(100.0),
            "pat" | "feed" | "poke" | "tail" => (),
            _ => return Err("互动无效。".into()),
        },
        "personality" => p.personality = op.data["value"].as_str().ok_or("性格无效。")?.into(),
        "memory" => {
            let mut m: Memory =
                serde_json::from_value(op.data.clone()).map_err(|_| "记忆格式无效。")?;
            if doc.memories.iter().any(|x| x.id == m.id && x.deleted) {
                return Ok(());
            }
            m.text = if m.deleted {
                String::new()
            } else {
                scrub(&m.text, 2000)
            };
            m.at = m.at.min(at);
            if !m.deleted && doc.memories.iter().any(|x| x.id == m.id && x.at > m.at) {
                return Ok(());
            }
            if m.deleted {
                doc.brain.memories.remove(&m.id);
            }
            doc.memories.retain(|x| x.id != m.id);
            doc.memories.push(m);
        }
        "conversation" => {
            let mut m: Message =
                serde_json::from_value(op.data.clone()).map_err(|_| "对话格式无效。")?;
            m.text = scrub(&m.text, 6000);
            if !doc.conversations.iter().any(|x| x.id == m.id) {
                doc.conversations.push(m);
                doc.conversations
                    .sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
                if doc.conversations.len() > 500 {
                    doc.conversations.remove(0);
                }
            }
        }
        "mood" => {
            let delta = op.data["delta"]
                .as_f64()
                .filter(|x| x.is_finite() && (-10.0..=10.0).contains(x))
                .ok_or("心情变化无效。")?;
            p.mood = (p.mood + delta).clamp(0.0, 100.0);
            p.mood_reason = scrub(op.data["reason"].as_str().unwrap_or(""), 400);
            p.mood_at = at;
            doc.mood_history.push(Mood {
                id: op.id.clone(),
                value: p.mood,
                reason: p.mood_reason.clone(),
                at,
            });
            if doc.mood_history.len() > 200 {
                doc.mood_history.remove(0);
            }
        }
        "restore" => {
            let mut source: Document = serde_json::from_value(op.data["document"].clone())
                .map_err(|_| "导入数据无效。")?;
            source.validate()?;
            source.sanitize();
            for m in source.memories {
                apply(
                    doc,
                    &Operation {
                        id: op.id.clone(),
                        kind: "memory".into(),
                        data: serde_json::to_value(m).unwrap(),
                    },
                )?;
            }
            for m in source.conversations {
                apply(
                    doc,
                    &Operation {
                        id: op.id.clone(),
                        kind: "conversation".into(),
                        data: serde_json::to_value(m).unwrap(),
                    },
                )?;
            }
            doc.pet = source.pet;
            doc.mood_history = source.mood_history;
            let current_scheduler = (
                doc.brain.next_check_at,
                doc.brain.misses,
                doc.brain.decisions.clone(),
            );
            doc.brain = source.brain;
            doc.brain.next_check_at = current_scheduler.0;
            doc.brain.misses = current_scheduler.1;
            doc.brain.decisions = current_scheduler.2;
            doc.brain.manual = false;
            doc.brain
                .memories
                .retain(|id, _| doc.memories.iter().any(|m| m.id == *id && !m.deleted));
            // Restoring a backup must not rewind shared send cooldowns or unanswered counts.
        }
        _ => return Err("未知的数据操作。".into()),
    }
    while serde_json::to_vec(doc).map_err(|_| "数据无法编码。")?.len() > 4 * 1024 * 1024
        && !doc.conversations.is_empty()
    {
        doc.conversations.remove(0);
    }
    doc.validate()
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = scrub(text, 6000),
        Value::Array(items) => items.iter_mut().for_each(sanitize_value),
        Value::Object(items) => items.values_mut().for_each(sanitize_value),
        _ => (),
    }
}
fn recover_mood(doc: &mut Document) {
    let at = now();
    let hours = at.saturating_sub(doc.updated_at).min(86400000) as f64 / 3600000.0;
    let cfg = &doc.brain.config;
    doc.pet.mood = (cfg.baseline
        + (doc.pet.mood - cfg.baseline) * 0.5_f64.powf(hours / cfg.mood_half_hours))
    .clamp(cfg.mood_floor, 100.0);
    if !doc.brain.is_default() {
        doc.brain.advance(at);
    }
    doc.updated_at = at;
}
impl Service {
    fn archive_document(&self, document: &Document) -> Result<(), String> {
        self.history.store(
            &document
                .conversations
                .iter()
                .map(|m| serde_json::to_value(m).unwrap())
                .collect::<Vec<_>>(),
            &document.social.analyzed,
        )
    }
    pub fn history_sources(&self) -> Result<Vec<Value>, String> {
        self.history.pending()
    }
    pub fn history_done(&self, id: &str) -> Result<(), String> {
        self.history.done(id)
    }
    pub fn history_note(&self, key: &str, value: &str) -> Result<(), String> {
        self.history.note(key, value)
    }
    pub fn history_search(&self, data: &Value) -> Result<Value, String> {
        self.history.search(data)
    }
    pub fn scan_history(&self) -> Result<(), String> {
        if !self.inner.lock().map_err(|_| "同步状态忙。")?.disk.capture {
            return Ok(());
        }
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
            .ok_or("未找到本机 Codex 目录。")?;
        self.scan_history_from(&home)
    }
    fn scan_history_from(&self, home: &Path) -> Result<(), String> {
        self.history.scan(home)?;
        // Publish only the recent window, in one durable write. Full history stays in the archive.
        let recent = self.history.recent()?;
        let mut state = self.inner.lock().map_err(|_| "同步状态忙。")?;
        let mut next = state.disk.clone();
        let mut changed = false;
        let mut added = 0;
        for message in recent {
            if added >= 50 {
                break;
            }
            let message: Message =
                serde_json::from_value(message).map_err(|_| "历史档案格式无效。")?;
            if next
                .document
                .conversations
                .iter()
                .any(|m| m.id == message.id)
                || (next.document.conversations.len() >= 500
                    && next
                        .document
                        .conversations
                        .first()
                        .is_some_and(|m| (m.at, &m.id) >= (message.at, &message.id)))
            {
                continue;
            }
            let message_id = message.id.clone();
            let op = Operation {
                id: digest(format!("capture:{}", message.id).as_bytes()),
                kind: "conversation".into(),
                data: serde_json::to_value(message).unwrap(),
            };
            apply(&mut next.document, &op)?;
            // The byte limit may shrink the window below 500. Do not keep publishing
            // an older message that would immediately prune itself on every pass.
            if !next
                .document
                .conversations
                .iter()
                .any(|m| m.id == message_id)
            {
                break;
            }
            if next.server.is_some() {
                next.pending.push(op);
            }
            changed = true;
            added += 1;
        }
        if changed {
            if next.pending.len() > 10000 {
                return Err("待同步队列已满；历史档案已保留，联网后继续。".into());
            }
            if next.server.is_none() {
                next.document.revision += 1;
            }
            self.save(&next)?;
            state.disk = next;
        }
        Ok(())
    }
    pub fn chat_root(&self) -> Result<PathBuf, String> {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .ok_or("陪伴目录无效。".into())
    }
    pub fn sync_chat(&self) -> Result<(), String> {
        // The 15-second chat and background sync timers can arrive together.
        let _running = acquire_sync(&self.running, Duration::from_secs(30))?;
        if self.inner.lock().map_err(|_| "同步状态忙。")?.next_attempt > now() {
            return Err("服务器同步正在退避或等待修正连接，请在同步设置中查看。".into());
        }
        let result = self.synchronize(false);
        if let Err(error) = &result {
            self.note_sync_failure(error);
        }
        result
    }
    fn note_sync_failure(&self, error: &str) {
        if let Ok(mut state) = self.inner.lock() {
            state.status = format!("同步未完成，数据保留待同步：{error}");
            state.active = false;
            state.failures = state.failures.saturating_add(1);
            state.next_attempt = if error.contains("登录失败")
                || error.contains("密钥")
                || error.contains("指纹")
                || error.contains("身份或版本")
            {
                u64::MAX
            } else {
                now() + 15000 * (1u64 << state.failures.min(5))
            };
        }
    }
}

#[tauri::command]
pub async fn companion_sync(
    window: WebviewWindow,
    service: tauri::State<'_, Service>,
    action: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("仅允许桌宠主窗口调用。".into());
    }
    let service = service.inner().clone();
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let data=payload.unwrap_or(Value::Null);
        match action.as_str(){
            "status"=>return service.status(),
            "probe"=>{
                let server:Server=serde_json::from_value(data).map_err(|_|"服务器配置无效。")?;
                let (_,fingerprint,known)=transport::session(&server)?;
                return Ok(json!({"fingerprint":fingerprint,"known":known}));
            }
            "care"=>service.record("care",data)?,
            "personality"=>service.record("personality",data)?,
            "memory"=>service.record("memory",json!({"id":id(),"text":data["text"],"at":now(),"deleted":false}))?,
            "forget"=>service.record("memory",json!({"id":data["id"],"text":"","at":now(),"deleted":true}))?,
            "mood"=>service.record("mood",data)?,
            "capture"=>{
                let mut state=service.inner.lock().map_err(|_|"同步状态忙。")?;let mut next=state.disk.clone();
                next.capture=data["enabled"].as_bool().ok_or("采集开关无效。")?;service.save(&next)?;state.disk=next;
            }
            "local"=>{
                if service.running.swap(true,Ordering::AcqRel){return Err("正在同步，请稍后切换。".into());}
                let _running=Running(service.running.clone());
                let mut state=service.inner.lock().map_err(|_|"同步状态忙。")?;let mut next=state.disk.clone();
                next.server=None;next.pending.clear();service.save(&next)?;state.disk=next;state.active=false;state.status="本地保存；当前缓存及待同步变化已保留为本地状态".into();
            }
            "sync"|"configure"=>{
                if service.running.swap(true,Ordering::AcqRel){return if action=="sync"{service.status()}else{Err("正在同步，请稍后配置。".into())};}
                let _running=Running(service.running.clone());
                let result=if action=="sync"{service.synchronize(data["manual"].as_bool().unwrap_or(true))}else{
                    serde_json::from_value(data["server"].clone()).map_err(|_|"服务器配置无效。".to_string())
                        .and_then(|server|service.configure(server,data["password"].as_str().unwrap_or("")))
                };
                if let Err(ref error)=result{service.note_sync_failure(error);}
                result?;
            }
            "export"=>{
                service.ingest()?;
                let document=service.inner.lock().map_err(|_|"同步状态忙。")?.disk.document.clone();
                let bundle=backup(&document);
                let Some(file)=app.dialog().file().add_filter("企鹅数据",&["json"]).set_file_name("penguinpal-backup.json").blocking_save_file()else{return Ok(json!({"cancelled":true}));};
                let path=file.into_path().map_err(|_|"导出路径无效。")?;write(&path,&serde_json::to_vec_pretty(&bundle).unwrap())?;
                return Ok(json!({"path":path}));
            }
            "previewImport"=>{
                let Some(file)=app.dialog().file().add_filter("企鹅数据",&["json"]).blocking_pick_file()else{return Ok(json!({"cancelled":true}));};
                let document=decode_backup(&read(&file.into_path().map_err(|_|"导入路径无效。")?)?)?;
                let summary=json!({"memories":document.memories.iter().filter(|m|!m.deleted).count(),"conversations":document.conversations.len(),"mood":document.pet.mood});
                service.inner.lock().map_err(|_|"同步状态忙。")?.import=Some(document);return Ok(summary);
            }
            "import"=>{
                let document=service.inner.lock().map_err(|_|"同步状态忙。")?.import.take().ok_or("请先选择并预览备份。")?;
                service.record("restore",json!({"document":document}))?;
            }
            "backfill"=>return service.backfill(),
            "history"=>return service.history_search(&data),
            _=>return Err("不支持的同步操作。".into()),
        }
        service.status()
    }).await.map_err(|_|"同步任务中断；未确认的数据保留待重试。".to_string())?
}

impl Service {
    fn backfill(&self) -> Result<Value, String> {
        self.scan_history()?;
        Ok(json!({"status":self.status()?}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rust_and_server_agree_on_shared_memory_mood_and_manual_outreach() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path(), json!({})).unwrap();
        let initial = service.status().unwrap()["document"].clone();
        let at = 1800000000000u64;
        let operations = vec![
            json!({"id":"settings","kind":"brain","data":{"type":"settings","config":{"probability":100,"romance":false}}}),
            json!({"id":"user","kind":"social","data":{"type":"user","text":"我喜欢清淡小鱼，谢谢你陪我"}}),
            json!({"id":"learn","kind":"social","data":{"type":"analysis","sourceId":"user","memories":[{"kind":"preference","key":"饮食","text":"喜欢清淡小鱼","quote":"我喜欢清淡小鱼","tags":["小鱼","清淡"],"importance":0.9,"emotion":0.2}],"delta":2,"confidence":85,"dims":{"happy":4,"jealous":6},"reason":"被感谢","quote":"谢谢你陪我"}}),
            json!({"id":"consume","kind":"brain","data":{"type":"consume","eventId":"learn","text":"被感谢"}}),
            json!({"id":"reserve","kind":"social","data":{"type":"reserve","token":"manual","manual":true}}),
            json!({"id":"commit","kind":"social","data":{"type":"commit","token":"manual","text":"今天的小鱼想清蒸吗？","topic":"小鱼","selfMood":{"delta":1,"dims":{"calm":2},"confidence":80,"reason":"主动关心"}}}),
            json!({"id":"decision","kind":"brain","data":{"type":"decision","chance":60,"roll":80,"reason":"随机未命中"}}),
        ];
        let mut actual = initial.clone();
        for op in &operations {
            if op["kind"] == "brain" {
                crate::companion_brain::apply(
                    &mut actual,
                    &op["data"],
                    op["id"].as_str().unwrap(),
                    at,
                )
                .unwrap();
            } else {
                crate::companion_social::apply(
                    &mut actual,
                    &op["data"],
                    op["id"].as_str().unwrap(),
                    at,
                )
                .unwrap();
            }
        }
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/brain-protocol-fixture.py");
        let mut child = Command::new("python")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Python protocol test runtime");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                &serde_json::to_vec(&json!({"document":initial,"operations":operations,"now":at}))
                    .unwrap(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Document = serde_json::from_slice(&output.stdout).unwrap();
        let actual: Document = serde_json::from_value(actual).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    #[test]
    fn chat_sync_waits_for_background_exchange_and_bounds_failure() {
        let running = Arc::new(AtomicBool::new(true));
        assert!(acquire_sync(&running, Duration::ZERO).is_err());
        assert!(running.load(Ordering::Acquire));
        let holder = Running(running.clone());
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(holder);
        });
        let acquired = acquire_sync(&running, Duration::from_secs(2)).unwrap();
        release.join().unwrap();
        assert!(running.load(Ordering::Acquire));
        assert!(acquire_sync(&running, Duration::ZERO).is_err());
        drop(acquired);
        assert!(!running.load(Ordering::Acquire));
    }
    fn service() -> (tempfile::TempDir, Service) {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::new(dir.path(), json!({})).unwrap();
        (dir, s)
    }
    #[test]
    fn local_roundtrip_redaction_and_tombstone() {
        let (dir, s) = service();
        s.record(
            "memory",
            json!({"id":"fact-1","text":"password=example-secret","at":1,"deleted":false}),
        )
        .unwrap();
        assert!(!serde_json::to_string(&s.status().unwrap())
            .unwrap()
            .contains("example-secret"));
        s.record(
            "memory",
            json!({"id":"fact-1","text":"","at":2,"deleted":true}),
        )
        .unwrap();
        s.record(
            "memory",
            json!({"id":"fact-1","text":"stale","at":1,"deleted":false}),
        )
        .unwrap();
        let reopened = Service::new(dir.path(), json!({})).unwrap();
        assert_eq!(
            reopened.status().unwrap()["document"]["memories"][0]["deleted"],
            true
        );
    }
    #[test]
    fn invalid_data_does_not_replace_previous() {
        let (_, s) = service();
        let before = s.status().unwrap()["document"].clone();
        assert!(s.record("mood", json!({"delta":1000})).is_err());
        assert_eq!(s.status().unwrap()["document"], before);
        assert!(s
            .record(
                "conversation",
                json!({"id":"a","session":"b","role":"tool","text":"secret","at":1})
            )
            .is_err());
    }
    #[test]
    fn local_restore_adopts_the_exported_pet_identity() {
        let (_a, a) = service();
        let (_b, b) = service();
        let exported = a.status().unwrap()["document"].clone();
        let original = b.status().unwrap()["document"]["petId"].clone();
        b.record("restore", json!({"document":exported})).unwrap();
        let restored = b.status().unwrap()["document"]["petId"].clone();
        assert_ne!(restored, original);
        assert_eq!(restored, exported["petId"]);
    }
    #[test]
    fn corrupt_storage_fails_closed() {
        let (dir, _s) = service();
        fs::write(dir.path().join("sync/local.json"), b"broken").unwrap();
        fs::write(dir.path().join("sync/local.bak.json"), b"also broken").unwrap();
        assert!(Service::new(dir.path(), json!({})).is_err());
    }
    #[test]
    fn old_backup_and_social_roundtrip_keep_cooldowns() {
        let (dir, s) = service();
        let old = s.inner.lock().unwrap().disk.document.clone();
        assert!(serde_json::to_value(&old).unwrap().get("social").is_none());
        // Freeze the pre-social wire representation so this is not a new-writer roundtrip.
        let raw=br#"{"schema":1,"petId":"legacy-pet","revision":0,"updatedAt":1,"pet":{"energy":80.0,"hunger":20.0,"affection":65.0,"personality":"gentle","mood":70.0,"moodReason":"","moodAt":1},"memories":[],"conversations":[],"moodHistory":[],"lastFeedAt":0,"lastPatAt":0}"#;
        let legacy = json!({"format":"penguinpal-companion","version":1,"sha256":digest(raw),"data":serde_json::from_slice::<Value>(raw).unwrap()});
        assert!(decode_backup(&serde_json::to_vec(&legacy).unwrap()).is_ok());
        s.record(
            "social",
            json!({"type":"settings","enabled":true,"proactive":true}),
        )
        .unwrap();
        s.record("social", json!({"type":"user","text":"我喜欢清淡的小鱼"}))
            .unwrap();
        s.record("social", json!({"type":"reserve","token":"first"}))
            .unwrap();
        s.record(
            "social",
            json!({"type":"commit","token":"first","text":"聊聊小鱼？","topic":"小鱼"}),
        )
        .unwrap();
        let saved = s.inner.lock().unwrap().disk.document.clone();
        let decoded = decode_backup(&serde_json::to_vec(&backup(&saved)).unwrap()).unwrap();
        assert_eq!(
            decoded.social.last_outreach_at,
            saved.social.last_outreach_at
        );
        s.record("restore", json!({"document":old})).unwrap();
        let reopened = Service::new(dir.path(), json!({})).unwrap();
        let view = reopened.status().unwrap();
        assert_eq!(view["document"]["social"]["unanswered"], 1);
        assert_eq!(view["document"]["social"]["lastTopic"], "小鱼");
        assert_eq!(
            view["document"]["conversations"].as_array().unwrap().len(),
            2
        );
    }
    #[test]
    fn offline_queue_is_durable_and_backup_excludes_connection() {
        let (dir, s) = service();
        {
            let mut state = s.inner.lock().unwrap();
            state.disk.server = Some(Server {
                host: "sync.example.invalid".into(),
                port: 22,
                username: "test-user".into(),
                namespace: "my-pet".into(),
                fingerprint: String::new(),
            });
            s.save(&state.disk).unwrap();
        }
        s.record(
            "memory",
            json!({"id":"m","text":"remember","at":1,"deleted":false}),
        )
        .unwrap();
        let reopened = Service::new(dir.path(), json!({})).unwrap();
        assert_eq!(reopened.status().unwrap()["mode"], "server");
        assert_eq!(reopened.status().unwrap()["pending"], 1);
        let document = reopened.inner.lock().unwrap().disk.document.clone();
        let exported = backup(&document);
        let text = serde_json::to_string(&exported).unwrap();
        assert!(!text.contains("example.invalid"));
        assert!(!text.contains("username"));
        let decoded = decode_backup(text.as_bytes()).unwrap();
        assert_eq!(decoded.pet_id, document.pet_id);
        let mut invalid = exported;
        invalid["data"]["pet"]["mood"] = json!(99);
        assert!(decode_backup(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    #[test]
    fn offline_queue_and_rolling_backup_never_store_unredacted_text() {
        let (dir, s) = service();
        s.inner.lock().unwrap().disk.server = Some(Server {
            host: "sync.example.invalid".into(),
            port: 22,
            username: "test-user".into(),
            namespace: "my-pet".into(),
            fingerprint: String::new(),
        });
        let sensitive = "password=queue-only-secret";
        s.record(
            "memory",
            json!({"id":"m","text":sensitive,"at":1,"deleted":false}),
        )
        .unwrap();
        s.record(
            "conversation",
            json!({"id":"c","session":"s","role":"user","text":sensitive,"at":1}),
        )
        .unwrap();
        s.record("mood", json!({"delta":5,"reason":sensitive}))
            .unwrap();
        let mut restored = s.status().unwrap()["document"].clone();
        restored["memories"][0]["text"] = json!(sensitive);
        restored["pet"]["moodReason"] = json!(sensitive);
        s.record("restore", json!({"document":restored})).unwrap();
        for name in ["local.json", "local.bak.json"] {
            let bytes = fs::read_to_string(dir.path().join("sync").join(name)).unwrap();
            assert!(
                !bytes.contains("queue-only-secret"),
                "unredacted content in {name}"
            );
        }
        let reopened = Service::new(dir.path(), json!({})).unwrap();
        assert_eq!(reopened.status().unwrap()["pending"], 4);
    }
    #[test]
    fn local_mood_recovers_toward_baseline_without_creating_interactions() {
        let (_dir, s) = service();
        let mut data = s.inner.lock().unwrap();
        data.disk.document.pet.mood = 90.0;
        data.disk.document.updated_at = now() - 12 * 3600000;
        recover_mood(&mut data.disk.document);
        assert!((data.disk.document.pet.mood - 80.0).abs() < 0.001);
        assert!(data.disk.document.mood_history.is_empty());
    }
    #[test]
    fn chat_background_sync_respects_authentication_pause_and_network_backoff() {
        let (_dir, s) = service();
        s.note_sync_failure("登录失败");
        assert!(s.sync_chat().is_err());
        assert_eq!(s.inner.lock().unwrap().next_attempt, u64::MAX);
        assert_eq!(s.inner.lock().unwrap().failures, 1);
        s.note_sync_failure("连接超时");
        assert!(s.sync_chat().is_err());
        assert!(s.inner.lock().unwrap().next_attempt > now());
    }
    #[test]
    fn multiple_large_offline_imports_fit_in_successive_sync_requests() {
        let (_dir, s) = service();
        let mut document = s.inner.lock().unwrap().disk.document.clone();
        document.memories = (0..1500)
            .map(|n| Memory {
                id: format!("m-{n}"),
                text: "x".repeat(1900),
                at: 1,
                deleted: false,
            })
            .collect();
        document.validate().unwrap();
        let mut pending: Vec<_> = (0..3)
            .map(|n| Operation {
                id: format!("import-{n}"),
                kind: "restore".into(),
                data: json!({"document":document}),
            })
            .collect();
        assert!(serde_json::to_vec(&pending).unwrap().len() > 8 * 1024 * 1024);
        let mut acknowledged = Vec::new();
        while !pending.is_empty() {
            let batch = pending_batch(&pending).unwrap();
            assert!(!batch.is_empty());
            let request = json!({"protocol":1,"namespace":"pet","device":"device","claim":true,"seed":{},"operations":batch});
            assert!(serde_json::to_vec(&request).unwrap().len() < 8 * 1024 * 1024);
            acknowledged.extend(batch.iter().map(|op| op.id.clone()));
            pending.drain(..batch.len());
        }
        assert_eq!(acknowledged, ["import-0", "import-1", "import-2"]);
    }
    #[test]
    fn backfill_keeps_the_newest_500_and_skips_subagent_transcripts() {
        let (dir, s) = service();
        let home = dir.path().join("codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let started = now().saturating_sub(60000);
        let mut records =
            vec![json!({"type":"session_meta","payload":{"id":"main-session","source":"cli"}})];
        for n in 0..510 {
            let stamp = chrono::DateTime::from_timestamp_millis((started + n) as i64)
                .unwrap()
                .to_rfc3339();
            records.push(json!({"type":"event_msg","timestamp":stamp,"payload":{"type":"user_message","turn_id":format!("t-{n}"),"message":format!("message-{n}")}}));
        }
        fs::write(
            home.join("sessions/main.jsonl"),
            records
                .iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        let child = [
            json!({"type":"session_meta","payload":{"id":"child-session","source":{"subagent":{"thread_spawn":{}}}}}),
            json!({"type":"event_msg","timestamp":chrono::Utc::now().to_rfc3339(),"payload":{"type":"user_message","message":"private child task"}}),
        ];
        fs::write(
            home.join("sessions/child.jsonl"),
            child
                .iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        for _ in 0..12 {
            s.scan_history_from(&home).unwrap();
        }
        let status = s.status().unwrap();
        assert_eq!(status["history"]["archived"], 510);
        assert_eq!(
            s.history_sources().unwrap().first().unwrap()["text"],
            "message-0"
        );
        let messages = status["document"]["conversations"].as_array().unwrap();
        assert_eq!(messages.len(), 500);
        assert_eq!(messages.first().unwrap()["text"], "message-10");
        assert_eq!(messages.last().unwrap()["text"], "message-509");
        assert!(!status.to_string().contains("private child task"));
    }
    #[test]
    fn backfill_filters_setup_tools_and_reasoning() {
        let (dir, s) = service();
        let home = dir.path().join("codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let stamp = chrono::Utc::now().to_rfc3339();
        let records = vec![
            json!({"type":"session_meta","payload":{"id":"test-session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            json!({"type":"response_item","timestamp":stamp,"payload":{"type":"message","role":"user","content":[{"text":"# AGENTS.md instructions\nsetup"}]}}),
            json!({"type":"response_item","timestamp":stamp,"payload":{"type":"message","role":"user","content":[{"text":"记住我喜欢小鱼"}]}}),
            json!({"type":"response_item","timestamp":stamp,"payload":{"type":"function_call_output","role":"tool","content":[{"text":"private tool output"}]}}),
            json!({"type":"response_item","timestamp":stamp,"payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"text":"progress"}]}}),
            json!({"type":"response_item","timestamp":stamp,"payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"已经记下了"}]}}),
        ];
        fs::write(
            home.join("sessions/rollout.jsonl"),
            records
                .iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        s.scan_history_from(&home).unwrap();
        s.scan_history_from(&home).unwrap();
        let value = s.status().unwrap();
        assert_eq!(
            value["document"]["conversations"].as_array().unwrap().len(),
            2
        );
        let text = value.to_string();
        assert!(!text.contains("private tool output"));
        assert!(!text.contains("AGENTS.md"));
        assert!(!text.contains("progress"));
    }
    #[test]
    fn byte_limited_recent_window_does_not_requeue_pruned_history_forever() {
        let (dir, s) = service();
        let messages:Vec<Value>=(0..300).map(|n|json!({"id":format!("full-{n}"),"session":"thread","role":"user","text":"鱼".repeat(6000),"at":n+1})).collect();
        s.history.store(&messages, &[]).unwrap();
        {
            let mut state = s.inner.lock().unwrap();
            state.disk.server = Some(Server {
                host: "sync.example.invalid".into(),
                port: 22,
                username: "fixture".into(),
                namespace: "pet".into(),
                fingerprint: String::new(),
            });
        }
        for _ in 0..8 {
            s.scan_history_from(dir.path()).unwrap();
        }
        let before = s.status().unwrap();
        assert!(
            before["document"]["conversations"]
                .as_array()
                .unwrap()
                .len()
                < 500
        );
        assert_eq!(before["history"]["archived"], 300);
        for _ in 0..3 {
            s.scan_history_from(dir.path()).unwrap();
        }
        assert_eq!(s.status().unwrap()["pending"], before["pending"]);
    }
}
