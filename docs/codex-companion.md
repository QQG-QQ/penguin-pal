# PenguinPal · Codex 附属桌宠

默认启动模式现在是 Codex 陪伴模式：点击企鹅或 `Codex ↗` 激活已经打开的 Codex 桌面应用，在 Codex 原有界面聊天和使用语音。登录、会话、项目和长期记忆由当前 Codex 管理。企鹅不创建第二套模型会话，也不会将旧助手的记忆自动导入 Codex。

## 日常互动

- 快点企鹅：打开 Codex，同时根据头部/身体给出摸摸或轻戳反馈。单击有 330 毫秒延迟以区分双击；双击打开投喂小面板。按住超过 400 毫秒再松开只作本地摸摸，继续按住会连续摸头。
- 拖动后松手：重力、抛掷和轻微碰撞；可以在设置中关闭。窗口顶部可以落脚，并随该窗口移动；窗口消失后继续下落。
- `摸摸`、`喂小鱼`、`休息`：本地陪伴，连续投喂和连续戳动有频率限制。体力、饥饿、亲近程度会保存，离线变化最多计算 24 小时。
- 散步默认关闭，可手动开启。工作中、等待确认、鼠标交互、菜单、全屏场景会优先暂停自主移动。
- `减少动态` 遵循系统偏好并关闭自主运动与抛掷；`安静陪伴` 关闭本地气泡和自主活动。
- 透明留白允许点击下面的窗口。右键打开深蓝快速面板，默认只显示投喂、Codex、开心、休息和移动模式；展开后可调整大小、节奏、速度、悬停、全屏、置顶、穿透、自启和本地语气。面板中的“隐藏”和托盘可控制显示或退出；启用角色穿透后用托盘“快速控制”恢复设置入口。
- 当前“内置企鹅”统一使用 `penguin/companion-v4` 素材。豆包视频按原角色提取透明帧，补齐循环及进入/退出片段；视频和 PNG 回退使用同一套形象。素材来源和校验信息见该目录的 `asset-provenance.json`，旧素材目录保留给已有工具和测试依赖。
- 0.4 新增「陪伴 / 动作 / 素材」入口、带速度采样的弹性拖拽和落地回弹。可关闭弹性拖拽或视频；原有工具箱和各设置入口保留。[动画包指南](animation-packs.md) 说明本地 PNG / WebM 导入、限制、回退与参考来源。
- 多显示器拖放会在松手时重新选择当前显示器边界。不同缩放比例的显示器、远程桌面和虚拟桌面切换仍需在对应环境中验收。
- 开启 `贴边停靠` 后，把企鹅拖至显示器左、右、上边缘附近松开即可停靠。开启 `自动半隐藏` 后，离开约 1.4 秒会缩回边缘；悬停、打开控制面板或收到任务提醒时展开。拖出可解除停靠，`回到屏幕内` 可恢复可见位置。
- 行走播放进入、循环、退出阶段；拖拽和 Codex 紧急状态可中断过渡。阶段复用现有 32 帧，没有新增专门的起步/收步绘图。
- 空闲逻辑 5 Hz，动作约 15 Hz，运动最高约 30 Hz，隐藏时 1 Hz；体力与饥饿按实际经过时间计算。实际资源占用还取决于 WebView2 和桌面环境。

## Codex 联动

1. 先打开已经登录的 Codex 桌面应用。
2. 在企鹅设置中点 `安装 / 更新 Codex 联动`。安装器会合并当前 `CODEX_HOME/hooks.json`（未设置时为用户目录下 `.codex/hooks.json`），保留其他 hooks，并保存带时间戳的备份。
3. 按 Codex 当前版本提供的 hooks 管理入口查看并信任 `PenguinPal 本地状态` 的定义，必要时重新打开会话。官方文档给出的入口为 `/hooks`。安装器不会替用户写入信任记录。
4. 在 Codex 发起一个任务，企鹅应显示 `Codex 正在工作`；等待工具审批时显示 `Codex 等待你的确认`；Stop 显示 `本轮已结束`，不把它当成任务成功证明；中断显示 `已中断`。

`任务提醒` 控制等待确认、本轮结束、中断、工具显式错误时的本地气泡，安静模式仍优先。只有工具响应中的 `isError`、`is_error`、`success:false` 或非零整数 `exit_code` 会触发短暂错误状态；不分析回复文字。PostToolUse 使用后台 hook，避免每个工具调用等待桌宠脚本；结束类事件保持同步。跨会话并发显示优先级为等待确认、工具错误、工作中、已结束、中断。

事件桥使用 Node.js 标准库和已有的 Node.js 安装，不需要 API key 或新账户。安装时记录当前 `node.exe` 路径；移动/卸载 Node.js 后需重新安装联动。当前支持的事件为 UserPromptSubmit、PermissionRequest、PostToolUse、Stop、Interrupt、SessionEnd。事件是尽力而为的状态提示，长时间无事件会回到待机。

