# Windows 真机烟测基线

日期：`2026-03-11`

## 基线结论

控制层第一版已经在 Windows 真机上完成一轮最小烟测，当前可作为后续回归基线。

当前默认授权策略已更新：

- `list_windows`
- `focus_window`
- `open_app`
- `read_clipboard`
- `type_text`
- `send_hotkey`

以上工具默认直通，不再要求在设置页提升权限，也不会再进入一次性确认。

仍然保留确认的高风险动作：

- `click_at`
- `click_element`
- `set_element_value`

已确认通过：

1. `pending / confirm / cancel` 机制
2. `focus_window`
3. `click_element`
4. `get_element_text`
5. `set_element_value` 的 `confirm`
6. `set_element_value` 的 `cancel`
7. `send_hotkey`
8. `click_at`

## 已通过的工具链

### 1. 高风险控制确认链

已验证通过：

- 高风险动作返回 `pending_confirmation`
- `GET /v1/pending` 能看到对应 `id`
- `POST /v1/pending/:id/confirm` 成功
- `POST /v1/pending/:id/cancel` 成功
- 取消后 pending 会从列表消失

### 2. 窗口链

已验证通过：

- `list_windows`
- `focus_window exact`
- `focus_window contains`
- 中文标题窗口匹配

已知有效示例：

- `Program Manager`
- `无标题 - Notepad`
- `微信`

### 3. UI Automation 读写链

已验证通过：

- `find_element`
- `wait_for_element`
- `get_element_text`
- `set_element_value`

Notepad 示例结果：

- 菜单项 `文件`
  - `automationId=File`
  - `controlType=MenuItem`
- `get_element_text`
  - 返回 `text=文件`

### 4. 点击链

已验证通过：

- `click_element`
- `click_at`

说明：

- `click_element` 对 `MenuItem` 可用
- 对 `Notepad -> 文件` 菜单项，当前已验证成功
- 该类控件不一定提供标准 UIA pattern
- 当前在真机上可能走兜底策略：`SetFocus+CenterClick`

### 5. 键盘输入链

已验证通过：

- `type_text`
- `send_hotkey`

示例：

- `send_hotkey CTRL+V`
  - 基线验证时为 `confirm -> success`
  - `result.sequence="^v"`

## Notepad 推荐 selector

### 菜单项“文件”

```json
{
  "windowTitle": "Notepad",
  "automationId": "File",
  "controlType": "MenuItem",
  "matchMode": "contains"
}
```

如果窗口标题更具体，也可以直接用：

```json
{
  "windowTitle": "无标题 - Notepad",
  "automationId": "File",
  "controlType": "MenuItem",
  "matchMode": "exact"
}
```

### 编辑区

实测更稳定的 selector 不是 `Edit`，而是：

```json
{
  "windowTitle": "Notepad",
  "controlType": "Document",
  "className": "RichEditD2DPT",
  "matchMode": "contains"
}
```

结论：

- `controlType=Document + className=RichEditD2DPT`
- 比单纯使用 `Edit` 更稳定

## PowerShell 推荐调用方式

不要手写 JSON 字符串。推荐统一使用“对象 + `ConvertTo-Json` + `application/json; charset=utf-8`”：

```powershell
$base = "http://127.0.0.1:48765"

function Invoke-Control($tool, $args) {
  $body = @{
    tool = $tool
    args = $args
  } | ConvertTo-Json -Depth 8

  Invoke-RestMethod -Method Post "$base/v1/tools/invoke" `
    -ContentType "application/json; charset=utf-8" `
    -Body $body
}

function Confirm-Control($id) {
  Invoke-RestMethod -Method Post "$base/v1/pending/$id/confirm"
}

function Cancel-Control($id) {
  Invoke-RestMethod -Method Post "$base/v1/pending/$id/cancel"
}
```

## 当前基线含义

这轮烟测说明：

1. 控制层第一版已经具备最小可用性
2. `pending / confirm / cancel` 已从机制上跑通
3. UIA 读写与点击在 Notepad 场景下已可用
4. `click_element` 对部分控件仍可能依赖兜底点击策略，但当前行为已可接受

## 后续回归建议

后续每次改动控制层，至少回归这几条：

1. `focus_window`
2. `find_element`
3. `get_element_text`
4. `set_element_value confirm`
5. `set_element_value cancel`
6. `send_hotkey`
7. `click_at`
8. `click_element`
9. `GET /v1/pending`
10. `confirm / cancel`
