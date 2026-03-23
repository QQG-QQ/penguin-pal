import { invoke } from '@tauri-apps/api/core'
import { emit, emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ActionApprovalRequest,
  AppUpdateStatus,
  AssistantWindowView,
  AiConstraintProfile,
  ActionExecutionResult,
  AssistantSnapshot,
  BubbleLayoutMetrics,
  BubbleWindowState,
  ChatMessage,
  ChatResponse,
  CodexCliStatus,
  CodexUpdateStatus,
  ControlPendingRequest,
  ControlServiceStatus,
  ControlToolInvokeResponse,
  DownloadProgress,
  ManagedMemoryKind,
  MemoryManagementSnapshot,
  ModelInfo,
  OAuthFlowResult,
  OAuthState,
  ProviderConfigInput,
  ProviderKind,
  RecordingState,
  ResearchBriefSnapshot,
  ReplyHistoryEntry,
  TranscriptionResult,
  VisionChannelConfig,
  VisionProviderStatus,
  VoiceInputMode,
  WhisperModel,
  WhisperPushToTalkEvent,
  WhisperStatus
} from '../types/assistant'

const providerModels: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  codexCli: 'gpt-5-codex',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

const DEFAULT_PUSH_TO_TALK_SHORTCUT = 'CommandOrControl+Alt+Space'
const defaultResearchConfig = () => ({
  enabled: false,
  startupPopup: true,
  bubbleAlerts: true,
  watchlist: [] as string[],
  funds: [] as string[],
  themes: ['地缘政治', '财报', '基金风格'],
  habitNotes: '',
  decisionFramework:
    '先看结论和证据，再看反证、风险、失效条件、跟踪指标，最后才决定是否继续研究。'
})

const defaultOAuthState = (): OAuthState => ({
  status: 'signedOut',
  authorizeUrl: null,
  tokenUrl: null,
  clientId: null,
  redirectUrl: 'http://127.0.0.1:8976/oauth/callback',
  scopes: [],
  accountHint: null,
  pendingAuthUrl: null,
  accessTokenLoaded: false,
  lastError: null,
  startedAt: null,
  expiresAt: null
})

const defaultConstraintsProfile = (): AiConstraintProfile => ({
  label: 'Codex Guardrails',
  version: '2026-03-10',
  summary: '这套约束由后端强制执行，角色设定只能补充风格，不能覆盖安全边界。',
  immutableRules: [
    {
      id: 'no-freeform-exec',
      title: '禁止自由执行',
      summary: 'AI 不能直接执行 shell、脚本、下载、安装、浏览器自动化或任意软件控制。',
      status: '硬限制'
    },
    {
      id: 'whitelist-only-actions',
      title: '只允许白名单动作',
      summary: '任何电脑控制都必须走后端白名单，高风险动作还要经过一次性确认。',
      status: '硬限制'
    },
    {
      id: 'privacy-first',
      title: '禁止隐私外泄',
      summary: 'AI 不能请求、上传、整理或暴露密钥、令牌、密码、私人文件和聊天隐私。',
      status: '硬限制'
    }
  ],
  capabilityGates: [
    {
      id: 'chat',
      title: '对话陪伴',
      summary: '允许正常对话、提醒、解释风险和引导用户使用受控入口。',
      status: '可用'
    },
    {
      id: 'model-gateway',
      title: '模型网关访问',
      summary: '当前仅在浏览器调试 fallback 下不连接外部 AI 网关。',
      status: '已阻止'
    },
    {
      id: 'desktop-actions',
      title: '桌面动作申请',
      summary: '仅允许白名单动作，而且高风险动作仍然需要人工确认。',
      status: '需审批'
    }
  ],
  runtimeBoundaries: [
    {
      id: 'permission-level',
      title: '权限等级',
      summary: '当前浏览器调试 fallback 默认处于 L2。',
      status: 'L2'
    },
    {
      id: 'auth-mode',
      title: '认证门禁',
      summary: '浏览器调试 fallback 不持有 API Key 或 OAuth 令牌。',
      status: 'fallback'
    }
  ]
})

const defaultVisionChannel = (): VisionChannelConfig => ({
  enabled: false,
  kind: 'disabled',
  model: 'gpt-4.1-mini',
  baseUrl: null,
  allowNetwork: true,
  apiKeyLoaded: false,
  timeoutMs: 12000,
  maxImageBytes: 3 * 1024 * 1024,
  maxImageWidth: 1600,
  maxImageHeight: 1200,
  lastError: null
})

const defaultShellPermissions = () => ({
  enabled: false,
  allowExecute: false,
  allowFileModify: false,
  allowFileDelete: false,
  allowNetwork: false,
  allowSystem: false,
  durationHours: 1
})

