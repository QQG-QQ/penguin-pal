# Windows 安装与启动

双击 `PenguinPal-版本号-Setup.exe`，按安装向导完成安装。安装到当前用户，不需要管理员权限；提供桌面快捷方式、开始菜单入口和卸载程序。

安装后双击桌面“PenguinPal 企鹅桌宠”即可启动，无需源码、Node.js、Rust 或开发服务器。企鹅本身可以独立运行；打开 Codex 后，已配置的本地联动继续显示任务状态，聊天仍由现有 Codex 窗口提供。

设置和已导入素材使用原有 `com.penguinpal.app/companion` 用户数据目录。更新安装后沿用原设置。需要退出时使用企鹅控制面板或系统托盘的“退出”。

开发时，先构建默认 companion 应用，再用 PowerShell 7 单独生成安装包，不重新编译已验收的应用：

```powershell
pwsh -NoProfile -File ./scripts/package-companion.ps1 -OutputDirectory ./outputs/installers
```

此命令仅适用于默认 companion 构建。安装包包含已构建的应用和内置动画；私有独立助手模式的 Codex/MCP 运行时不打入此安装包。Windows 缺少 WebView2 时，安装向导会使用 Tauri 默认引导方式安装所需运行时。
