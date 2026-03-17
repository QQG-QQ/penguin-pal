//! Permission Checker - 权限检查器
//!
//! 核心原则：AI 不能自主修改权限，所有权限变更必须经过用户确认。

use super::types::*;
use std::collections::HashMap;

/// 权限检查器
pub struct PermissionChecker {
    permissions: HashMap<String, Permission>,
    policies: Vec<PermissionPolicy>,
    pending_requests: HashMap<String, PermissionRequest>,
    audit_log: Vec<PermissionAuditEntry>,
}

impl PermissionChecker {
    pub fn new() -> Self {
        Self {
            permissions: HashMap::new(),
            policies: Vec::new(),
            pending_requests: HashMap::new(),
            audit_log: Vec::new(),
        }
    }

    /// 检查权限
    pub fn check(&mut self, permission_id: &str, actor: &str) -> PermissionCheckResult {
        // 1. 检查是否有直接授权
        if let Some(permission) = self.permissions.get(permission_id) {
            if permission.is_valid() {
                // 检查是否需要重新确认
                if permission.needs_reconfirmation() {
                    let request = PermissionRequest::new(permission_id, actor, "权限需要重新确认");
                    self.pending_requests.insert(request.id.clone(), request.clone());
                    self.log_audit(AuditAction::Check, permission_id, actor, "needs_reconfirmation");
                    return PermissionCheckResult::needs_confirmation(permission_id, request);
                }

                self.log_audit(AuditAction::Check, permission_id, actor, "allowed");
                return PermissionCheckResult::allowed(permission_id);
            }
        }

        // 2. 检查策略
        let policy_result = self.check_policies(permission_id, actor);
        if let Some(result) = policy_result {
            return result;
        }

        // 3. 默认拒绝
        self.log_audit(AuditAction::Check, permission_id, actor, "denied");
        PermissionCheckResult::denied(permission_id, "权限未授予")
    }

    /// 检查策略
    fn check_policies(&mut self, permission_id: &str, actor: &str) -> Option<PermissionCheckResult> {
        // 按优先级排序
        let mut sorted_policies: Vec<_> = self.policies.iter().filter(|p| p.enabled).collect();
        sorted_policies.sort_by(|a, b| b.priority.cmp(&a.priority));

        for policy in sorted_policies {
            for rule in &policy.rules {
                if self.rule_matches(rule, permission_id) {
                    match rule.action {
                        PolicyAction::Allow => {
                            self.log_audit(AuditAction::Check, permission_id, actor, "allowed_by_policy");
                            return Some(PermissionCheckResult::allowed(permission_id));
                        }
                        PolicyAction::Deny => {
                            self.log_audit(AuditAction::Check, permission_id, actor, "denied_by_policy");
                            return Some(PermissionCheckResult::denied(permission_id, "被策略禁止"));
                        }
                        PolicyAction::RequireConfirmation => {
                            let request = PermissionRequest::new(permission_id, actor, "策略要求用户确认");
                            self.pending_requests.insert(request.id.clone(), request.clone());
                            self.log_audit(AuditAction::Check, permission_id, actor, "needs_confirmation");
                            return Some(PermissionCheckResult::needs_confirmation(permission_id, request));
                        }
                        PolicyAction::Delegate => {
                            // 委托给用户决定
                            let request = PermissionRequest::new(permission_id, actor, "请用户决定是否授权");
                            self.pending_requests.insert(request.id.clone(), request.clone());
                            self.log_audit(AuditAction::Check, permission_id, actor, "delegated");
                            return Some(PermissionCheckResult::needs_confirmation(permission_id, request));
                        }
                    }
                }
            }
        }

        None
    }

    /// 检查规则是否匹配
    fn rule_matches(&self, rule: &PolicyRule, permission_id: &str) -> bool {
        // 检查权限模式
        if let Some(pattern) = &rule.permission_pattern {
            if !Self::pattern_matches(pattern, permission_id) {
                return false;
            }
        }

        // 检查类别
        if let Some(category) = &rule.category {
            if let Some(permission) = self.permissions.get(permission_id) {
                if permission.category != *category {
                    return false;
                }
            }
        }

        true
    }

