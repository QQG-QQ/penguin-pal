//! User-owned drawing runtime. Never launch a command supplied by model output.
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const BASE_URL: &str = "http://127.0.0.1:8198";
const MODEL: &str = "DreamShaper8_LCM.safetensors";
const COMMIT: &str = "2c75dc1b379a59e0880619d029607af5169a62ef";
const MODEL_BYTES: u64 = 2_133_804_992;
const IDLE_SECONDS: u64 = 300;
#[derive(Default)]
struct Runtime {
    server: Option<Child>,
    installer: Option<Child>,
    last_used: Option<Instant>,
    error: String,
}
static STATE: OnceLock<Mutex<Runtime>> = OnceLock::new();
fn state() -> &'static Mutex<Runtime> { STATE.get_or_init(|| Mutex::new(Runtime::default())) }
fn config_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("com.penguinpal.app"))
        .ok_or("内置绘图目前需要 Windows 本机环境。".into())
}
fn read_json(path: &Path) -> Result<Value, String> {
    if fs::metadata(path).map_err(|_| "内置绘图尚未安装。")?.len() > 32 * 1024 {
        return Err("绘图运行时配置过大。".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| "无法读取绘图运行时。")?)
        .map_err(|_| "绘图运行时配置损坏，请修复安装。".into())
}
fn validate_root(pointer: &Value) -> Result<PathBuf, String> {
    if pointer["schema"] != 1 || pointer["provider"] != "penguinpal-comfyui" {
        return Err("不是桌宠管理的绘图运行时。".into());
    }
    let text = pointer["root"].as_str().ok_or("绘图目录无效。")?;
    let path = PathBuf::from(text);
    if !path.is_absolute() || text.starts_with("\\\\") || text.starts_with("//") {
        return Err("绘图运行时必须位于本机磁盘。".into());
    }
    Ok(path)
}
fn installed_at(directory: &Path) -> Result<(PathBuf, Value), String> {
    let root = validate_root(&read_json(&directory.join("comfy-runtime.json"))?)?;
    let manifest = read_json(&root.join("runtime.json"))?;
    if validate_root(&manifest)? != root || manifest["comfyCommit"] != COMMIT
        || manifest["mode"] != "cpu" || manifest["port"] != 8198 || manifest["model"] != MODEL
        || !root.join("python/python.exe").is_file() || !root.join("app/main.py").is_file()
        || fs::metadata(root.join("app/models/checkpoints").join(MODEL)).map(|m| m.len()).ok() != Some(MODEL_BYTES) {
        return Err("内置绘图文件不完整或版本不匹配，请修复安装。".into());
    }
    Ok((root, manifest))
}
fn reap(runtime: &mut Runtime) {
    if let Some(child) = runtime.server.as_mut() {
        if let Ok(Some(exit)) = child.try_wait() {
            if !exit.success() { runtime.error = "绘图服务已退出，请查看运行日志。".into(); }
            runtime.server = None;
        }
    }
    if let Some(child) = runtime.installer.as_mut() {
        if let Ok(Some(exit)) = child.try_wait() {
            if !exit.success() { runtime.error = "绘图安装未完成，请查看安装状态与日志。".into(); }
            runtime.installer = None;
        }
    }
}
fn stop_owned(child: &mut Option<Child>) -> Result<(), String> {
    if let Some(process) = child.as_mut() {
        if process.try_wait().map_err(|_| "无法检查绘图进程。")?.is_none() {
            process.kill().map_err(|_| "无法停止桌宠绘图进程。")?;
        }
        process.wait().map_err(|_| "无法确认绘图进程退出。")?;
    }
    *child = None;
    Ok(())
}
fn external_install_active(install: &Value) -> bool {
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        if install["state"] != "installing" { return false; }
        let Some(root) = install["root"].as_str() else { return false };
        if !Path::new(root).is_absolute() || root.starts_with("\\\\") || root.starts_with("//") { return false; }
        let lock = Path::new(root).join("install.lock");
        return lock.exists() && fs::OpenOptions::new().read(true).write(true).share_mode(0).open(lock).is_err();
    }
    #[cfg(not(windows))] { let _=install; false }
}
fn port_open() -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127,0,0,1],8198)), Duration::from_millis(150)).is_ok()
}
fn healthy_response(bytes: &[u8]) -> bool {
    let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
    let Some(line_end) = bytes.windows(2).position(|w| w == b"\r\n") else { return false };
    let line = String::from_utf8_lossy(&bytes[..line_end]);
    if line.split_ascii_whitespace().nth(1) != Some("200") { return false; }
    serde_json::from_slice::<Value>(&bytes[split+4..]).is_ok_and(|v| v["system"].is_object() && v["devices"].is_array())
}
fn healthy() -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&SocketAddr::from(([127,0,0,1],8198)), Duration::from_millis(200)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream.write_all(b"GET /system_stats HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").is_err() { return false; }
    let mut bytes = Vec::new();
    if stream.take(64*1024).read_to_end(&mut bytes).is_err() { return false; }
    healthy_response(&bytes)
}
pub fn status(busy: bool) -> Value {
    let Ok(dir) = config_dir() else { return json!({"installed":false,"state":"unavailable"}) };
    let Ok(mut runtime) = state().lock() else { return json!({"installed":false,"state":"busy"}) };
    reap(&mut runtime);
    if busy && runtime.server.is_some() { runtime.last_used = Some(Instant::now()); }
    if !busy && runtime.last_used.is_some_and(|t| t.elapsed() > Duration::from_secs(IDLE_SECONDS)) {
        if let Err(error) = stop_owned(&mut runtime.server) { runtime.error = error; }
        runtime.last_used = None;
    }
    let install = read_json(&dir.join("comfy-install-status.json")).unwrap_or(Value::Null);
    let installed = installed_at(&dir);
    let running = runtime.server.is_some();
    let state = if runtime.installer.is_some() || external_install_active(&install) {"installing"} else if running {if healthy(){"running"}else{"starting"}} else if installed.is_ok(){"stopped"}else{"missing"};
    let (root, manifest) = installed.map(|(p,v)|(json!(p),v)).unwrap_or((Value::Null,Value::Null));
    json!({"installed":manifest.is_object(),"state":state,"root":root,"model":MODEL,"version":manifest["comfyVersion"],
        "baseUrl":BASE_URL,"mode":"cpu","idleSeconds":IDLE_SECONDS,"install":install,"error":runtime.error})
}
fn hidden(command: &mut Command) {
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
}
pub fn install() -> Result<(), String> {
    if !cfg!(all(windows,target_arch="x86_64")) { return Err("内置安装器目前支持 Windows x64。".into()); }
    let mut runtime = state().lock().map_err(|_| "绘图运行时忙。")?;
    reap(&mut runtime);
    if runtime.installer.is_some() || external_install_active(&read_json(&config_dir()?.join("comfy-install-status.json")).unwrap_or(Value::Null)) { return Ok(()); }
    if runtime.server.is_some() { return Err("请先停止内置绘图服务，再修复安装。".into()); }
    let dir = config_dir()?;
    fs::create_dir_all(&dir).map_err(|_| "无法建立绘图组件目录。")?;
    let script = dir.join("install-comfy.ps1");
    fs::write(&script, include_bytes!("../resources/companion/install-comfy.ps1")).map_err(|_| "无法准备绘图安装器。")?;
    let log = fs::File::create(dir.join("comfy-install.log")).map_err(|_| "无法创建安装日志。")?;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"]).arg(script)
        .stdin(Stdio::null()).stdout(log.try_clone().map_err(|_| "无法创建安装输出。")?).stderr(log);
    hidden(&mut command);
    runtime.installer = Some(command.spawn().map_err(|_| "无法启动内置绘图安装。")?);
    runtime.error.clear();
    Ok(())
}
pub fn start() -> Result<(), String> {
    let mut runtime = state().lock().map_err(|_| "绘图运行时忙。")?;
    reap(&mut runtime);
    if runtime.installer.is_some() || external_install_active(&read_json(&config_dir()?.join("comfy-install-status.json")).unwrap_or(Value::Null)) { return Err("绘图组件正在安装，请稍候。".into()); }
    if runtime.server.is_some() { runtime.last_used=Some(Instant::now()); return Ok(()); }
    let (root, _) = installed_at(&config_dir()?)?;
    if port_open() { return Err("内置绘图端口 8198 已被占用；不会接管或停止其他服务。".into()); }
    fs::write(root.join("managed-comfy.py"),include_bytes!("../resources/companion/managed-comfy.py")).map_err(|_| "无法准备绘图启动器。")?;
    let log = fs::OpenOptions::new().create(true).append(true).open(root.join("logs/server.log")).map_err(|_| "无法打开绘图日志。")?;
    let mut command = Command::new(root.join("python/python.exe"));
    command.arg("-s").arg(root.join("managed-comfy.py")).arg(std::process::id().to_string())
        .args(["--cpu","--listen","127.0.0.1","--port","8198","--disable-auto-launch","--disable-all-custom-nodes","--offline","--disable-metadata","--disable-assets","--preview-method","none"])
        .arg("--input-directory").arg(root.join("input")).arg("--output-directory").arg(root.join("output"))
        .arg("--temp-directory").arg(root.join("temp")).arg("--user-directory").arg(root.join("user"))
        .current_dir(root.join("app")).env("OMP_NUM_THREADS","4").env("MKL_NUM_THREADS","4")
        .env("HF_HUB_OFFLINE","1").env("HF_HOME",root.join("cache/huggingface")).env("TORCH_HOME",root.join("cache/torch"))
        .env("COMFYUI_DISABLE_PRODUCT_TIPS","1").stdin(Stdio::null()).stdout(log.try_clone().map_err(|_| "无法复制绘图日志。")?).stderr(log);
    hidden(&mut command);
    runtime.server=Some(command.spawn().map_err(|_| "无法启动内置 ComfyUI，请修复安装。")?);
    runtime.last_used=Some(Instant::now()); runtime.error.clear();
    Ok(())
}
pub fn ensure(cancel: Option<&Path>) -> Result<(), String> {
    start()?;
    let began=Instant::now();
    while began.elapsed()<Duration::from_secs(120) {
        if cancel.is_some_and(Path::exists) { return Err("绘图已取消。".into()); }
        { let mut runtime=state().lock().map_err(|_|"绘图运行时忙。")?;reap(&mut runtime);if runtime.server.is_none(){return Err("内置绘图已退出，请查看运行日志。".into());} }
        if healthy(){return Ok(());}
        std::thread::sleep(Duration::from_millis(300));
    }
    Err("内置 ComfyUI 启动超时，请查看运行日志。".into())
}
pub fn stop() -> Result<(), String> {
    let mut runtime=state().lock().map_err(|_|"绘图运行时忙。")?;
    stop_owned(&mut runtime.server)?;runtime.last_used=None;Ok(())
}
pub fn shutdown() { let _=stop(); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_rejects_untrusted_and_incomplete_locations() {
        for pointer in [json!({}),json!({"schema":1,"provider":"other","root":"C:/runtime"}),json!({"schema":1,"provider":"penguinpal-comfyui","root":"../runtime"}),json!({"schema":1,"provider":"penguinpal-comfyui","root":"//remote/runtime"})] { assert!(validate_root(&pointer).is_err()); }
        let dir=tempfile::tempdir().unwrap();
        fs::write(dir.path().join("comfy-runtime.json"),b"{}").unwrap();
        assert!(installed_at(dir.path()).is_err());
        fs::write(dir.path().join("comfy-runtime.json"),vec![b'x';33*1024]).unwrap();
        assert!(read_json(&dir.path().join("comfy-runtime.json")).is_err());
    }
    #[test]
    fn readiness_requires_success_and_comfy_structure() {
        assert!(healthy_response(b"HTTP/1.0 200 OK\r\n\r\n{\"system\":{},\"devices\":[]}"));
        assert!(!healthy_response(b"HTTP/1.0 500 Error\r\n\r\n{\"system\":{},\"devices\":[]}"));
        assert!(!healthy_response(b"HTTP/1.0 200 OK\r\n\r\n{}"));
        assert!(!healthy_response(b"invalid"));
    }
    #[cfg(windows)]
    #[test]
    fn stopping_owned_child_leaves_other_process_alone() {
        let spawn=||{let mut cmd=Command::new("powershell.exe");cmd.args(["-NoProfile","-NonInteractive","-Command","Start-Sleep -Seconds 30"]);hidden(&mut cmd);cmd.spawn().unwrap()};
        let mut owned=Some(spawn());let mut other=Some(spawn());
        stop_owned(&mut owned).unwrap();assert!(owned.is_none());
        assert!(other.as_mut().unwrap().try_wait().unwrap().is_none());
        stop_owned(&mut other).unwrap();stop_owned(&mut owned).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires the installed private ComfyUI runtime and a free local port"]
    fn installed_runtime_starts_restarts_and_never_takes_over_another_server() {
        use std::net::TcpListener;
        struct Cleanup;
        impl Drop for Cleanup { fn drop(&mut self) { shutdown(); } }
        let _cleanup=Cleanup;
        assert!(status(false)["installed"]==true);
        assert!(!port_open(),"Explicit integration test requires a free managed port");
        ensure(None).unwrap();assert_eq!(status(false)["state"],"running");
        stop().unwrap();assert!(!port_open());
        ensure(None).unwrap();assert_eq!(status(false)["state"],"running");
        stop().unwrap();
        let other=TcpListener::bind(("127.0.0.1",8198)).unwrap();
        assert!(start().unwrap_err().contains("占用"));
        stop().unwrap();assert!(other.local_addr().is_ok());
    }
}