const fallbackVisionStatus = (
  visionChannel: VisionChannelConfig,
  apiKey?: string | null
): VisionProviderStatus => {
  if (
    !visionChannel.enabled ||
    visionChannel.kind === 'disabled'
  ) {
    return {
      kind: 'unsupported',
      message: '视觉副通道未启用。'
    }
  }

  if (!visionChannel.allowNetwork) {
    return {
      kind: 'disabledOffline',
      message: '当前处于离线安全模式，已阻止视觉分析。'
    }
  }

  if (visionChannel.lastError?.trim()) {
    return {
      kind: visionChannel.lastError.includes('超时') ? 'timeout' : 'analysisFailed',
      message: visionChannel.lastError
    }
  }

  if (visionChannel.kind === 'openAi') {
    if (!apiKey?.trim()) {
      return {
        kind: 'unsupported',
        message: '视觉副通道缺少 OpenAI API Key。'
      }
    }

    return {
      kind: 'supported',
      message: '视觉副通道已启用 OpenAI 图像分析。'
    }
  }

  return {
    kind: 'unknown',
    message: 'OpenAI-Compatible 视觉副通道将按最佳努力尝试图像分析。'
  }
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
const now = () => Date.now()

const fallbackMessage = (role: ChatMessage['role'], content: string): ChatMessage => ({
  id: `${role}-${now()}`,
  role,
  content,
  createdAt: now()
})

const buildFallbackSnapshot = (): AssistantSnapshot => ({
  mode: 'idle',
  messages: [
    fallbackMessage(
      'assistant',
      '当前是浏览器调试 fallback。正式能力需要 Tauri 后端可用。'
    )
  ],
  provider: {
    kind: 'mock',
    model: providerModels.mock,
    baseUrl: null,
    systemPrompt:
      '你是一只管理员企鹅桌宠。普通聊天时直接回答，只有涉及权限、隐私或电脑控制时再简短说明限制。',
    allowNetwork: false,
    voiceReply: true,
    retainHistory: true,
    voiceInputMode: 'continuous',
    pushToTalkShortcut: DEFAULT_PUSH_TO_TALK_SHORTCUT,
    apiKeyLoaded: false,
    authMode: 'apiKey',
    oauth: defaultOAuthState()
  },
  launchAtStartup: false,
  autoUpdateCodex: true,
  autoCheckAppUpdate: true,
  research: defaultResearchConfig(),
  workspaceRoot: null,
  visionChannel: defaultVisionChannel(),
  visionChannelStatus: fallbackVisionStatus(defaultVisionChannel()),
  permissionLevel: 2,
  allowedActions: [
    {
      id: 'show_window',
      title: '显示主面板',
      summary: '重新显示桌宠控制面板。',
      riskLevel: 0,
      minimumLevel: 0,
      requiresConfirmation: false,
      enabled: true
    },
    {
      id: 'hide_window',
      title: '收起主面板',
      summary: '保留托盘驻留，仅隐藏主窗口。',
      riskLevel: 0,
      minimumLevel: 0,
      requiresConfirmation: false,
      enabled: true
    },
    {
      id: 'open_notepad',
      title: '打开记事本',
      summary: '示例级白名单动作，需要更严格的一次性确认。',
      riskLevel: 2,
      minimumLevel: 2,
      requiresConfirmation: true,
      enabled: true
    },
    {
      id: 'open_calculator',
      title: '打开计算器',
      summary: '示例级白名单动作，需要更严格的一次性确认。',
      riskLevel: 2,
      minimumLevel: 2,
      requiresConfirmation: true,
      enabled: true
    }
  ],
  auditTrail: [
    {
      id: `audit-${now()}`,
      action: 'ui_bootstrap',
      outcome: 'fallback',
      detail: '当前运行在浏览器/无 Tauri 后端的调试回退模式。',
      createdAt: now(),
      riskLevel: 0
    }
  ],
  audioProfile: {
    inputMode: 'auto-listen',
    outputMode: 'speech-synthesis',
    stages: [
      {
        id: 'recorder',
        title: '自动语音监听',
        summary: '检测到麦克风后，前端会优先使用 Web Speech 自动进入监听。',
        status: 'ready'
      },
      {
        id: 'transcribe',
        title: '语音转写',
        summary: '识别完成后自动回填到对话框。',
        status: 'ready'
      },
      {
        id: 'tts',
        title: '语音播报',
        summary: '助手回复可使用系统语音播报。',
        status: 'ready'
      }
    ]
  },
  aiConstraints: defaultConstraintsProfile(),
  shellPermissions: defaultShellPermissions()
})

let fallbackSnapshot = buildFallbackSnapshot()
let fallbackPendingApproval: ActionApprovalRequest | null = null
let fallbackOAuthStateValue = 'demo-oauth-state'
const FALLBACK_INPUT_HISTORY_KEY = 'penguinpal-input-history'
const FALLBACK_TODAY_REPLY_HISTORY_KEY = 'penguinpal-today-reply-history'
const fallbackCodexStatus = (): CodexCliStatus => ({
  installed: false,
  version: null,
  loggedIn: false,
  credentialPresent: false,
  authPath: null,
  runtimePath: null,
  source: '未找到',
  statusKind: 'unavailable',
  statusLabel: '未检测',
  reloginRecommended: false,
  message: '浏览器调试模式下无法检测本机 Codex CLI。'
})

const localDateKey = (value = new Date()) =>
  `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(
    value.getDate()
  ).padStart(2, '0')}`

const readFallbackStorage = <T>(key: string, fallback: T): T => {
  if (typeof window === 'undefined') {
    return fallback
  }

  try {
    const raw = window.localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch {
    return fallback
  }
}

const writeFallbackStorage = <T>(key: string, value: T) => {
  if (typeof window === 'undefined') {
    return
  }

  try {
    window.localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // ignore fallback persistence failures
  }
}

const readFallbackInputHistory = () =>
  readFallbackStorage<string[]>(FALLBACK_INPUT_HISTORY_KEY, [])

type FallbackReplyHistoryFile = {
  date: string
  entries: ReplyHistoryEntry[]
}

const readFallbackTodayReplyHistoryFile = (): FallbackReplyHistoryFile => {
  const fallback = {
    date: localDateKey(),
    entries: [] as ReplyHistoryEntry[]
  }
  const stored = readFallbackStorage<FallbackReplyHistoryFile>(
    FALLBACK_TODAY_REPLY_HISTORY_KEY,
    fallback
  )

  if (stored.date !== localDateKey()) {
    return fallback
  }

  return stored
}

const writeFallbackTodayReplyHistory = (entries: ReplyHistoryEntry[]) => {
  writeFallbackStorage(FALLBACK_TODAY_REPLY_HISTORY_KEY, {
    date: localDateKey(),
    entries
  })
}

const isTauriRuntime = () =>
  typeof window !== 'undefined' && typeof window.__TAURI_INTERNALS__ !== 'undefined'

const normalizeRuntimeError = (error: unknown): Error => {
  if (error instanceof Error) {
    return error
  }

  if (typeof error === 'string' && error.trim()) {
    return new Error(error)
  }

  if (typeof error === 'number' || typeof error === 'boolean' || typeof error === 'bigint') {
    return new Error(String(error))
  }

  if (error && typeof error === 'object') {
    const record = error as Record<string, unknown>
    const message = [record.message, record.error, record.cause].find(
      (value) => typeof value === 'string' && value.trim()
    )
    if (typeof message === 'string') {
      return new Error(message)
    }

    try {
      const serialized = JSON.stringify(record)
      if (serialized && serialized !== '{}' && serialized !== 'null') {
        return new Error(serialized)
      }
    } catch {
      // ignore JSON serialization errors and fall back to default message
    }

    const text = String(error)
    if (text && text !== '[object Object]') {
      return new Error(text)
    }
  }

  return new Error('Tauri backend call failed')
}

const rethrowIfDesktopRuntime = (error: unknown): void => {
  if (isTauriRuntime()) {
    throw normalizeRuntimeError(error)
  }
}

export type SettingsSection = 'settings' | 'actions'

const MAIN_WINDOW_LABEL = 'main'
const SETTINGS_WINDOW_LABEL = 'settings'
const BUBBLE_WINDOW_LABEL = 'bubble'
const SNAPSHOT_UPDATED_EVENT = 'penguinpal://assistant-snapshot'
const SETTINGS_SECTION_EVENT = 'penguinpal://settings-section'
const BUBBLE_STATE_EVENT = 'penguinpal://bubble-state'
const BUBBLE_INTERACTION_EVENT = 'penguinpal://bubble-interaction'
const BUBBLE_LAYOUT_METRICS_EVENT = 'penguinpal://bubble-layout-metrics'
const BUBBLE_DISMISS_EVENT = 'penguinpal://bubble-dismiss'
const TODAY_REPLY_HISTORY_EVENT = 'penguinpal://today-reply-history'
const WHISPER_STATUS_EVENT = 'penguinpal://whisper-status'
const WHISPER_PUSH_TO_TALK_EVENT = 'penguinpal://whisper-push-to-talk'

let browserSettingsWindow: Window | null = null
let browserResearchWindow: Window | null = null
let cachedControlBaseUrl: string | null = null

const normalizeSettingsSection = (value: string | null | undefined): SettingsSection =>
  value === 'actions' ? 'actions' : 'settings'

const normalizeWindowView = (value: string | null | undefined): AssistantWindowView => {
  if (value === 'settings' || value === 'bubble' || value === 'research') {
    return value
  }

  return 'pet'
}

const settingsWindowUrl = (section: SettingsSection) =>
  `/?view=settings&section=${section}`

const researchWindowUrl = () => '/?view=research'

export const readWindowView = (): AssistantWindowView => {
  if (typeof window === 'undefined') {
    return 'pet'
  }

  return normalizeWindowView(new URL(window.location.href).searchParams.get('view'))
}

export const isSettingsWindowView = (): boolean => {
  return readWindowView() === 'settings'
}

export const isBubbleWindowView = (): boolean => {
  return readWindowView() === 'bubble'
}

export const isResearchWindowView = (): boolean => {
  return readWindowView() === 'research'
}

export const readRequestedSettingsSection = (): SettingsSection => {
  if (typeof window === 'undefined') {
    return 'settings'
  }

  return normalizeSettingsSection(new URL(window.location.href).searchParams.get('section'))
}

export const publishAssistantSnapshot = async (snapshot: AssistantSnapshot): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emit(SNAPSHOT_UPDATED_EVENT, snapshot)
}

export const listenForAssistantSnapshot = async (
  handler: (snapshot: AssistantSnapshot) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<AssistantSnapshot>(SNAPSHOT_UPDATED_EVENT, (event) => {
    handler(event.payload)
  })
}

export const publishBubbleWindowState = async (state: BubbleWindowState): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emitTo(BUBBLE_WINDOW_LABEL, BUBBLE_STATE_EVENT, state)
}

