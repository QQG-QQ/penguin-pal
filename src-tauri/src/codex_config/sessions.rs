//! Codex 会话管理
//!
//! 支持会话持久化和恢复，兼容标准 Codex CLI 的 sessions 目录结构。

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// 会话 ID (UUID)
    pub id: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后活动时间
    pub updated_at: DateTime<Utc>,
    /// 会话标题/摘要
    pub title: Option<String>,
    /// 关联的项目路径
    pub project_path: Option<String>,
    /// 消息历史
    pub messages: Vec<SessionMessage>,
    /// 会话状态
    pub status: SessionStatus,
}

/// 会话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// 会话状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Abandoned,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Active
    }
}

impl Session {
    /// 创建新会话
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now,
            updated_at: now,
            title: None,
            project_path: None,
            messages: Vec::new(),
            status: SessionStatus::Active,
        }
    }

    /// 添加消息
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.add_message_at(role, content, Utc::now());
    }

    pub fn add_message_at(
        &mut self,
        role: &str,
        content: &str,
        timestamp: DateTime<Utc>,
    ) {
        self.messages.push(SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp,
        });
        self.updated_at = timestamp;

        // 自动设置标题（取第一条用户消息的前 50 字符）
        if self.title.is_none() && role == "user" {
            self.title = Some(content.chars().take(50).collect::<String>());
        }
    }

    /// 标记为完成
    pub fn complete(&mut self) {
        self.status = SessionStatus::Completed;
        self.updated_at = Utc::now();
    }

    /// 获取会话文件路径
    fn file_path(sessions_dir: &Path, id: &str, created_at: DateTime<Utc>) -> PathBuf {
        let local: DateTime<Local> = created_at.into();
        sessions_dir
            .join(local.format("%Y").to_string())
            .join(local.format("%m").to_string())
            .join(format!("{}.json", id))
    }

    /// 保存会话
    pub fn save(&self, sessions_dir: &Path) -> Result<(), String> {
        let path = Self::file_path(sessions_dir, &self.id, self.created_at);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建会话目录失败: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化会话失败: {}", e))?;

        fs::write(&path, content).map_err(|e| format!("保存会话失败: {}", e))
    }

    /// 加载会话
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("读取会话文件失败: {}", e))?;

        serde_json::from_str(&content).map_err(|e| format!("解析会话失败: {}", e))
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// 会话管理器
pub struct SessionManager {
    /// sessions 目录路径
    sessions_dir: PathBuf,
    /// 当前活动会话
    current_session: Option<Session>,
}

impl SessionManager {
    /// 创建会话管理器
    pub fn new(codex_home: &Path) -> Self {
        Self {
            sessions_dir: codex_home.join("sessions"),
            current_session: None,
        }
    }

    /// 确保 sessions 目录存在
    fn ensure_dir(&self) -> Result<(), String> {
        fs::create_dir_all(&self.sessions_dir)
            .map_err(|e| format!("创建 sessions 目录失败: {}", e))
    }

    /// 创建新会话
    pub fn new_session(&mut self) -> Result<&Session, String> {
        self.ensure_dir()?;

        // 保存之前的会话
        if let Some(ref session) = self.current_session {
            session.save(&self.sessions_dir)?;
        }

        let session = Session::new();
        self.current_session = Some(session);
        Ok(self.current_session.as_ref().unwrap())
    }

    /// 获取当前会话
    pub fn current(&self) -> Option<&Session> {
        self.current_session.as_ref()
    }

    /// 获取当前会话（可变）
    pub fn current_mut(&mut self) -> Option<&mut Session> {
        self.current_session.as_mut()
    }

    /// 获取当前会话 ID
    pub fn current_id(&self) -> Option<&str> {
        self.current_session.as_ref().map(|s| s.id.as_str())
    }

    /// 添加消息到当前会话
    pub fn add_message(&mut self, role: &str, content: &str) -> Result<(), String> {
        if let Some(ref mut session) = self.current_session {
            session.add_message(role, content);
            session.save(&self.sessions_dir)?;
            Ok(())
        } else {
            Err("当前没有活动会话".to_string())
        }
    }

