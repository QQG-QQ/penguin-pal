# 桌宠内置 Codex 运行时

目标：让 `PenguinPal` 优先使用应用自身携带的 `Codex CLI`，而不是依赖系统全局安装。

## 开发模式

如果你只是想在 Windows 真机上直接跑：

```powershell
npx.cmd tauri dev
```

现在已经支持自动引导：

- `beforeDevCommand` 会执行 `npm run dev:tauri`
- `dev:tauri` 会先运行 `scripts/ensure-codex-runtime.mjs`
- 如果项目私有 Codex 运行时不存在，就自动安装到：

```text
src-tauri/.codex-runtime/windows-x64
```

这不是系统全局安装，只是这个项目自己的私有运行时。

## 运行时解析顺序

1. 应用私有目录：`%LOCALAPPDATA%/com.penguinpal.app/codex/...`
2. 应用资源目录：`src-tauri/resources/codex/windows-x64/...`
3. 开发目录私有运行时：`src-tauri/.codex-runtime/windows-x64/...`
4. 开发目录资源：`src-tauri/resources/codex/windows-x64/...`
5. 最后才回退到系统 `codex`

## 私有登录目录

桌宠会把 `Codex` 的登录状态保存在自己的私有目录，而不是系统 `~/.codex`：

- 私有 home 根目录：应用 `app_data_dir()/codex-runtime`
- 登录文件：`app_data_dir()/codex-runtime/.codex/auth.json`

这样可以做到：

- 桌宠自带 Codex 运行时
- 桌宠自带私有登录状态
- 不污染系统全局 `Codex` 配置

## 打包前准备

先准备一个可运行的私有 Codex 目录，至少包含：

- `node_modules/.bin/codex.cmd`
- `node_modules/.bin/node.exe`
- `node_modules/@openai/codex/...`

然后在仓库根目录执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\prepare-embedded-codex-runtime.ps1 -SourceDir C:\path\to\codex-runtime
```

执行后会复制到：

```text
src-tauri/resources/codex/windows-x64
```

之后再进行：

```powershell
npx.cmd tauri build
```

## 电脑控制边界

即使桌宠内置了 Codex，`Codex` 也不能直接获得无限制系统权限。

它对电脑的操作仍然必须经过 `PenguinPal` 的动作网关：

- 白名单动作
- 权限等级
- 高风险确认短语
- 审计日志

这样才能满足“能操作电脑”但不放开成任意执行。