export const listenForBubbleWindowState = async (
  handler: (state: BubbleWindowState) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<BubbleWindowState>(BUBBLE_STATE_EVENT, (event) => {
    handler(event.payload)
  })
}

export const publishBubbleInteractionState = async (active: boolean): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emitTo(MAIN_WINDOW_LABEL, BUBBLE_INTERACTION_EVENT, active)
}

export const listenForBubbleInteractionState = async (
  handler: (active: boolean) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<boolean>(BUBBLE_INTERACTION_EVENT, (event) => {
    handler(Boolean(event.payload))
  })
}

export const publishBubbleLayoutMetrics = async (metrics: BubbleLayoutMetrics): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emitTo(MAIN_WINDOW_LABEL, BUBBLE_LAYOUT_METRICS_EVENT, metrics)
}

export const listenForBubbleLayoutMetrics = async (
  handler: (metrics: BubbleLayoutMetrics) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<BubbleLayoutMetrics>(BUBBLE_LAYOUT_METRICS_EVENT, (event) => {
    handler(event.payload)
  })
}

export const requestBubbleDismiss = async (messageId: number): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emitTo(MAIN_WINDOW_LABEL, BUBBLE_DISMISS_EVENT, messageId)
}

export const listenForBubbleDismissRequest = async (
  handler: (messageId: number) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<number>(BUBBLE_DISMISS_EVENT, (event) => {
    handler(Number(event.payload ?? 0))
  })
}

export const publishTodayReplyHistory = async (entries: ReplyHistoryEntry[]): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emit(TODAY_REPLY_HISTORY_EVENT, entries)
}

export const publishWhisperStatus = async (status: WhisperStatus): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emit(WHISPER_STATUS_EVENT, status)
}

export const listenForTodayReplyHistory = async (
  handler: (entries: ReplyHistoryEntry[]) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<ReplyHistoryEntry[]>(TODAY_REPLY_HISTORY_EVENT, (event) => {
    handler(event.payload)
  })
}

export const listenForWhisperStatus = async (
  handler: (status: WhisperStatus) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<WhisperStatus>(WHISPER_STATUS_EVENT, (event) => {
    handler(event.payload)
  })
}

export const listenForWhisperPushToTalk = async (
  handler: (event: WhisperPushToTalkEvent) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<WhisperPushToTalkEvent>(WHISPER_PUSH_TO_TALK_EVENT, (event) => {
    handler(event.payload)
  })
}

export const listenForSettingsSectionChange = async (
  handler: (section: SettingsSection) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<{ section?: string }>(SETTINGS_SECTION_EVENT, (event) => {
    handler(normalizeSettingsSection(event.payload?.section))
  })
}

export const openSettingsWindow = async (section: SettingsSection = 'settings'): Promise<boolean> => {
  const url = settingsWindowUrl(section)

  if (!isTauriRuntime()) {
    if (typeof window === 'undefined') {
      return false
    }

    browserSettingsWindow = window.open(url, 'PenguinPalSettings', 'width=860,height=760')
    browserSettingsWindow?.focus()
    return browserSettingsWindow !== null
  }

  const opened = await safeInvoke<boolean>('show_settings_window')
  if (!opened) {
    return false
  }

  await emitTo(SETTINGS_WINDOW_LABEL, SETTINGS_SECTION_EVENT, { section })
  return true
}

