use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub namespace: String,
    pub fingerprint: String,
}
impl Server {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.is_empty()
            || self.host.len() > 253
            || self.host.starts_with('-')
            || !self
                .host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
            || self.port == 0
            || self.username.is_empty()
            || self.username.len() > 64
            || !self
                .username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            || !super::companion_sync::valid_id(&self.namespace)
        {
            return Err("请检查服务器地址、SSH 端口、用户名和宠物存储名。".into());
        }
        Ok(())
    }
    fn credential_key(&self) -> String {
        format!(
            "PenguinPal.Sync.{}",
            super::companion_sync::digest(
                format!("{}:{}:{}", self.host, self.port, self.username).as_bytes()
            )
        )
    }
}

pub fn session(server: &Server) -> Result<(ssh2::Session, String, bool), String> {
    server.validate()?;
    let address = (server.host.as_str(), server.port)
        .to_socket_addrs()
        .map_err(|_| "无法解析服务器地址。")?
        .next()
        .ok_or("服务器地址无效。")?;
    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(8))
        .map_err(|_| "无法连接服务器 SSH 服务。")?;
    socket
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "无法设置连接超时。")?;
    socket
        .set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "无法设置连接超时。")?;
    let mut session = ssh2::Session::new().map_err(|_| "无法初始化 SSH。")?;
    session.set_timeout(15000);
    session.set_compress(true);
    session.set_tcp_stream(socket);
    session
        .handshake()
        .map_err(|_| "SSH 握手失败；请确认服务和加密算法兼容。")?;
    let (key, _) = session.host_key().ok_or("服务器没有提供主机密钥。")?;
    let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(key)));
    let mut known = session.known_hosts().map_err(|_| "无法校验服务器密钥。")?;
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let mut matched = false;
    if let Some(path) = home
        .map(|p| p.join(".ssh/known_hosts"))
        .filter(|p| p.is_file())
    {
        known
            .read_file(&path, ssh2::KnownHostFileKind::OpenSSH)
            .map_err(|_| "无法读取已知主机记录。")?;
        match known.check_port(&server.host, server.port, key) {
            ssh2::CheckResult::Mismatch => {
                return Err("服务器主机密钥已变化；已拒绝发送密码和数据。".into())
            }
            ssh2::CheckResult::Match => matched = true,
            ssh2::CheckResult::Failure => return Err("服务器主机密钥校验失败。".into()),
            ssh2::CheckResult::NotFound => (),
        }
    }
    if !server.fingerprint.is_empty() && server.fingerprint != fingerprint {
        return Err("服务器指纹与保存值不同；已拒绝发送密码和数据。".into());
    }
    Ok((session, fingerprint, matched))
}

pub fn exchange(server: &Server, password: Option<&str>, request: &Value) -> Result<Value, String> {
    let (session, fingerprint, known) = session(server)?;
    if server.fingerprint != fingerprint && !known {
        return Err("请先核对并确认服务器指纹。".into());
    }
    let password = password.filter(|s| !s.is_empty());
    let stored = if password.is_none() {
        credential_read(server)?
    } else {
        None
    };
    let password = password.or(stored.as_deref());
    if let Some(secret) = password.filter(|s| !s.is_empty()) {
        session
            .userauth_password(&server.username, secret)
            .map_err(|_| "SSH 登录失败；请检查用户名、密码及服务器是否允许密码登录。")?;
    } else {
        session
            .userauth_agent(&server.username)
            .map_err(|_| "没有可用的已保存密码或 SSH 代理身份，请填写密码。")?;
    }
    if !session.authenticated() {
        return Err("服务器身份验证未完成。".into());
    }
    let bytes = serde_json::to_vec(request).map_err(|_| "同步数据无法编码。")?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("本次同步数据超过 8 MB。".into());
    }
    let mut channel = session
        .channel_session()
        .map_err(|_| "无法建立同步通道。")?;
    channel
        .handle_extended_data(ssh2::ExtendedData::Ignore)
        .map_err(|_| "无法设置同步通道。")?;
    // Only the bundled, fixed program enters the command string; all user data uses stdin.
    let program = include_str!("../resources/companion/sync-server.py").replace('\'', "'\\''");
    channel
        .exec(&format!("python3 -c '{program}'"))
        .map_err(|_| "无法启动服务器 Python 3。")?;
    channel
        .write_all(&bytes)
        .map_err(|_| "同步发送中断；数据保留待重试。")?;
    channel.send_eof().map_err(|_| "同步发送中断。")?;
    let mut output = Vec::new();
    Read::by_ref(&mut channel)
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut output)
        .map_err(|_| "同步接收中断；下次会去重重试。")?;
    if output.len() > 8 * 1024 * 1024 {
        return Err("服务器响应超过安全上限。".into());
    }
    channel.wait_close().map_err(|_| "无法确认同步结果。")?;
    if channel.exit_status().map_err(|_| "无法读取服务器状态。")? != 0 {
        return Err("服务器同步失败；需要 Python 3 和当前账号目录的读写权限。".into());
    }
    serde_json::from_slice(&output).map_err(|_| "服务器返回的数据格式无效。".into())
}

