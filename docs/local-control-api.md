# 本地控制层接口说明

## 总览

- 传输：`HTTP`，默认监听 `127.0.0.1:48765..48775`
- 基础路由：
  - `GET /healthz`
  - `GET /v1/tools`
  - `GET /v1/pending`
  - `POST /v1/tools/invoke`
  - `POST /v1/pending/:id/confirm`
  - `POST /v1/pending/:id/cancel`
- 统一返回状态：
  - `success`
  - `pending_confirmation`
  - `error`

## 统一响应结构

### 成功

```json
{
  "status": "success",
  "result": {
    "title": "记事本"
  },
  "message": "聚焦窗口 已执行。"
}
```

### 错误

```json
{
  "status": "error",
  "message": "未找到匹配窗口。",
  "error": {
    "code": "window_not_found",
    "message": "未找到匹配窗口。",
    "detail": null,
    "retryable": false
  }
}
```

### 待确认

```json
{
  "status": "pending_confirmation",
  "message": "该控制动作需要先确认后执行。",
  "pendingRequest": {
    "id": "control-1711111111111-abcd1234",
    "tool": "click_at",
    "title": "待确认：点击坐标",
    "prompt": "即将对当前活动窗口执行坐标点击：x=120，y=240，button=left。",
    "preview": {
      "x": 120,
      "y": 240,
      "button": "left"
    },
    "args": {
      "x": 120,
      "y": 240,
      "button": "left"
    },
    "createdAt": 1711111111111,
    "expiresAt": 1711111141111,
    "minimumPermissionLevel": 2,
    "riskLevel": "writeHigh"
  }
}
```

## 工具清单

### L0 默认直通

#### `list_windows`

- `args`: `{}`
- `success.result` 示例：

```json
[
  {
    "handle": 123456,
    "title": "记事本",
    "isActive": true,
    "bounds": {
      "left": 100,
      "top": 80,
      "width": 960,
      "height": 700
    }
  }
]
```

#### `focus_window`

- `args`
  - `title: string` 必填
  - `match?: "contains" | "exact" | "prefix"`
- `success.result` 示例：

```json
{
  "handle": 123456,
  "title": "微信"
}
```

#### `open_app`

- `args`
  - `name: "notepad" | "calculator" | "explorer" | "paint" | "settings"`
- `success.result` 示例：

```json
{
  "app": "notepad",
  "message": "已启动 notepad。"
}
```

#### `read_clipboard`

- `args`: `{}`
- `success.result` 示例：

```json
{
  "text": "剪贴板里的文本"
}
```

#### `type_text`

- `args`
  - `text: string` 必填
- 限制：单行纯文本，最大 `500` 字符
- `success.result` 示例：

```json
{
  "typedLength": 12
}
```

#### `send_hotkey`

- `args`
  - `keys: string[]` 必填
- 限制：
  - 仅支持一个主键
  - 修饰键支持 `CTRL/ALT/SHIFT`
  - 主键支持单字符、方向键、`ENTER/TAB/ESC/DELETE/BACKSPACE`、`F1..F24`
- `success.result` 示例：

```json
{
  "sequence": "^v"
}
```

### L1 低风险写 / 敏感只读

#### `capture_active_window`

- `args`: `{}`
- `success.result` 示例：

```json
{
  "title": "记事本",
  "path": "C:\\Users\\...\\AppData\\Roaming\\com.penguinpal.app\\captures\\active-window-20260311-103011123.png",
  "width": 960,
  "height": 700
}
```

#### `scroll_at`

- `args`
  - `delta: integer` 必填，范围会被收敛到 `-1200..1200`
  - `steps?: integer` 默认 `1`，最大 `10`
  - `x?: integer`
  - `y?: integer`
- 说明：`x/y` 要么都不传，要么同时传；不传时默认活动窗口中心点
- `success.result` 示例：

```json
{
  "delta": -120,
  "steps": 3,
  "screenX": 900,
  "screenY": 540
}
```

#### `find_element`

- `args`
  - `selector.windowTitle: string` 必填
  - `selector.automationId?: string`
  - `selector.name?: string`
  - `selector.controlType?: string`
  - `selector.className?: string`
  - `selector.matchMode?: "contains" | "exact" | "prefix"`
- `success.result` 示例：