export const closeSettingsWindow = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    if (browserSettingsWindow && !browserSettingsWindow.closed) {
      browserSettingsWindow.close()
      browserSettingsWindow = null
      return true
    }

    return false
  }

  return safeInvoke<boolean>('hide_settings_window')
}

export const openResearchWindow = async (): Promise<boolean> => {
  const url = researchWindowUrl()

  if (!isTauriRuntime()) {
    if (typeof window === 'undefined') {
      return false
    }

    browserResearchWindow = window.open(url, 'PenguinPalResearch', 'width=760,height=820')
    browserResearchWindow?.focus()
    return browserResearchWindow !== null
  }

  return safeInvoke<boolean>('show_research_window')
}

export const closeResearchWindow = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    if (browserResearchWindow && !browserResearchWindow.closed) {
      browserResearchWindow.close()
      browserResearchWindow = null
      return true
    }

    return false
  }

  return safeInvoke<boolean>('hide_research_window')
}

const safeInvoke = async <T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> => {
  if (!isTauriRuntime()) {
    throw new Error('Tauri backend unavailable')
  }

  return invoke<T>(command, args)
}

const fallbackControlServiceStatus = (): ControlServiceStatus => ({
  running: false,
  baseUrl: null,
  toolCount: 0,
  message: '当前不是桌宠运行时，本地控制层不可用。'
})

const emptyMemoryManagementSnapshot = (): MemoryManagementSnapshot => ({
  stats: {
    profileCount: 0,
    episodicCount: 0,
    proceduralCount: 0,
    policyCount: 0,
    semanticCount: 0,
    metaCount: 0,
    stableCount: 0,
    candidateCount: 0,
    conflictCount: 0
  },
  stableRecords: [],
  candidateRecords: [],
  conflicts: []
})

const normalizeControlResponseMessage = (payload: unknown) => {
  if (!payload || typeof payload !== 'object') {
    return '本地控制服务调用失败。'
  }

  const record = payload as Record<string, unknown>
  const errorRecord =
    record.error && typeof record.error === 'object'
      ? (record.error as Record<string, unknown>)
      : null

  const candidates = [
    errorRecord?.message,
    record.message,
    errorRecord?.detail
  ]

  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate
    }
  }

  return '本地控制服务调用失败。'
}

const getControlBaseUrl = async () => {
  if (cachedControlBaseUrl) {
    return cachedControlBaseUrl
  }

  const status = await getControlServiceStatus()
  if (!status.running || !status.baseUrl) {
    throw new Error(status.message || '本地控制服务未启动。')
  }

  cachedControlBaseUrl = status.baseUrl
  return cachedControlBaseUrl
}

const controlFetchJson = async <T>(path: string, init?: RequestInit): Promise<T> => {
  if (typeof window === 'undefined' || typeof window.fetch !== 'function') {
    throw new Error('当前环境不支持本地控制请求。')
  }

  const baseUrl = await getControlBaseUrl()
  let response: Response
  try {
    response = await window.fetch(`${baseUrl}${path}`, {
      ...init,
      headers: {
        'Content-Type': 'application/json; charset=utf-8',
        ...(init?.headers ?? {})
      }
    })
  } catch (error) {
    cachedControlBaseUrl = null
    throw normalizeRuntimeError(error)
  }

  const raw = await response.text()
  const payload = raw ? (JSON.parse(raw) as unknown) : null
  if (!response.ok) {
    throw new Error(normalizeControlResponseMessage(payload))
  }

  if (
    payload &&
    typeof payload === 'object' &&
    'status' in payload &&
    (payload as Record<string, unknown>).status === 'error'
  ) {
    throw new Error(normalizeControlResponseMessage(payload))
  }

  return payload as T
}

const snapshotWithRuntimeFlags = (snapshot: AssistantSnapshot): AssistantSnapshot => {
  const visionChannel = {
    ...defaultVisionChannel(),
    ...(snapshot.visionChannel ?? {})
  }
  const provider = {
    ...snapshot.provider,
    voiceInputMode: (snapshot.provider.voiceInputMode ?? 'continuous') as VoiceInputMode,
    pushToTalkShortcut:
      snapshot.provider.pushToTalkShortcut?.trim() || DEFAULT_PUSH_TO_TALK_SHORTCUT
  }

  return {
    ...snapshot,
    launchAtStartup: Boolean(snapshot.launchAtStartup),
    autoUpdateCodex: snapshot.autoUpdateCodex !== false,
    autoCheckAppUpdate: snapshot.autoCheckAppUpdate !== false,
    research: {
      ...defaultResearchConfig(),
      ...(snapshot.research ?? {})
    },
    workspaceRoot: snapshot.workspaceRoot?.trim() || null,
    provider: {
      ...provider,
      apiKeyLoaded: Boolean(provider.apiKeyLoaded),
      oauth: {
        ...provider.oauth,
        scopes: [...provider.oauth.scopes],
        accessTokenLoaded: Boolean(provider.oauth.accessTokenLoaded)
      }
    },
    visionChannel: {
      ...visionChannel,
      apiKeyLoaded: Boolean(visionChannel.apiKeyLoaded)
    },
    visionChannelStatus:
      snapshot.visionChannelStatus ??
      fallbackVisionStatus(visionChannel, visionChannel.apiKeyLoaded ? 'loaded' : '')
  }
}

const nextMockReply = (content: string) => {
  if (content.includes('什么模型') || content.includes('你是谁') || content.includes('怎么运行')) {
    return '我现在以 PenguinPal 桌宠助手身份运行。当前如果还是 Mock，说明还没切到真实模型；切到 Codex CLI 或其他 Provider 后，我会按对应模型回复。'
  }

  if (content.includes('安全') || content.includes('权限')) {
    return '当前是严格白名单模式。AI 只能建议动作，真正的电脑控制必须走一次性授权票据并经过人工逐项确认。'
  }

  if (content.includes('OAuth') || content.includes('登录')) {
    return '现在的设置里已经有 OAuth 准备流。它默认采用 PKCE 授权码思路，但前提是你的上游模型网关真的支持 OAuth bearer token。'
  }

  if (content.includes('记事本') || content.includes('计算器') || content.includes('控制电脑')) {
    return '桌面控制入口已经准备好了，但仍然只允许白名单动作。高风险动作会弹出逐项确认清单和确认短语输入。'
  }

  if (content.includes('语音')) {
    return '检测到麦克风后会自动进入语音监听，文字输入仍然随时可用。回复也会默认进行系统语音播报，并同步显示头顶气泡。'
  }

  return 'UI、安全壳、OAuth 准备流和更严格的动作确认协议已经就位。下一步可以继续接入真实模型网关和 Windows 真机验证。'
}

