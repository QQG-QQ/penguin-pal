# Windows 桌宠验收基线

日期：`2026-10-22`。适用于默认 Codex 附属桌宠 0.5.8。

旧助手的对外 HTTP 控制接口和外部 Agent 反应租约已移除，历史端口/API 烟测不再适用。旧助手内部桌面工具仅保留主窗口 IPC 及其确认流程。

## 自动验证

```powershell
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
cargo clippy --manifest-path src-tauri/Cargo.toml --lib --offline -- -D warnings
```

前端覆盖拖放重力、屏幕边界、落脚平台、Codex 状态优先级、动画中断/退出、视频失败后的 PNG 回退、导入限制和取消等场景。Rust 覆盖状态恢复、事件桥、素材导入校验、更新摘要检查等场景；在线更新测试需显式运行。

旧助手兼容检查需要本地 LLVM/Clang、CMake、Ninja 和 MSVC：

```powershell
cd src-tauri
cargo check --lib --features legacy-assistant --offline -j 1
```

## 原生验收

- 桌面快捷方式启动安装版，确认桌宠和 WebView2 正常运行；重复双击不产生第二只企鹅。
- 检查原有设置和已导入素材保留；不自动改动 Codex hook 信任或登录。
- 播放各内置动作，检查透明背景、循环首尾和切换过渡；PNG 回退保持形象一致。
- 取消素材选择窗口后可以再次导入；同名素材返回明确结果。
- 开启重力时拖到屏幕上方松手，应下落；减少动态和安静模式按各自设置执行。
- Codex hooks 已信任时，真实任务显示工作/等待/结束反馈。

0.5.8 的动画及安装验收记录保存在本地 artifacts 中。多显示器混合 DPI、缺少 WebView2 的首次安装及其他电脑仍需要对应环境验收。