```json
{
  "name": "发送",
  "automationId": "SendButton",
  "controlType": "Button",
  "className": "Button",
  "windowTitle": "微信",
  "bounds": {
    "left": 1600,
    "top": 900,
    "width": 88,
    "height": 32
  }
}
```

#### `get_element_text`

- `args`: 同 `find_element`
- `success.result` 示例：

```json
{
  "text": "聊天输入框当前内容",
  "element": {
    "name": "消息输入",
    "automationId": "InputTextBox",
    "controlType": "Edit",
    "className": "RichEdit",
    "windowTitle": "微信",
    "bounds": {
      "left": 1200,
      "top": 820,
      "width": 460,
      "height": 120
    }
  }
}
```

#### `wait_for_element`

- `args`
  - `selector: object` 必填
  - `timeoutMs?: integer` 默认 `3000`，范围 `500..10000`
- `success.result` 示例：同 `find_element`

### L2 高风险写，需要确认

#### `click_at`

- `args`
  - `x: integer` 必填
  - `y: integer` 必填
  - `button?: "left" | "right" | "double"`
- 说明：坐标以活动窗口左上角为原点
- `success.result` 示例：

```json
{
  "screenX": 1510,
  "screenY": 930,
  "button": "left"
}
```

#### `click_element`

- `args`: 同 `find_element`
- 行为：优先 `InvokePattern`，失败后回退到元素中心点点击
- `success.result` 示例：同 `find_element`

#### `set_element_value`

- `args`
  - `selector: object` 必填
  - `text: string` 必填
- 限制：单行纯文本，最大 `500` 字符
- `success.result` 示例：

```json
{
  "usedFallback": false,
  "textLength": 8,
  "element": {
    "name": "搜索",
    "automationId": "SearchTextBox",
    "controlType": "Edit",
    "className": "Edit",
    "windowTitle": "微信",
    "bounds": {
      "left": 1200,
      "top": 140,
      "width": 260,
      "height": 36
    }
  }
}
```

## 常见错误码

- `validation_error`
- `permission_denied`
- `tool_not_found`
- `route_not_found`
- `window_not_found`
- `element_not_found`
- `timeout`
- `backend_exec_failed`
- `invalid_backend_response`
- `pending_not_found`
- `internal_error`

## Windows 实机回归清单

### 基础服务

1. `GET /healthz`
   - 预期：`running=true`，`baseUrl` 非空
2. `GET /v1/tools`
   - 预期：返回完整工具列表，名称与本文档一致
3. `GET /v1/pending`
   - 预期：初始为空数组

### 基础窗口与系统能力

1. `list_windows`
   - 预期：能看到当前打开的 `记事本/资源管理器/微信` 等窗口
2. `focus_window`
   - 预期：目标窗口切到前台
3. `open_app`
   - 预期：allowlist 应用能启动，非 allowlist 返回 `validation_error`
4. `capture_active_window`
   - 预期：返回截图路径，文件实际存在
5. `read_clipboard`
   - 预期：返回当前文本剪贴板
6. `type_text`
   - 预期：默认直接执行，不进入 `pending_confirmation`
7. `send_hotkey`
   - 预期：默认直接执行，不进入 `pending_confirmation`

### 高风险确认流

1. `click_at`
   - 首次调用预期：`pending_confirmation`
   - `POST /confirm` 后预期：指定窗口内相对坐标被点击
2. `click_element`
   - 首次调用预期：`pending_confirmation`
   - `POST /confirm` 后预期：元素被点击
3. `set_element_value`
   - 首次调用预期：`pending_confirmation`
   - `POST /confirm` 后预期：元素文本被写入

### 最小 UIA

1. `find_element`
   - 预期：能按 `windowTitle + name/controlType` 找到元素
2. `click_element`
   - 首次调用预期：`pending_confirmation`
   - `POST /confirm` 后预期：元素被点击
3. `get_element_text`
   - 预期：能返回 `Value/Text/Name`
4. `set_element_value`
   - 首次调用预期：`pending_confirmation`
   - `POST /confirm` 后预期：元素文本被写入
5. `wait_for_element`
   - 预期：元素出现前等待，出现后返回，超时则 `timeout` 或 `element_not_found`

### 兼容别名

1. `click_element_or_coords`
   - 传 `selector`：内部等价 `click_element`
   - 传 `x/y`：内部等价 `click_at`
2. `scroll_active_window`
   - 预期：内部等价 `scroll_at`
