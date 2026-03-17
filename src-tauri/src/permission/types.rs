//! Permission Types - 权限系统类型定义
//!
//! 独立于 AI 的权限层：AI 可以自主决定记忆与规则，但不能自主决定权限。

use serde::{Deserialize, Serialize};

// ============================================================================
// 权限核心类型
// ============================================================================

/// 权限条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: PermissionCategory,
    pub level: PermissionLevel,
    pub granted: bool,
    pub granted_by: GrantSource,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
    pub scope: PermissionScope,
    pub requires_confirmation: bool,
    pub confirmation_cooldown_ms: Option<u64>,
    pub last_confirmed_at: Option<u64>,
}

impl Permission {
    pub fn new(id: &str, name: &str, category: PermissionCategory, level: PermissionLevel) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            category,
            level,
            granted: false,
            granted_by: GrantSource::NotGranted,
            granted_at: 0,
            expires_at: None,
            scope: PermissionScope::Global,
            requires_confirmation: false,
            confirmation_cooldown_ms: None,
            last_confirmed_at: None,
        }
    }

    /// 检查权限是否有效
    pub fn is_valid(&self) -> bool {
        if !self.granted {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if crate::memory::now_millis() > expires_at {
                return false;
            }
        }

        true
    }

    /// 检查是否需要重新确认
    pub fn needs_reconfirmation(&self) -> bool {
        if !self.requires_confirmation {
            return false;
        }

        let Some(cooldown) = self.confirmation_cooldown_ms else {
            return false;
        };

        let Some(last_confirmed) = self.last_confirmed_at else {
            return true;
        };

        crate::memory::now_millis() - last_confirmed > cooldown
    }
}

/// 权限类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCategory {
    FileSystem,      // 文件系统操作
    Network,         // 网络访问
    Process,         // 进程管理
    Shell,           // Shell 命令执行
    System,          // 系统设置
    Privacy,         // 隐私数据
    Desktop,         // 桌面控制
    Memory,          // 记忆系统
    Rule,            // 规则系统
}

impl PermissionCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FileSystem => "文件系统",
            Self::Network => "网络访问",
            Self::Process => "进程管理",
            Self::Shell => "Shell 命令",
            Self::System => "系统设置",
            Self::Privacy => "隐私数据",
            Self::Desktop => "桌面控制",
            Self::Memory => "记忆系统",
            Self::Rule => "规则系统",
        }
    }
}

/// 权限级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    ReadOnly = 1,    // 只读
    Limited = 2,     // 受限操作
    Standard = 3,    // 标准操作
    Elevated = 4,    // 提升操作
    Full = 5,        // 完全访问
}

impl Default for PermissionLevel {
    fn default() -> Self {
        Self::ReadOnly
    }
}

/// 权限作用域
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScope {
    Global,
    Session,
    Task(String),
    Path(String),
    Domain(String),
}

impl Default for PermissionScope {
    fn default() -> Self {
        Self::Global
    }
}

/// 授权来源
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantSource {
    NotGranted,
    System,           // 系统默认
    User,             // 用户手动授权
    Policy,           // 策略文件
    Session,          // 会话临时授权
    Inherited,        // 继承自上级权限
}

// ============================================================================
// 权限请求
// ============================================================================

/// 权限请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub permission_id: String,
    pub requested_by: String,      // 请求来源（AI agent ID 或 tool name）
    pub requested_at: u64,
    pub reason: String,
    pub context: Option<String>,
    pub status: RequestStatus,
    pub response: Option<RequestResponse>,
    pub expires_at: u64,
}

impl PermissionRequest {
    pub fn new(permission_id: &str, requested_by: &str, reason: &str) -> Self {
        let now = crate::memory::now_millis();
        Self {
            id: crate::memory::generate_id("perm_req"),
            permission_id: permission_id.to_string(),
            requested_by: requested_by.to_string(),
            requested_at: now,
            reason: reason.to_string(),
            context: None,
            status: RequestStatus::Pending,
            response: None,
            expires_at: now + 5 * 60 * 1000,  // 5 分钟过期
        }
    }

    pub fn is_expired(&self) -> bool {
        crate::memory::now_millis() > self.expires_at
    }
}

/// 请求状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
    Expired,
    Cancelled,
}

/// 请求响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResponse {
    pub decided_by: GrantSource,
    pub decided_at: u64,
    pub granted_scope: Option<PermissionScope>,
    pub granted_duration_ms: Option<u64>,
    pub message: Option<String>,
}

// ============================================================================
// 权限检查结果
// ============================================================================

/// 权限检查结果
#[derive(Debug, Clone)]
pub struct PermissionCheckResult {
    pub allowed: bool,
    pub permission_id: String,
    pub reason: String,
    pub requires_confirmation: bool,
    pub pending_request: Option<PermissionRequest>,
}

impl PermissionCheckResult {
    pub fn allowed(permission_id: &str) -> Self {
        Self {
            allowed: true,
            permission_id: permission_id.to_string(),
            reason: "权限已授予".to_string(),
            requires_confirmation: false,
            pending_request: None,
        }
    }

    pub fn denied(permission_id: &str, reason: &str) -> Self {
        Self {
            allowed: false,
            permission_id: permission_id.to_string(),
            reason: reason.to_string(),
            requires_confirmation: false,
            pending_request: None,
        }
    }

    pub fn needs_confirmation(permission_id: &str, request: PermissionRequest) -> Self {
        Self {
            allowed: false,
            permission_id: permission_id.to_string(),
            reason: "需要用户确认".to_string(),
            requires_confirmation: true,
            pending_request: Some(request),
        }
    }
}

// ============================================================================
// 权限策略
// ============================================================================

/// 权限策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPolicy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<PolicyRule>,
    pub priority: u32,
    pub enabled: bool,
}

/// 策略规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub category: Option<PermissionCategory>,
    pub permission_pattern: Option<String>,  // 通配符匹配
    pub action: PolicyAction,
    pub conditions: Vec<PolicyCondition>,
}

/// 策略动作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    Allow,
    Deny,
    RequireConfirmation,
    Delegate,  // 委托给用户决定
}

/// 策略条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCondition {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

// ============================================================================
// 审计日志
// ============================================================================

/// 权限审计日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAuditEntry {
    pub id: String,
    pub timestamp: u64,
    pub action: AuditAction,
    pub permission_id: String,
    pub actor: String,
    pub result: String,
    pub detail: Option<String>,
}

/// 审计动作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Check,
    Grant,
    Revoke,
    Request,
    Approve,
    Deny,
    Expire,
    Confirm,
}

impl PermissionAuditEntry {
    pub fn new(action: AuditAction, permission_id: &str, actor: &str, result: &str) -> Self {
        Self {
            id: crate::memory::generate_id("perm_audit"),
            timestamp: crate::memory::now_millis(),
            action,
            permission_id: permission_id.to_string(),
            actor: actor.to_string(),
            result: result.to_string(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
}