    /// 通配符匹配
    fn pattern_matches(pattern: &str, value: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return value.starts_with(prefix);
        }

        pattern == value
    }

    /// 授予权限（必须由用户调用）
    pub fn grant(&mut self, permission_id: &str, granted_by: GrantSource, scope: PermissionScope, duration_ms: Option<u64>) -> Result<(), String> {
        // 关键安全检查：只有 User 或 System 可以授权
        match &granted_by {
            GrantSource::User | GrantSource::System | GrantSource::Policy => {}
            _ => return Err("只有用户、系统或策略可以授予权限".to_string()),
        }

        let now = crate::memory::now_millis();
        let expires_at = duration_ms.map(|d| now + d);

        let mut permission = self.permissions
            .entry(permission_id.to_string())
            .or_insert_with(|| Permission::new(permission_id, permission_id, PermissionCategory::System, PermissionLevel::Standard));

        permission.granted = true;
        permission.granted_by = granted_by.clone();
        permission.granted_at = now;
        permission.expires_at = expires_at;
        permission.scope = scope;

        self.log_audit(AuditAction::Grant, permission_id, &format!("{:?}", granted_by), "granted");
        Ok(())
    }

    /// 撤销权限
    pub fn revoke(&mut self, permission_id: &str, revoked_by: &str) -> Result<(), String> {
        if let Some(permission) = self.permissions.get_mut(permission_id) {
            permission.granted = false;
            permission.granted_by = GrantSource::NotGranted;
            self.log_audit(AuditAction::Revoke, permission_id, revoked_by, "revoked");
            Ok(())
        } else {
            Err("权限不存在".to_string())
        }
    }

    /// 批准待处理请求
    pub fn approve_request(&mut self, request_id: &str, scope: PermissionScope, duration_ms: Option<u64>) -> Result<(), String> {
        let request = self.pending_requests.get_mut(request_id)
            .ok_or_else(|| "请求不存在".to_string())?;

        if request.is_expired() {
            request.status = RequestStatus::Expired;
            return Err("请求已过期".to_string());
        }

        request.status = RequestStatus::Approved;
        request.response = Some(RequestResponse {
            decided_by: GrantSource::User,
            decided_at: crate::memory::now_millis(),
            granted_scope: Some(scope.clone()),
            granted_duration_ms: duration_ms,
            message: None,
        });

        let permission_id = request.permission_id.clone();
        self.grant(&permission_id, GrantSource::User, scope, duration_ms)?;
        self.log_audit(AuditAction::Approve, &permission_id, "user", "approved");
        Ok(())
    }

    /// 拒绝待处理请求
    pub fn deny_request(&mut self, request_id: &str, message: Option<String>) -> Result<(), String> {
        let request = self.pending_requests.get_mut(request_id)
            .ok_or_else(|| "请求不存在".to_string())?;

        request.status = RequestStatus::Denied;
        request.response = Some(RequestResponse {
            decided_by: GrantSource::User,
            decided_at: crate::memory::now_millis(),
            granted_scope: None,
            granted_duration_ms: None,
            message,
        });

        let permission_id = request.permission_id.clone();
        self.log_audit(AuditAction::Deny, &permission_id, "user", "denied");
        Ok(())
    }

    /// 确认权限使用
    pub fn confirm(&mut self, permission_id: &str) -> Result<(), String> {
        let permission = self.permissions.get_mut(permission_id)
            .ok_or_else(|| "权限不存在".to_string())?;

        if !permission.is_valid() {
            return Err("权限无效".to_string());
        }

        permission.last_confirmed_at = Some(crate::memory::now_millis());
        self.log_audit(AuditAction::Confirm, permission_id, "user", "confirmed");
        Ok(())
    }

    /// 添加策略
    pub fn add_policy(&mut self, policy: PermissionPolicy) {
        // 检查是否已存在同 ID 策略
        if let Some(pos) = self.policies.iter().position(|p| p.id == policy.id) {
            self.policies[pos] = policy;
        } else {
            self.policies.push(policy);
        }
    }

    /// 获取待处理请求
    pub fn pending_requests(&self) -> Vec<&PermissionRequest> {
        self.pending_requests
            .values()
            .filter(|r| matches!(r.status, RequestStatus::Pending) && !r.is_expired())
            .collect()
    }

    /// 清理过期请求
    pub fn cleanup_expired(&mut self) {
        let now = crate::memory::now_millis();

        // 清理过期请求
        for request in self.pending_requests.values_mut() {
            if request.is_expired() && matches!(request.status, RequestStatus::Pending) {
                request.status = RequestStatus::Expired;
                self.audit_log.push(
                    PermissionAuditEntry::new(AuditAction::Expire, &request.permission_id, "system", "expired")
                );
            }
        }

        // 清理过期权限
        for permission in self.permissions.values_mut() {
            if let Some(expires_at) = permission.expires_at {
                if now > expires_at && permission.granted {
                    permission.granted = false;
                    permission.granted_by = GrantSource::NotGranted;
                    self.audit_log.push(
                        PermissionAuditEntry::new(AuditAction::Expire, &permission.id, "system", "permission_expired")
                    );
                }
            }
        }
    }

    /// 获取审计日志
    pub fn audit_log(&self) -> &[PermissionAuditEntry] {
        &self.audit_log
    }

    /// 记录审计日志
    fn log_audit(&mut self, action: AuditAction, permission_id: &str, actor: &str, result: &str) {
        self.audit_log.push(
            PermissionAuditEntry::new(action, permission_id, actor, result)
        );

        // 限制审计日志大小
        const MAX_AUDIT_LOG_SIZE: usize = 1000;
        if self.audit_log.len() > MAX_AUDIT_LOG_SIZE {
            self.audit_log = self.audit_log.split_off(self.audit_log.len() - MAX_AUDIT_LOG_SIZE);
        }
    }

    /// 获取所有权限
    pub fn all_permissions(&self) -> Vec<&Permission> {
        self.permissions.values().collect()
    }

    /// 获取授权权限
    pub fn granted_permissions(&self) -> Vec<&Permission> {
        self.permissions.values().filter(|p| p.is_valid()).collect()
    }

    /// 获取所有策略
    pub fn all_policies(&self) -> Vec<&PermissionPolicy> {
        self.policies.iter().collect()
    }

    /// 恢复权限（从持久化存储加载）
    pub fn restore_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission.id.clone(), permission);
    }
}

impl Default for PermissionChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_check_denied_by_default() {
        let mut checker = PermissionChecker::new();
        let result = checker.check("shell:execute", "ai_agent");
        assert!(!result.allowed);
    }

    #[test]
    fn test_permission_grant_and_check() {
        let mut checker = PermissionChecker::new();

        // 授予权限
        checker.grant("shell:execute", GrantSource::User, PermissionScope::Global, None).unwrap();

        // 检查权限
        let result = checker.check("shell:execute", "ai_agent");
        assert!(result.allowed);
    }

    #[test]
    fn test_permission_revoke() {
        let mut checker = PermissionChecker::new();

        checker.grant("shell:execute", GrantSource::User, PermissionScope::Global, None).unwrap();
        assert!(checker.check("shell:execute", "ai_agent").allowed);

        checker.revoke("shell:execute", "user").unwrap();
        assert!(!checker.check("shell:execute", "ai_agent").allowed);
    }

    #[test]
    fn test_ai_cannot_grant_permissions() {
        let mut checker = PermissionChecker::new();

        // AI 尝试授予权限应该失败
        let result = checker.grant("shell:execute", GrantSource::Session, PermissionScope::Global, None);
        assert!(result.is_err());
    }
}
