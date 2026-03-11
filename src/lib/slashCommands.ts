export type SlashCommand =
  | { kind: 'help' }
  | { kind: 'modelCurrent' }
  | { kind: 'modelList' }
  | { kind: 'modelSet'; target: string }
  | { kind: 'history' }
  | { kind: 'clearConversation' }
  | { kind: 'openSettings' }

export type SlashCommandParseResult =
  | { ok: true; command: SlashCommand }
  | { ok: false; message: string }

const tokenize = (input: string) => input.trim().split(/\s+/).filter(Boolean)

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
/model
/model list
/model set <name>
/history
/clear
/settings

说明：
- /model set 和 /clear 会先进入确认状态
- 确认时可点按钮，或输入 yes / no`
