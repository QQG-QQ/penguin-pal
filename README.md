# PenguinPal

一个基于 Tauri + Vue + Rust 的 Windows 桌宠助手项目，目标是实现轻量化企鹅形象、语音交互和受控桌面动作执行。

## 技术栈

- 前端：Vue 3 + TypeScript + Vite
- 客户端容器：Tauri 2
- 后端：Rust
- 动画：Lottie
- 语音识别：本地 Whisper (whisper-rs)
- 音频采集：cpal

## 当前能力

- 透明置顶桌宠窗口
- 托盘图标与菜单（显示/隐藏、设置、退出）
- 聊天界面与消息历史
- 语音输入（Web Speech / 本地 Whisper）与语音播报（系统 TTS）
- Provider 配置（Mock / Codex CLI / OpenAI / Anthropic / OpenAI-compatible）
- 本地 Whisper 语音识别（支持模型下载/加载/管理）
- Shell Agent 权限控制
- 白名单动作网关、权限等级、人工确认与审计日志

## 快速开始

### 环境要求

- Node.js 20+
- Rust stable（建议 1.78+）
- Windows 10 1903+（目标平台）
- LLVM/Clang（whisper-rs 依赖，首次构建时自动安装）

### 安装依赖

```bash
npm install
```

### 开发运行

```bash
npm run tauri dev
```

首次运行会自动下载并安装 LLVM 到 `src-tauri/.llvm/`（约 400MB），请耐心等待。

如果 PowerShell 的执行策略拦截了 `npx`，可改用：

```bash
npx.cmd tauri dev
```

### 仅编译后端

```bash
cd src-tauri

# 首次需要安装 LLVM
powershell -ExecutionPolicy Bypass -File setup-llvm.ps1

# 编译
cargo build
```

### 打包构建

```bash
npm run tauri build
```

## 目录结构

```text
penguin-pal/
├─ src-tauri/                 # Rust + Tauri
│  ├─ src/
│  │  ├─ main.rs              # 程序入口
│  │  ├─ lib.rs               # Tauri 命令与主流程
│  │  ├─ audio/               # 音频模块
│  │  │  ├─ recorder.rs       # 麦克风采集 (cpal)
│  │  │  ├─ whisper.rs        # Whisper 推理引擎
│  │  │  ├─ transcriber.rs    # 转写服务
│  │  │  └─ model_manager.rs  # 模型下载管理
│  │  ├─ tray.rs              # 托盘逻辑
│  │  └─ window.rs            # 窗口行为
│  ├─ .llvm/                  # LLVM 本地安装 (自动下载，已 gitignore)
│  ├─ setup-llvm.ps1          # LLVM 安装脚本
│  └─ tauri.conf.json         # Tauri 配置
├─ src/                       # Vue 前端
│  ├─ App.vue                 # 主界面
│  ├─ components/             # UI 组件
│  ├─ lib/assistant.ts        # 前端与后端桥接
│  └─ types/assistant.ts      # 类型定义
├─ scripts/
│  ├─ ensure-llvm.mjs         # LLVM 自动检测安装
│  └─ ensure-codex-runtime.mjs
├─ public/animations/         # 桌宠动画资源
└─ package.json
```

## Whisper 语音识别

项目内置本地 Whisper 语音识别，无需外网即可使用。

### 支持的模型

| 模型 | 大小 | 说明 |
|------|------|------|
| Tiny | 75MB | 速度最快，准确率较低 |
| Base | 142MB | 推荐，平衡速度和准确率 |
| Small | 466MB | 准确率更高 |
| Medium | 1.5GB | 高准确率 |
| Large | 2.9GB | 最高准确率 |

### 使用方式

1. 打开设置面板
2. 在"本地 Whisper 语音识别"区域下载模型
3. 加载模型后即可使用本地语音识别

模型文件保存在 `%APPDATA%/com.penguinpal.app/whisper-models/`。

## 安全边界

- AI API 调用从 Rust 后端发起，避免前端直接暴露密钥
- API Key 默认只保留在运行期内存中，不写入磁盘明文
- 桌面动作仅允许白名单指令，禁止任意命令执行
- 高风险动作必须人工确认，并写入审计日志
- Shell Agent 权限独立控制，支持细粒度权限管理

## 构建依赖

whisper-rs 需要 LLVM/Clang 编译。项目已配置自动安装：

- `npm run tauri dev`：自动检测并安装 LLVM
- `cargo build`：需先运行 `setup-llvm.ps1`

LLVM 安装位置：`src-tauri/.llvm/`（约 1.2GB，已加入 .gitignore）
