import type {
  PetMode,
  PetReaction,
  PetReactionEvent,
  PetReactionSource
} from '../types/assistant'

export type PetReactionInput = PetReaction | PetMode

export interface PetReactionPresentation {
  mode: PetMode
  priority: number
  defaultDurationMs: number | null
}

const presentations: Record<PetReaction, PetReactionPresentation> = {
  idle: { mode: 'idle', priority: 0, defaultDurationMs: 0 },
  listening: { mode: 'listening', priority: 30, defaultDurationMs: null },
  thinking: { mode: 'thinking', priority: 20, defaultDurationMs: null },
  working: { mode: 'thinking', priority: 25, defaultDurationMs: null },
  speaking: { mode: 'speaking', priority: 30, defaultDurationMs: null },
  waitingApproval: { mode: 'guarded', priority: 80, defaultDurationMs: null },
  success: { mode: 'speaking', priority: 50, defaultDurationMs: 4_000 },
  error: { mode: 'guarded', priority: 90, defaultDurationMs: 6_000 },
  notification: { mode: 'guarded', priority: 60, defaultDurationMs: 5_000 }
}

const legacyModeReactions: Record<PetMode, PetReaction> = {
  idle: 'idle',
  listening: 'listening',
  thinking: 'thinking',
  speaking: 'speaking',
  guarded: 'notification'
}

const reactionNames = new Set<PetReaction>(Object.keys(presentations) as PetReaction[])
const reactionSources = new Set<PetReactionSource>([
  'user',
  'assistant',
  'agent',
  'voice',
  'control',
  'system',
  'plugin'
])

const terminalReactions = new Set<PetReaction>(['success', 'error', 'notification'])
const terminalTransitionSources = new Set<PetReactionSource>(['assistant', 'agent', 'control', 'system'])

export const PET_REACTION_EVENT_MAX_AGE_MS = 60_000
export const PET_REACTION_EVENT_MAX_FUTURE_SKEW_MS = 5_000

export const isPetReaction = (value: unknown): value is PetReaction =>
  typeof value === 'string' && reactionNames.has(value as PetReaction)

export const isPetReactionSource = (value: unknown): value is PetReactionSource =>
  typeof value === 'string' && reactionSources.has(value as PetReactionSource)

export const normalizePetReaction = (reaction: PetReactionInput): PetReaction =>
  reaction === 'guarded' ? 'notification' : reaction

export const reactionForLegacyMode = (mode: PetMode): PetReaction => legacyModeReactions[mode]

export const presentationForReaction = (
  reaction: PetReactionInput
): PetReactionPresentation => presentations[normalizePetReaction(reaction)]

export const modeForReaction = (reaction: PetReactionInput): PetMode =>
  presentationForReaction(reaction).mode

export const createPetReactionEvent = (
  reaction: PetReactionInput,
  source: PetReactionSource,
  options: { message?: string; durationMs?: number | null; now?: number } = {}
): PetReactionEvent => {
  const normalized = normalizePetReaction(reaction)
  const presentation = presentations[normalized]
  const defaultDuration = normalized === 'waitingApproval'
    ? null
    : presentation.defaultDurationMs ?? undefined
  const durationMs = options.durationMs === undefined
    ? defaultDuration
    : options.durationMs
  return {
    reaction: normalized,
    source,
    ...(options.message === undefined ? {} : { message: options.message }),
    createdAt: options.now ?? Date.now(),
    ...(durationMs === undefined ? {} : { durationMs })
  }
}

export const sanitizePetReactionEvent = (
  value: unknown,
  now = Date.now()
): PetReactionEvent | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null
  }

  const record = value as Record<string, unknown>
  if (
    !isPetReaction(record.reaction) ||
    !isPetReactionSource(record.source) ||
    typeof record.createdAt !== 'number' ||
    !Number.isFinite(record.createdAt) ||
    record.createdAt < now - PET_REACTION_EVENT_MAX_AGE_MS ||
    record.createdAt > now + PET_REACTION_EVENT_MAX_FUTURE_SKEW_MS
  ) {
    return null
  }

  if (
    record.message !== undefined &&
    typeof record.message !== 'string'
  ) {
    return null
  }

  let durationMs: number | null | undefined
  if (record.durationMs === null) {
    durationMs = null
  } else if (record.durationMs !== undefined) {
    if (typeof record.durationMs !== 'number' || !Number.isFinite(record.durationMs)) {
      return null
    }
    durationMs = Math.min(Math.max(record.durationMs, 500), 60_000)
  }

  const message = typeof record.message === 'string'
    ? record.message.trim().slice(0, 500)
    : undefined

  return {
    reaction: record.reaction,
    source: record.source,
    ...(message ? { message } : {}),
    createdAt: record.createdAt,
    ...(durationMs === undefined ? {} : { durationMs })
  }
}

export const shouldReplacePetReaction = (
  current: PetReaction | null,
  next: PetReactionInput
): boolean => {
  if (current === null) {
    return true
  }

  const normalized = normalizePetReaction(next)
  if (normalized === 'idle') {
    return true
  }

  if (
    (current === 'waitingApproval' || current === 'working' || current === 'thinking') &&
    (normalized === 'success' || normalized === 'error' || normalized === 'notification')
  ) {
    return true
  }

  return presentations[normalized].priority >= presentations[current].priority
}

export const shouldReplacePetReactionEvent = (
  current: PetReaction | null,
  event: PetReactionEvent
): boolean => {
  if (
    current === 'waitingApproval' &&
    terminalReactions.has(event.reaction) &&
    terminalTransitionSources.has(event.source)
  ) {
    return true
  }

  return shouldReplacePetReaction(current, event.reaction)
}

export const reactionForAgentTaskStatus = (status: string | null | undefined): PetReaction => {
  switch (status) {
    case 'waitingConfirmation':
      return 'waitingApproval'
    case 'completed':
      return 'success'
    case 'failed':
      return 'error'
    case 'cancelled':
      return 'notification'
    case 'running':
      return 'working'
    default:
      return 'speaking'
  }
}