#[cfg(windows)]
pub fn credential_write(server: &Server, password: &str) -> Result<(), String> {
    use windows::{core::PWSTR, Win32::Security::Credentials::*};
    if password.len() > 1024 {
        return Err("密码过长。".into());
    }
    let mut target: Vec<u16> = server
        .credential_key()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut user: Vec<u16> = server.username.encode_utf16().chain(Some(0)).collect();
    let mut blob = password.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    let result = unsafe { CredWriteW(&credential, 0) }
        .map_err(|_| "无法将密码保存到 Windows 凭据管理器。".to_string());
    blob.fill(0);
    result
}
#[cfg(windows)]
pub fn credential_read(server: &Server) -> Result<Option<String>, String> {
    use windows::{core::PCWSTR, Win32::Security::Credentials::*};
    let target: Vec<u16> = server
        .credential_key()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut credential = std::ptr::null_mut();
    if unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    }
    .is_err()
    {
        return Ok(None);
    }
    if credential.is_null() {
        return Err("凭据管理器返回空记录。".into());
    }
    let result = unsafe {
        let item = &*credential;
        if item.CredentialBlobSize > 1024 {
            CredFree(credential.cast());
            return Err("保存的凭据无效。".into());
        }
        if item.CredentialBlobSize == 0 {
            CredFree(credential.cast());
            return Ok(None);
        }
        if item.CredentialBlob.is_null() {
            CredFree(credential.cast());
            return Err("保存的凭据内容无效。".into());
        }
        let bytes =
            std::slice::from_raw_parts(item.CredentialBlob, item.CredentialBlobSize as usize);
        let result = String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|_| "保存的凭据无法读取。".into());
        CredFree(credential.cast());
        result
    };
    result
}
#[cfg(not(windows))]
pub fn credential_write(_: &Server, _: &str) -> Result<(), String> {
    Err("当前版本的受保护密码存储需要 Windows。".into())
}
#[cfg(not(windows))]
pub fn credential_read(_: &Server) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_endpoint_is_rejected_before_network() {
        let server = Server {
            host: "host;command".into(),
            port: 22,
            username: "user".into(),
            namespace: "pet".into(),
            fingerprint: String::new(),
        };
        assert!(server.validate().is_err());
        assert!(Server {
            host: "example.invalid".into(),
            namespace: "../escape".into(),
            ..server
        }
        .validate()
        .is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_credential_roundtrip_and_cleanup() {
        use windows::{core::PCWSTR, Win32::Security::Credentials::*};
        let server = Server {
            host: "credential-test.invalid".into(),
            port: 22,
            username: uuid::Uuid::new_v4().to_string(),
            namespace: "test".into(),
            fingerprint: String::new(),
        };
        struct Cleanup(Vec<u16>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = unsafe { CredDeleteW(PCWSTR(self.0.as_ptr()), CRED_TYPE_GENERIC, None) };
            }
        }
        let _cleanup = Cleanup(
            server
                .credential_key()
                .encode_utf16()
                .chain(Some(0))
                .collect(),
        );
        credential_write(&server, "temporary-test-only").unwrap();
        assert_eq!(
            credential_read(&server).unwrap().as_deref(),
            Some("temporary-test-only")
        );
    }
    #[test]
    #[ignore = "requires an explicitly configured private endpoint"]
    fn configured_host_identity_probe() {
        let server = Server {
            host: std::env::var("PENGUIN_SYNC_TEST_HOST").unwrap(),
            port: std::env::var("PENGUIN_SYNC_TEST_PORT")
                .unwrap_or("22".into())
                .parse()
                .unwrap(),
            username: std::env::var("PENGUIN_SYNC_TEST_USER").unwrap(),
            namespace: "protocol-test".into(),
            fingerprint: String::new(),
        };
        let (_, fingerprint, known) = session(&server).unwrap();
        assert!(fingerprint.starts_with("SHA256:"));
        assert!(known);
    }
}