关闭 `跟随 Codex 工作状态` 会停止写入并清除待消费事件。桌宠退出后，事件队列有上限；重新启动时会忽略过期事件。若要卸载联动，在 `hooks.json` 中删除 statusMessage 为 `PenguinPal 本地状态` 的六组处理器，保留其他处理器。不要整份恢复旧备份而覆盖后来添加的设置。

## 数据和权限

0.4 首次读取旧宠物状态时，会在写入新选项前原样保存 `companion/state.pre-0.4.json`，已有检查点不会覆盖。需要回退到 0.3 时，先退出桌宠，再备份当前 `state.json`，将该检查点复制为 `state.json`；旧版本无法直接读取 0.4 新增的偏好字段。检查点只包含原来的宠物状态，不含 Codex 聊天或记忆。

新模式仅保存 `%LOCALAPPDATA%/com.penguinpal.app/companion/` 中的本地宠物状态、备份、联动脚本、临时事件队列、有限日志、会话恢复标记及用户主动生成的导出/下载文件。事件内容仅含事件名、散列后的会话/轮次标识、接收时间和可选错误布尔值。脚本不记录提示词、回复、工具输入、transcript 路径或登录凭据。

`本地场景感知` 默认关闭。开启后只在内存中识别白名单进程类别、是否全屏和输入空闲时间，显示稀疏的本地陪伴语句；浏览器和未知程序保持安静。不读取窗口标题、文档正文或截图。窗口落脚单独读取可见窗口的位置和大小。

默认原生入口只注册 companion 命令。旧版对外 HTTP 控制服务、发现文件和外部 Agent 反应租约已从源码移除。开机自启仅在你切换开关后通过原有 autostart 插件设置。旧版助手源码、数据和用户已有修改保留；默认二进制不启用旧语音识别、模型聊天或电脑控制模块，网络客户端仅用于用户主动检查/下载 GitHub 更新。原有配置中的 CSP 限制继续生效。

## 工具箱、恢复与更新

右键企鹅 → `工具箱`：

- `状态与恢复` 显示 Node、处理器数量、脚本版本、最近接收事件、无效事件数与日志。处理器存在不等于用户已经信任。运行数据只统计 Rust 宿主，完整 WebView2 进程树使用下面的采样脚本。日志只有事件代码和时间，每份约 256 KiB，最多当前文件及三个备份。
- 24 小时内检测到两次连续非正常退出后进入安全模式，临时关闭自主运动和感知。原偏好保留；可手动退出安全模式，正常退出会清除异常计数。系统强制终止、断电也可能计为异常，尚不能准确定位崩溃根因。
- `旧资料导出` 仅枚举旧记忆、聊天和最多 100 份归档；每份最多 2 MiB，每次最多 20 份、总计 20 MiB。内容只在明确点击查看后读取，导出为本地 JSON；不读取 provider/auth/settings 配置，也不把内容迁移到 Codex。
- `软件更新` 手动检查 `QQG-QQ/penguin-pal` 发布页，仅接受正式 `companion-vX.Y.Z` 标签与对应架构的 `PenguinPal-Companion-X.Y.Z-windows-x64[-setup].exe`（ARM64 使用 arm64）。必须有 GitHub 资产 `sha256:` digest 或同名 `.exe.sha256` 文件，缺少摘要则禁用下载。下载不超过 256 MiB，SHA-256 通过后保存在本地 updates 目录，不自动运行或安装。没有 Companion 发布包时不会选择旧助手版本。

本次不恢复投研、私有模型登录、独立长期记忆写入和另一套电脑操作/审批入口。原模块留在 legacy 源码中便于回退，默认无入口。

完整进程树采样（先退出已运行的企鹅，脚本会启动并在采样后正常退出）：

```powershell
powershell -NoProfile -File scripts/measure-companion.ps1 -Executable "完整路径/PenguinPal-Companion.exe" -Output "artifacts/performance.json" -Seconds 30
```

## 开发和回退

```powershell
npm ci
npm run tauri:dev
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
```

独立助手作为 `legacy-assistant` Cargo feature 保留。需要回到旧模式时，前后端必须成对切换：

```powershell
$env:VITE_PENGUIN_LEGACY = '1'
npm run tauri:dev -- --features legacy-assistant --config src-tauri/tauri.legacy.conf.json
Remove-Item Env:VITE_PENGUIN_LEGACY
```

旧模式仍需要原 README 中的 LLVM/Whisper 和模型运行时依赖，旧运行时准备步骤需显式执行。新模式不要求准备这些依赖。不要用 companion 前端配 legacy 后端，或反向混用。

实现借鉴大肥鱼 ds-local-pet 的桌面陪伴思路、动作类型和紧凑界面结构，使用 PenguinPal 形象新生成素材并独立编写交互、运动和联动代码。日志、恢复、资料导出及更新为 Companion 自身实现；没有整套复制其安装程序或全部功能。

参考：[Codex hooks 官方文档](https://learn.chatgpt.com/docs/hooks)、[ds-local-pet](https://github.com/QCYTSN/ds-local-pet)。
