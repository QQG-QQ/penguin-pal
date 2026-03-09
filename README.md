# PenguinPal

一个基于 Tauri + Vue + Rust 的 Windows 桌宠助手项目，目标是实现轻量化企鹅形象、语音交互和受控桌面动作执行。

## 技术栈

- 前端：Vue 3 + TypeScript + Vite
- 客户端容器：Tauri 2
- 后端：Rust
- 动画：Lottie

## 当前能力

- 透明置顶桌宠窗口
- 托盘图标与菜单（显示/隐藏、设置、退出）
- 聊天界面与消息历史
- 语音输入（Web Speech）与语音播报（系统 TTS）
- Provider 配置（Mock / OpenAI / Anthropic / OpenAI-compatible）
- 白名单动作网关、权限等级、人工确认与审计日志

## 快速开始

### 环境要求

- Node.js 20+
- Rust stable（建议 1.70+）
- Windows 10 1903+（目标平台）

### 安装依赖

```bash
npm install
```

### 开发运行

```bash
npm run tauri dev
```

如果 PowerShell 的执行策略拦截了 `npx`，可改用：

```bash
npx.cmd tauri dev
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
│  │  ├─ tray.rs              # 托盘逻辑
│  │  └─ window.rs            # 窗口行为
│  └─ tauri.conf.json         # Tauri 配置
├─ src/                       # Vue 前端
│  ├─ App.vue                 # 主界面
│  ├─ components/             # UI 组件
│  ├─ lib/assistant.ts        # 前端与后端桥接
│  └─ types/assistant.ts      # 类型定义
├─ public/animations/         # 桌宠动画资源
└─ package.json
```

## 安全边界

- AI API 调用从 Rust 后端发起，避免前端直接暴露密钥
- API Key 默认只保留在运行期内存中，不写入磁盘明文
- 桌面动作仅允许白名单指令，禁止任意命令执行
- 高风险动作必须人工确认，并写入审计日志
