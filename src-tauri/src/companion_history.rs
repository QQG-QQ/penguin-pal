//! Local, restartable transcript archive. Never synchronize a live Codex database.
use crate::companion_sync::{digest, scrub, valid_id};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const LINE_LIMIT: usize = 1024 * 1024;
const ROUND_BYTES: usize = 4 * 1024 * 1024;
const DB_ERROR: &str = "对话档案读写失败；进度保留，请检查存储空间后重试。";
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn stamp(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|x| x.duration_since(UNIX_EPOCH).ok())
        .map(|x| x.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
#[derive(Clone)]
pub struct History(Arc<Mutex<Connection>>);

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Cursor {
    offset: u64,
    session: String,
    turn: String,
    line: u64,
    excluded: bool,
    discarding: bool,
    anchor: String,
    head: String,
    size: u64,
    stamp: i64,
    last_role: String,
    last_text: String,
    last_kind: String,
    last_id: String,
    last_at: i64,
}
fn archive(db: &Connection, message: &Value) -> rusqlite::Result<()> {
    if !message["id"].as_str().is_some_and(valid_id)
        || !message["session"].as_str().is_some_and(valid_id)
        || !matches!(message["role"].as_str(), Some("user" | "assistant"))
        || message["text"]
            .as_str()
            .is_none_or(|s| s.chars().count() > 6000)
        || message["at"].as_u64().is_none()
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    db.execute(
        "INSERT OR IGNORE INTO messages(id,session,role,text,at,done) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            message["id"].as_str(),
            message["session"].as_str(),
            message["role"].as_str(),
            message["text"].as_str(),
            message["at"].as_u64().unwrap_or(0).min(i64::MAX as u64) as i64,
            i32::from(message["role"] != "user")
        ],
    )?;
    Ok(())
}
fn row_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":row.get::<_,String>(0)?,"session":row.get::<_,String>(1)?,
        "role":row.get::<_,String>(2)?,"text":row.get::<_,String>(3)?,"at":row.get::<_,i64>(4)?}),
    )
}
impl History {
    pub fn open(path: &Path) -> Result<Self, String> {
        if path.exists()
            && !fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            return Err("对话档案必须是普通文件。".into());
        }
        let db = Connection::open(path).map_err(|_| DB_ERROR)?;
        db.busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| DB_ERROR)?;
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,session TEXT NOT NULL,role TEXT NOT NULL,text TEXT NOT NULL,at INTEGER NOT NULL,done INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS learning ON messages(done,at,id);
            CREATE INDEX IF NOT EXISTS recent ON messages(at DESC,id DESC);
            CREATE TABLE IF NOT EXISTS directories(path TEXT PRIMARY KEY,checked INTEGER NOT NULL DEFAULT 0,error TEXT NOT NULL DEFAULT '');
            CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY,size INTEGER NOT NULL,stamp INTEGER NOT NULL,read_size INTEGER NOT NULL DEFAULT -1,read_stamp INTEGER NOT NULL DEFAULT -1,checked INTEGER NOT NULL DEFAULT 0,cursor TEXT NOT NULL DEFAULT '{}',error TEXT NOT NULL DEFAULT '');
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);").map_err(|_| DB_ERROR)?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }
    /// Called before pruning the recent window; acknowledgements survive its 500-item limit.
    pub fn store(&self, messages: &[Value], analyzed: &[String]) -> Result<(), String> {
        let mut db = self.0.lock().map_err(|_| DB_ERROR)?;
        let tx = db.transaction().map_err(|_| DB_ERROR)?;
        for message in messages {
            archive(&tx, message).map_err(|_| DB_ERROR)?;
        }
        for id in analyzed {
            tx.execute("UPDATE messages SET done=1 WHERE id=?1", [id])
                .map_err(|_| DB_ERROR)?;
        }
        tx.commit().map_err(|_| DB_ERROR.into())
    }
    pub fn done(&self, id: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| DB_ERROR)?
            .execute("UPDATE messages SET done=1 WHERE id=?1", [id])
            .map_err(|_| DB_ERROR)?;
        Ok(())
    }
    pub fn pending(&self) -> Result<Vec<Value>, String> {
        self.query(
            "SELECT id,session,role,text,at FROM messages WHERE done=0 ORDER BY at,id LIMIT 8",
            &[],
        )
    }
    pub fn recent(&self) -> Result<Vec<Value>, String> {
        self.query(
            "SELECT id,session,role,text,at FROM messages ORDER BY at DESC,id DESC LIMIT 500",
            &[],
        )
    }
    fn query(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>, String> {
        let db = self.0.lock().map_err(|_| DB_ERROR)?;
        let mut statement = db.prepare(sql).map_err(|_| DB_ERROR)?;
        let rows = statement
            .query_map(args, row_message)
            .map_err(|_| DB_ERROR)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| DB_ERROR.into())
    }
    pub fn search(&self, data: &Value) -> Result<Value, String> {
        let source = data["sourceId"].as_str().unwrap_or("");
        let query = scrub(data["query"].as_str().unwrap_or(""), 100);
        let messages = if !source.is_empty() {
            if !valid_id(source) {
                return Err("来源标识无效。".into());
            }
            self.query(
                "SELECT id,session,role,text,at FROM messages WHERE id=?1",
                &[&source],
            )?
        } else {
            self.query("SELECT id,session,role,text,at FROM messages WHERE instr(text,?1)>0 ORDER BY at DESC,id DESC LIMIT 30", &[&query])?
        };
        Ok(json!({"messages":messages}))
    }
    pub fn note(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| DB_ERROR)?.execute("INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,scrub(value,200)]).map_err(|_| DB_ERROR)?;
        Ok(())
    }
    pub fn status(&self) -> Result<Value, String> {
        let db = self.0.lock().map_err(|_| DB_ERROR)?;
        let count = |sql| {
            db.query_row(sql, [], |r| r.get::<_, i64>(0))
                .map_err(|_| DB_ERROR)
        };
        let mut state = json!({"archived":count("SELECT count(*) FROM messages")?,
            "pending":count("SELECT count(*) FROM messages WHERE done=0")?,
            "processed":count("SELECT count(*) FROM messages WHERE role='user' AND done=1")?,
            "files":count("SELECT count(*) FROM files")?,
            "pendingFiles":count("SELECT count(*) FROM files WHERE size!=read_size OR stamp!=read_stamp")?,
            "pendingDirectories":count("SELECT count(*) FROM directories WHERE checked=0")?,
            "fileErrors":count("SELECT (SELECT count(*) FROM files WHERE error!='')+(SELECT count(*) FROM directories WHERE error!='')")?});
        let mut stmt = db
            .prepare("SELECT key,value FROM meta")
            .map_err(|_| DB_ERROR)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|_| DB_ERROR)?;
        for row in rows {
            let (key, value) = row.map_err(|_| DB_ERROR)?;
            state[key] = json!(value);
        }
        Ok(state)
    }
    /// Fair, bounded passes. Discover both roots without age, project or file-size cutoffs.
    pub fn scan(&self, home: &Path) -> Result<(), String> {
        let mut db = self.0.lock().map_err(|_| DB_ERROR)?;
        for name in ["sessions", "archived_sessions"] {
            let root = home.join(name);
            if root.exists() {
                db.execute(
                    "INSERT OR IGNORE INTO directories(path) VALUES(?1)",
                    [root.to_string_lossy()],
                )
                .map_err(|_| DB_ERROR)?;
            }
        }
        let dirs = {
            let mut stmt = db
                .prepare("SELECT path FROM directories ORDER BY checked,path LIMIT 4")
                .map_err(|_| DB_ERROR)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|_| DB_ERROR)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| DB_ERROR)?
        };
        for path in dirs {
            let tx = db.transaction().map_err(|_| DB_ERROR)?;
            let error = discover(&tx, Path::new(&path))?;
            tx.execute(
                "UPDATE directories SET checked=?2,error=?3 WHERE path=?1",
                params![path, now(), error],
            )
            .map_err(|_| DB_ERROR)?;
            tx.commit().map_err(|_| DB_ERROR)?;
        }
        let files = {
            let mut stmt = db.prepare("SELECT path,cursor FROM files WHERE (size!=read_size OR stamp!=read_stamp) AND (error='' OR checked<?1) ORDER BY checked,path LIMIT 8").map_err(|_| DB_ERROR)?;
            let rows = stmt
                .query_map([now() - 60000], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|_| DB_ERROR)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| DB_ERROR)?
        };
        let mut bytes = 0;
        for (path, raw) in files {
            if bytes >= ROUND_BYTES {
                break;
            }
            let mut cursor: Cursor =
                serde_json::from_str(&raw).map_err(|_| "扫描进度损坏，未跳过文件。")?;
            let tx = db.transaction().map_err(|_| DB_ERROR)?;
            match scan_file(&tx, Path::new(&path), &mut cursor, &mut bytes) {
                Ok(complete) => {
                    tx.execute("UPDATE files SET cursor=?2,read_size=?3,read_stamp=?4,size=?5,stamp=?6,checked=?7,error='' WHERE path=?1",
                        params![path,serde_json::to_string(&cursor).map_err(|_| DB_ERROR)?,if complete {cursor.size as i64}else{-1},if complete {cursor.stamp}else{-1},cursor.size as i64,cursor.stamp,now()]).map_err(|_| DB_ERROR)?;
                    tx.commit().map_err(|_| DB_ERROR)?;
                }
                Err(error) => {
                    tx.rollback().map_err(|_| DB_ERROR)?;
                    db.execute(
                        "UPDATE files SET checked=?2,error=?3 WHERE path=?1",
                        params![path, now(), error],
                    )
                    .map_err(|_| DB_ERROR)?;
                }
            }
        }
        db.execute("INSERT INTO meta VALUES('lastScan',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [now().to_string()]).map_err(|_| DB_ERROR)?;
        Ok(())
    }
}

