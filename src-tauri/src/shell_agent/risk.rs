//! Shell 命令风险判断
//!
//! 判断命令是否需要用户确认

/// 判断 shell 命令是否为高风险
pub fn is_high_risk_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();

    // 删除类操作
    if contains_any(&cmd_lower, &[
        "del ", "del/", "erase ", "rmdir", "rd ", "rd/",
        "remove-item", "rm ", "rm -",
    ]) {
        return true;
    }

    // 系统关键操作
    if contains_any(&cmd_lower, &[
        "format", "shutdown", "restart", "reboot",
        "taskkill", "kill ", "stop-process",
        "net stop", "net user", "net localgroup",
        "bcdedit", "diskpart", "chkdsk",
    ]) {
        return true;
    }

    // 注册表操作
    if contains_any(&cmd_lower, &[
        "reg delete", "reg add", "remove-itemproperty",
        "set-itemproperty", "new-itemproperty",
    ]) {
        return true;
    }

    // 安装/执行程序
    if contains_any(&cmd_lower, &[
        ".exe", ".msi", ".bat", ".cmd", ".ps1",
        "msiexec", "start-process",
    ]) {
        // 排除一些安全的查询命令
        if !contains_any(&cmd_lower, &["where ", "which ", "get-command", "dir ", "ls "]) {
            return true;
        }
    }

    // 网络外发
    if contains_any(&cmd_lower, &[
        "curl ", "wget ", "invoke-webrequest", "invoke-restmethod",
        "ftp ", "sftp ", "scp ", "ssh ",
        "net use", "new-psdrive",
    ]) {
        return true;
    }

    // 权限提升
    if contains_any(&cmd_lower, &[
        "runas", "sudo", "gsudo",
        "set-executionpolicy", "bypass",
    ]) {
        return true;
    }

    // 服务操作
    if contains_any(&cmd_lower, &[
        "sc delete", "sc stop", "sc config",
        "stop-service", "remove-service", "set-service",
    ]) {
        return true;
    }

    // 防火墙/安全
    if contains_any(&cmd_lower, &[
        "netsh advfirewall", "set-netfirewallrule",
        "disable-windowsoptionalfeature", "enable-windowsoptionalfeature",
    ]) {
        return true;
    }

    false
}

/// 判断命令是否被禁止（即使确认也不执行）
pub fn is_forbidden_command(cmd: &str) -> Option<&'static str> {
    let cmd_lower = cmd.to_lowercase();

    // 格式化系统盘
    if cmd_lower.contains("format c:") || cmd_lower.contains("format c ") {
        return Some("禁止格式化系统盘");
    }

    // 删除系统目录
    if contains_any(&cmd_lower, &[
        "del c:\\windows", "rmdir c:\\windows",
        "rd c:\\windows", "remove-item c:\\windows",
        "del /s c:\\", "rd /s c:\\",
    ]) {
        return Some("禁止删除系统目录");
    }

    // 破坏性注册表操作
    if contains_any(&cmd_lower, &[
        "reg delete hklm\\system", "reg delete hklm\\software\\microsoft\\windows",
    ]) {
        return Some("禁止删除系统关键注册表");
    }

    None
}

/// 获取风险说明
pub fn get_risk_description(cmd: &str) -> String {
    let cmd_lower = cmd.to_lowercase();

    if contains_any(&cmd_lower, &["del ", "rm ", "rmdir", "remove-item"]) {
        return "此命令会删除文件或目录".to_string();
    }
    if contains_any(&cmd_lower, &["shutdown", "restart", "reboot"]) {
        return "此命令会关闭或重启系统".to_string();
    }
    if contains_any(&cmd_lower, &["reg "]) {
        return "此命令会修改注册表".to_string();
    }
    if contains_any(&cmd_lower, &[".exe", ".msi"]) {
        return "此命令会执行程序".to_string();
    }
    if contains_any(&cmd_lower, &["curl", "wget", "invoke-webrequest"]) {
        return "此命令会进行网络请求".to_string();
    }
    if contains_any(&cmd_lower, &["taskkill", "kill ", "stop-process"]) {
        return "此命令会终止进程".to_string();
    }

    "此命令可能产生系统影响".to_string()
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| text.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_commands() {
        assert!(!is_high_risk_command("dir"));
        assert!(!is_high_risk_command("cd C:\\Users"));
        assert!(!is_high_risk_command("echo hello"));
        assert!(!is_high_risk_command("type file.txt"));
        assert!(!is_high_risk_command("where notepad"));
        assert!(!is_high_risk_command("hostname"));
    }

    #[test]
    fn test_high_risk_commands() {
        assert!(is_high_risk_command("del file.txt"));
        assert!(is_high_risk_command("rmdir /s folder"));
        assert!(is_high_risk_command("shutdown /s"));
        assert!(is_high_risk_command("reg delete HKCU\\Software\\Test"));
        assert!(is_high_risk_command("curl http://example.com"));
        assert!(is_high_risk_command("taskkill /im notepad.exe"));
    }

    #[test]
    fn test_forbidden_commands() {
        assert!(is_forbidden_command("format c:").is_some());
        assert!(is_forbidden_command("del /s c:\\windows").is_some());
        assert!(is_forbidden_command("dir c:\\").is_none());
    }
}