const createFallbackApproval = (actionId: string): ActionApprovalRequest => {
  const action = fallbackSnapshot.allowedActions.find((item) => item.id === actionId)
  if (!action) {
    throw new Error('未找到动作定义')
  }

  return {
    id: `approval-${now()}`,
    action,
    prompt: `你即将执行“${action.title}”。这次授权只对本次动作生效，不会开放后续自由控制。`,
    requiredPhrase: `确认执行 ${action.title}`,
    checks: [
      {
        id: 'one_time',
        label: '我确认这是一次性授权，不会放开自由控制电脑的权限'
      },
      {
        id: 'visible_effect',
        label: '我知道这个动作会直接影响当前 Windows 软件或窗口状态'
      },
      {
        id: 'privacy_boundary',
        label: '我确认本次动作不应读取、上传或暴露我的隐私数据'
      }
    ],
    createdAt: now(),
    expiresAt: now() + 2 * 60 * 1000
  }
}

export const getAssistantSnapshot = async (): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('get_assistant_snapshot')
    return snapshotWithRuntimeFlags(snapshot)
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return clone(fallbackSnapshot)
  }
}

export const saveProviderConfig = async (
  input: ProviderConfigInput
): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('save_provider_config', { input })
    return snapshotWithRuntimeFlags(snapshot)
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    const oauth = {
      ...fallbackSnapshot.provider.oauth,
      authorizeUrl: input.oauthAuthorizeUrl,
      tokenUrl: input.oauthTokenUrl,
      clientId: input.oauthClientId,
      redirectUrl: input.oauthRedirectUrl || fallbackSnapshot.provider.oauth.redirectUrl,
      scopes: input.oauthScopes
        .split(/[\s,]+/)
        .map((value) => value.trim())
        .filter(Boolean)
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      launchAtStartup: input.launchAtStartup,
      autoUpdateCodex: input.autoUpdateCodex,
      autoCheckAppUpdate: input.autoCheckAppUpdate,
      research: {
        ...defaultResearchConfig(),
        ...(input.research ?? {})
      },
      provider: {
        ...fallbackSnapshot.provider,
        kind: input.kind,
        model: input.model || providerModels[input.kind],
        baseUrl: input.baseUrl,
        systemPrompt: input.systemPrompt,
        allowNetwork: input.allowNetwork,
        voiceReply: input.voiceReply,
        retainHistory: input.retainHistory,
        voiceInputMode: input.voiceInputMode,
        pushToTalkShortcut: input.pushToTalkShortcut?.trim() || DEFAULT_PUSH_TO_TALK_SHORTCUT,
        apiKeyLoaded: Boolean(input.apiKey && input.apiKey.trim()),
        authMode: input.authMode,
        oauth: {
          ...oauth,
          accessTokenLoaded: input.clearOAuthToken ? false : oauth.accessTokenLoaded,
          status: input.clearOAuthToken ? 'signedOut' : oauth.status,
          pendingAuthUrl: input.clearOAuthToken ? null : oauth.pendingAuthUrl,
          accountHint: input.clearOAuthToken ? null : oauth.accountHint,
          lastError: input.clearOAuthToken ? null : oauth.lastError,
          startedAt: input.clearOAuthToken ? null : oauth.startedAt,
          expiresAt: input.clearOAuthToken ? null : oauth.expiresAt
        }
      },
      workspaceRoot: input.workspaceRoot?.trim() || null,
      visionChannel: {
        ...fallbackSnapshot.visionChannel,
        enabled: input.visionChannel.enabled,
        kind: input.visionChannel.kind,
        model: input.visionChannel.model || defaultVisionChannel().model,
        baseUrl: input.visionChannel.baseUrl,
        allowNetwork: input.visionChannel.allowNetwork,
        apiKeyLoaded: Boolean(input.visionChannel.apiKey?.trim()),
        timeoutMs: input.visionChannel.timeoutMs,
        maxImageBytes: input.visionChannel.maxImageBytes,
        maxImageWidth: input.visionChannel.maxImageWidth,
        maxImageHeight: input.visionChannel.maxImageHeight,
        lastError: null
      },
      visionChannelStatus: fallbackVisionStatus(
        {
          ...fallbackSnapshot.visionChannel,
          enabled: input.visionChannel.enabled,
          kind: input.visionChannel.kind,
          model: input.visionChannel.model || defaultVisionChannel().model,
          baseUrl: input.visionChannel.baseUrl,
          allowNetwork: input.visionChannel.allowNetwork,
          apiKeyLoaded: Boolean(input.visionChannel.apiKey?.trim()),
          timeoutMs: input.visionChannel.timeoutMs,
          maxImageBytes: input.visionChannel.maxImageBytes,
          maxImageWidth: input.visionChannel.maxImageWidth,
          maxImageHeight: input.visionChannel.maxImageHeight,
          lastError: null
        },
        input.visionChannel.apiKey
      ),
      permissionLevel: input.permissionLevel,
      allowedActions: fallbackSnapshot.allowedActions.map((action) => ({
        ...action,
        enabled: input.permissionLevel >= action.minimumLevel
      }))
    }
    return clone(fallbackSnapshot)
  }
}

