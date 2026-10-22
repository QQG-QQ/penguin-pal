# PenguinPal

PenguinPal 基于 Tauri 2 + Vue 3 + Rust。默认以 **Codex 附属桌宠** 启动：点击企鹅打开现有 Codex 窗口，沿用那里的登录、聊天、语音和长期记忆；企鹅负责本地互动、散步、拖放物理和任务状态提示。

新模式的使用、联动安装、数据边界、验证命令和旧模式回退方法见 [Codex 附属桌宠指南](docs/codex-companion.md)。

下面保留独立助手模式的原有功能和开发文档；该模式需要显式开启 `legacy-assistant`，其中的私有 Codex、语音和电脑控制服务不在默认模式启动。

当前版本：`0.5.8`。接入豆包原片的被轻戳、摇尾巴、吃小鱼、休息、扫地、说话和小脾气七个动作。统一脚线和比例，循环首尾补间，并增加进入/退出片段；普通切换完成当前周期后回到公共姿势，拖拽和下落立即响应。视频及PNG逐帧回退均为24fps，不重新绘制角色。

## 技术栈

- 前端：Vue 3 + TypeScript + Vite
- 客户端容器：Tauri 2
- 后端：Rust
- 动画：Lottie + 企鹅 PNG 资源
- 语音识别：Web Speech / 本地 Whisper (`whisper-rs`)
- 音频采集：cpal
- 旧助手内部桌面工具：Windows API / UI Automation

## 当前功能

### 桌宠体验

- 透明、置顶、无边框桌宠窗口
- 企鹅多状态展示：待机、聆听、思考、说话、警戒
- 可拖动，并自动记住主窗口位置
- 靠屏幕边缘自动停靠，支持左侧、右侧和顶部停靠状态
- 托盘图标与菜单：显示/隐藏、设置、退出
- 独立窗口视图：设置页、气泡消息、投研简报

### 聊天与模型

- 聊天输入、消息历史、今日回复历史
- 输入历史上/下切换
- Slash command：

```text
/help
/windows list
/window focus <title>
/clipboard read
/pending list
/confirm
/cancel
/type <text>
/hotkey <keys>
/click <x> <y> [button]
/model
/model list
/model set <name>
/history
/clear
/settings
```

- Provider 配置：
  - Mock
  - Codex CLI
  - OpenAI
  - Anthropic
  - OpenAI-compatible
- Codex CLI 私有运行时与私有登录目录
- Codex CLI 登录状态检测、启动登录、重新登录
- Codex 自动更新与手动更新

### 语音能力

- Web Speech 语音输入
- 本地 Whisper 语音识别
- 连续监听与按键说话模式
- 默认按键说话快捷键：`CommandOrControl+Alt+Space`
- 系统 TTS 语音播报回复
- Whisper 模型下载、加载、卸载、删除

支持的 Whisper 模型：

| 模型 | 大小 | 说明 |
| --- | --- | --- |
| Tiny | 75MB | 速度最快，准确率较低 |
| Base | 142MB | 推荐，平衡速度和准确率 |
| Small | 466MB | 准确率更高 |
| Medium | 1.5GB | 高准确率 |
| Large | 2.9GB | 最高准确率 |

模型文件保存在：

```text
%APPDATA%/com.penguinpal.app/whisper-models/
```

### 旧助手内部桌面工具

仅在显式启用的旧助手中使用。对外 HTTP 控制服务和外部 Agent 反应租约已移除，不再监听本机控制端口。内部工具仍通过主窗口 IPC 和权限确认调用。

内部工具能力：

- 列出窗口、聚焦窗口
- 打开白名单应用：记事本、计算器、资源管理器、画图、系统设置
- 读取剪贴板文本
- 输入单行文本
- 发送快捷键
- 截图当前活动窗口
- 滚动活动窗口或指定坐标
- 通过 Windows UI Automation 查找元素
- 读取元素文本
- 等待元素出现
- 点击坐标或 UI 元素
- 设置 UI 元素文本

高风险写操作会进入待确认流，用户确认后才会执行。

### Shell Agent 权限

Shell Agent 权限和桌面控制权限分离。可以独立开关：

- 命令执行
- 文件修改
- 文件删除
- 网络访问
- 系统级操作
- 权限有效时长

### 长期记忆

- 长期记忆管理面板
- 稳定记忆、候选记忆、冲突记忆
- 记忆刷新与维护
- 记忆导出和导入
- 删除记忆
- 提升候选记忆
- 解决冲突记忆
- 历史归档搜索与回忆预览

### 本地投研模式

- 独立投研简报窗口
- 自选股票 / ETF / 基金统一研究
- 每日启动弹出投研简报
- 气泡提醒
- AI 自动分析与分段展示
- 自选资产涨跌图表
- 名称输入与代码输入的 best-effort 解析
- 研究主题、习惯笔记、决策框架配置
- 联动长期记忆记录研究习惯

### Agent 路线能力

设置页中已经提供这些 Agent 能力入口：

- 插件 / MCP 工具目录扫描
- 文档导入本地知识库
- 文档内容写入长期记忆
- 浏览器自动化任务草案队列
- 本地语音闭环配置：唤醒词、本地 TTS、离线 LLM fallback

其中浏览器自动化目前主要是任务入口和队列能力，完整执行器仍属于后续扩展方向。

### 更新与健康检查

- 软件更新检查
- 打开适合当前平台的 GitHub Release 安装包
- 启动时自动检查软件更新开关
- 启动时自动更新 Codex 开关
- 健康检查：
  - Codex CLI
  - Whisper 模型
  - 麦克风
  - 插件 / MCP 工具层
  - 文档记忆
  - 浏览器自动化入口
  - 本地语音闭环
  - 长期记忆

