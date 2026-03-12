export type SlashCommand =
  | { kind: 'help' }
  | { kind: 'modelCurrent' }
  | { kind: 'modelList' }
  | { kind: 'modelSet'; target: string }
  | { kind: 'history' }
  | { kind: 'clearConversation' }
  | { kind: 'openSettings' }
  | { kind: 'windowsList' }
  | { kind: 'windowFocus'; title: string }
  | { kind: 'clipboardRead' }
  | { kind: 'controlPendingList' }
  | { kind: 'controlConfirm' }
  | { kind: 'controlCancel' }
  | { kind: 'controlType'; text: string }
  | { kind: 'controlHotkey'; keys: string[] }
  | { kind: 'controlClick'; x: number; y: number; button: 'left' | 'right' | 'double' }

export type SlashCommandParseResult =
  | { ok: true; command: SlashCommand }
  | { ok: false; message: string }

const tokenize = (input: string) => input.trim().split(/\s+/).filter(Boolean)

const parseHotkeyTokens = (tokens: string[]) =>
  tokens
    .flatMap((token) => token.split(/[+,]/))
    .map((token) => token.trim())
    .filter(Boolean)

const parseInteger = (value: string) => {
  const parsed = Number.parseInt(value, 10)
  return Number.isFinite(parsed) ? parsed : null
}

export const parseSlashCommand = (input: string): SlashCommandParseResult | null => {
  const trimmed = input.trim()
  if (!trimmed.startsWith('/')) {
    return null
  }

  const parts = tokenize(trimmed)
  const head = parts[0]?.slice(1).toLowerCase()

  switch (head) {
    case 'help':
      return { ok: true, command: { kind: 'help' } }
    case 'history':
      return { ok: true, command: { kind: 'history' } }
    case 'clear':
      return { ok: true, command: { kind: 'clearConversation' } }
    case 'settings':
      return { ok: true, command: { kind: 'openSettings' } }
    case 'confirm':
      return parts.length === 1
        ? { ok: true, command: { kind: 'controlConfirm' } }
        : { ok: false, message: '/confirm 不接受额外参数。' }
    case 'cancel':
      return parts.length === 1
        ? { ok: true, command: { kind: 'controlCancel' } }
        : { ok: false, message: '/cancel 不接受额外参数。' }
    case 'windows': {
      if (parts[1]?.toLowerCase() === 'list' && parts.length === 2) {
        return { ok: true, command: { kind: 'windowsList' } }
      }

      return {
        ok: false,
        message: '可用的窗口命令只有 /windows list。'
      }
    }
    case 'window': {
      if (parts[1]?.toLowerCase() !== 'focus') {
        return {
          ok: false,
          message: '可用的窗口命令只有 /window focus <title>。'
        }
      }

      const title = parts.slice(2).join(' ').trim()
      if (!title) {
        return {
          ok: false,
          message: '请在 /window focus 后面带上窗口标题，例如：/window focus 微信'
        }
      }

      return { ok: true, command: { kind: 'windowFocus', title } }
    }
    case 'clipboard': {
      if (parts[1]?.toLowerCase() === 'read' && parts.length === 2) {
        return { ok: true, command: { kind: 'clipboardRead' } }
      }

      return {
        ok: false,
        message: '可用的剪贴板命令只有 /clipboard read。'
      }
    }
    case 'pending': {
      if (parts.length === 1 || (parts[1]?.toLowerCase() === 'list' && parts.length === 2)) {
        return { ok: true, command: { kind: 'controlPendingList' } }
      }

      return {
        ok: false,
        message: '可用的待确认命令只有 /pending 或 /pending list。'
      }
    }
    case 'type': {
      const text = trimmed.slice('/type'.length).trim()
      if (!text) {
        return {
          ok: false,
          message: '请在 /type 后面带上要输入的文本，例如：/type hello'
        }
      }

      return { ok: true, command: { kind: 'controlType', text } }
    }
    case 'hotkey': {
      const keys = parseHotkeyTokens(parts.slice(1))
      if (keys.length === 0) {
        return {
          ok: false,
          message: '请在 /hotkey 后面带上按键，例如：/hotkey ctrl+v'
        }
      }

      return { ok: true, command: { kind: 'controlHotkey', keys } }
    }
    case 'click': {
      if (parts.length < 3) {
        return {
          ok: false,
          message: '请使用 /click <x> <y>，例如：/click 120 240'
        }
      }

      const x = parseInteger(parts[1] ?? '')
      const y = parseInteger(parts[2] ?? '')
      if (x === null || y === null) {
        return {
          ok: false,
          message: '/click 的坐标必须是整数，例如：/click 120 240'
        }
      }

      const button = parts[3]?.toLowerCase()
      if (button && !['left', 'right', 'double'].includes(button)) {
        return {
          ok: false,
          message: '/click 的第三个参数只能是 left、right 或 double。'
        }
      }

      return {
        ok: true,
        command: {
          kind: 'controlClick',
          x,
          y,
          button: (button as 'left' | 'right' | 'double' | undefined) ?? 'left'
        }
      }
    }
    case 'model': {
      if (parts.length === 1) {
        return { ok: true, command: { kind: 'modelCurrent' } }
      }

      const subcommand = parts[1]?.toLowerCase()
      if (subcommand === 'list' && parts.length === 2) {
        return { ok: true, command: { kind: 'modelList' } }
      }

      if (subcommand === 'set') {
        const target = parts.slice(2).join(' ').trim()
        if (!target) {
          return {
            ok: false,
            message: '请在 /model set 后面带上目标模型名称，例如：/model set codex-cli'
          }
        }

        return { ok: true, command: { kind: 'modelSet', target } }
      }

      return {
        ok: false,
        message: '可用的模型命令只有 /model、/model list、/model set <name>。'
      }
    }
    default:
      return {
        ok: false,
        message: '未知命令。输入 /help 查看当前支持的 slash command。'
      }
  }
}

export const slashHelpText = `可用命令：
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

说明：
- /model set 和 /clear 会先进入确认状态
- /type、/hotkey 默认直接执行；/click 会走本地控制层的高风险确认
- 有待确认动作时，优先输入 yes / no，或使用 /confirm /cancel`