export const startOAuthSignIn = async (): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('start_oauth_sign_in')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    if (fallbackSnapshot.provider.authMode !== 'oauth') {
      throw new Error('请先在设置中把认证方式切换到 OAuth。')
    }

    const oauth = fallbackSnapshot.provider.oauth
    if (!oauth.authorizeUrl || !oauth.clientId || !oauth.redirectUrl) {
      throw new Error('OAuth 配置不完整：至少需要 Client ID、Authorize URL 和 Redirect URL。')
    }

    fallbackOAuthStateValue = `demo-state-${now()}`
    const url = new URL(oauth.authorizeUrl)
    url.searchParams.set('response_type', 'code')
    url.searchParams.set('client_id', oauth.clientId)
    url.searchParams.set('redirect_uri', oauth.redirectUrl)
    url.searchParams.set('state', fallbackOAuthStateValue)
    url.searchParams.set('code_challenge_method', 'S256')
    url.searchParams.set('code_challenge', 'demo-code-challenge')
    if (oauth.scopes.length > 0) {
      url.searchParams.set('scope', oauth.scopes.join(' '))
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...oauth,
          status: 'pending',
          pendingAuthUrl: url.toString(),
          startedAt: now(),
          expiresAt: now() + 5 * 60 * 1000,
          lastError: null
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_login_started',
          outcome: 'demo',
          detail: '浏览器演示模式仅生成 OAuth 授权链接，不会真正访问远端登录。',
          createdAt: now(),
          riskLevel: 1
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: '已生成 OAuth 授权链接。登录完成后，把浏览器回调地址粘贴回来。',
      authorizationUrl: fallbackSnapshot.provider.oauth.pendingAuthUrl,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const startOAuthSignInAuto = async (): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('start_oauth_sign_in_auto')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    const started = await startOAuthSignIn()
    if (!started.snapshot.provider.oauth.pendingAuthUrl) {
      return started
    }

    fallbackSnapshot = {
      ...started.snapshot,
      provider: {
        ...started.snapshot.provider,
        oauth: {
          ...started.snapshot.provider.oauth,
          status: 'authorized',
          pendingAuthUrl: null,
          accessTokenLoaded: true,
          accountHint: 'demo-oauth-user',
          lastError: null,
          startedAt: null,
          expiresAt: now() + 60 * 60 * 1000
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_login_completed',
          outcome: 'demo',
          detail: '浏览器演示模式已自动完成 OAuth 登录。',
          createdAt: now(),
          riskLevel: 1
        },
        ...started.snapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: 'OAuth 演示自动登录成功。当前仅把访问令牌状态保留在运行内存中。',
      authorizationUrl: started.authorizationUrl,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const completeOAuthSignIn = async (callbackUrl: string): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('complete_oauth_sign_in', { callbackUrl })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    if (!callbackUrl.trim()) {
      throw new Error('请先粘贴浏览器回调地址。')
    }

    const url = new URL(callbackUrl.trim())
    const returnedState = url.searchParams.get('state')
    const code = url.searchParams.get('code')

    if (returnedState !== fallbackOAuthStateValue) {
      throw new Error('OAuth 状态校验失败，请重新生成授权链接。')
    }

    if (!code) {
      throw new Error('回调地址中没有 code，无法完成登录。')
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...fallbackSnapshot.provider.oauth,
          status: 'authorized',
          pendingAuthUrl: null,
          accessTokenLoaded: true,
          accountHint: 'demo-oauth-user',
          lastError: null,
          startedAt: null,
          expiresAt: now() + 60 * 60 * 1000
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_login_completed',
          outcome: 'demo',
          detail: '浏览器演示模式已在内存中标记 OAuth 登录成功。',
          createdAt: now(),
          riskLevel: 1
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: 'OAuth 演示登录成功。当前仅把访问令牌状态保留在运行内存中。',
      authorizationUrl: null,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const disconnectOAuthSignIn = async (): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('disconnect_oauth_sign_in')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...fallbackSnapshot.provider.oauth,
          status: 'signedOut',
          pendingAuthUrl: null,
          accessTokenLoaded: false,
          accountHint: null,
          lastError: null,
          startedAt: null,
          expiresAt: null
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_logout',
          outcome: 'demo',
          detail: '浏览器演示模式已清空 OAuth 登录状态。',
          createdAt: now(),
          riskLevel: 0
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: '已退出 OAuth 登录，并清空内存中的令牌状态。',
      authorizationUrl: null,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const sendChatMessage = async (content: string): Promise<ChatResponse> => {
  try {
    return await safeInvoke<ChatResponse>('send_chat_message', { content })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    const userMessage = fallbackMessage('user', content)
    const replyMessage = fallbackMessage('assistant', nextMockReply(content))
    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      messages: [...fallbackSnapshot.messages, userMessage, replyMessage],
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'chat_completion',
          outcome: 'mock',
          detail: '当前为本地 UI 演示回复。',
          createdAt: now(),
          riskLevel: 0
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }
    const trimmed = content.trim()
    if (trimmed) {
      const nextInputHistory = readFallbackInputHistory()
      if (nextInputHistory[nextInputHistory.length - 1] !== trimmed) {
        writeFallbackStorage(FALLBACK_INPUT_HISTORY_KEY, [...nextInputHistory, trimmed].slice(-50))
      }

      const nextReplyHistory = readFallbackTodayReplyHistoryFile().entries
      writeFallbackTodayReplyHistory([
        ...nextReplyHistory,
        {
          id: `reply-${now()}`,
          timestamp: now(),
          userInput: trimmed,
          assistantReply: replyMessage.content
        }
      ])
    }
    return {
      reply: replyMessage,
      providerLabel: 'Mock Assistant',
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const requestDesktopAction = async (
  actionId: string
): Promise<ActionExecutionResult> => {
  try {
    return await safeInvoke<ActionExecutionResult>('request_desktop_action', {
      actionId
    })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    const selectedAction = fallbackSnapshot.allowedActions.find((action) => action.id === actionId)

    if (!selectedAction) {
      throw new Error('未找到动作定义')
    }

    if (!selectedAction.enabled) {
      throw new Error('当前权限级别不允许执行该动作')
    }

    if (selectedAction.requiresConfirmation) {
      const approvalRequest = createFallbackApproval(actionId)
      fallbackPendingApproval = approvalRequest
      fallbackSnapshot = {
        ...fallbackSnapshot,
        auditTrail: [
          {
            id: `audit-${now()}`,
            action: 'action_approval_requested',
            outcome: 'demo',
            detail: `${selectedAction.title} 已进入一次性授权确认阶段。`,
            createdAt: now(),
            riskLevel: selectedAction.riskLevel
          },
          ...fallbackSnapshot.auditTrail
        ].slice(0, 8)
      }

      return {
        status: 'needs_confirmation',
        message: `${selectedAction.title} 需要逐项确认后才能执行。`,
        snapshot: clone(fallbackSnapshot),
        approvalRequest
      }
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: actionId,
          outcome: 'demo',
          detail: '浏览器演示模式未真正调用系统能力。',
          createdAt: now(),
          riskLevel: selectedAction.riskLevel
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      status: 'demo',
      message: `${selectedAction.title} 已通过演示模式记录审计，但未真正执行系统操作。`,
      snapshot: clone(fallbackSnapshot),
      approvalRequest: null
    }
  }
}

export const confirmDesktopAction = async (
  approvalId: string,
  typedPhrase: string,
  acknowledgedChecks: string[]
): Promise<ActionExecutionResult> => {
  try {
    return await safeInvoke<ActionExecutionResult>('confirm_desktop_action', {
      approvalId,
      typedPhrase,
      acknowledgedChecks
    })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    if (!fallbackPendingApproval || fallbackPendingApproval.id !== approvalId) {
      throw new Error('未找到待确认的动作授权。')
    }

    if (fallbackPendingApproval.expiresAt < now()) {
      fallbackPendingApproval = null
      throw new Error('这次动作授权已经过期，请重新发起。')
    }

    if (typedPhrase.trim() !== fallbackPendingApproval.requiredPhrase) {
      throw new Error(`请完整输入确认短语：${fallbackPendingApproval.requiredPhrase}`)
    }

    const acknowledged = new Set(acknowledgedChecks)
    const missing = fallbackPendingApproval.checks.find((check) => !acknowledged.has(check.id))
    if (missing) {
      throw new Error('请先完成所有确认项。')
    }

    const action = fallbackPendingApproval.action
    fallbackPendingApproval = null
    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: action.id,
          outcome: 'demo',
          detail: '演示模式已通过更严格的确认流记录本次动作，但未真正执行系统操作。',
          createdAt: now(),
          riskLevel: action.riskLevel
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      status: 'demo',
      message: `${action.title} 已通过演示模式完成更严格的确认流。`,
      snapshot: clone(fallbackSnapshot),
      approvalRequest: null
    }
  }
}

export const cancelDesktopActionApproval = async (
  approvalId: string
): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('cancel_desktop_action_approval', {
      approvalId
    })
    return snapshotWithRuntimeFlags(snapshot)
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    if (fallbackPendingApproval?.id === approvalId) {
      fallbackSnapshot = {
        ...fallbackSnapshot,
        auditTrail: [
          {
            id: `audit-${now()}`,
            action: 'action_approval_cancelled',
            outcome: 'demo',
            detail: `${fallbackPendingApproval.action.title} 的一次性授权已被取消。`,
            createdAt: now(),
            riskLevel: fallbackPendingApproval.action.riskLevel
          },
          ...fallbackSnapshot.auditTrail
        ].slice(0, 8)
      }
      fallbackPendingApproval = null
    }

    return clone(fallbackSnapshot)
  }
}

export const clearConversation = async (): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('clear_conversation')
    return snapshotWithRuntimeFlags(snapshot)
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    fallbackSnapshot = buildFallbackSnapshot()
    fallbackPendingApproval = null
    return clone(fallbackSnapshot)
  }
}

export const getInputHistory = async (): Promise<string[]> => {
  try {
    return await safeInvoke<string[]>('get_input_history')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return readFallbackInputHistory()
  }
}

export const getTodayReplyHistory = async (): Promise<ReplyHistoryEntry[]> => {
  try {
    return await safeInvoke<ReplyHistoryEntry[]>('get_today_reply_history')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return readFallbackTodayReplyHistoryFile().entries
  }
}

export const getMemoryManagementSnapshot = async (): Promise<MemoryManagementSnapshot> => {
  try {
    return await safeInvoke<MemoryManagementSnapshot>('get_memory_management_snapshot')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return emptyMemoryManagementSnapshot()
  }
}

export const deleteManagedMemory = async (
  kind: ManagedMemoryKind,
  id: string
): Promise<MemoryManagementSnapshot> => {
  try {
    return await safeInvoke<MemoryManagementSnapshot>('delete_managed_memory', { kind, id })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('删除记忆需要桌宠运行时')
  }
}

export const promoteMemoryCandidate = async (id: string): Promise<MemoryManagementSnapshot> => {
  try {
    return await safeInvoke<MemoryManagementSnapshot>('promote_memory_candidate', { id })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('提升候选记忆需要桌宠运行时')
  }
}

export const resolveMemoryConflict = async (
  kind: ManagedMemoryKind,
  group: string,
  keepId: string
): Promise<MemoryManagementSnapshot> => {
  try {
    return await safeInvoke<MemoryManagementSnapshot>('resolve_memory_conflict', {
      kind,
      group,
      keepId
    })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('处理记忆冲突需要桌宠运行时')
  }
}

export const clearTodayReplyHistory = async (): Promise<ReplyHistoryEntry[]> => {
  try {
    return await safeInvoke<ReplyHistoryEntry[]>('clear_today_reply_history')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    writeFallbackTodayReplyHistory([])
    return []
  }
}

export const getCodexCliStatus = async (): Promise<CodexCliStatus> => {
  try {
    return await safeInvoke<CodexCliStatus>('get_codex_cli_status')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return fallbackCodexStatus()
  }
}

export const getControlServiceStatus = async (): Promise<ControlServiceStatus> => {
  try {
    const status = await safeInvoke<ControlServiceStatus>('get_control_service_status')
    cachedControlBaseUrl = status.running ? status.baseUrl : null
    return status
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    cachedControlBaseUrl = null
    return fallbackControlServiceStatus()
  }
}

export const invokeControlTool = async (
  tool: string,
  args: Record<string, unknown>
): Promise<ControlToolInvokeResponse> => {
  if (!isTauriRuntime()) {
    throw new Error(fallbackControlServiceStatus().message)
  }

  return controlFetchJson<ControlToolInvokeResponse>('/v1/tools/invoke', {
    method: 'POST',
    body: JSON.stringify({ tool, args })
  })
}

export const listControlPending = async (): Promise<ControlPendingRequest[]> => {
  if (!isTauriRuntime()) {
    return []
  }

  return controlFetchJson<ControlPendingRequest[]>('/v1/pending')
}

export const confirmControlPending = async (
  pendingId: string
): Promise<ControlToolInvokeResponse> => {
  try {
    return await safeInvoke<ControlToolInvokeResponse>('confirm_control_pending', { pendingId })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error(fallbackControlServiceStatus().message)
  }
}

export const cancelControlPending = async (
  pendingId: string
): Promise<ControlToolInvokeResponse> => {
  try {
    return await safeInvoke<ControlToolInvokeResponse>('cancel_control_pending', { pendingId })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error(fallbackControlServiceStatus().message)
  }
}

export const startCodexCliLogin = async (): Promise<CodexCliStatus> => {
  try {
    return await safeInvoke<CodexCliStatus>('start_codex_cli_login')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      ...fallbackCodexStatus(),
      message: '浏览器调试模式无法启动 codex login。'
    }
  }
}

