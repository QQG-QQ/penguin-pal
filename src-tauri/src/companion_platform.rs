//! Read-only desktop geometry and fixed local app/folder launch actions. No window text or screenshots.
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Surface {
    pub id: usize,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Desktop {
    pub surfaces: Vec<Surface>,
    pub context: &'static str,
    pub idle_seconds: u32,
    pub fullscreen: bool,
    pub left_button_down: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetrics {
    pub working_set_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
    pub cpu_millis: Option<u64>,
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct LastInput {
        size: u32,
        tick: u32,
    }
    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }
    #[repr(C)]
    struct MemoryCounters {
        size: u32,
        faults: u32,
        values: [usize; 9],
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, data: isize)
            -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn IsIconic(hwnd: Hwnd) -> i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn GetForegroundWindow() -> Hwnd;
        fn GetLastInputInfo(info: *mut LastInput) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
        fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> Hwnd;
        fn GetMonitorInfoW(monitor: Hwnd, info: *mut MonitorInfo) -> i32;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Hwnd;
        fn QueryFullProcessImageNameW(
            process: Hwnd,
            flags: u32,
            path: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: Hwnd) -> i32;
        fn GetTickCount() -> u32;
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> Hwnd;
        fn GetLastError() -> u32;
        fn GetCurrentProcess() -> Hwnd;
        fn K32GetProcessMemoryInfo(process: Hwnd, counters: *mut MemoryCounters, size: u32) -> i32;
        fn GetProcessTimes(
            process: Hwnd,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    fn executable(hwnd: Hwnd) -> String {
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process = OpenProcess(0x1000, 0, pid);
            if process.is_null() {
                return String::new();
            }
            let mut path = [0u16; 1024];
            let mut size = path.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size);
            CloseHandle(process);
            if ok == 0 {
                return String::new();
            }
            String::from_utf16_lossy(&path[..size as usize])
                .rsplit('\\')
                .next()
                .unwrap_or_default()
                .to_lowercase()
        }
    }

    unsafe extern "system" fn collect(hwnd: Hwnd, data: isize) -> i32 {
        let result = &mut *(data as *mut Vec<Surface>);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id()
            || IsWindowVisible(hwnd) == 0
            || IsIconic(hwnd) != 0
            || GetWindowLongPtrW(hwnd, -20) & 0x80 != 0
        {
            return 1;
        }
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) != 0
            && rect.right - rect.left >= 160
            && rect.bottom - rect.top >= 100
        {
            // EnumWindows is in Z order. Do not land on a fully covered window.
            if !result.iter().any(|front| {
                front.left <= rect.left
                    && front.right >= rect.right
                    && front.top <= rect.top
                    && front.bottom >= rect.top + 8
            }) {
                result.push(Surface {
                    id: hwnd as usize,
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                });
            }
        }
        i32::from(result.len() < 32)
    }

    pub fn desktop(surfaces: bool, awareness: bool) -> Desktop {
        let mut result = Desktop {
            context: "quiet",
            ..Desktop::default()
        };
        unsafe {
            result.left_button_down = GetAsyncKeyState(1) < 0;
            if surfaces {
                EnumWindows(collect, &mut result.surfaces as *mut _ as isize);
            }
            if awareness {
                let hwnd = GetForegroundWindow();
                let name = executable(hwnd);
                result.context = super::classify(&name);
                let mut input = LastInput {
                    size: std::mem::size_of::<LastInput>() as u32,
                    tick: 0,
                };
                if GetLastInputInfo(&mut input) != 0 {
                    result.idle_seconds = GetTickCount().wrapping_sub(input.tick) / 1000;
                }
            }
            {
                // Fullscreen suppression only reads geometry, even with context sensing off.
                let hwnd = GetForegroundWindow();
                let mut info = MonitorInfo {
                    size: std::mem::size_of::<MonitorInfo>() as u32,
                    monitor: Rect::default(),
                    work: Rect::default(),
                    flags: 0,
                };
                let mut rect = Rect::default();
                if GetMonitorInfoW(MonitorFromWindow(hwnd, 2), &mut info) != 0
                    && GetWindowRect(hwnd, &mut rect) != 0
                    && executable(hwnd) != "explorer.exe"
                {
                    result.fullscreen = rect.left <= info.monitor.left
                        && rect.top <= info.monitor.top
                        && rect.right >= info.monitor.right
                        && rect.bottom >= info.monitor.bottom;
                }
            }
        }
        result
    }

    unsafe extern "system" fn find_codex(hwnd: Hwnd, data: isize) -> i32 {
        // Skip tray/tool windows; retain minimized main windows so they can be restored.
        if IsWindowVisible(hwnd) == 0 || GetWindowLongPtrW(hwnd, -20) & 0x80 != 0 {
            return 1;
        }
        if matches!(executable(hwnd).as_str(), "codex.exe" | "chatgpt.exe") {
            *(data as *mut Hwnd) = hwnd;
            return 0;
        }
        1
    }

    pub fn open_codex() -> Result<(), String> {
        unsafe {
            let mut hwnd: Hwnd = std::ptr::null_mut();
            EnumWindows(find_codex, &mut hwnd as *mut _ as isize);
            if hwnd.is_null() {
                return Err("请先打开已登录的 Codex 桌面应用。".into());
            }
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, 9);
            } else {
                ShowWindow(hwnd, 5);
            }
            if SetForegroundWindow(hwnd) == 0 {
                return Err(
                    "Codex 已打开；Windows 暂未允许切换焦点，请点击任务栏中的 Codex。".into(),
                );
            }
        }
        Ok(())
    }
    pub struct InstanceGuard(Hwnd);
    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    unsafe extern "system" fn reveal_pet(hwnd: Hwnd, _: isize) -> i32 {
        let mut rect = Rect::default();
        if matches!(
            executable(hwnd).as_str(),
            "penguin-pal.exe" | "penguinpal-companion.exe"
        ) && GetWindowRect(hwnd, &mut rect) != 0
            && rect.right - rect.left >= 160
            && rect.bottom - rect.top >= 100
        {
            ShowWindow(hwnd, 5);
            SetForegroundWindow(hwnd);
            return 0;
        }
        1
    }
    pub fn metrics() -> ProcessMetrics {
        let mut result = ProcessMetrics::default();
        unsafe {
            let process = GetCurrentProcess();
            let size = std::mem::size_of::<MemoryCounters>() as u32;
            let mut counters = MemoryCounters {
                size,
                faults: 0,
                values: [0; 9],
            };
            if K32GetProcessMemoryInfo(process, &mut counters, size) != 0 {
                result.working_set_bytes = Some(counters.values[1] as u64);
                result.private_bytes = Some(counters.values[8] as u64);
            }
            let (mut created, mut exited, mut kernel, mut user) = (
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
            );
            if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
                let ticks = |f: FileTime| (f.high as u64) << 32 | f.low as u64;
                result.cpu_millis = Some(ticks(kernel).saturating_add(ticks(user)) / 10000);
            }
        }
        result
    }
    pub fn claim_instance() -> Result<Option<InstanceGuard>, String> {
        let name: Vec<u16> = "Local\\PenguinPal.CodexCompanion.v1\0"
            .encode_utf16()
            .collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err("无法建立桌宠单实例锁".into());
            }
            if GetLastError() == 183 {
                CloseHandle(handle);
                EnumWindows(reveal_pet, 0);
                return Ok(None);
            }
            Ok(Some(InstanceGuard(handle)))
        }
    }
}

