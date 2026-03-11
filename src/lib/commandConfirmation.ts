import type { ModelCatalogEntry } from './modelCatalog'

export const COMMAND_CONFIRMATION_TIMEOUT_MS = 20_000

export type PendingCommandConfirmation =
  | {
      id: string
      kind: 'modelSet'
      title: string
      prompt: string
      createdAt: number
      expiresAt: number
      payload: { modelId: string; label: string }
    }
  | {
      id: string
      kind: 'clearConversation'
      title: string
      prompt: string
      createdAt: number
      expiresAt: number
      payload: Record<string, never>
    }

export type PendingCommandInputDecision = 'confirm' | 'cancel' | 'blocked'

const now = () => Date.now()

const buildPendingId = (prefix: string) => `${prefix}-${now()}`

const normalizeInput = (value: string) => value.trim().toLowerCase()

export const createModelSetConfirmation = (
  entry: ModelCatalogEntry
): PendingCommandConfirmation => ({
  id: buildPendingId('command-model-set'),
  kind: 'modelSet',
  title: '待确认：切换模型',
  prompt: `即将把当前对话引擎切换为 ${entry.label}。确认后会立即保存设置并切换。`,
  createdAt: now(),
  expiresAt: now() + COMMAND_CONFIRMATION_TIMEOUT_MS,
  payload: {
    modelId: entry.id,
    label: entry.label
  }
})

export const createClearConversationConfirmation = (): PendingCommandConfirmation => ({
  id: buildPendingId('command-clear'),
  kind: 'clearConversation',
  title: '待确认：清空当前对话',
  prompt: '即将清空当前会话消息，不会删除今日回复历史。确认后立即执行。',
  createdAt: now(),
  expiresAt: now() + COMMAND_CONFIRMATION_TIMEOUT_MS,
  payload: {}
})

export const isPendingCommandExpired = (
  pending: PendingCommandConfirmation,
  timestamp = now()
) => pending.expiresAt <= timestamp

export const resolvePendingCommandInput = (
  input: string
): PendingCommandInputDecision => {
  const normalized = normalizeInput(input)
  if (['yes', 'y', '确认'].includes(normalized)) {
    return 'confirm'
  }

  if (['no', 'n', '取消'].includes(normalized)) {
    return 'cancel'
  }

  return 'blocked'
}
