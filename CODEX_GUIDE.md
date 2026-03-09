# Codex 开发衔接说明

本文档用于快速了解 `penguin-pal` 当前实现状态，便于继续开发。

## 当前状态

项目已经具备可运行的 MVP：

- Tauri 2 + Vue 3 + Rust 全栈桌面工程
- 透明无边框、置顶、托盘驻留窗口
- 桌宠动画展示（Lottie）
- 聊天面板、控制面板、设置抽屉
- 语音输入（按住说话）与语音播报
- Provider 配置（Mock/OpenAI/Anthropic/OpenAI-compatible）
- 白名单动作、权限等级、人工确认、审计日志

## 安全策略（必须保持）

- 默认最小权限（permission level 1）
- API Key 仅在运行期内存中使用，不落盘
- 禁止任意命令执行，仅允许白名单动作
- 风险动作必须人工确认
- 所有动作记录审计日志

## 建议开发顺序

1. 优先桌宠体验
- 小窗体、可拖动、可关闭/隐藏
- 角色形象与动画状态一致

2. 再完善语音链路
- 输入：稳定识别与状态反馈
- 输出：失败回退与中断控制

3. 最后增强 AI 与动作执行
- Provider 错误处理与降级策略
- 动作网关细粒度权限与审计字段

## 常用命令

```bash
# 安装依赖
npm install

# 开发运行
npm run tauri dev

# 如果 PowerShell 拦截 npx
npx.cmd tauri dev

# Rust 编译检查
cd src-tauri
cargo check
```

## 关键文件

- `src-tauri/tauri.conf.json`：窗口尺寸、透明、托盘配置
- `src-tauri/src/lib.rs`：Tauri 命令入口与状态管理
- `src-tauri/src/tray.rs`：托盘菜单和点击行为
- `src-tauri/src/window.rs`：窗口行为控制
- `src/App.vue`：前端主界面与交互编排
- `src/components/Penguin.vue`：桌宠形象与拖拽交互
- `src/lib/assistant.ts`：前端调用后端与浏览器回退逻辑