fn classify(name: &str) -> &'static str {
    match name {
        "code.exe" | "devenv.exe" | "codex.exe" | "chatgpt.exe" => "coding",
        "winword.exe" | "excel.exe" | "powerpnt.exe" => "document",
        "vlc.exe" | "mpv.exe" | "potplayermini64.exe" => "video",
        // Browsers and sensitive/unknown apps deliberately stay quiet. No title inference.
        _ => "quiet",
    }
}

#[cfg(windows)]
pub use windows::{claim_instance, desktop, metrics, open_codex};
#[cfg(not(windows))]
pub fn desktop(_: bool, _: bool) -> Desktop {
    Desktop::default()
}
#[cfg(not(windows))]
pub fn open_codex() -> Result<(), String> {
    Err("当前附属桌宠仅支持 Windows。".into())
}
#[cfg(not(windows))]
pub fn claim_instance() -> Result<Option<()>, String> {
    Ok(Some(()))
}
#[cfg(not(windows))]
pub fn metrics() -> ProcessMetrics {
    ProcessMetrics::default()
}

pub fn open_folder(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .arg(path)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| "无法打开文件夹")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("仅支持 Windows".into())
    }
}
pub fn open_releases() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32.exe")
            .args([
                "url.dll,FileProtocolHandler",
                "https://github.com/QQG-QQ/penguin-pal/releases",
            ])
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| "无法打开发布页")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("仅支持 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_allowlisted_process_categories_are_exposed() {
        assert_eq!(classify("code.exe"), "coding");
        for name in ["chrome.exe", "1password.exe", "outlook.exe", "unknown.exe"] {
            assert_eq!(classify(name), "quiet");
        }
    }
}