// Return filesystem errors as a visible per-directory status; propagate SQL failures.
fn discover(db: &Connection, path: &Path) -> Result<String, String> {
    let entries =
        if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
            fs::read_dir(path).ok()
        } else {
            None
        };
    let Some(entries) = entries else {
        return Ok("目录读取失败".into());
    };
    let mut error = String::new();
    for entry in entries {
        let Ok(entry) = entry else {
            error = "目录枚举中断，等待重试。".into();
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            error = "文件信息读取失败，等待重试。".into();
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            db.execute(
                "INSERT OR IGNORE INTO directories(path) VALUES(?1)",
                [path.to_string_lossy()],
            )
            .map_err(|_| DB_ERROR)?;
        } else if meta.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            db.execute("INSERT INTO files(path,size,stamp) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET size=excluded.size,stamp=excluded.stamp",
                params![path.to_string_lossy(),meta.len() as i64,stamp(&meta)]).map_err(|_| DB_ERROR)?;
        }
    }
    Ok(error)
}
fn fingerprint(file: &mut fs::File, end: u64) -> Result<String, String> {
    let start = end.saturating_sub(128);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| "无法定位对话文件。")?;
    let mut bytes = vec![0; (end - start) as usize];
    file.read_exact(&mut bytes)
        .map_err(|_| "对话文件发生变化，等待重试。")?;
    Ok(digest(&bytes))
}
fn issue(db: &Connection, key: &str) -> Result<(), String> {
    db.execute("INSERT INTO meta VALUES(?1,'1') ON CONFLICT(key) DO UPDATE SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT)", [key]).map_err(|_| DB_ERROR)?;
    Ok(())
}
fn scan_file(
    db: &Connection,
    path: &Path,
    cursor: &mut Cursor,
    bytes: &mut usize,
) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "文件暂不可读，等待重试。")?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("跳过非普通对话文件。".into());
    }
    let mut file = fs::File::open(path).map_err(|_| "文件暂不可读，等待重试。")?;
    let head = fingerprint(&mut file, meta.len().min(128))?;
    let replaced = cursor.offset > meta.len()
        || (cursor.offset > 0
            && (fingerprint(&mut file, cursor.offset)? != cursor.anchor
                || (cursor.size == meta.len() && cursor.stamp != stamp(&meta))
                || (cursor.size >= 128 && cursor.head != head)));
    if replaced {
        *cursor = Cursor::default();
    }
    cursor.size = meta.len();
    cursor.stamp = stamp(&meta);
    cursor.head = head;
    if cursor.session.is_empty() {
        cursor.session = digest(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
    }
    file.seek(SeekFrom::Start(cursor.offset))
        .map_err(|_| "无法恢复扫描位置。")?;
    let mut reader = BufReader::new(file);
    let start = *bytes;
    while *bytes < ROUND_BYTES && *bytes - start < 512 * 1024 {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take((LINE_LIMIT + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "对话读取中断，等待重试。")?;
        if count == 0 {
            break;
        }
        *bytes += count;
        let newline = line.last() == Some(&b'\n');
        if cursor.discarding {
            cursor.offset += count as u64;
            cursor.discarding = !newline;
            continue;
        }
        if count > LINE_LIMIT {
            cursor.offset += count as u64;
            cursor.discarding = !newline;
            issue(db, "oversizedLines")?;
            continue;
        }
        let parsed = serde_json::from_slice::<Value>(&line);
        if !newline && parsed.is_err() {
            break;
        } // An actively appended JSONL line: retry without advancing.
        cursor.offset += count as u64;
        cursor.line += 1;
        match parsed {
            Ok(value) => {
                if let Some(message) = visible(&value, cursor) {
                    archive(db, &message).map_err(|_| DB_ERROR)?;
                }
            }
            Err(_) => {
                if !line.iter().all(u8::is_ascii_whitespace) {
                    issue(db, "invalidLines")?;
                }
            }
        }
    }
    let mut file = reader.into_inner();
    cursor.anchor = fingerprint(&mut file, cursor.offset)?;
    Ok(cursor.offset == cursor.size)
}
fn visible(value: &Value, cursor: &mut Cursor) -> Option<Value> {
    let p = &value["payload"];
    if value["type"] == "session_meta" {
        cursor.excluded = p["source"] == "subagent"
            || p["source"].get("subagent").is_some()
            || p.get("agent_id").is_some_and(|v| !v.is_null())
            || p.get("subagent_id").is_some_and(|v| !v.is_null());
        if let Some(id) = p["id"].as_str().or_else(|| p["session_id"].as_str()) {
            cursor.session = digest(id.as_bytes());
        }
        return None;
    }
    if cursor.excluded {
        return None;
    }
    if let Some(turn) = p["turn_id"].as_str() {
        cursor.turn = turn.into();
    }
    let (role, text) = if value["type"] == "response_item" && p["type"] == "message" {
        let role = p["role"].as_str()?;
        if !["user", "assistant"].contains(&role)
            || (role == "assistant" && p["phase"].as_str().is_some_and(|x| x != "final_answer"))
        {
            return None;
        }
        (
            role,
            p["content"]
                .as_array()?
                .iter()
                .filter_map(|x| x["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else if value["type"] == "event_msg" {
        match p["type"].as_str()? {
            "user_message" => ("user", p["message"].as_str()?.into()),
            "agent_message" if p["phase"].as_str().is_none_or(|x| x == "final_answer") => {
                ("assistant", p["message"].as_str()?.into())
            }
            "task_complete" => ("assistant", p["last_agent_message"].as_str()?.into()),
            _ => return None,
        }
    } else {
        return None;
    };
    if text.trim().is_empty()
        || [
            "# AGENTS.md instructions",
            "<environment_context>",
            "<external_codex_apps_open_page>",
        ]
        .iter()
        .any(|x| text.starts_with(x))
    {
        return None;
    }
    let text = scrub(&text, 6000);
    let at = value["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis().max(0))
        .unwrap_or(0)
        .min(now());
    let kind = format!(
        "{}:{}",
        value["type"].as_str().unwrap_or(""),
        p["type"].as_str().unwrap_or("")
    );
    let text_hash = digest(text.as_bytes());
    // Legacy transcripts may repeat "continue" without a turn ID. Retain separate turns,
    // while coalescing the response/event mirror of the same visible message.
    let id = if cursor.turn.is_empty()
        && cursor.last_role == role
        && cursor.last_text == text_hash
        && cursor.last_kind != kind
        && (at - cursor.last_at).abs() < 10000
    {
        cursor.last_id.clone()
    } else {
        let turn = if cursor.turn.is_empty() {
            format!("line:{}", cursor.line)
        } else {
            cursor.turn.clone()
        };
        digest(format!("{}:{turn}:{role}:{text}", cursor.session).as_bytes())
    };
    cursor.last_role = role.into();
    cursor.last_text = text_hash;
    cursor.last_kind = kind;
    cursor.last_id = id.clone();
    cursor.last_at = at;
    Some(json!({"id":id,"session":cursor.session,"role":role,"text":text,"at":at}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn record(turn: usize, text: &str) -> Value {
        json!({"type":"event_msg","timestamp":"2020-01-01T00:00:00Z","payload":{"type":"user_message","turn_id":format!("t-{turn}"),"message":text}})
    }
    fn write_transcript(path: &Path, count: usize) {
        let mut file = fs::File::create(path).unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"id":"test-history"}})
        )
        .unwrap();
        for n in 0..count {
            writeln!(
                file,
                "{}",
                record(n, &format!("old-{n} password=fixture-secret"))
            )
            .unwrap();
        }
    }
    #[test]
    fn all_dates_archives_projects_restart_ack_and_source_lookup() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sessions/project-a/2020")).unwrap();
        fs::create_dir_all(root.path().join("archived_sessions/project-b")).unwrap();
        write_transcript(&root.path().join("sessions/project-a/2020/old.jsonl"), 620);
        // A relocated/archived transcript must not be learned a second time.
        write_transcript(
            &root.path().join("archived_sessions/project-b/old.jsonl"),
            620,
        );
        let path = root.path().join("archive.sqlite3");
        let archive = History::open(&path).unwrap();
        for _ in 0..8 {
            archive.scan(root.path()).unwrap();
        }
        assert_eq!(archive.status().unwrap()["archived"], 620);
        assert_eq!(archive.status().unwrap()["pending"], 620);
        let batch = archive.pending().unwrap();
        let id = batch[0]["id"].as_str().unwrap().to_owned();
        assert_eq!(batch.len(), 8);
        assert_eq!(batch[0]["at"], 1577836800000u64);
        assert!(!batch[0]["text"]
            .as_str()
            .unwrap()
            .contains("fixture-secret"));
        archive.done(&id).unwrap();
        drop(archive);
        let archive = History::open(&path).unwrap();
        for _ in 0..4 {
            archive.scan(root.path()).unwrap();
        }
        assert_eq!(archive.status().unwrap()["pending"], 619);
        assert_eq!(
            archive.search(&json!({"sourceId":id})).unwrap()["messages"][0]["id"],
            id
        );
        assert_eq!(archive.recent().unwrap().len(), 500);
    }
    #[test]
    fn large_file_bounded_pass_resume_append_partial_and_rotation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"id":"large"}})
        )
        .unwrap();
        let tool=json!({"type":"response_item","payload":{"type":"function_call_output","output":"x".repeat(60000)}}).to_string();
        for _ in 0..360 {
            writeln!(file, "{tool}").unwrap();
        }
        writeln!(file, "{}", record(1, "喜欢安静")).unwrap();
        drop(file);
        let archive = History::open(&root.path().join("archive.sqlite3")).unwrap();
        let mut cursor = Cursor::default();
        let mut bytes = 0;
        {
            let db = archive.0.lock().unwrap();
            assert!(!scan_file(&db, &path, &mut cursor, &mut bytes).unwrap());
        }
        assert!(bytes <= 2 * 1024 * 1024);
        assert!(cursor.offset > 0);
        let saved = serde_json::to_string(&cursor).unwrap();
        cursor = serde_json::from_str(&saved).unwrap();
        for _ in 0..50 {
            let db = archive.0.lock().unwrap();
            if scan_file(&db, &path, &mut cursor, &mut 0).unwrap() {
                break;
            }
        }
        assert_eq!(archive.status().unwrap()["archived"], 1);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{{\"type\":").unwrap();
        file.flush().unwrap();
        {
            let db = archive.0.lock().unwrap();
            assert!(!scan_file(&db, &path, &mut cursor, &mut 0).unwrap());
        }
        let offset = cursor.offset;
        write!(file,"\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"turn_id\":\"t-2\",\"message\":\"新追加\"}}}}\n").unwrap();
        drop(file);
        {
            let db = archive.0.lock().unwrap();
            assert!(scan_file(&db, &path, &mut cursor, &mut 0).unwrap());
        }
        assert!(cursor.offset > offset);
        assert_eq!(archive.status().unwrap()["archived"], 2);
        write_transcript(&path, 1);
        {
            let db = archive.0.lock().unwrap();
            assert!(scan_file(&db, &path, &mut cursor, &mut 0).unwrap());
        }
        assert_eq!(archive.status().unwrap()["archived"], 3);
    }
    #[test]
    fn malformed_and_oversized_lines_make_progress_and_report_omissions() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sessions")).unwrap();
        let path = root.path().join("sessions/test.jsonl");
        fs::write(
            &path,
            format!(
                "broken\n{}\n{}\n",
                "x".repeat(LINE_LIMIT + 300),
                record(1, "可恢复的下一条")
            ),
        )
        .unwrap();
        let archive = History::open(&root.path().join("archive.sqlite3")).unwrap();
        for _ in 0..6 {
            archive.scan(root.path()).unwrap();
        }
        let status = archive.status().unwrap();
        assert_eq!(status["invalidLines"], "1");
        assert_eq!(status["oversizedLines"], "1");
        assert_eq!(status["archived"], 1);
        assert_eq!(status["pendingFiles"], 0);
    }
    #[test]
    fn missing_files_report_failure_without_discarding_prior_progress() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sessions")).unwrap();
        let path = root.path().join("sessions/test.jsonl");
        write_transcript(&path, 1);
        let archive = History::open(&root.path().join("archive.sqlite3")).unwrap();
        archive.scan(root.path()).unwrap();
        fs::remove_file(&path).unwrap();
        {
            let db = archive.0.lock().unwrap();
            db.execute("UPDATE files SET size=size+1", []).unwrap();
        }
        archive.scan(root.path()).unwrap();
        assert_eq!(archive.status().unwrap()["fileErrors"], 1);
        assert_eq!(archive.status().unwrap()["archived"], 1);
    }
    #[test]
    fn mirrored_messages_dedupe_but_repeated_legacy_prompts_are_retained() {
        let mut cursor = Cursor {
            session: digest(b"thread"),
            ..Default::default()
        };
        let first = json!({"type":"event_msg","payload":{"type":"user_message","message":"继续"}});
        cursor.line = 1;
        let one = visible(&first, &mut cursor).unwrap();
        cursor.line = 2;
        let two = visible(&first, &mut cursor).unwrap();
        assert_ne!(one["id"], two["id"]);
        let mirror = json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"继续"}]}});
        cursor.line = 3;
        assert_eq!(visible(&mirror, &mut cursor).unwrap()["id"], two["id"]);
    }
}
