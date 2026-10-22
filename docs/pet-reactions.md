# 桌宠反应协议

PenguinPal 将“业务状态”和“当前企鹅素材”分开。Agent、语音、桌面控制和未来插件发送反应，渲染层再把反应映射到现有五种视觉模式。

| 反应 | 当前视觉模式 | 典型来源 |
| --- | --- | --- |
| `idle` | `idle` | 空闲、任务结束后复位 |
| `listening` | `listening` | 麦克风正在采集 |
| `thinking` | `thinking` | 模型推理、语音转写 |
| `working` | `thinking` | Agent 或桌面工具正在执行 |
| `speaking` | `speaking` | 普通聊天回复 |
| `waitingApproval` | `guarded` | 等待用户确认高风险动作 |
| `success` | `speaking` | Agent 或工具成功完成 |
| `error` | `guarded` | 失败、拒绝或异常 |
| `notification` | `guarded` | 普通提醒和取消状态 |

前端协议类型位于 `src/types/assistant.ts`，映射、优先级、默认展示时间和兼容逻辑位于 `src/lib/petReactions.ts`。

Tauri 窗口间事件名为：

```text
penguinpal://pet-reaction
```

事件结构：

```json
{
  "reaction": "working",
  "source": "agent",
  "message": "正在检查工作区",
  "createdAt": 1786400000000,
  "durationMs": 4000
}
```

渲染层会拒绝未知反应、未知来源、格式错误以及超过 60 秒的过期事件，将消息限制到 500 个字符，并把临时展示时间收敛到 500 毫秒至 60 秒。只有主桌宠窗口消费反应事件，并忽略晚于已接收事件到达的旧事件。高优先级反应不会被低优先级临时状态覆盖，例如等待确认不会被普通 `thinking` 覆盖；同一流程的 `success`、`error` 和 `notification` 终态可以结束 `thinking`、`working` 或 `waitingApproval`。

`waitingApproval` 默认固定展示，待确认期间自动监听和手动语音输入都不会覆盖该状态。显式 `durationMs` 会优先于按气泡文本长度计算的展示时间。

后续 MCP 或插件接入时应发送反应事件，不应直接操作企鹅图片名称或 Vue 组件状态。

此协议仅用于旧助手窗口间的本地事件。对外 HTTP 接口和外部 Agent 反应租约已移除；当前 Codex 附属桌宠通过独立 hooks 事件桥显示任务状态。