软件更新发布仓库：

```text
https://github.com/QQG-QQ/penguin-pal/releases
```

## 快速开始

### 环境要求

- Node.js 20+
- Rust stable，建议 1.78+
- Windows 10 1903+，当前目标平台
- Visual Studio Build Tools，包含 Desktop C++ 与 Windows SDK
- LLVM/Clang + CMake，供 `whisper-rs` 使用，首次构建时可自动安装

### 安装依赖

```bash
npm install
```

### 开发运行

```bash
npm run tauri:dev
```

首次运行会自动准备项目私有 Codex 运行时，并自动检测/安装 LLVM 与 CMake 到 `src-tauri/` 下。MSVC 编译器和 Windows SDK 仍需通过 Visual Studio Build Tools 预先安装。

如果 PowerShell 执行策略拦截了脚本，可改用：

```bash
npx.cmd tauri dev
```

### 前端开发

```bash
npm run dev
```

该命令只启动 Vite 前端。没有 Tauri 后端时，部分能力会进入浏览器调试 fallback。

### 构建前端

```bash
npm run build
```

### 仅编译后端

```bash
cd src-tauri

# 首次需要安装 LLVM 和 CMake
powershell -ExecutionPolicy Bypass -File setup-llvm.ps1
powershell -ExecutionPolicy Bypass -File setup-cmake.ps1

# 编译
cargo build
```

### 打包构建

```bash
npm run tauri:build
```

如果 Windows `release` 打包阶段出现这类错误：

- `failed to mmap rmeta metadata`
- `found invalid metadata files`
- `页面文件太小，无法完成操作`

可先清理 release 构建产物后重试：

```bash
cd src-tauri
cargo clean --release

cd ..
npm run tauri:build
```

当前打包脚本在 Windows 发布模式下会自动改成单并发 Cargo，以降低 `rmeta` 损坏和分页文件压力导致的编译失败概率。如果仍然报相同错误，需要增大 Windows 虚拟内存，也就是页面文件。

## 目录结构

```text
penguin-pal/
├─ src/                       # Vue 前端
│  ├─ App.vue                 # 主界面与交互编排
│  ├─ components/             # UI 组件
│  ├─ lib/                    # 前端桥接、命令解析、模型目录、桌宠布局
│  └─ types/                  # TypeScript 类型
├─ src-tauri/                 # Rust + Tauri 后端
│  ├─ src/
│  │  ├─ agent/               # Agent 路由、规划、执行、测试循环
│  │  ├─ ai/                  # Provider、guardrails、AI 记忆
│  │  ├─ audio/               # Whisper、录音、TTS、模型管理
│  │  ├─ codex_config/        # Codex 私有配置
│  │  ├─ control/             # 本地控制服务与 Windows 控制适配
│  │  ├─ memory/              # 长期记忆存储、检索、维护
│  │  ├─ permission/          # 权限授予与检查
│  │  ├─ rule_engine/         # 规则生成与解析
│  │  ├─ security/            # 审计、OAuth、安全策略
│  │  ├─ shell_agent/         # Shell Agent 权限与执行器
│  │  ├─ lib.rs               # Tauri 命令入口与主流程
│  │  ├─ main.rs              # 程序入口
│  │  ├─ tray.rs              # 托盘逻辑
│  │  └─ window.rs            # 窗口行为
│  ├─ capabilities/           # Tauri 权限能力配置
│  ├─ resources/              # 内置资源，例如 Codex 运行时
│  ├─ setup-llvm.ps1          # LLVM 安装脚本
│  ├─ setup-cmake.ps1         # CMake 安装脚本
│  └─ tauri.conf.json         # Tauri 配置
├─ docs/                      # 设计与接口文档
├─ penguin/                   # 企鹅图像资源
├─ public/animations/         # Lottie 动画
├─ scripts/                   # 构建、运行时准备和发布脚本
└─ package.json
```

## 安全边界

- AI API 调用由 Rust 后端发起，避免前端直接暴露密钥
- API Key 默认只保留在运行期内存中，不写入磁盘明文
- 桌面动作必须通过白名单动作网关
- 高风险动作必须人工确认，并写入审计日志
- 本地控制层按权限等级区分只读、低风险写、高风险写
- 已移除旧助手对外 HTTP 控制接口及外部 Agent 反应租约
- 旧助手 Doctor 诊断覆盖 MCP 和应用数据目录，并提供脱敏摘要
- Tauri WebView 启用显式 CSP，默认禁止外部脚本、框架和对象内容
- Shell Agent 权限独立控制，默认不授予任意命令能力
- Codex CLI 即使使用内置私有运行时，也不能绕过 PenguinPal 的动作网关

## 构建依赖说明

`whisper-rs` 需要 LLVM/Clang、CMake、MSVC C++ 工具链和 Windows SDK。项目已配置本地自动安装 LLVM/CMake：

- `npm run tauri:dev`：自动检测并安装 LLVM、CMake，并准备开发用 Codex 私有运行时
- `cargo build`：需先运行 `setup-llvm.ps1` 和 `setup-cmake.ps1`

安装位置已加入 `.gitignore`：

- LLVM：`src-tauri/.llvm/`，约 400MB
- CMake：`src-tauri/.cmake/`，约 50MB
- 开发私有 Codex 运行时：`src-tauri/.codex-runtime/`

## 相关文档

- [Windows 安装与启动](docs/windows-companion-install.md)
- [桌宠反应协议](docs/pet-reactions.md)
- [桌宠内置 Codex 运行时](docs/embedded-codex-runtime.md)
- [Windows 烟测基线](docs/windows-smoke-baseline.md)
- [角色形象再生成说明](docs/character-regeneration.md)
- [Codex 开发衔接说明](CODEX_GUIDE.md)