export const restartCodexCliLogin = async (): Promise<CodexCliStatus> => {
  try {
    return await safeInvoke<CodexCliStatus>('restart_codex_cli_login')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      ...fallbackCodexStatus(),
      message: '浏览器调试模式无法执行 Codex CLI 重新登录。'
    }
  }
}

export const hideAssistantWindow = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    return false
  }

  try {
    return await safeInvoke<boolean>('hide_main_window')
  } catch (error) {
    throw error instanceof Error ? error : new Error('桌宠隐藏失败，请改用托盘菜单恢复或退出。')
  }
}

export const startMainWindowDrag = async (): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await safeInvoke<void>('start_main_window_drag')
}

export const rememberMainWindowPosition = async (
  x: number,
  y: number
): Promise<boolean> => {
  if (!isTauriRuntime()) {
    return false
  }

  return await safeInvoke<boolean>('remember_main_window_position', {
    x,
    y
  })
}

// ============================================================================
// Whisper 语音识别 API
// ============================================================================

export const getWhisperStatus = async (): Promise<WhisperStatus> => {
  try {
    return await safeInvoke<WhisperStatus>('get_whisper_status')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      modelLoaded: false,
      currentModel: null,
      availableModels: [],
      recordingState: 'idle',
      inputReady: false,
      inputMessage: null
    }
  }
}