    pub fn add_message_at(
        &mut self,
        role: &str,
        content: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<(), String> {
        if let Some(ref mut session) = self.current_session {
            session.add_message_at(role, content, timestamp);
            session.save(&self.sessions_dir)?;
            Ok(())
        } else {
            Err("当前没有活动会话".to_string())
        }
    }

    /// 恢复指定会话
    pub fn resume(&mut self, session_id: &str) -> Result<&Session, String> {
        self.ensure_dir()?;

        // 保存之前的会话
        if let Some(ref session) = self.current_session {
            session.save(&self.sessions_dir)?;
        }

        // 搜索会话文件
        let session = self.find_session(session_id)?;
        self.current_session = Some(session);
        Ok(self.current_session.as_ref().unwrap())
    }

    /// 恢复最近的会话
    pub fn resume_last(&mut self) -> Result<&Session, String> {
        self.ensure_dir()?;

        let latest = self.find_latest_session()?;
        self.current_session = Some(latest);
        Ok(self.current_session.as_ref().unwrap())
    }

    /// 搜索会话
    fn find_session(&self, session_id: &str) -> Result<Session, String> {
        // 递归搜索 sessions 目录
        self.search_sessions_recursive(&self.sessions_dir, session_id)
            .ok_or_else(|| format!("未找到会话: {}", session_id))
    }

    fn search_sessions_recursive(&self, dir: &Path, session_id: &str) -> Option<Session> {
        if !dir.is_dir() {
            return None;
        }

        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(session) = self.search_sessions_recursive(&path, session_id) {
                    return Some(session);
                }
            } else if path.is_file() {
                let file_name = path.file_stem()?.to_str()?;
                if file_name == session_id {
                    return Session::load(&path).ok();
                }
            }
        }
        None
    }

    /// 查找最近的会话
    fn find_latest_session(&self) -> Result<Session, String> {
        let mut latest: Option<(PathBuf, DateTime<Utc>)> = None;

        self.collect_session_files(&self.sessions_dir, &mut |path| {
            if let Ok(session) = Session::load(path) {
                match &latest {
                    Some((_, time)) if session.updated_at > *time => {
                        latest = Some((path.to_path_buf(), session.updated_at));
                    }
                    None => {
                        latest = Some((path.to_path_buf(), session.updated_at));
                    }
                    _ => {}
                }
            }
        });

        latest
            .map(|(path, _)| Session::load(&path))
            .ok_or_else(|| "没有找到任何会话".to_string())?
    }

    fn collect_session_files(&self, dir: &Path, callback: &mut dyn FnMut(&Path)) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    self.collect_session_files(&path, callback);
                } else if path.extension().map(|e| e == "json").unwrap_or(false) {
                    callback(&path);
                }
            }
        }
    }

    /// 列出最近的会话
    pub fn list_recent(&self, limit: usize) -> Vec<Session> {
        let mut sessions: Vec<Session> = Vec::new();

        self.collect_session_files(&self.sessions_dir, &mut |path| {
            if let Ok(session) = Session::load(path) {
                sessions.push(session);
            }
        });

        // 按更新时间排序
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(limit);
        sessions
    }

    /// 保存当前会话
    pub fn save_current(&self) -> Result<(), String> {
        if let Some(ref session) = self.current_session {
            session.save(&self.sessions_dir)
        } else {
            Ok(())
        }
    }

    /// 完成当前会话
    pub fn complete_current(&mut self) -> Result<(), String> {
        if let Some(ref mut session) = self.current_session {
            session.complete();
            session.save(&self.sessions_dir)?;
        }
        self.current_session = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session() {
        let session = Session::new();
        assert!(!session.id.is_empty());
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[test]
    fn test_add_message() {
        let mut session = Session::new();
        session.add_message("user", "Hello!");
        session.add_message("assistant", "Hi there!");

        assert_eq!(session.messages.len(), 2);
        assert!(session.title.is_some());
    }
}
