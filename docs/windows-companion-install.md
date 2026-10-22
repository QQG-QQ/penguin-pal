# Windows 安装与启动

从 [GitHub Releases](https://github.com/QQG-QQ/penguin-pal/releases/latest) 下载 `PenguinPal-Companion-0.6.1-windows-x64-setup.exe`，双击并按向导完成安装。安装到当前用户；提供桌面快捷方式、开始菜单入口和卸载程序。全新安装通常不需要管理员权限；迁移机器级旧版 MSI 或安装缺少的 WebView2 时可能需要管理员确认。

安装后双击桌面“PenguinPal 企鹅桌宠”即可启动，无需源码、Node.js、Rust 或开发服务器。企鹅本身可以独立运行；打开 Codex 后，已配置的本地联动继续显示任务状态，聊天仍由现有 Codex 窗口提供。

设置和已导入素材使用原有 `com.penguinpal.app/companion` 用户数据目录。更新安装后沿用原设置。需要退出时使用企鹅控制面板或系统托盘的“退出”。

## 从旧版升级

1. 在旧版“检查软件更新”中检查，打开下载页；也可直接访问上方 Releases 链接。
2. 从托盘正常退出 PenguinPal，再运行新版安装包。旧版只提供检查和下载入口，需要手动运行安装包。
3. 安装器识别旧版 `PenguinPal Assistant` 的 EXE / MSI 安装。EXE 使用保留用户数据的卸载模式；MSI 通过原产品升级代码交给 Windows Installer 卸载，可能弹出 UAC。取消授权或卸载失败会中止安装，可重试。
4. 完成后使用桌面“PenguinPal 企鹅桌宠”。新版继续使用相同应用数据标识，不删除旧聊天、语音模型或 `companion` 设置及素材，也不修改现有 Codex 登录、记忆或 hook 信任。

此次升级切换到 Codex 附属桌宠，旧独立助手的私有聊天/语音界面不在正式安装包中。历史数据保留在原目录，聊天入口打开当前 Codex；两者的聊天记录不会自动合并。识别到的旧版自启入口会迁移到新版，关闭自启的用户不会被强制开启。

如果旧版安装记录损坏、卸载程序缺失，请先修复或卸载旧版，保留用户数据后重试。安装中途失败也可重新运行新版安装包。无安装记录的旧便携 EXE 不会被自动删除，应自行停止使用其旧快捷方式。

发布同时提供同名 `.exe.sha256` 校验文件。新桌宠的“软件更新”只下载已通过 SHA-256 校验的正式 Companion 包，仍由用户运行安装程序。

开发时，以安装配置构建默认 companion 应用，再用 PowerShell 7 单独生成安装包：

```powershell
npm exec tauri build -- --no-bundle --config src-tauri/tauri.companion-installer.conf.json
pwsh -NoProfile -File ./scripts/package-companion.ps1 -OutputDirectory ./outputs/installers
```

此命令仅适用于默认 companion 构建。安装包包含已构建的应用和内置动画；私有独立助手模式的 Codex/MCP 运行时不打入此安装包。Windows 缺少 WebView2 时，安装向导会使用 Tauri 默认引导方式安装所需运行时。