export const getWhisperModels = async (): Promise<ModelInfo[]> => {
  try {
    return await safeInvoke<ModelInfo[]>('get_whisper_models')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return []
  }
}

export const downloadWhisperModel = async (
  model: WhisperModel,
  onProgress?: (progress: DownloadProgress) => void
): Promise<string> => {
  if (!isTauriRuntime()) {
    throw new Error('Whisper 功能需要桌宠运行时')
  }

  const { Channel } = await import('@tauri-apps/api/core')
  const progressChannel = new Channel<DownloadProgress>()

  if (onProgress) {
    progressChannel.onmessage = onProgress
  }

  return safeInvoke<string>('download_whisper_model', {
    model,
    progress: progressChannel
  })
}

export const loadWhisperModel = async (model: WhisperModel): Promise<WhisperStatus> => {
  try {
    return await safeInvoke<WhisperStatus>('load_whisper_model', { model })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('加载 Whisper 模型需要桌宠运行时')
  }
}

export const unloadWhisperModel = async (): Promise<WhisperStatus> => {
  try {
    return await safeInvoke<WhisperStatus>('unload_whisper_model')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      modelLoaded: false,
      currentModel: null,
      availableModels: [],
      recordingState: 'idle',
      inputReady: false,
      inputMessage: null
    }
  }
}

export const deleteWhisperModel = async (model: WhisperModel): Promise<WhisperStatus> => {
  try {
    return await safeInvoke<WhisperStatus>('delete_whisper_model', { model })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('删除 Whisper 模型需要桌宠运行时')
  }
}

export const startWhisperRecording = async (): Promise<RecordingState> => {
  try {
    return await safeInvoke<RecordingState>('start_whisper_recording')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('Whisper 录音需要桌宠运行时')
  }
}

export const stopWhisperRecording = async (): Promise<TranscriptionResult> => {
  try {
    return await safeInvoke<TranscriptionResult>('stop_whisper_recording')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('Whisper 录音需要桌宠运行时')
  }
}

export const getWhisperRecordingState = async (): Promise<RecordingState> => {
  try {
    return await safeInvoke<RecordingState>('get_whisper_recording_state')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return 'idle'
  }
}

// ============================================================================
// Codex 更新 API
// ============================================================================

export const checkCodexUpdate = async (): Promise<CodexUpdateStatus> => {
  try {
    return await safeInvoke<CodexUpdateStatus>('check_codex_update')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      currentVersion: null,
      latestVersion: null,
      updateAvailable: false,
      installPath: null,
      message: '浏览器调试模式无法检查 Codex 更新'
    }
  }
}

export const updateCodex = async (): Promise<CodexUpdateStatus> => {
  try {
    return await safeInvoke<CodexUpdateStatus>('update_codex')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('Codex 更新需要桌宠运行时')
  }
}

// ============================================================================
// 软件更新 API
// ============================================================================

export const checkAppUpdate = async (): Promise<AppUpdateStatus> => {
  try {
    return await safeInvoke<AppUpdateStatus>('check_app_update')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      currentVersion: null,
      latestVersion: null,
      updateAvailable: false,
      releaseUrl: null,
      downloadUrl: null,
      assetName: null,
      message: '浏览器调试模式无法检查软件更新'
    }
  }
}

export const openAppUpdateDownload = async (): Promise<AppUpdateStatus> => {
  try {
    return await safeInvoke<AppUpdateStatus>('open_app_update_download')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    throw new Error('打开软件更新下载页需要桌宠运行时')
  }
}

// ============================================================================
// 投研模式 API
// ============================================================================

export const getResearchBriefSnapshot = async (): Promise<ResearchBriefSnapshot> => {
  try {
    return await safeInvoke<ResearchBriefSnapshot>('get_research_brief_snapshot')
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return {
      generatedAt: Date.now(),
      dayKey: new Date().toISOString().slice(0, 10),
      enabled: false,
      title: '本地投研模式未启用',
      summary: '当前是浏览器调试模式，投研简报需要桌宠运行时。',
      sections: [],
      alerts: [],
      fundQuotes: [],
      memoryHints: [],
      alertFingerprint: '',
      hasUpdates: false,
      startupPopupDue: false,
      updateSummary: null,
      analysisStatus: 'unavailable',
      analysisProviderLabel: null,
      analysisResult: null,
      analysisNotice: '当前是浏览器调试模式，无法生成 AI 投研分析。'
    }
  }
}

export const acknowledgeResearchBrief = async (
  dayKey: string,
  alertFingerprint?: string | null,
  markStartupPopup?: boolean
): Promise<boolean> => {
  try {
    return await safeInvoke<boolean>('acknowledge_research_brief', {
      dayKey,
      alertFingerprint: alertFingerprint ?? null,
      markStartupPopup: markStartupPopup ?? false
    })
  } catch (error) {
    rethrowIfDesktopRuntime(error)
    return false
  }
}
