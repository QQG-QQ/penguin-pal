<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { cursorPosition, currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import FloatingBubble from './components/FloatingBubble.vue'
import InputBox from './components/InputBox.vue'
import Penguin from './components/Penguin.vue'
import ResearchBriefWindow from './components/ResearchBriefWindow.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import {
  COMMAND_CONFIRMATION_TIMEOUT_MS,
  createClearConversationConfirmation,
  createModelSetConfirmation,
  isPendingCommandExpired,
  resolvePendingCommandInput,
  type PendingCommandConfirmation
} from './lib/commandConfirmation'
import { findModelCatalogEntry, modelCatalog } from './lib/modelCatalog'
import {
  PET_DOCK_IDLE_DELAY_MS,
  PET_DOCK_EDGE_THRESHOLD_PX,
  choosePetDockState,
  isPetNearDockEdge,
  planDockedWindowFrame,
  type PetWindowFrame
} from './lib/petDocking'
import { buildWorkAreaRect, clampWindowPositionToWorkArea } from './lib/petLayout'
import { parseSlashCommand, slashHelpText } from './lib/slashCommands'
import {
  cancelDesktopActionApproval,
  cancelControlPending,
  clearConversation,
  clearTodayReplyHistory,
  checkAppUpdate,
  closeSettingsWindow,
  confirmControlPending,
  confirmDesktopAction,
  deleteManagedMemory,
  deleteWhisperModel,
  downloadWhisperModel,
  getAssistantSnapshot,
  getControlServiceStatus,
  getInputHistory,
  getMemoryManagementSnapshot,
  getResearchBriefSnapshot,
  getTodayReplyHistory,
  getCodexCliStatus,
  getWhisperStatus,
  hideAssistantWindow,
  invokeControlTool,
  isBubbleWindowView,
  isResearchWindowView,
  listenForAssistantSnapshot,
  listenForBubbleInteractionState,
  listenForBubbleDismissRequest,
  listenForBubbleLayoutMetrics,
  listenForBubbleWindowState,
  listenForSettingsSectionChange,
  listenForWhisperPushToTalk,
  listenForWhisperStatus,
  listenForTodayReplyHistory,
  loadWhisperModel,
  openSettingsWindow,
  openResearchWindow,
  openAppUpdateDownload,
  publishWhisperStatus,
  publishTodayReplyHistory,
  publishBubbleWindowState,
  publishAssistantSnapshot,
  readWindowView,
  readRequestedSettingsSection,
  rememberMainWindowPosition,
  resolveMemoryConflict,
  requestDesktopAction,
  saveProviderConfig,
  sendChatMessage,
  restartCodexCliLogin,
  startCodexCliLogin,
  startWhisperRecording,
  listControlPending,
  stopWhisperRecording,
  unloadWhisperModel,
  acknowledgeResearchBrief,
  closeResearchWindow,
  promoteMemoryCandidate,
  type SettingsSection
} from './lib/assistant'
import type {
  ActionApprovalRequest,
  AgentTaskProgress,
  AppUpdateStatus,
  AssistantWindowView,
  AiConstraintProfile,
  AssistantSnapshot,
  BubbleLayoutMetrics,
  BubbleMessageTier,
  BubbleWindowState,
  CodexCliStatus,
  ControlPendingRequest,
  ControlToolInvokeResponse,
  DesktopAction,
  DownloadProgress,
  ManagedMemoryKind,
  MemoryManagementSnapshot,
  PetLayoutMetrics,
  PetDockState,
  PetMode,
  ProviderConfigInput,
  ProviderKind,
  ResearchBriefSnapshot,
  ReplyHistoryEntry,
  WhisperModel,
  WhisperPushToTalkEvent,
  WhisperStatus,
  PendingShellConfirmation
} from './types/assistant'

const providerDefaults: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  codexCli: 'gpt-5-codex',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

const providerLabels: Record<ProviderKind, string> = {
  mock: 'Mock',
  codexCli: 'Codex CLI',
  openAi: 'OpenAI',
  anthropic: 'Anthropic',
  openAiCompatible: 'OpenAI-Compatible'
}

const DEFAULT_OAUTH_REDIRECT_URL = 'http://127.0.0.1:8976/oauth/callback'
const DEFAULT_PUSH_TO_TALK_SHORTCUT = 'CommandOrControl+Alt+Space'
const WHISPER_CAPTURE_WINDOW_MS = 3600

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

const actionCommandMap: Record<string, string[]> = {
  focus_window: ['唤起桌宠', '聚焦桌宠', '显示桌宠'],
  show_window: ['显示主面板', '显示窗口']
}

const localDirectActionIds = new Set(['focus_window', 'show_window'])

const PET_WINDOW_BASE = { width: 252, height: 278 }
const PET_WINDOW_PANEL_WIDTH = 320
const PET_WINDOW_PANEL_GAP = 8
const PET_WINDOW_PANEL_HEIGHTS = {
  composer: 88,
  task: 112,
  controlPending: 130,
  commandPending: 122
} as const
const hiddenBubbleState = (): BubbleWindowState => ({
  messageId: 0,
  visible: false,
  text: '',
  anchorX: 0,
  anchorY: 0,
  petLeft: 0,
  petTop: 0,
  petRight: 0,
  petBottom: 0,
  faceLeft: 0,
  faceTop: 0,
  faceRight: 0,
  faceBottom: 0
})

const emptyConstraints = (): AiConstraintProfile => ({
  label: 'Codex Guardrails',
  version: '2026-03-10',
  summary: '当前还没有从后端加载 AI 约束配置。',
  immutableRules: [],
  capabilityGates: [],
  runtimeBoundaries: []
})

const emptySnapshot = (): AssistantSnapshot => ({
  mode: 'idle',
  messages: [],
  provider: {
    kind: 'mock',
    model: providerDefaults.mock,
    baseUrl: null,
    systemPrompt:
      '你是一只管理员企鹅桌宠。普通聊天时直接回答，只有涉及权限、隐私或电脑控制时再简短说明限制。',
    allowNetwork: true,
    voiceReply: true,
    retainHistory: true,
    voiceInputMode: 'continuous',
    pushToTalkShortcut: DEFAULT_PUSH_TO_TALK_SHORTCUT,
    apiKeyLoaded: false,
    authMode: 'apiKey',
    oauth: {
      status: 'signedOut',
      authorizeUrl: null,
      tokenUrl: null,
      clientId: null,
      redirectUrl: DEFAULT_OAUTH_REDIRECT_URL,
      scopes: [],
      accountHint: null,
      pendingAuthUrl: null,
      accessTokenLoaded: false,
      lastError: null,
      startedAt: null,
      expiresAt: null
    }
  },
  launchAtStartup: false,
  autoUpdateCodex: true,
  autoCheckAppUpdate: true,
  research: defaultResearchConfig(),
  workspaceRoot: null,
  visionChannel: {
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
  },
  visionChannelStatus: {
    kind: 'unsupported',
    message: '视觉副通道未启用。'
  },
  permissionLevel: 2,
  allowedActions: [],
  auditTrail: [],
  audioProfile: {
    inputMode: 'auto-listen',
    outputMode: 'speech-synthesis',
    stages: []
  },
  aiConstraints: emptyConstraints(),
  shellPermissions: {
    enabled: false,
    allowExecute: false,
    allowFileModify: false,
    allowFileDelete: false,
    allowNetwork: false,
    allowSystem: false,
    durationHours: 1
  }
})

const emptyCodexStatus = (): CodexCliStatus => ({
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
  message: '尚未检测 Codex CLI 登录状态。'
})

const emptyAppUpdateStatus = (): AppUpdateStatus => ({
  currentVersion: null,
  latestVersion: null,
  updateAvailable: false,
  releaseUrl: null,
  downloadUrl: null,
  assetName: null,
  message: '尚未检查软件更新。'
})

const emptyResearchBrief = (): ResearchBriefSnapshot => ({
  generatedAt: Date.now(),
  dayKey: new Date().toISOString().slice(0, 10),
  enabled: false,
  title: '本地投研模式未启用',
  summary: '开启本地投研模式后，这里会展示每日研究简报、基金风格比较和长期记忆提示。',
  sections: [],
  alerts: [],
  fundQuotes: [],
  memoryHints: [],
  alertFingerprint: '',
  hasUpdates: false,
  startupPopupDue: false,
  updateSummary: null,
  analysisStatus: 'disabled',
  analysisProviderLabel: null,
  analysisResult: null,
  analysisNotice: '开启本地投研模式后，这里会展示 AI 自动生成的研究分析。'
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

const toDraft = (state: AssistantSnapshot): ProviderConfigInput => ({
  kind: state.provider.kind,
  model: state.provider.model || providerDefaults[state.provider.kind],
  baseUrl: state.provider.baseUrl,
  systemPrompt: state.provider.systemPrompt,
  allowNetwork: state.provider.allowNetwork,
  launchAtStartup: state.launchAtStartup,
  autoUpdateCodex: state.autoUpdateCodex,
  autoCheckAppUpdate: state.autoCheckAppUpdate,
  research: {
    ...defaultResearchConfig(),
    ...(state.research ?? {})
  },
  voiceReply: state.provider.voiceReply,
  retainHistory: state.provider.retainHistory,
  voiceInputMode: state.provider.voiceInputMode,
  pushToTalkShortcut: state.provider.pushToTalkShortcut || DEFAULT_PUSH_TO_TALK_SHORTCUT,
  workspaceRoot: state.workspaceRoot,
  permissionLevel: state.permissionLevel,
  authMode: state.provider.authMode,
  oauthAuthorizeUrl: state.provider.oauth.authorizeUrl,
  oauthTokenUrl: state.provider.oauth.tokenUrl,
  oauthClientId: state.provider.oauth.clientId,
  oauthRedirectUrl: state.provider.oauth.redirectUrl,
  oauthScopes: state.provider.oauth.scopes.join(' '),
  apiKey: '',
  clearApiKey: false,
  clearOAuthToken: false,
  visionChannel: {
    enabled: state.visionChannel.enabled,
    kind: state.visionChannel.kind,
    model: state.visionChannel.model,
    baseUrl: state.visionChannel.baseUrl,
    allowNetwork: state.visionChannel.allowNetwork,
    timeoutMs: state.visionChannel.timeoutMs,
    maxImageBytes: state.visionChannel.maxImageBytes,
    maxImageWidth: state.visionChannel.maxImageWidth,
    maxImageHeight: state.visionChannel.maxImageHeight,
    apiKey: '',
    clearApiKey: false
  },
  shellPermissions: state.shellPermissions ?? {
    enabled: false,
    allowExecute: false,
    allowFileModify: false,
    allowFileDelete: false,
    allowNetwork: false,
    allowSystem: false,
    durationHours: 1
  }
})

const windowView = ref<AssistantWindowView>(readWindowView())
const snapshot = ref<AssistantSnapshot>(emptySnapshot())
const settingsDraft = ref<ProviderConfigInput>(toDraft(snapshot.value))
const drawerSection = ref<SettingsSection>(readRequestedSettingsSection())
const messageDraft = ref('')
const inputHistory = ref<string[]>([])
const todayReplyHistory = ref<ReplyHistoryEntry[]>([])
const bubbleText = ref('')
const bubbleMessageId = ref(0)
const bubbleLayoutMetrics = ref<BubbleLayoutMetrics | null>(null)
const bubbleMessageTier = ref<BubbleMessageTier | null>(null)
const bubbleWindowState = ref<BubbleWindowState>(hiddenBubbleState())
const busy = ref(false)
const savingSettings = ref(false)
const authBusy = ref(false)
const oauthNotice = ref('')
const codexStatus = ref<CodexCliStatus>(emptyCodexStatus())
const appUpdateStatus = ref<AppUpdateStatus>(emptyAppUpdateStatus())
const appUpdateBusy = ref(false)
const researchBrief = ref<ResearchBriefSnapshot>(emptyResearchBrief())
const researchBriefBusy = ref(false)
const memoryDashboard = ref<MemoryManagementSnapshot>(emptyMemoryManagementSnapshot())
const memoryBusy = ref(false)
const whisperStatus = ref<WhisperStatus>({
  modelLoaded: false,
  currentModel: null,
  availableModels: [],
  recordingState: 'idle',
  inputReady: false,
  inputMessage: null
})
const whisperDownloading = ref(false)
const whisperDownloadProgress = ref<DownloadProgress | null>(null)
const pendingApproval = ref<ActionApprovalRequest | null>(null)
const controlPendingRequest = ref<ControlPendingRequest | null>(null)
const pendingShellConfirmation = ref<PendingShellConfirmation | null>(null)
const pendingCommandConfirmation = ref<PendingCommandConfirmation | null>(null)
const agentTaskProgress = ref<AgentTaskProgress | null>(null)
const approvalPhrase = ref('')
const approvalChecks = ref<Record<string, boolean>>({})
const listening = ref(false)
const visualMode = ref<PetMode | null>(null)
const microphoneAvailable = ref(false)
const textInputFocused = ref(false)
const composerVisible = ref(false)
const inputBoxRef = ref<{ focusComposer: () => void } | null>(null)
const penguinRef = ref<{
  getLayoutMetrics: () => PetLayoutMetrics | null
  isOpaqueClientHit: (clientX: number, clientY: number) => boolean
} | null>(null)

let recognition: SpeechRecognition | null = null
let recognitionBuffer = ''
let submitVoiceAfterStop = false
let bubbleTimer: number | null = null
let bubbleResumeTimer: number | null = null
let bubbleDismissRemainingMs: number | null = null
let bubbleDismissDeadline = 0
let speechSession = 0
let mediaDevicesCleanup: (() => void) | null = null
let microphonePermissionRequested = false
let snapshotListenerCleanup: (() => void) | null = null
let sectionListenerCleanup: (() => void) | null = null
let bubbleStateListenerCleanup: (() => void) | null = null
let bubbleInteractionListenerCleanup: (() => void) | null = null
let bubbleLayoutMetricsListenerCleanup: (() => void) | null = null
let bubbleDismissRequestCleanup: (() => void) | null = null
let todayReplyHistoryListenerCleanup: (() => void) | null = null
let whisperStatusListenerCleanup: (() => void) | null = null
let whisperPushToTalkCleanup: (() => void) | null = null
let windowMovedCleanup: (() => void) | null = null
let windowResizedCleanup: (() => void) | null = null
let autoListenTimer: number | null = null
let whisperCaptureTimer: number | null = null
let speechPlaybackActive = false
let petClampTimer: number | null = null
let petDockTimer: number | null = null
let petHitTestTimer: number | null = null
let persistWindowPositionTimer: number | null = null
let controlPendingTimer: number | null = null
let shellConfirmationTimer: number | null = null
let pendingCommandTimer: number | null = null
let agentTaskTimer: number | null = null
let petClampInFlight = false
let inputHistoryCursor = -1
let draftBeforeHistoryNavigation = ''
const bubbleInteractionActive = ref(false)
let lastBubbleDebugKey = ''
const dockState = ref<PetDockState>('normal')
let restorePetFrame: PetWindowFrame | null = null
let cursorPassthroughEnabled = false
let cursorPassthroughSuspendUntil = 0
let researchStartupHandled = false
let researchStartupRetryTimer: number | null = null
let lastResearchAlertSignature = ''

const isSettingsView = computed(() => windowView.value === 'settings')
const isBubbleView = computed(() => windowView.value === 'bubble' || isBubbleWindowView())
const isResearchView = computed(() => windowView.value === 'research' || isResearchWindowView())
const isNonPetWindowView = computed(
  () => isSettingsView.value || isBubbleView.value || isResearchView.value
)
const activeMode = computed<PetMode>(() => visualMode.value ?? snapshot.value.mode)
const isPetDocked = computed(() => dockState.value !== 'normal')
const activeProviderLabel = computed(() => providerLabels[snapshot.value.provider.kind])
const showAgentTaskStrip = computed(() => Boolean(agentTaskProgress.value))
const hasControlPending = computed(() => Boolean(controlPendingRequest.value))
const hasPendingCommand = computed(() => Boolean(pendingCommandConfirmation.value))
const agentTaskStatusLabel = computed(() => {
  const status = agentTaskProgress.value?.status
  switch (status) {
    case 'running':
      return '运行中'
    case 'waitingConfirmation':
      return '等待确认'
    case 'completed':
      return '已完成'
    case 'failed':
      return '失败'
    case 'cancelled':
      return '已取消'
    default:
      return ''
  }
})
const agentTaskStepLabel = computed(() => {
  const task = agentTaskProgress.value
  if (!task) {
    return ''
  }

  return `第 ${task.stepIndex}/${task.stepCount} 步`
})
const agentTaskToneClass = computed(() => {
  const status = agentTaskProgress.value?.status
  switch (status) {
    case 'completed':
      return 'task-status-strip success'
    case 'failed':
      return 'task-status-strip failure'
    case 'cancelled':
      return 'task-status-strip muted'
    default:
      return 'task-status-strip'
  }
})
const petWindowFrame = computed(() => {
  const panelCount = [
    showComposer.value,
    showAgentTaskStrip.value,
    hasControlPending.value,
    hasPendingCommand.value
  ].filter(Boolean).length

  const height =
    PET_WINDOW_BASE.height +
    (showComposer.value ? PET_WINDOW_PANEL_HEIGHTS.composer : 0) +
    (showAgentTaskStrip.value ? PET_WINDOW_PANEL_HEIGHTS.task : 0) +
    (hasControlPending.value ? PET_WINDOW_PANEL_HEIGHTS.controlPending : 0) +
    (hasPendingCommand.value ? PET_WINDOW_PANEL_HEIGHTS.commandPending : 0) +
    panelCount * PET_WINDOW_PANEL_GAP

  return {
    width: panelCount > 0 ? PET_WINDOW_PANEL_WIDTH : PET_WINDOW_BASE.width,
    height
  }
})
const showComposer = computed(
  () =>
    composerVisible.value ||
    textInputFocused.value ||
    Boolean(messageDraft.value.trim()) ||
    Boolean(pendingCommandConfirmation.value)
)
const canEnterDockedIdle = computed(
  () =>
    isTauriDesktop() &&
    !isNonPetWindowView.value &&
    activeMode.value === 'idle' &&
    !busy.value &&
    !listening.value &&
    !bubbleText.value.trim() &&
    !showComposer.value &&
    !showAgentTaskStrip.value &&
    !hasControlPending.value &&
    !hasPendingCommand.value &&
    !pendingApproval.value &&
    !pendingShellConfirmation.value
)
const shouldEnterDockedIdle = computed(
  () => canEnterDockedIdle.value && !isPetDocked.value
)

const canSubmitApproval = computed(() => {
  if (!pendingApproval.value || busy.value) {
    return false
  }

  const phraseMatches = approvalPhrase.value.trim() === pendingApproval.value.requiredPhrase
  const checksReady = pendingApproval.value.checks.every((check) => approvalChecks.value[check.id])
  return phraseMatches && checksReady
})

const speechRecognitionSupported = computed(
  () =>
    typeof window !== 'undefined' &&
    Boolean(window.SpeechRecognition || window.webkitSpeechRecognition)
)

const useLocalWhisperInput = computed(() => isTauriDesktop())
const whisperVoiceInputMode = computed(() => snapshot.value.provider.voiceInputMode)

const voiceInputAvailable = computed(
  () =>
    useLocalWhisperInput.value
      ? whisperStatus.value.modelLoaded && whisperStatus.value.inputReady
      : speechRecognitionSupported.value && microphoneAvailable.value
)

const voiceReplySupported = computed(
  () => typeof window !== 'undefined' && 'speechSynthesis' in window
)

const shouldAutoListen = computed(
  () =>
    voiceInputAvailable.value &&
    whisperVoiceInputMode.value === 'continuous' &&
    !isNonPetWindowView.value &&
    !busy.value &&
    !textInputFocused.value &&
    !pendingApproval.value &&
    !pendingCommandConfirmation.value &&
    !speechPlaybackActive &&
    !messageDraft.value.trim()
)

const normalizeCommand = (value: string) => value.replace(/\s+/g, '').toLowerCase()

const isTauriDesktop = () =>
  typeof window !== 'undefined' && typeof window.__TAURI_INTERNALS__ !== 'undefined'

const resolveErrorMessage = (error: unknown, fallback: string): string => {
  if (error instanceof Error && error.message.trim()) {
    return error.message
  }

  if (typeof error === 'string' && error.trim()) {
    return error
  }

  try {
    const serialized = JSON.stringify(error)
    if (serialized && serialized !== '{}' && serialized !== 'null') {
      return serialized
    }
  } catch {
    // ignore JSON serialization errors and use fallback message
  }

  if (error !== undefined && error !== null) {
    const text = String(error)
    if (text && text !== '[object Object]') {
      return text
    }
  }

  return fallback
}

const applySnapshot = (nextSnapshot: AssistantSnapshot) => {
  snapshot.value = nextSnapshot
  settingsDraft.value = toDraft(nextSnapshot)
}

const syncSnapshot = async (nextSnapshot: AssistantSnapshot) => {
  applySnapshot(nextSnapshot)
  await publishAssistantSnapshot(nextSnapshot)
}

const clearBubbleTimer = () => {
  if (bubbleTimer !== null) {
    window.clearTimeout(bubbleTimer)
    bubbleTimer = null
  }
}

const clearBubbleResumeTimer = () => {
  if (bubbleResumeTimer !== null) {
    window.clearTimeout(bubbleResumeTimer)
    bubbleResumeTimer = null
  }
}

const resetBubbleDismissState = () => {
  clearBubbleTimer()
  clearBubbleResumeTimer()
  bubbleDismissRemainingMs = null
  bubbleDismissDeadline = 0
  bubbleInteractionActive.value = false
}

const clearBubble = () => {
  resetBubbleDismissState()
  bubbleMessageId.value = 0
  bubbleLayoutMetrics.value = null
  bubbleMessageTier.value = null
  lastBubbleDebugKey = ''
  bubbleText.value = ''
}

const resetInputHistoryNavigation = () => {
  inputHistoryCursor = -1
  draftBeforeHistoryNavigation = ''
}

const loadInputHistory = async () => {
  inputHistory.value = await getInputHistory()
}

const applyTodayReplyHistory = async (entries: ReplyHistoryEntry[], publish = true) => {
  todayReplyHistory.value = entries
  if (publish) {
    await publishTodayReplyHistory(entries)
  }
}

const refreshTodayReplyHistory = async (publish = true) => {
  await applyTodayReplyHistory(await getTodayReplyHistory(), publish)
}

const refreshMemoryDashboard = async () => {
  memoryDashboard.value = await getMemoryManagementSnapshot()
}

const buildResearchAlertSignature = (brief: ResearchBriefSnapshot) =>
  brief.alerts.map((alert) => `${alert.severity}:${alert.title}:${alert.summary}`).join('|')

const clearResearchStartupRetryTimer = () => {
  if (researchStartupRetryTimer !== null) {
    window.clearTimeout(researchStartupRetryTimer)
    researchStartupRetryTimer = null
  }
}

const acknowledgeResearchBriefState = async (
  brief: ResearchBriefSnapshot,
  options: { markStartupPopup?: boolean } = {}
) => {
  if (!brief.enabled) {
    return false
  }

  try {
    return await acknowledgeResearchBrief(
      brief.dayKey,
      brief.alertFingerprint,
      options.markStartupPopup ?? false
    )
  } catch {
    return false
  }
}

const maybeAnnounceResearchAlerts = (brief: ResearchBriefSnapshot) => {
  if (
    !snapshot.value.research.enabled ||
    !snapshot.value.research.bubbleAlerts ||
    isNonPetWindowView.value ||
    !brief.alerts.length ||
    !brief.hasUpdates
  ) {
    return
  }

  if (snapshot.value.research.startupPopup && !researchStartupHandled) {
    return
  }

  const signature = brief.alertFingerprint || buildResearchAlertSignature(brief)
  if (!signature || signature === lastResearchAlertSignature) {
    return
  }

  lastResearchAlertSignature = signature
  const primaryAlert =
    brief.alerts.find((item) => item.severity === 'urgent') ??
    brief.alerts.find((item) => item.severity === 'watch') ??
    brief.alerts[0]

  announce(`投研提醒：${primaryAlert.title}。${primaryAlert.summary}`, 'guarded')
  void acknowledgeResearchBriefState(brief)
}

const refreshResearchBrief = async (options: { silent?: boolean } = {}) => {
  const { silent = false } = options
  researchBriefBusy.value = true

  try {
    const brief = await getResearchBriefSnapshot()
    researchBrief.value = brief
    maybeAnnounceResearchAlerts(brief)
    if (!silent && isResearchView.value) {
      announce(brief.updateSummary || '今日投研简报已刷新。', 'idle')
    }
  } catch (error) {
    if (!silent) {
      announce(resolveErrorMessage(error, '刷新投研简报失败'), 'guarded')
    }
  } finally {
    researchBriefBusy.value = false
  }
}

const scheduleResearchStartupPopupRetry = (loaded: AssistantSnapshot, delay = 1400) => {
  if (researchStartupHandled || isNonPetWindowView.value) {
    return
  }

  clearResearchStartupRetryTimer()
  researchStartupRetryTimer = window.setTimeout(() => {
    researchStartupRetryTimer = null
    void maybeOpenResearchWindowOnStartup(loaded)
  }, delay)
}

const maybeOpenResearchWindowOnStartup = async (loaded: AssistantSnapshot) => {
  if (researchStartupHandled || isNonPetWindowView.value) {
    return
  }

  if (!loaded.research.enabled || !loaded.research.startupPopup) {
    researchStartupHandled = true
    return
  }

  if (!researchBrief.value.enabled) {
    scheduleResearchStartupPopupRetry(loaded)
    return
  }

  try {
    if (!researchBrief.value.startupPopupDue) {
      researchStartupHandled = true
      return
    }

    const opened = await openResearchWindow()
    if (!opened) {
      scheduleResearchStartupPopupRetry(loaded, 1800)
      return
    }

    await acknowledgeResearchBriefState(researchBrief.value, { markStartupPopup: true })
    lastResearchAlertSignature = researchBrief.value.alertFingerprint || lastResearchAlertSignature
    researchStartupHandled = true
  } catch (error) {
    scheduleResearchStartupPopupRetry(loaded, 1800)
    announce(resolveErrorMessage(error, '打开投研简报窗口失败'), 'guarded')
  }
}

const clampDuration = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max)

const computeBubbleDismissDecision = (
  metrics: BubbleLayoutMetrics,
  override?: number | null
): { tier: BubbleMessageTier; autoHideDuration: number | null; showCloseButton: boolean } => {
  if (override !== undefined) {
    return {
      tier: override === null ? 'pinned' : 'short',
      autoHideDuration: override,
      showCloseButton: override === null
    }
  }

  if (metrics.isScrollable) {
    return {
      tier: 'pinned',
      autoHideDuration: null,
      showCloseButton: true
    }
  }

  if (metrics.contentHeight <= 96) {
    return {
      tier: 'short',
      autoHideDuration: clampDuration(4000 + metrics.charCount * 18, 4000, 6000),
      showCloseButton: false
    }
  }

  if (metrics.contentHeight <= 168) {
    return {
      tier: 'medium',
      autoHideDuration: clampDuration(8000 + metrics.charCount * 24, 8000, 14000),
      showCloseButton: false
    }
  }

  return {
    tier: 'long',
    autoHideDuration: clampDuration(15000 + metrics.charCount * 18, 15000, 24000),
    showCloseButton: false
  }
}

const logBubbleDecision = (
  metrics: BubbleLayoutMetrics,
  decision: { tier: BubbleMessageTier; autoHideDuration: number | null; showCloseButton: boolean }
) => {
  if (!import.meta.env.DEV) {
    return
  }

  const logKey = [
    metrics.messageId,
    metrics.charCount,
    metrics.contentHeight,
    metrics.scrollHeight,
    metrics.clientHeight,
    metrics.isScrollable,
    decision.tier,
    decision.autoHideDuration
  ].join(':')

  if (lastBubbleDebugKey === logKey) {
    return
  }

  lastBubbleDebugKey = logKey
  console.info('[PenguinPal bubble decision]', {
    charCount: metrics.charCount,
    contentHeight: metrics.contentHeight,
    scrollHeight: metrics.scrollHeight,
    clientHeight: metrics.clientHeight,
    isScrollable: metrics.isScrollable,
    tier: decision.tier,
    autoHideDuration: decision.autoHideDuration,
    showCloseButton: decision.showCloseButton
  })
}

const applyBubbleLayoutMetrics = (metrics: BubbleLayoutMetrics) => {
  if (metrics.messageId !== bubbleMessageId.value || !bubbleText.value.trim()) {
    return
  }

  bubbleLayoutMetrics.value = metrics
  const decision = computeBubbleDismissDecision(metrics)
  bubbleMessageTier.value = decision.tier
  bubbleDismissRemainingMs = decision.autoHideDuration
  logBubbleDecision(metrics, decision)

  if (!speechPlaybackActive) {
    scheduleBubbleDismiss(metrics.messageId, decision.autoHideDuration)
  }
}

const finalizeBubbleDismiss = (session: number) => {
  if (session !== bubbleMessageId.value) {
    return
  }

  clearBubble()
  resetVisualModeSoon(0)
  scheduleAutoListening()
}

const scheduleBubbleDismiss = (session: number, delayMs: number | null) => {
  clearBubbleTimer()

  if (delayMs === null) {
    bubbleDismissRemainingMs = null
    bubbleDismissDeadline = 0
    return
  }

  const nextDelay = Math.max(1500, delayMs)
  bubbleDismissRemainingMs = nextDelay

  if (bubbleInteractionActive.value || speechPlaybackActive || !bubbleText.value.trim()) {
    return
  }

  bubbleDismissDeadline = Date.now() + nextDelay
  bubbleTimer = window.setTimeout(() => {
    finalizeBubbleDismiss(session)
  }, nextDelay)
}

const pauseBubbleDismiss = () => {
  clearBubbleResumeTimer()
  if (bubbleTimer === null) {
    return
  }

  bubbleDismissRemainingMs = Math.max(2200, bubbleDismissDeadline - Date.now())
  clearBubbleTimer()
}

const resumeBubbleDismiss = (delay = 0) => {
  clearBubbleResumeTimer()

  if (
    bubbleDismissRemainingMs === null ||
    bubbleInteractionActive.value ||
    speechPlaybackActive ||
    !bubbleText.value.trim()
  ) {
    return
  }

  const session = bubbleMessageId.value
  const resume = () => {
    scheduleBubbleDismiss(session, bubbleDismissRemainingMs)
  }

  if (delay > 0) {
    bubbleResumeTimer = window.setTimeout(resume, delay)
    return
  }

  resume()
}

const handleBubbleInteractionState = (active: boolean) => {
  bubbleInteractionActive.value = active

  if (active) {
    pauseBubbleDismiss()
    return
  }

  resumeBubbleDismiss(220)
}

const handleBubbleDismissRequest = (messageId: number) => {
  if (messageId !== bubbleMessageId.value) {
    return
  }

  clearBubble()
  resetVisualModeSoon(0)

  if (!speechPlaybackActive) {
    scheduleAutoListening(220)
  }
}

const pushInputHistoryLocally = (content: string) => {
  const trimmed = content.trim()
  if (!trimmed) {
    return
  }

  if (inputHistory.value[inputHistory.value.length - 1] === trimmed) {
    resetInputHistoryNavigation()
    return
  }

  inputHistory.value = [...inputHistory.value, trimmed].slice(-50)
  resetInputHistoryNavigation()
}

const recallOlderInput = async () => {
  if (!inputHistory.value.length || isSettingsView.value || isBubbleView.value) {
    return
  }

  if (inputHistoryCursor === -1) {
    draftBeforeHistoryNavigation = messageDraft.value
    inputHistoryCursor = inputHistory.value.length - 1
  } else if (inputHistoryCursor > 0) {
    inputHistoryCursor -= 1
  }

  messageDraft.value = inputHistory.value[inputHistoryCursor] ?? messageDraft.value
  composerVisible.value = true
  await nextTick()
  inputBoxRef.value?.focusComposer()
}

const recallNewerInput = async () => {
  if (inputHistoryCursor === -1 || isSettingsView.value || isBubbleView.value) {
    return
  }

  if (inputHistoryCursor < inputHistory.value.length - 1) {
    inputHistoryCursor += 1
    messageDraft.value = inputHistory.value[inputHistoryCursor] ?? ''
  } else {
    inputHistoryCursor = -1
    messageDraft.value = draftBeforeHistoryNavigation
    draftBeforeHistoryNavigation = ''
  }

  composerVisible.value = true
  await nextTick()
  inputBoxRef.value?.focusComposer()
}

const clearPetClampTimer = () => {
  if (petClampTimer !== null) {
    window.clearTimeout(petClampTimer)
    petClampTimer = null
  }
}

const clearPetDockTimer = () => {
  if (petDockTimer !== null) {
    window.clearTimeout(petDockTimer)
    petDockTimer = null
  }
}

const clearPetHitTestTimer = () => {
  if (petHitTestTimer !== null) {
    window.clearInterval(petHitTestTimer)
    petHitTestTimer = null
  }
}

const clearPersistWindowPositionTimer = () => {
  if (persistWindowPositionTimer !== null) {
    window.clearTimeout(persistWindowPositionTimer)
    persistWindowPositionTimer = null
  }
}

const resolveCurrentWorkArea = async () => {
  const monitor = await currentMonitor()
  const position = monitor?.workArea.position ?? { x: 0, y: 0 }
  const size = monitor?.workArea.size ?? monitor?.size ?? {
    width: window.screen.availWidth,
    height: window.screen.availHeight
  }

  return buildWorkAreaRect(position, size)
}

const collectPetLayoutMetrics = async (): Promise<PetLayoutMetrics | null> => {
  await nextTick()
  return penguinRef.value?.getLayoutMetrics() ?? null
}

const collectGlobalPetBounds = async (): Promise<PetWindowFrame | null> => {
  const petLayout = await collectPetLayoutMetrics()
  if (!petLayout || !isTauriDesktop()) {
    return null
  }

  const appWindow = getCurrentWindow()
  const position = await appWindow.outerPosition()

  return {
    left: Math.round(position.x + petLayout.petLeft),
    top: Math.round(position.y + petLayout.petTop),
    width: Math.round(petLayout.petRight - petLayout.petLeft),
    height: Math.round(petLayout.petBottom - petLayout.petTop)
  }
}

const snapVisiblePetBoundsToEdges = (
  position: { x: number; y: number },
  visibleBounds: PetWindowFrame,
  workArea: Awaited<ReturnType<typeof resolveCurrentWorkArea>>,
  threshold = PET_DOCK_EDGE_THRESHOLD_PX
) => {
  let nextLeft = position.x
  let nextTop = position.y
  const visibleRight = visibleBounds.left + visibleBounds.width
  const visibleBottom = visibleBounds.top + visibleBounds.height

  if (
    visibleBounds.left < workArea.left ||
    Math.abs(visibleBounds.left - workArea.left) <= threshold
  ) {
    nextLeft += workArea.left - visibleBounds.left
  } else if (
    visibleRight > workArea.right ||
    Math.abs(workArea.right - visibleRight) <= threshold
  ) {
    nextLeft -= visibleRight - workArea.right
  }

  if (
    visibleBounds.top < workArea.top ||
    Math.abs(visibleBounds.top - workArea.top) <= threshold
  ) {
    nextTop += workArea.top - visibleBounds.top
  } else if (visibleBottom > workArea.bottom) {
    nextTop -= visibleBottom - workArea.bottom
  }

  return { left: nextLeft, top: nextTop }
}

const captureCurrentPetFrame = async (): Promise<PetWindowFrame | null> => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return null
  }

  const appWindow = getCurrentWindow()
  const position = await appWindow.outerPosition()
  const size = await appWindow.outerSize()

  return {
    left: position.x,
    top: position.y,
    width: size.width,
    height: size.height
  }
}

const persistCurrentMainWindowPosition = async () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value || dockState.value !== 'normal') {
    return
  }

  const appWindow = getCurrentWindow()
  const position = await appWindow.outerPosition()
  await rememberMainWindowPosition(position.x, position.y)
}

const schedulePersistMainWindowPosition = (delay = 220) => {
  clearPersistWindowPositionTimer()

  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value || dockState.value !== 'normal') {
    return
  }

  persistWindowPositionTimer = window.setTimeout(() => {
    void persistCurrentMainWindowPosition()
  }, delay)
}

const clampPetWindowToMonitor = async () => {
  if (
    !isTauriDesktop() ||
    isSettingsView.value ||
    isBubbleView.value ||
    petClampInFlight ||
    isPetDocked.value
  ) {
    return
  }

  petClampInFlight = true

  try {
    const appWindow = getCurrentWindow()
    const workArea = await resolveCurrentWorkArea()
    const position = await appWindow.outerPosition()
    const visibleBounds = await collectGlobalPetBounds()
    const nextPosition = visibleBounds
      ? snapVisiblePetBoundsToEdges(position, visibleBounds, workArea)
      : clampWindowPositionToWorkArea(
          {
            left: position.x,
            top: position.y,
            width: (await appWindow.outerSize()).width,
            height: (await appWindow.outerSize()).height
          },
          workArea
        )

    if (nextPosition.left !== position.x || nextPosition.top !== position.y) {
      await appWindow.setPosition(new PhysicalPosition(nextPosition.left, nextPosition.top))
    }
  } finally {
    petClampInFlight = false
  }

  await syncBubbleWindow()
}

const schedulePetWindowClamp = (delay = 70) => {
  clearPetClampTimer()

  petClampTimer = window.setTimeout(() => {
    void clampPetWindowToMonitor()
  }, delay)
}

const restoreDockedPet = async (syncBubble = true) => {
  if (!isTauriDesktop() || dockState.value === 'normal') {
    return
  }

  const nextFrame = restorePetFrame
  dockState.value = 'normal'
  clearPetDockTimer()
  await nextTick()

  if (nextFrame) {
    const appWindow = getCurrentWindow()
    await appWindow.setSize(new LogicalSize(nextFrame.width, nextFrame.height))
    await appWindow.setPosition(new PhysicalPosition(nextFrame.left, nextFrame.top))
  }

  restorePetFrame = null
  cursorPassthroughEnabled = false

  if (syncBubble) {
    await syncBubbleWindow()
  }

  if (canEnterDockedIdle.value) {
    schedulePetDockedIdle()
  }
}

const enterDockedIdle = async () => {
  if (!shouldEnterDockedIdle.value || dockState.value !== 'normal') {
    return
  }

  const currentFrame = await captureCurrentPetFrame()
  if (!currentFrame) {
    return
  }

  const workArea = await resolveCurrentWorkArea()
  const visibleBounds = await collectGlobalPetBounds()
  const dockBaseFrame = visibleBounds ?? currentFrame
  if (!isPetNearDockEdge(dockBaseFrame, workArea, PET_DOCK_EDGE_THRESHOLD_PX)) {
    return
  }

  const nextDockState = choosePetDockState(dockBaseFrame, workArea)
  const dockedFrame = planDockedWindowFrame(nextDockState, workArea, dockBaseFrame)
  restorePetFrame = currentFrame
  dockState.value = nextDockState
  await nextTick()

  const appWindow = getCurrentWindow()
  await appWindow.setSize(new LogicalSize(dockedFrame.width, dockedFrame.height))
  await appWindow.setPosition(new PhysicalPosition(dockedFrame.left, dockedFrame.top))
}

const schedulePetDockedIdle = (delay = PET_DOCK_IDLE_DELAY_MS) => {
  clearPetDockTimer()

  if (!shouldEnterDockedIdle.value || typeof window === 'undefined') {
    return
  }

  petDockTimer = window.setTimeout(() => {
    void enterDockedIdle()
  }, delay)
}

const handlePetInteract = () => {
  cursorPassthroughSuspendUntil = Date.now() + 1200
  if (isTauriDesktop() && cursorPassthroughEnabled) {
    void getCurrentWindow()
      .setIgnoreCursorEvents(false)
      .then(() => {
        cursorPassthroughEnabled = false
      })
      .catch(() => {})
  }
  clearPetDockTimer()
  if (dockState.value !== 'normal') {
    void restoreDockedPet(false)
  }
}

const shouldKeepWindowInteractive = (localX: number, localY: number) => {
  const element = document.elementFromPoint(localX, localY) as HTMLElement | null
  if (
    element?.closest(
      '.input-shell, .command-confirm-strip, .task-status-strip, .confirm-shell, .settings-window-shell'
    )
  ) {
    return true
  }

  return penguinRef.value?.isOpaqueClientHit(localX, localY) ?? false
}

const updatePetWindowCursorPassthrough = async () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return
  }

  const appWindow = getCurrentWindow()
  if (Date.now() < cursorPassthroughSuspendUntil) {
    if (cursorPassthroughEnabled) {
      await appWindow.setIgnoreCursorEvents(false)
      cursorPassthroughEnabled = false
    }
    return
  }

  const windowPosition = await appWindow.outerPosition()
  const windowSize = await appWindow.outerSize()
  const cursor = await cursorPosition()
  const localX = cursor.x - windowPosition.x
  const localY = cursor.y - windowPosition.y
  const insideWindow =
    localX >= 0 && localX <= windowSize.width && localY >= 0 && localY <= windowSize.height

  const shouldIgnore =
    insideWindow &&
    !textInputFocused.value &&
    !showComposer.value &&
    !hasControlPending.value &&
    !hasPendingCommand.value &&
    !showAgentTaskStrip.value &&
    !bubbleInteractionActive.value &&
    !busy.value &&
    !listening.value &&
    !shouldKeepWindowInteractive(localX, localY)

  if (shouldIgnore === cursorPassthroughEnabled) {
    return
  }

  await appWindow.setIgnoreCursorEvents(shouldIgnore)
  cursorPassthroughEnabled = shouldIgnore
}

const setupPetHitTestLoop = () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value || typeof window === 'undefined') {
    return
  }

  clearPetHitTestTimer()
  petHitTestTimer = window.setInterval(() => {
    void updatePetWindowCursorPassthrough()
  }, 80)
}

const syncPetWindowFrame = async () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value || isPetDocked.value) {
    return
  }

  const appWindow = getCurrentWindow()
  const nextSize = petWindowFrame.value
  const position = await appWindow.outerPosition()
  const size = await appWindow.outerSize()

  if (size.width === nextSize.width && size.height === nextSize.height) {
    return
  }

  const bottomCenterX = position.x + Math.round(size.width / 2)
  const bottomY = position.y + size.height

  await appWindow.setSize(new LogicalSize(nextSize.width, nextSize.height))
  await appWindow.setPosition(
    new PhysicalPosition(
      Math.round(bottomCenterX - nextSize.width / 2),
      Math.round(bottomY - nextSize.height)
    )
  )

  await clampPetWindowToMonitor()
}

const buildBubbleWindowState = async (): Promise<BubbleWindowState> => {
  const text = bubbleText.value.trim()
  if (!text || !isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return hiddenBubbleState()
  }

  const petLayout = await collectPetLayoutMetrics()
  if (!petLayout) {
    return hiddenBubbleState()
  }

  const appWindow = getCurrentWindow()
  const position = await appWindow.outerPosition()

  return {
    messageId: bubbleMessageId.value,
    visible: true,
    text,
    anchorX: Math.round(position.x + petLayout.anchorX),
    anchorY: Math.round(position.y + petLayout.anchorY),
    petLeft: Math.round(position.x + petLayout.petLeft),
    petTop: Math.round(position.y + petLayout.petTop),
    petRight: Math.round(position.x + petLayout.petRight),
    petBottom: Math.round(position.y + petLayout.petBottom),
    faceLeft: Math.round(position.x + petLayout.faceLeft),
    faceTop: Math.round(position.y + petLayout.faceTop),
    faceRight: Math.round(position.x + petLayout.faceRight),
    faceBottom: Math.round(position.y + petLayout.faceBottom)
  }
}

const syncBubbleWindow = async () => {
  if (!isTauriDesktop() || isBubbleView.value) {
    return
  }

  if (dockState.value !== 'normal' && bubbleText.value.trim()) {
    await restoreDockedPet(false)
  }

  const nextState = await buildBubbleWindowState()
  bubbleWindowState.value = nextState
  await publishBubbleWindowState(nextState)
}

const revealComposer = async () => {
  if (isSettingsView.value || isBubbleView.value) {
    return
  }

  if (dockState.value !== 'normal') {
    await restoreDockedPet(false)
  }

  composerVisible.value = true
  await syncPetWindowFrame()
  await nextTick()
  inputBoxRef.value?.focusComposer()
}

const clearAutoListenTimer = () => {
  if (autoListenTimer !== null) {
    window.clearTimeout(autoListenTimer)
    autoListenTimer = null
  }
}

const clearWhisperCaptureTimer = () => {
  if (whisperCaptureTimer !== null) {
    window.clearTimeout(whisperCaptureTimer)
    whisperCaptureTimer = null
  }
}

const scheduleAutoListening = (delay = 260) => {
  clearAutoListenTimer()

  if (typeof window === 'undefined') {
    return
  }

  autoListenTimer = window.setTimeout(() => {
    if (!shouldAutoListen.value || listening.value) {
      return
    }

    void startListening(true)
  }, delay)
}

const resetVisualModeSoon = (delay = 700) => {
  window.setTimeout(() => {
    if (!listening.value && !busy.value && !bubbleText.value) {
      visualMode.value = null
    }
  }, delay)
}

const showBubble = (content: string, mode: PetMode = 'speaking', duration?: number | null) => {
  clearPetDockTimer()
  if (dockState.value !== 'normal') {
    void restoreDockedPet(false)
  }
  const session = ++speechSession
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
  resetBubbleDismissState()
  bubbleMessageId.value = session
  bubbleLayoutMetrics.value = null
  bubbleMessageTier.value = null
  lastBubbleDebugKey = ''
  if (duration !== undefined) {
    bubbleDismissRemainingMs = duration
  }
  bubbleText.value = content
  visualMode.value = mode

  if (duration === null) {
    bubbleMessageTier.value = 'pinned'
  }
}

const speakReply = (content: string) => {
  if (!snapshot.value.provider.voiceReply || !voiceReplySupported.value) {
    showBubble(content, 'speaking')
    return
  }

  const session = ++speechSession
  resetBubbleDismissState()
  bubbleMessageId.value = session
  bubbleLayoutMetrics.value = null
  bubbleMessageTier.value = null
  lastBubbleDebugKey = ''
  window.speechSynthesis.cancel()
  clearAutoListenTimer()
  speechPlaybackActive = true
  bubbleDismissRemainingMs = null

  if (recognition && listening.value) {
    submitVoiceAfterStop = false
    recognition.stop()
  }

  const utterance = new SpeechSynthesisUtterance(content)
  utterance.lang = 'zh-CN'
  utterance.rate = 1
  utterance.pitch = 1.04
  utterance.onstart = () => {
    if (session !== speechSession) {
      return
    }
    bubbleText.value = content
    visualMode.value = 'speaking'
  }
  utterance.onend = () => {
    if (session !== speechSession) {
      return
    }
    speechPlaybackActive = false
    visualMode.value = 'idle'
    resumeBubbleDismiss(160)
  }
  utterance.onerror = () => {
    if (session !== speechSession) {
      return
    }
    speechPlaybackActive = false
    showBubble(content, 'speaking')
  }

  window.speechSynthesis.speak(utterance)
}

const announce = (content: string, mode: PetMode = 'speaking') => {
  if (mode === 'speaking') {
    speakReply(content)
    return
  }

  showBubble(content, mode)
}

const clearPendingApproval = () => {
  pendingApproval.value = null
  approvalPhrase.value = ''
  approvalChecks.value = {}
}

const clearControlPendingTimer = () => {
  if (controlPendingTimer !== null) {
    window.clearTimeout(controlPendingTimer)
    controlPendingTimer = null
  }
}

const clearAgentTaskTimer = () => {
  if (agentTaskTimer !== null) {
    window.clearTimeout(agentTaskTimer)
    agentTaskTimer = null
  }
}

const clearControlPendingRequest = () => {
  clearControlPendingTimer()
  controlPendingRequest.value = null
}

const handleControlPendingExpiry = () => {
  const pending = controlPendingRequest.value
  if (!pending || pending.expiresAt > Date.now()) {
    return
  }

  clearControlPendingRequest()
  if (agentTaskProgress.value?.status === 'waitingConfirmation') {
    setAgentTaskProgress({
      ...agentTaskProgress.value,
      status: 'failed',
      detail: '等待本地控制确认已超时，任务已停止。'
    })
  }
  announce('这次本地控制确认已超时，请重新输入命令。', 'guarded')
}

const setControlPendingRequest = (nextPending: ControlPendingRequest | null) => {
  clearControlPendingTimer()
  controlPendingRequest.value = nextPending

  if (!nextPending) {
    return
  }

  composerVisible.value = true
  const ttl = Math.max(0, nextPending.expiresAt - Date.now())
  controlPendingTimer = window.setTimeout(() => {
    handleControlPendingExpiry()
  }, ttl)
}

const getControlPendingRequest = () => {
  const pending = controlPendingRequest.value
  if (!pending) {
    return null
  }

  if (pending.expiresAt <= Date.now()) {
    handleControlPendingExpiry()
    return null
  }

  return pending
}

// Shell Agent 确认相关
const SHELL_CONFIRMATION_TIMEOUT_MS = 60_000 // 60 秒超时

const clearShellConfirmationTimer = () => {
  if (shellConfirmationTimer !== null) {
    window.clearTimeout(shellConfirmationTimer)
    shellConfirmationTimer = null
  }
}

const clearPendingShellConfirmation = () => {
  clearShellConfirmationTimer()
  pendingShellConfirmation.value = null
}

const handleShellConfirmationExpiry = () => {
  const pending = pendingShellConfirmation.value
  if (!pending) {
    return
  }

  clearPendingShellConfirmation()
  announce('Shell 命令确认已超时，已取消执行。', 'guarded')
}

const setPendingShellConfirmation = (nextPending: PendingShellConfirmation | null) => {
  clearShellConfirmationTimer()
  pendingShellConfirmation.value = nextPending

  if (!nextPending) {
    return
  }

  composerVisible.value = true
  // 设置超时定时器
  shellConfirmationTimer = window.setTimeout(() => {
    handleShellConfirmationExpiry()
  }, SHELL_CONFIRMATION_TIMEOUT_MS)
}

const confirmShellCommand = async () => {
  const pending = pendingShellConfirmation.value
  if (!pending || busy.value) {
    return
  }

  clearPendingShellConfirmation()
  // 发送 yes 确认
  await sendMessage('yes')
}

const cancelShellCommand = async () => {
  const pending = pendingShellConfirmation.value
  if (!pending || busy.value) {
    return
  }

  clearPendingShellConfirmation()
  // 发送 no 取消
  await sendMessage('no')
}

const clearPendingCommandTimer = () => {
  if (pendingCommandTimer !== null) {
    window.clearTimeout(pendingCommandTimer)
    pendingCommandTimer = null
  }
}

const clearPendingCommandConfirmation = () => {
  clearPendingCommandTimer()
  pendingCommandConfirmation.value = null
}

const handlePendingCommandExpiry = () => {
  const pending = pendingCommandConfirmation.value
  if (!pending || !isPendingCommandExpired(pending)) {
    return
  }

  clearPendingCommandConfirmation()
  announce('这次命令确认已超时，请重新输入命令。', 'guarded')
}

const setPendingCommandConfirmation = (nextPending: PendingCommandConfirmation | null) => {
  clearPendingCommandTimer()
  pendingCommandConfirmation.value = nextPending

  if (!nextPending) {
    return
  }

  composerVisible.value = true
  const ttl = Math.max(0, nextPending.expiresAt - Date.now())
  pendingCommandTimer = window.setTimeout(() => {
    handlePendingCommandExpiry()
  }, ttl)
}

const getPendingCommandConfirmation = () => {
  const pending = pendingCommandConfirmation.value
  if (!pending) {
    return null
  }

  if (isPendingCommandExpired(pending)) {
    handlePendingCommandExpiry()
    return null
  }

  return pending
}

const setPendingApproval = (approvalRequest: ActionApprovalRequest | null | undefined) => {
  if (!approvalRequest) {
    clearPendingApproval()
    return
  }

  clearPendingCommandConfirmation()
  pendingApproval.value = approvalRequest
  approvalPhrase.value = ''
  approvalChecks.value = Object.fromEntries(
    approvalRequest.checks.map((check) => [check.id, false])
  )
}

const toggleApprovalCheck = (checkId: string, checked: boolean) => {
  approvalChecks.value = {
    ...approvalChecks.value,
    [checkId]: checked
  }
}

const persistSettings = async (draft: ProviderConfigInput) => {
  const nextDraft = JSON.parse(JSON.stringify(draft)) as ProviderConfigInput

  if (nextDraft.kind === 'codexCli') {
    nextDraft.authMode = 'oauth'
    nextDraft.baseUrl = null
    nextDraft.oauthAuthorizeUrl = null
    nextDraft.oauthTokenUrl = null
    nextDraft.oauthClientId = null
    nextDraft.oauthScopes = ''
    nextDraft.oauthRedirectUrl = DEFAULT_OAUTH_REDIRECT_URL
  } else {
    nextDraft.authMode = 'apiKey'
    nextDraft.oauthAuthorizeUrl = null
    nextDraft.oauthTokenUrl = null
    nextDraft.oauthClientId = null
    nextDraft.oauthScopes = ''
    nextDraft.clearOAuthToken = true
  }

  if (!nextDraft.model.trim()) {
    nextDraft.model = providerDefaults[nextDraft.kind]
  }

  nextDraft.pushToTalkShortcut =
    nextDraft.pushToTalkShortcut?.trim() || DEFAULT_PUSH_TO_TALK_SHORTCUT
  nextDraft.workspaceRoot = nextDraft.workspaceRoot?.trim() || null

  if (!nextDraft.visionChannel.model.trim()) {
    nextDraft.visionChannel.model =
      nextDraft.visionChannel.kind === 'openAiCompatible' ? 'gpt-4.1-mini' : 'gpt-4.1-mini'
  }

  if (nextDraft.visionChannel.kind === 'disabled' || !nextDraft.visionChannel.enabled) {
    nextDraft.visionChannel.enabled = false
    nextDraft.visionChannel.baseUrl = null
  } else if (nextDraft.visionChannel.kind === 'openAi') {
    nextDraft.visionChannel.baseUrl = null
  }

  return saveProviderConfig(nextDraft)
}

const isCurrentModelEntry = (entry: { kind: ProviderKind; model: string; baseUrl: string | null }) =>
  snapshot.value.provider.kind === entry.kind &&
  snapshot.value.provider.model === entry.model &&
  snapshot.value.provider.baseUrl === entry.baseUrl

const buildModelListText = () => {
  const lines = modelCatalog.map((entry) => {
    const marker = isCurrentModelEntry(entry) ? '• 当前' : '•'
    return `${marker} ${entry.id}：${entry.label}`
  })

  return `可切换模型：\n${lines.join('\n')}\n\n使用 /model set <name> 切换。`
}

const buildHistorySummaryText = () => {
  const recentInputs = inputHistory.value.slice(-3)
  const inputSummary =
    recentInputs.length > 0
      ? recentInputs
          .map((item, index) => `${index + 1}. ${item}`)
          .join('\n')
      : '暂无已发送输入。'

  return `输入历史：${inputHistory.value.length} 条\n今日回复历史：${todayReplyHistory.value.length} 条\n\n最近输入：\n${inputSummary}\n\n完整今日回复历史可在设置窗口查看。`
}

const applyModelCatalogEntry = async (modelId: string) => {
  const entry = findModelCatalogEntry(modelId)
  if (!entry) {
    announce('未找到待切换的模型配置，请重新执行 /model set。', 'guarded')
    return
  }

  const nextDraft = toDraft(snapshot.value)
  nextDraft.kind = entry.kind
  nextDraft.model = entry.model
  nextDraft.baseUrl = entry.baseUrl
  nextDraft.authMode = entry.authMode
  nextDraft.clearApiKey = false
  nextDraft.clearOAuthToken = false

  const nextSnapshot = await persistSettings(nextDraft)
  await syncSnapshot(nextSnapshot)
  const authHint =
    entry.authMode === 'apiKey' && !nextSnapshot.provider.apiKeyLoaded
      ? ' 当前还没有加载 API Key，如需实际调用外部模型，还要去设置页补充密钥。'
      : ''

  announce(`已切换到 ${entry.label}。${authHint}`)
}

const executePendingCommand = async (pending: PendingCommandConfirmation) => {
  busy.value = true
  visualMode.value = 'guarded'

  try {
    if (pending.kind === 'modelSet') {
      await applyModelCatalogEntry(pending.payload.modelId)
      return
    }

    if (pending.kind === 'clearConversation') {
      await resetConversation(true)
      return
    }
  } catch (error) {
    announce(error instanceof Error ? error.message : '命令执行失败', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const confirmPendingCommand = async () => {
  const pending = getPendingCommandConfirmation()
  if (!pending || busy.value) {
    return
  }

  clearPendingCommandConfirmation()
  await executePendingCommand(pending)
}

const cancelPendingCommand = () => {
  const pending = getPendingCommandConfirmation()
  if (!pending) {
    return
  }

  clearPendingCommandConfirmation()
  announce('本次命令已取消。', 'guarded')
}

const resolvePendingDecisionInput = (content: string) => {
  const normalized = content.trim().toLowerCase()
  if (normalized === '/confirm') {
    return 'confirm' as const
  }

  if (normalized === '/cancel') {
    return 'cancel' as const
  }

  return resolvePendingCommandInput(content)
}

const handlePendingCommandInput = async (content: string) => {
  const pending = getPendingCommandConfirmation()
  if (!pending) {
    return false
  }

  const decision = resolvePendingDecisionInput(content)
  if (decision === 'confirm') {
    await confirmPendingCommand()
    return true
  }

  if (decision === 'cancel') {
    cancelPendingCommand()
    return true
  }

  announce('当前有待确认命令，请先输入 yes / no，或点击确认 / 取消。', 'guarded')
  return true
}

const selectLatestControlPending = (pendingList: ControlPendingRequest[]) =>
  [...pendingList].sort((left, right) => right.createdAt - left.createdAt)[0] ?? null

const refreshControlPendingRequests = async () => {
  const pendingList = await listControlPending()
  setControlPendingRequest(selectLatestControlPending(pendingList))
  return pendingList
}

const ensureActiveControlPending = async () => {
  const current = getControlPendingRequest()
  if (current) {
    return current
  }

  const pendingList = await refreshControlPendingRequests()
  return selectLatestControlPending(pendingList)
}

const summarizeControlPendingList = (pendingList: ControlPendingRequest[]) => {
  if (pendingList.length === 0) {
    return '当前没有本地控制待确认请求。'
  }

  const lines = pendingList
    .sort((left, right) => right.createdAt - left.createdAt)
    .slice(0, 6)
    .map((pending, index) => {
      const seconds = Math.max(0, Math.ceil((pending.expiresAt - Date.now()) / 1000))
      return `${index + 1}. ${pending.tool} · ${pending.id}\n${pending.prompt}\n剩余约 ${seconds} 秒`
    })

  const remainder =
    pendingList.length > 6 ? `\n\n其余 ${pendingList.length - 6} 条请继续用 /pending list 查看。` : ''

  return `当前本地控制待确认：${pendingList.length} 条\n\n${lines.join('\n\n')}${remainder}`
}

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null

const asArray = (value: unknown): unknown[] => (Array.isArray(value) ? value : [])

const asAgentTaskProgress = (value: unknown): AgentTaskProgress | null => {
  const record = asRecord(value)
  if (!record) {
    return null
  }

  const taskId = typeof record.taskId === 'string' ? record.taskId : ''
  const taskTitle = typeof record.taskTitle === 'string' ? record.taskTitle : ''
  const stepIndex = typeof record.stepIndex === 'number' ? record.stepIndex : 0
  const stepCount = typeof record.stepCount === 'number' ? record.stepCount : 0
  const rawStatus = typeof record.status === 'string' ? record.status : null
  const status =
    rawStatus &&
    ['running', 'waitingConfirmation', 'completed', 'failed', 'cancelled'].includes(rawStatus)
      ? (rawStatus as AgentTaskProgress['status'])
      : null

  if (!taskId || !taskTitle || !stepCount || !status) {
    return null
  }

  return {
    taskId,
    taskTitle,
    stepIndex,
    stepCount,
    status,
    stepSummary: typeof record.stepSummary === 'string' ? record.stepSummary : null,
    detail: typeof record.detail === 'string' ? record.detail : null
  }
}

const extractAgentTaskProgress = (response: ControlToolInvokeResponse) => {
  const record = asRecord(response.result)
  return asAgentTaskProgress(record?.task)
}

const setAgentTaskProgress = (nextTask: AgentTaskProgress | null) => {
  clearAgentTaskTimer()
  agentTaskProgress.value = nextTask

  if (!nextTask) {
    return
  }

  if (nextTask.status === 'completed' || nextTask.status === 'cancelled') {
    agentTaskTimer = window.setTimeout(() => {
      agentTaskProgress.value = null
      void syncPetWindowFrame().then(() => syncBubbleWindow())
    }, 4200)
    return
  }

  if (nextTask.status === 'failed') {
    agentTaskTimer = window.setTimeout(() => {
      agentTaskProgress.value = null
      void syncPetWindowFrame().then(() => syncBubbleWindow())
    }, 7200)
  }
}

const buildWindowListText = (result: unknown) => {
  const windows = asArray(result)
    .map((item) => asRecord(item))
    .filter((item): item is Record<string, unknown> => Boolean(item))
    .map((item) => ({
      title: typeof item.title === 'string' ? item.title.trim() : '',
      isActive: Boolean(item.isActive)
    }))
    .filter((item) => item.title)

  if (windows.length === 0) {
    return '当前没有读到可见窗口。'
  }

  const preview = windows.slice(0, 10).map((item, index) => {
    const marker = item.isActive ? '• 当前' : '•'
    return `${index + 1}. ${marker} ${item.title}`
  })
  const remainder =
    windows.length > 10 ? `\n\n其余 ${windows.length - 10} 个窗口已省略。` : ''

  return `当前可见窗口：${windows.length} 个\n\n${preview.join('\n')}${remainder}`
}

const buildClipboardReadText = (result: unknown) => {
  const record = asRecord(result)
  const text = typeof record?.text === 'string' ? record.text : ''
  if (!text) {
    return '剪贴板当前没有文本内容。'
  }

  const preview = text.length > 240 ? `${text.slice(0, 240)}…` : text
  return `剪贴板文本：\n${preview}`
}

const summarizeControlSuccess = (
  tool: string,
  response: ControlToolInvokeResponse,
  fallbackLabel?: string
) => {
  if (response.message?.trim()) {
    return response.message
  }

  const result = response.result
  const record = asRecord(result)

  switch (tool) {
    case 'list_windows':
      return buildWindowListText(result)
    case 'focus_window':
      return `已聚焦窗口：${typeof record?.title === 'string' ? record.title : fallbackLabel ?? '目标窗口'}。`
    case 'read_clipboard':
      return buildClipboardReadText(result)
    case 'type_text':
      return `已向当前活动窗口输入 ${typeof record?.typedLength === 'number' ? record.typedLength : 0} 个字符。`
    case 'send_hotkey':
      return `已发送快捷键：${typeof record?.sequence === 'string' ? record.sequence : '指定按键'}。`
    case 'click_at':
      return `已执行坐标点击：${typeof record?.button === 'string' ? record.button : 'left'}。`
    case 'cancel_pending':
      return '本次本地控制待确认请求已取消。'
    default:
      return fallbackLabel ? `${fallbackLabel} 已执行。` : '本地控制命令已执行。'
  }
}

const summarizeControlOutcome = (
  tool: string,
  response: ControlToolInvokeResponse,
  fallbackLabel?: string
) => {
  if (response.status === 'error') {
    return (
      response.message?.trim() ||
      response.error?.detail ||
      response.error?.message ||
      '本地控制命令执行失败。'
    )
  }

  return summarizeControlSuccess(tool, response, fallbackLabel)
}

const ensureControlCommandAvailable = () => {
  if (pendingApproval.value) {
    announce('当前还有桌面动作待确认，请先处理那个确认面板。', 'guarded')
    return false
  }

  if (getPendingCommandConfirmation()) {
    announce('当前还有 slash command 待确认，请先输入 yes / no，或点击确认 / 取消。', 'guarded')
    return false
  }

  return true
}

const ensureControlServiceReady = async () => {
  try {
    const status = await getControlServiceStatus()
    if (!status.running || !status.baseUrl) {
      announce(status.message || '本地控制服务未启动。', 'guarded')
      return false
    }

    return true
  } catch (error) {
    announce(error instanceof Error ? error.message : '本地控制服务状态检查失败。', 'guarded')
    return false
  }
}

const invokeSlashControlTool = async (
  tool: string,
  args: Record<string, unknown>,
  options?: {
    label?: string
  }
) => {
  if (!ensureControlCommandAvailable()) {
    return true
  }

  if (!(await ensureControlServiceReady())) {
    return true
  }

  busy.value = true
  visualMode.value = 'guarded'

  try {
    const response = await invokeControlTool(tool, args)
    setAgentTaskProgress(extractAgentTaskProgress(response))

    if (response.status === 'pending_confirmation' && response.pendingRequest) {
      setControlPendingRequest(response.pendingRequest)
      announce(
        `${response.pendingRequest.prompt} 请在 30 秒内输入 yes / no，或使用 /confirm /cancel。`,
        'guarded'
      )
      return true
    }

    clearControlPendingRequest()
    announce(summarizeControlOutcome(tool, response, options?.label), 'guarded')
    return true
  } catch (error) {
    announce(error instanceof Error ? error.message : '本地控制命令执行失败。', 'guarded')
    return true
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const confirmActiveControlPending = async () => {
  if (!(await ensureControlServiceReady())) {
    return
  }

  const pending = await ensureActiveControlPending()
  if (!pending || busy.value) {
    announce('当前没有可确认的本地控制请求。', 'guarded')
    return
  }

  busy.value = true
  visualMode.value = 'guarded'

  try {
    const response = await confirmControlPending(pending.id)
    setAgentTaskProgress(extractAgentTaskProgress(response))

    if (response.status === 'pending_confirmation' && response.pendingRequest) {
      setControlPendingRequest(response.pendingRequest)
    } else {
      clearControlPendingRequest()
      await refreshControlPendingRequests()
    }

    announce(summarizeControlOutcome(pending.tool, response), 'guarded')
  } catch (error) {
    announce(error instanceof Error ? error.message : '确认本地控制请求失败。', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const cancelActiveControlPending = async () => {
  if (!(await ensureControlServiceReady())) {
    return
  }

  const pending = await ensureActiveControlPending()
  if (!pending || busy.value) {
    announce('当前没有可取消的本地控制请求。', 'guarded')
    return
  }

  busy.value = true
  visualMode.value = 'guarded'

  try {
    const response = await cancelControlPending(pending.id)
    setAgentTaskProgress(extractAgentTaskProgress(response))
    clearControlPendingRequest()
    await refreshControlPendingRequests()
    announce(summarizeControlOutcome('cancel_pending', response), 'guarded')
  } catch (error) {
    announce(error instanceof Error ? error.message : '取消本地控制请求失败。', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const handleControlPendingInput = async (content: string) => {
  let pending = getControlPendingRequest()
  const decision = resolvePendingDecisionInput(content)

  if (!pending) {
    if (decision === 'blocked') {
      return false
    }

    try {
      pending = await ensureActiveControlPending()
    } catch {
      return false
    }
  }

  if (!pending) {
    return false
  }

  if (decision === 'confirm') {
    await confirmActiveControlPending()
    return true
  }

  if (decision === 'cancel') {
    await cancelActiveControlPending()
    return true
  }

  announce('当前有本地控制待确认，请先输入 yes / no，或使用 /confirm /cancel。', 'guarded')
  return true
}

const beginPendingCommandConfirmation = (pending: PendingCommandConfirmation) => {
  setPendingCommandConfirmation(pending)
  announce(
    `${pending.prompt} 请在 ${Math.round(COMMAND_CONFIRMATION_TIMEOUT_MS / 1000)} 秒内输入 yes / no，或点击确认 / 取消。`,
    'guarded'
  )
}

const refreshMicrophoneAvailability = async (requestPermission = false) => {
  if (typeof navigator === 'undefined' || !navigator.mediaDevices?.enumerateDevices) {
    microphoneAvailable.value = false
    return false
  }

  const detect = async () => {
    const devices = await navigator.mediaDevices.enumerateDevices()
    return devices.some((device) => device.kind === 'audioinput')
  }

  try {
    let available = await detect()

    if (
      !available &&
      requestPermission &&
      !microphonePermissionRequested &&
      navigator.mediaDevices.getUserMedia
    ) {
      microphonePermissionRequested = true
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      stream.getTracks().forEach((track) => track.stop())
      available = await detect()
    }

    microphoneAvailable.value = available
    return available
  } catch {
    microphoneAvailable.value = false
    return false
  }
}

const setupMediaDeviceWatcher = () => {
  if (typeof navigator === 'undefined' || !navigator.mediaDevices?.addEventListener) {
    mediaDevicesCleanup = null
    return
  }

  const onDeviceChange = () => {
    void refreshMicrophoneAvailability().then(() => {
      scheduleAutoListening(260)
    })
  }

  navigator.mediaDevices.addEventListener('devicechange', onDeviceChange)
  mediaDevicesCleanup = () => {
    navigator.mediaDevices.removeEventListener('devicechange', onDeviceChange)
  }
}

const loadSnapshot = async () => {
  try {
    const loaded = await getAssistantSnapshot()
    applySnapshot(loaded)
    await refreshMemoryDashboard()
    if (loaded.research.enabled || isResearchView.value) {
      await refreshResearchBrief({ silent: true })
    } else {
      researchBrief.value = emptyResearchBrief()
      lastResearchAlertSignature = ''
    }
    if (loaded.autoCheckAppUpdate && !isNonPetWindowView.value) {
      void refreshAppUpdateStatus(true)
    }
    await maybeOpenResearchWindowOnStartup(loaded)
  } catch (error) {
    announce(
      error instanceof Error ? error.message : '加载助手状态失败，已保留本地默认配置。',
      'guarded'
    )
  }
}

const findDirectAction = (content: string) => {
  const normalized = normalizeCommand(content)

  return (
    snapshot.value.allowedActions.find((action) => {
      if (!localDirectActionIds.has(action.id)) {
        return false
      }
      const keywords = actionCommandMap[action.id] ?? [action.title]
      return keywords.some((keyword) => normalized.includes(normalizeCommand(keyword)))
    }) ?? null
  )
}

const openDrawer = async (section: SettingsSection) => {
  if (isSettingsView.value) {
    drawerSection.value = section
    return true
  }

  return openSettingsWindow(section)
}

const closeDrawer = async () => closeSettingsWindow()

const openResearchBriefWindow = async () => {
  try {
    if (snapshot.value.research.enabled || isResearchView.value) {
      await refreshResearchBrief({ silent: true })
    }
    const opened = await openResearchWindow()
    if (opened && researchBrief.value.enabled) {
      await acknowledgeResearchBriefState(researchBrief.value)
      lastResearchAlertSignature = researchBrief.value.alertFingerprint || lastResearchAlertSignature
    }
    return opened
  } catch (error) {
    announce(resolveErrorMessage(error, '打开投研简报窗口失败'), 'guarded')
    return false
  }
}

const closeResearchBriefWindow = async () => {
  try {
    return await closeResearchWindow()
  } catch (error) {
    if (!isResearchView.value) {
      announce(resolveErrorMessage(error, '关闭投研简报窗口失败'), 'guarded')
    }
    return false
  }
}

const hidePet = async () => {
  try {
    const hidden = await hideAssistantWindow()
    if (!hidden) {
      announce('当前不是 Tauri 运行时，已仅收起弹出的浮层。', 'guarded')
    }
  } catch (error) {
    announce(error instanceof Error ? error.message : '隐藏桌宠失败', 'guarded')
  }
}

const resetConversation = async (announceAfter = false) => {
  try {
    const nextSnapshot = await clearConversation()
    await syncSnapshot(nextSnapshot)
    clearPendingApproval()
    if (announceAfter) {
      announce('对话已经清空，重新回到默认陪伴状态。')
    }
  } catch (error) {
    announce(error instanceof Error ? error.message : '清空会话失败', 'guarded')
  }
}

const clearTodayHistoryRecords = async () => {
  try {
    await applyTodayReplyHistory(await clearTodayReplyHistory())
    announce('今日回复历史已经清空。', 'speaking')
  } catch (error) {
    announce(error instanceof Error ? error.message : '清空今日回复历史失败', 'guarded')
  }
}

const triggerAction = async (action: DesktopAction) => {
  if (busy.value) {
    return
  }

  busy.value = true
  visualMode.value = 'guarded'

  try {
    const result = await requestDesktopAction(action.id)
    await syncSnapshot(result.snapshot)
    setPendingApproval(result.approvalRequest)
    announce(result.message, result.approvalRequest ? 'guarded' : 'speaking')
  } catch (error) {
    announce(error instanceof Error ? error.message : '动作执行失败', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const handleActionTrigger = (action: DesktopAction) => {
  void triggerAction(action)
}

const maybeHandleLocalCommand = async (content: string) => {
  const normalized = normalizeCommand(content)
  if (!normalized) {
    return false
  }

  if (
    [
      '打开设置',
      '显示设置',
      '模型设置',
      '安全设置',
      '系统设置',
      '打开配置',
      'oauth设置',
      'oauth登录',
      'codex登录',
      'codex login',
      '登录codex'
    ].some((token) => normalized.includes(normalizeCommand(token)))
  ) {
    const opened = await openDrawer('settings')
    announce(
      opened
        ? '设置窗口已经打开，你可以在新窗口里调整模型、OAuth、安全边界和受控动作。'
        : '设置窗口打开失败，请检查当前运行环境。',
      opened ? 'speaking' : 'guarded'
    )
    return true
  }

  if (['关闭设置', '收起设置'].some((token) => normalized.includes(normalizeCommand(token)))) {
    const closed = await closeDrawer()
    announce(closed ? '设置窗口已经关闭。' : '当前没有打开的设置窗口。', closed ? 'speaking' : 'guarded')
    return true
  }

  if (
    ['打开动作面板', '显示动作面板', '受控动作', '动作面板', '打开动作', '动作设置'].some((token) =>
      normalized.includes(normalizeCommand(token))
    )
  ) {
    const opened = await openDrawer('actions')
    announce(
      opened ? '动作页已经在独立设置窗口中打开。' : '动作页打开失败，请检查当前运行环境。',
      opened ? 'speaking' : 'guarded'
    )
    return true
  }

  if (
    ['打开投研简报', '显示投研简报', '打开研究模式', '显示研究模式', '打开投研窗口'].some((token) =>
      normalized.includes(normalizeCommand(token))
    )
  ) {
    const opened = await openResearchBriefWindow()
    announce(
      opened ? '投研简报窗口已经打开。' : '投研简报窗口打开失败，请检查当前运行环境。',
      opened ? 'speaking' : 'guarded'
    )
    return true
  }

  if (
    ['关闭动作面板', '收起动作面板'].some((token) => normalized.includes(normalizeCommand(token)))
  ) {
    const closed = await closeDrawer()
    announce(closed ? '动作窗口已经关闭。' : '当前没有打开的动作窗口。', closed ? 'speaking' : 'guarded')
    return true
  }

  if (['关闭投研简报', '收起投研简报', '关闭研究窗口'].some((token) => normalized.includes(normalizeCommand(token)))) {
    const closed = await closeResearchBriefWindow()
    announce(closed ? '投研简报窗口已经关闭。' : '当前没有打开的投研简报窗口。', closed ? 'speaking' : 'guarded')
    return true
  }

  if (['清空对话', '清空会话', '重置会话'].some((token) => normalized.includes(normalizeCommand(token)))) {
    await resetConversation(true)
    return true
  }

  if (
    ['隐藏到托盘', '隐藏桌宠', '收起桌宠', '关闭桌宠', '最小化到托盘'].some((token) =>
      normalized.includes(normalizeCommand(token))
    )
  ) {
    await hidePet()
    return true
  }

  const directAction = findDirectAction(content)
  if (directAction) {
    await triggerAction(directAction)
    return true
  }

  return false
}

const handleSlashCommand = async (content: string) => {
  const parsed = parseSlashCommand(content)
  if (!parsed) {
    return false
  }

  if (!parsed.ok) {
    announce(parsed.message, 'guarded')
    return true
  }

  try {
    switch (parsed.command.kind) {
      case 'help':
        announce(slashHelpText, 'guarded')
        return true
      case 'windowsList':
        return invokeSlashControlTool('list_windows', {}, { label: '窗口列表' })
      case 'windowFocus':
        return invokeSlashControlTool(
          'focus_window',
          { title: parsed.command.title, match: 'contains' },
          { label: parsed.command.title }
        )
      case 'clipboardRead':
        return invokeSlashControlTool('read_clipboard', {}, { label: '剪贴板文本' })
      case 'controlPendingList': {
        if (!(await ensureControlServiceReady())) {
          return true
        }

        const pendingList = await refreshControlPendingRequests()
        announce(summarizeControlPendingList(pendingList), 'guarded')
        return true
      }
      case 'controlConfirm':
        await confirmActiveControlPending()
        return true
      case 'controlCancel':
        await cancelActiveControlPending()
        return true
      case 'controlType':
        return invokeSlashControlTool(
          'type_text',
          { text: parsed.command.text },
          { label: '输入文本' }
        )
      case 'controlHotkey':
        return invokeSlashControlTool(
          'send_hotkey',
          { keys: parsed.command.keys },
          { label: parsed.command.keys.join('+') }
        )
      case 'controlClick':
        return invokeSlashControlTool(
          'click_at',
          {
            x: parsed.command.x,
            y: parsed.command.y,
            button: parsed.command.button
          },
          { label: `${parsed.command.x}, ${parsed.command.y}` }
        )
      case 'modelCurrent':
        announce(
          `当前对话引擎：${activeProviderLabel.value}。\n模型标识：${snapshot.value.provider.model}。`,
          'guarded'
        )
        return true
      case 'modelList':
        announce(buildModelListText(), 'guarded')
        return true
      case 'modelSet': {
        const entry = findModelCatalogEntry(parsed.command.target)
        if (!entry) {
          announce(
            `未找到模型“${parsed.command.target}”。\n\n${buildModelListText()}`,
            'guarded'
          )
          return true
        }

        if (isCurrentModelEntry(entry)) {
          announce(`当前已经是 ${entry.label}。`, 'guarded')
          return true
        }

        if (pendingApproval.value) {
          announce('当前还有桌面动作待确认，请先处理那个确认流。', 'guarded')
          return true
        }

        beginPendingCommandConfirmation(createModelSetConfirmation(entry))
        return true
      }
      case 'history':
        announce(buildHistorySummaryText(), 'guarded')
        return true
      case 'clearConversation':
        if (pendingApproval.value) {
          announce('当前还有桌面动作待确认，请先处理那个确认流。', 'guarded')
          return true
        }

        beginPendingCommandConfirmation(createClearConversationConfirmation())
        return true
      case 'openSettings': {
        const opened = await openDrawer('settings')
        announce(
          opened ? '设置窗口已经打开。' : '设置窗口打开失败，请检查当前运行环境。',
          opened ? 'speaking' : 'guarded'
        )
        return true
      }
    }
  } catch (error) {
    announce(error instanceof Error ? error.message : 'Slash command 执行失败。', 'guarded')
    return true
  }
}

const sendMessage = async (value = messageDraft.value) => {
  const content = value.trim()
  if (!content || busy.value || isSettingsView.value) {
    return
  }

  messageDraft.value = ''
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
  if (listening.value) {
    if (useLocalWhisperInput.value) {
      void stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
    } else if (recognition) {
      submitVoiceAfterStop = false
      recognition.stop()
    }
  }
  clearAutoListenTimer()
  clearWhisperCaptureTimer()
  clearBubble()
  resetInputHistoryNavigation()

  if (await handleControlPendingInput(content)) {
    scheduleAutoListening(260)
    return
  }

  if (await handlePendingCommandInput(content)) {
    scheduleAutoListening(260)
    return
  }

  if (await handleSlashCommand(content)) {
    scheduleAutoListening(260)
    return
  }

  if (await maybeHandleLocalCommand(content)) {
    scheduleAutoListening(260)
    return
  }

  busy.value = true
  visualMode.value = 'thinking'

  try {
    const response = await sendChatMessage(content)
    pushInputHistoryLocally(content)
    applySnapshot(response.snapshot)
    await refreshTodayReplyHistory()
    await refreshMemoryDashboard()
    setAgentTaskProgress(
      response.agent && response.agent.route !== 'chat' ? (response.agent.task ?? null) : null
    )
    if (response.agent?.pendingRequest) {
      setControlPendingRequest(response.agent.pendingRequest)
    } else if (response.agent && response.agent.route !== 'chat') {
      clearControlPendingRequest()
    }
    // 处理 Shell Agent 待确认命令
    if (response.pendingShellConfirmation) {
      setPendingShellConfirmation(response.pendingShellConfirmation)
    }
    announce(
      response.reply.content,
      response.agent && response.agent.route !== 'chat' ? 'guarded' : 'speaking'
    )
  } catch (error) {
    announce(error instanceof Error ? error.message : '消息发送失败', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
    scheduleAutoListening(320)
  }
}

const pushWhisperStatus = async (status: WhisperStatus) => {
  whisperStatus.value = status
  await publishWhisperStatus(status)
}

const stopLocalWhisperListening = async ({
  shouldSend = false,
  silent = false,
  reschedule = true
}: {
  shouldSend?: boolean
  silent?: boolean
  reschedule?: boolean
} = {}) => {
  const recordingState = whisperStatus.value.recordingState
  if (recordingState !== 'recording' && !listening.value) {
    return
  }

  clearWhisperCaptureTimer()
  listening.value = false
  visualMode.value = 'thinking'
  await pushWhisperStatus({
    ...whisperStatus.value,
    recordingState: 'processing'
  })

  let sentTranscript = false

  try {
    const result = await stopWhisperRecording()
    const transcript = result.text.trim()
    await pushWhisperStatus({
      ...whisperStatus.value,
      recordingState: 'idle'
    })

    if (transcript) {
      if (shouldSend) {
        messageDraft.value = transcript
        sentTranscript = true
        await sendMessage(transcript)
        return
      }

      if (!silent) {
        messageDraft.value = transcript
      }
    } else if (!silent) {
      announce('这次没有识别到清晰的语音内容。', 'guarded')
    }
  } catch (error) {
    await refreshWhisperStatus()
    if (!silent) {
      announce(resolveErrorMessage(error, '本地 Whisper 停止录音失败'), 'guarded')
    }
  } finally {
    listening.value = false
    if (!sentTranscript) {
      resetVisualModeSoon(200)
      if (reschedule) {
        scheduleAutoListening(260)
      }
    }
  }
}

const startLocalWhisperListening = async (autoMode = false) => {
  if (whisperVoiceInputMode.value === 'disabled') {
    if (!autoMode) {
      announce('当前设置里已经关闭语音输入，请先在设置中改成常驻监听或按键说话。', 'guarded')
    }
    return
  }

  if (!voiceInputAvailable.value) {
    if (!autoMode) {
      announce(
        whisperStatus.value.modelLoaded
          ? whisperStatus.value.inputMessage || '当前麦克风输入还未就绪，请先检查录音设备或系统权限。'
          : '请先下载并加载一个 Whisper 模型，再启用本地语音输入。',
        'guarded'
      )
    }
    return
  }

  try {
    clearAutoListenTimer()
    clearWhisperCaptureTimer()
    if (voiceReplySupported.value) {
      window.speechSynthesis.cancel()
    }
    clearBubble()

    const nextState = await startWhisperRecording()
    await pushWhisperStatus({
      ...whisperStatus.value,
      recordingState: nextState
    })
    listening.value = true
    visualMode.value = 'listening'

    if (whisperVoiceInputMode.value === 'continuous') {
      whisperCaptureTimer = window.setTimeout(() => {
        void stopLocalWhisperListening({
          shouldSend: true,
          silent: true
        })
      }, WHISPER_CAPTURE_WINDOW_MS)
    }
  } catch (error) {
    listening.value = false
    await refreshWhisperStatus()
    if (!autoMode) {
      announce(resolveErrorMessage(error, '本地 Whisper 启动录音失败'), 'guarded')
    } else {
      scheduleAutoListening(1200)
    }
  }
}

const ensureRecognition = () => {
  if (!voiceInputAvailable.value || useLocalWhisperInput.value) {
    return null
  }

  if (recognition) {
    return recognition
  }

  const RecognitionCtor = window.SpeechRecognition ?? window.webkitSpeechRecognition
  if (!RecognitionCtor) {
    return null
  }

  recognition = new RecognitionCtor()
  recognition.lang = 'zh-CN'
  recognition.interimResults = true
  recognition.maxAlternatives = 1
  recognition.continuous = false

  recognition.onresult = (event: SpeechRecognitionEvent) => {
    const transcript = Array.from(event.results)
      .map((result) => result[0]?.transcript ?? '')
      .join('')
      .trim()

    recognitionBuffer = transcript
    messageDraft.value = transcript
  }

  recognition.onerror = (event: SpeechRecognitionErrorEvent) => {
    listening.value = false
    submitVoiceAfterStop = false
    announce(`语音识别失败：${event.error}`, 'guarded')
  }

  recognition.onend = () => {
    const transcript = recognitionBuffer.trim()
    const shouldSend = submitVoiceAfterStop && transcript.length > 0

    listening.value = false
    submitVoiceAfterStop = false

    if (shouldSend) {
      void sendMessage(transcript)
      return
    }

    resetVisualModeSoon(200)
    scheduleAutoListening(260)
  }

  return recognition
}

const startListening = async (autoMode = false) => {
  if (busy.value || listening.value || isSettingsView.value) {
    return
  }

  if (autoMode && !shouldAutoListen.value) {
    return
  }

  if (useLocalWhisperInput.value) {
    await startLocalWhisperListening(autoMode)
    return
  }

  let instance = ensureRecognition()
  if (!instance && speechRecognitionSupported.value) {
    await refreshMicrophoneAvailability(true)
    instance = ensureRecognition()
  }

  if (!instance) {
    if (!autoMode) {
      announce('当前没有检测到可用麦克风或语音识别环境，请改用文字输入。', 'guarded')
    }
    return
  }

  try {
    recognitionBuffer = ''
    submitVoiceAfterStop = autoMode
    listening.value = true
    visualMode.value = 'listening'
    if (voiceReplySupported.value) {
      window.speechSynthesis.cancel()
    }
    clearBubble()
    clearAutoListenTimer()
    instance.start()
  } catch {
    listening.value = false
    if (!autoMode) {
      announce('语音输入正在占用中，请稍后再试。', 'guarded')
    }
    scheduleAutoListening(420)
  }
}

const confirmPendingAction = async () => {
  if (!pendingApproval.value || busy.value) {
    return
  }

  busy.value = true
  visualMode.value = 'guarded'

  try {
    const acknowledgedChecks = pendingApproval.value.checks
      .filter((check) => approvalChecks.value[check.id])
      .map((check) => check.id)
    const result = await confirmDesktopAction(
      pendingApproval.value.id,
      approvalPhrase.value,
      acknowledgedChecks
    )
    await syncSnapshot(result.snapshot)
    clearPendingApproval()
    announce(result.message)
  } catch (error) {
    announce(error instanceof Error ? error.message : '动作确认失败', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
  }
}

const cancelPendingAction = async () => {
  if (!pendingApproval.value) {
    return
  }

  try {
    const nextSnapshot = await cancelDesktopActionApproval(pendingApproval.value.id)
    await syncSnapshot(nextSnapshot)
    announce('本次动作授权已取消。', 'guarded')
  } catch (error) {
    announce(error instanceof Error ? error.message : '取消动作授权失败', 'guarded')
  } finally {
    clearPendingApproval()
  }
}

const saveSettings = async (draft: ProviderConfigInput) => {
  savingSettings.value = true

  try {
    const nextSnapshot = await persistSettings(draft)
    await syncSnapshot(nextSnapshot)
    if (nextSnapshot.research.enabled || isResearchView.value) {
      await refreshResearchBrief({ silent: true })
    } else {
      researchBrief.value = emptyResearchBrief()
      lastResearchAlertSignature = ''
    }
    if (nextSnapshot.autoCheckAppUpdate) {
      void refreshAppUpdateStatus(true)
    } else if (!appUpdateBusy.value) {
      appUpdateStatus.value = {
        ...appUpdateStatus.value,
        message: '已关闭启动时自动检查软件更新。'
      }
    }
    announce(`设置已经保存，当前对话引擎：${providerLabels[nextSnapshot.provider.kind]}。`)
  } catch (error) {
    announce(error instanceof Error ? error.message : '保存配置失败', 'guarded')
  } finally {
    savingSettings.value = false
  }
}

const refreshCodexLoginStatus = async (silent = false) => {
  try {
    const status = await getCodexCliStatus()
    codexStatus.value = status
    oauthNotice.value = status.message
    if (!silent) {
      announce(status.message, status.loggedIn ? 'idle' : 'guarded')
    }
  } catch (error) {
    const message = resolveErrorMessage(error, '刷新 Codex 登录状态失败')
    oauthNotice.value = message
    if (!silent) {
      announce(message, 'guarded')
    }
  }
}

const restartCodexLogin = async () => {
  authBusy.value = true
  oauthNotice.value = '正在清理旧凭据并启动 codex login...'

  try {
    const status = await restartCodexCliLogin()
    codexStatus.value = status
    oauthNotice.value = status.message
    announce(status.message, 'idle')
  } catch (error) {
    const message = resolveErrorMessage(error, '重新启动 codex login 失败')
    oauthNotice.value = message
    announce(message, 'guarded')
  } finally {
    authBusy.value = false
  }
}

const refreshAppUpdateStatus = async (silent = false) => {
  appUpdateBusy.value = true
  try {
    const status = await checkAppUpdate()
    appUpdateStatus.value = status
    if (!silent) {
      announce(status.message, status.updateAvailable ? 'speaking' : 'idle')
    }
  } catch (error) {
    const message = resolveErrorMessage(error, '检查软件更新失败')
    appUpdateStatus.value = {
      ...appUpdateStatus.value,
      message
    }
    if (!silent) {
      announce(message, 'guarded')
    }
  } finally {
    appUpdateBusy.value = false
  }
}

const openSoftwareUpdateDownload = async () => {
  appUpdateBusy.value = true
  try {
    const status = await openAppUpdateDownload()
    appUpdateStatus.value = status
    announce(
      status.updateAvailable
        ? '已为你打开软件更新下载页。'
        : status.message,
      status.updateAvailable ? 'speaking' : 'idle'
    )
  } catch (error) {
    announce(resolveErrorMessage(error, '打开软件更新下载页失败'), 'guarded')
  } finally {
    appUpdateBusy.value = false
  }
}

const beginOAuthLogin = async (draft: ProviderConfigInput) => {
  if (draft.kind !== 'codexCli') {
    announce('请先把 Provider 切换到 Codex CLI，再执行一键登录。', 'guarded')
    return
  }

  authBusy.value = true
  oauthNotice.value = '正在启动 codex login...'

  try {
    const nextSnapshot = await persistSettings(draft)
    await syncSnapshot(nextSnapshot)
    const status = await startCodexCliLogin()
    codexStatus.value = status
    oauthNotice.value = status.message
    announce(`${status.message} 当前聊天已切换到 Codex CLI。`)
  } catch (error) {
    const message = resolveErrorMessage(error, '启动 codex login 失败')
    oauthNotice.value = message
    announce(message, 'guarded')
  } finally {
    authBusy.value = false
  }
}

const refreshWhisperStatus = async () => {
  try {
    await pushWhisperStatus(await getWhisperStatus())
  } catch {
    // 静默处理错误
  }
}

const handleMemoryRefresh = async () => {
  memoryBusy.value = true
  try {
    await refreshMemoryDashboard()
    announce('记忆管理面板已刷新。', 'idle')
  } catch (error) {
    announce(resolveErrorMessage(error, '刷新记忆管理面板失败'), 'guarded')
  } finally {
    memoryBusy.value = false
  }
}

const handleMemoryDelete = async (kind: ManagedMemoryKind, id: string) => {
  memoryBusy.value = true
  try {
    memoryDashboard.value = await deleteManagedMemory(kind, id)
    announce('记忆条目已删除。', 'idle')
  } catch (error) {
    announce(resolveErrorMessage(error, '删除记忆失败'), 'guarded')
  } finally {
    memoryBusy.value = false
  }
}

const handleMemoryPromote = async (id: string) => {
  memoryBusy.value = true
  try {
    memoryDashboard.value = await promoteMemoryCandidate(id)
    announce('候选记忆已提升为长期记忆。', 'idle')
  } catch (error) {
    announce(resolveErrorMessage(error, '提升候选记忆失败'), 'guarded')
  } finally {
    memoryBusy.value = false
  }
}

const handleMemoryResolve = async (
  kind: ManagedMemoryKind,
  group: string,
  keepId: string
) => {
  memoryBusy.value = true
  try {
    memoryDashboard.value = await resolveMemoryConflict(kind, group, keepId)
    announce('记忆冲突已处理。', 'idle')
  } catch (error) {
    announce(resolveErrorMessage(error, '处理记忆冲突失败'), 'guarded')
  } finally {
    memoryBusy.value = false
  }
}

const handleWhisperPushToTalkEvent = async (event: WhisperPushToTalkEvent) => {
  if (isNonPetWindowView.value) {
    return
  }

  if (!useLocalWhisperInput.value || whisperVoiceInputMode.value !== 'pushToTalk') {
    return
  }

  if (event.state === 'pressed') {
    await startListening(false)
    return
  }

  await stopLocalWhisperListening({
    shouldSend: true,
    silent: true
  })
}

const handleWhisperDownload = async (model: WhisperModel) => {
  if (whisperDownloading.value) return

  whisperDownloading.value = true
  whisperDownloadProgress.value = {
    model,
    downloadedBytes: 0,
    totalBytes: 0,
    progressPercent: 0
  }

  try {
    await downloadWhisperModel(model, (progress) => {
      whisperDownloadProgress.value = progress
    })
    await refreshWhisperStatus()
    announce(`Whisper ${model} 模型下载完成`)
  } catch (error) {
    announce(resolveErrorMessage(error, '模型下载失败'), 'guarded')
  } finally {
    whisperDownloading.value = false
    whisperDownloadProgress.value = null
  }
}

const handleWhisperLoad = async (model: WhisperModel) => {
  try {
    await pushWhisperStatus(await loadWhisperModel(model))
    scheduleAutoListening(260)
    announce(`已加载 Whisper ${model} 模型`)
  } catch (error) {
    announce(resolveErrorMessage(error, '加载模型失败'), 'guarded')
  }
}

const handleWhisperUnload = async () => {
  try {
    if (listening.value && useLocalWhisperInput.value) {
      await stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
    }
    await pushWhisperStatus(await unloadWhisperModel())
    announce('已卸载 Whisper 模型')
  } catch (error) {
    announce(resolveErrorMessage(error, '卸载模型失败'), 'guarded')
  }
}

const handleWhisperDelete = async (model: WhisperModel) => {
  try {
    if (whisperStatus.value.currentModel === model && listening.value && useLocalWhisperInput.value) {
      await stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
    }
    await pushWhisperStatus(await deleteWhisperModel(model))
    announce(`已删除 Whisper ${model} 模型`)
  } catch (error) {
    announce(resolveErrorMessage(error, '删除模型失败'), 'guarded')
  }
}

const handleInputFocus = () => {
  if (dockState.value !== 'normal') {
    void restoreDockedPet(false)
  }

  clearPetDockTimer()
  composerVisible.value = true
  textInputFocused.value = true
  clearAutoListenTimer()
  clearWhisperCaptureTimer()

  if (listening.value) {
    if (useLocalWhisperInput.value) {
      void stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
    } else if (recognition) {
      submitVoiceAfterStop = false
      recognition.stop()
    }
  }
}

const handleInputBlur = () => {
  textInputFocused.value = false
  if (!messageDraft.value.trim() && !busy.value) {
    composerVisible.value = false
    void syncPetWindowFrame()
  }
  scheduleAutoListening(320)
  schedulePetDockedIdle()
}

const setupPetWindowListeners = async () => {
  if (!isTauriDesktop() || isNonPetWindowView.value) {
    return
  }

  const appWindow = getCurrentWindow()
  windowMovedCleanup = await appWindow.onMoved(() => {
    cursorPassthroughSuspendUntil = Date.now() + 900
    void syncBubbleWindow()
    if (dockState.value === 'normal') {
      schedulePetWindowClamp()
      schedulePersistMainWindowPosition()
    }
    schedulePetDockedIdle()
  })
  windowResizedCleanup = await appWindow.onResized(() => {
    cursorPassthroughSuspendUntil = Date.now() + 900
    if (dockState.value === 'normal') {
      void clampPetWindowToMonitor()
    }
    schedulePetDockedIdle()
  })
}

const setupCrossWindowListeners = async () => {
  if (isBubbleView.value) {
    bubbleStateListenerCleanup = await listenForBubbleWindowState((nextState) => {
      bubbleWindowState.value = nextState
    })
    return
  }

  snapshotListenerCleanup = await listenForAssistantSnapshot((nextSnapshot) => {
    applySnapshot(nextSnapshot)
    if (nextSnapshot.research.enabled || researchBrief.value.enabled || isResearchView.value) {
      void refreshResearchBrief({ silent: true })
    }
  })

  bubbleInteractionListenerCleanup = await listenForBubbleInteractionState((active) => {
    handleBubbleInteractionState(active)
  })

  bubbleLayoutMetricsListenerCleanup = await listenForBubbleLayoutMetrics((metrics) => {
    applyBubbleLayoutMetrics(metrics)
  })

  bubbleDismissRequestCleanup = await listenForBubbleDismissRequest((messageId) => {
    handleBubbleDismissRequest(messageId)
  })

  todayReplyHistoryListenerCleanup = await listenForTodayReplyHistory((entries) => {
    todayReplyHistory.value = entries
  })

  whisperStatusListenerCleanup = await listenForWhisperStatus((status) => {
    whisperStatus.value = status
  })

  whisperPushToTalkCleanup = await listenForWhisperPushToTalk((event) => {
    void handleWhisperPushToTalkEvent(event)
  })

  if (isSettingsView.value) {
    sectionListenerCleanup = await listenForSettingsSectionChange((section) => {
      drawerSection.value = section
    })
  }
}

watch(showComposer, (visible, previousVisible) => {
  if (isNonPetWindowView.value) {
    return
  }

  if (visible && !previousVisible) {
    void nextTick(() => {
      inputBoxRef.value?.focusComposer()
    })
  }
})

watch(
  () => [
    showComposer.value,
    showAgentTaskStrip.value,
    hasControlPending.value,
    hasPendingCommand.value
  ],
  () => {
    if (isNonPetWindowView.value) {
      return
    }

    void syncPetWindowFrame().then(() => syncBubbleWindow())
  }
)

watch(
  shouldEnterDockedIdle,
  (canDock) => {
    if (!isTauriDesktop()) {
      return
    }

    if (canDock) {
      schedulePetDockedIdle()
      return
    }

    clearPetDockTimer()
  },
  { immediate: false }
)

watch(
  canEnterDockedIdle,
  (eligible) => {
    if (!eligible && dockState.value !== 'normal') {
      void restoreDockedPet()
    }
  },
  { immediate: false }
)

watch(
  () => bubbleText.value,
  () => {
    void syncBubbleWindow()
  }
)

watch(
  () => snapshot.value.messages.length,
  (nextLength, previousLength) => {
    if (nextLength !== previousLength) {
      void refreshTodayReplyHistory()
      void refreshMemoryDashboard()
    }
  }
)

watch(
  () => [whisperVoiceInputMode.value, voiceInputAvailable.value, isNonPetWindowView.value],
  ([mode, available, nonPetView]) => {
    if (!useLocalWhisperInput.value) {
      return
    }

    if (nonPetView || mode !== 'continuous' || !available) {
      clearAutoListenTimer()
      if (mode !== 'continuous' && listening.value) {
        void stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
      }
      return
    }

    scheduleAutoListening(260)
  }
)

onMounted(() => {
  if (isBubbleView.value) {
    void setupCrossWindowListeners()
    return
  }

  void loadInputHistory()
  void refreshTodayReplyHistory()
  void loadSnapshot()
  void refreshMemoryDashboard()
  void refreshCodexLoginStatus(true)
  void refreshWhisperStatus()
  void refreshMicrophoneAvailability(!isNonPetWindowView.value).then(() => {
    scheduleAutoListening(420)
  })
  if (!isNonPetWindowView.value) {
    setupMediaDeviceWatcher()
  }
  void setupCrossWindowListeners()
  if (!isNonPetWindowView.value) {
    void syncPetWindowFrame().then(() => syncBubbleWindow())
    void setupPetWindowListeners()
    setupPetHitTestLoop()
    schedulePetDockedIdle()
  }
})

onBeforeUnmount(() => {
  recognition?.stop()
  resetBubbleDismissState()
  clearAutoListenTimer()
  clearWhisperCaptureTimer()
  clearPetClampTimer()
  clearPetDockTimer()
  clearPetHitTestTimer()
  clearResearchStartupRetryTimer()
  clearPersistWindowPositionTimer()
  clearControlPendingTimer()
  clearShellConfirmationTimer()
  clearPendingCommandTimer()
  clearAgentTaskTimer()
  mediaDevicesCleanup?.()
  snapshotListenerCleanup?.()
  sectionListenerCleanup?.()
  bubbleStateListenerCleanup?.()
  bubbleInteractionListenerCleanup?.()
  bubbleLayoutMetricsListenerCleanup?.()
  bubbleDismissRequestCleanup?.()
  todayReplyHistoryListenerCleanup?.()
  whisperStatusListenerCleanup?.()
  whisperPushToTalkCleanup?.()
  windowMovedCleanup?.()
  windowResizedCleanup?.()
  if (isTauriDesktop() && cursorPassthroughEnabled) {
    void getCurrentWindow().setIgnoreCursorEvents(false)
  }
  if (useLocalWhisperInput.value && whisperStatus.value.recordingState === 'recording') {
    void stopLocalWhisperListening({ shouldSend: false, silent: true, reschedule: false })
  }
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
})
</script>

<template>
  <div v-if="isSettingsView" class="settings-window-shell">
    <SettingsDrawer
      :section="drawerSection"
      :draft="settingsDraft"
      :saving="savingSettings"
      :voice-input-available="voiceInputAvailable"
      :oauth-busy="authBusy"
      :oauth-notice="oauthNotice"
      :codex-status="codexStatus"
      :app-update-status="appUpdateStatus"
      :app-update-busy="appUpdateBusy"
      :current-provider-label="activeProviderLabel"
      :vision-channel-status="snapshot.visionChannelStatus"
      :actions="snapshot.allowedActions"
      :permission-level="snapshot.permissionLevel"
      :ai-constraints="snapshot.aiConstraints"
      :today-reply-history="todayReplyHistory"
      :memory-dashboard="memoryDashboard"
      :memory-busy="memoryBusy"
      :whisper-status="whisperStatus"
      :whisper-downloading="whisperDownloading"
      :whisper-download-progress="whisperDownloadProgress"
      @close="closeDrawer"
      @save="saveSettings"
      @section-change="drawerSection = $event"
      @oauth-start="beginOAuthLogin"
      @codex-relogin="restartCodexLogin"
      @codex-refresh="refreshCodexLoginStatus()"
      @app-update-check="refreshAppUpdateStatus()"
      @app-update-open="openSoftwareUpdateDownload"
      @open-research="openResearchBriefWindow"
      @memory-refresh="handleMemoryRefresh"
      @memory-delete="handleMemoryDelete"
      @memory-promote="handleMemoryPromote"
      @memory-resolve="handleMemoryResolve"
      @whisper-download="handleWhisperDownload"
      @whisper-load="handleWhisperLoad"
      @whisper-unload="handleWhisperUnload"
      @whisper-delete="handleWhisperDelete"
      @trigger-action="handleActionTrigger"
      @clear-today-history="clearTodayHistoryRecords"
    />

    <transition name="confirm">
      <div v-if="pendingApproval" class="confirm-shell settings-confirm-shell">
        <section class="confirm-panel">
          <p class="eyebrow dark">One-Time Approval</p>
          <h2>{{ pendingApproval.action.title }}</h2>
          <p>{{ pendingApproval.prompt }}</p>

          <div class="approval-list">
            <label
              v-for="check in pendingApproval.checks"
              :key="check.id"
              class="approval-check"
            >
              <input
                type="checkbox"
                :checked="Boolean(approvalChecks[check.id])"
                @change="toggleApprovalCheck(check.id, ($event.target as HTMLInputElement).checked)"
              />
              <span>{{ check.label }}</span>
            </label>
          </div>

          <label class="approval-field">
            <span>输入确认短语</span>
            <input
              :value="approvalPhrase"
              :placeholder="pendingApproval.requiredPhrase"
              @input="approvalPhrase = ($event.target as HTMLInputElement).value"
            />
          </label>

          <p class="approval-expiry">
            该授权短语两分钟内有效：<strong>{{ pendingApproval.requiredPhrase }}</strong>
          </p>

          <div class="confirm-actions">
            <button type="button" class="panel-chip muted" @click="cancelPendingAction">
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
              :disabled="!canSubmitApproval"
              @click="confirmPendingAction"
            >
              我确认执行
            </button>
          </div>
        </section>
      </div>
    </transition>
  </div>

  <div v-else-if="isResearchView" class="research-window-shell">
    <ResearchBriefWindow
      :brief="researchBrief"
      :loading="researchBriefBusy"
      @close="closeResearchBriefWindow"
      @refresh="refreshResearchBrief()"
    />
  </div>

  <div v-else-if="isBubbleView" class="bubble-shell">
    <FloatingBubble :state="bubbleWindowState" />
  </div>

  <div v-else class="app-shell">
    <div class="pet-stack">
      <Penguin
        ref="penguinRef"
        :mode="activeMode"
        :dock-state="dockState"
        @activate="revealComposer"
        @interact="handlePetInteract"
      />

      <transition name="composer">
        <InputBox
          v-if="showComposer"
          ref="inputBoxRef"
          v-model="messageDraft"
          :busy="busy"
          @send="sendMessage()"
          @focus="handleInputFocus"
          @blur="handleInputBlur"
          @history-up="recallOlderInput"
          @history-down="recallNewerInput"
        />
      </transition>

      <transition name="composer">
        <section v-if="showAgentTaskStrip" :class="agentTaskToneClass">
          <div class="command-confirm-copy">
            <p class="eyebrow dark">Desktop Task</p>
            <strong>{{ agentTaskProgress?.taskTitle }}</strong>
            <p>{{ agentTaskStepLabel }} · {{ agentTaskStatusLabel }}</p>
            <p v-if="agentTaskProgress?.stepSummary" class="task-step-summary">
              当前步骤：{{ agentTaskProgress.stepSummary }}
            </p>
            <span v-if="agentTaskProgress?.detail" class="command-confirm-hint">
              {{ agentTaskProgress.detail }}
            </span>
          </div>
        </section>
      </transition>

      <transition name="composer">
        <section v-if="controlPendingRequest" class="command-confirm-strip">
          <div class="command-confirm-copy">
            <p class="eyebrow dark">Local Control Pending</p>
            <strong>{{ controlPendingRequest.title }}</strong>
            <p>{{ controlPendingRequest.prompt }}</p>
            <span class="command-confirm-hint">输入 yes / no，或使用 /confirm /cancel。30 秒内有效。</span>
          </div>

          <div class="command-confirm-actions">
            <button
              type="button"
              class="panel-chip muted"
              :disabled="busy"
              @click="cancelActiveControlPending"
            >
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
              :disabled="busy"
              @click="confirmActiveControlPending"
            >
              确认
            </button>
          </div>
        </section>
      </transition>

      <transition name="composer">
        <section v-if="pendingShellConfirmation" class="command-confirm-strip shell-confirm-strip">
          <div class="command-confirm-copy">
            <p class="eyebrow dark">Shell Agent 待确认</p>
            <strong>执行命令确认</strong>
            <p class="shell-command-preview">{{ pendingShellConfirmation.command }}</p>
            <p>{{ pendingShellConfirmation.riskDescription }}</p>
            <span class="command-confirm-hint">点击按钮确认或取消执行。60 秒内有效。</span>
          </div>

          <div class="command-confirm-actions">
            <button
              type="button"
              class="panel-chip muted"
              :disabled="busy"
              @click="cancelShellCommand"
            >
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
              :disabled="busy"
              @click="confirmShellCommand"
            >
              确认执行
            </button>
          </div>
        </section>
      </transition>

      <transition name="composer">
        <section v-if="pendingCommandConfirmation" class="command-confirm-strip">
          <div class="command-confirm-copy">
            <p class="eyebrow dark">Slash Command Pending</p>
            <strong>{{ pendingCommandConfirmation.title }}</strong>
            <p>{{ pendingCommandConfirmation.prompt }}</p>
            <span class="command-confirm-hint">输入 yes / no，或点击按钮。20 秒内有效。</span>
          </div>

          <div class="command-confirm-actions">
            <button
              type="button"
              class="panel-chip muted"
              :disabled="busy"
              @click="cancelPendingCommand"
            >
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
              :disabled="busy"
              @click="confirmPendingCommand"
            >
              确认
            </button>
          </div>
        </section>
      </transition>
    </div>

    <transition name="confirm">
      <div v-if="pendingApproval" class="confirm-shell">
        <section class="confirm-panel">
          <p class="eyebrow">One-Time Approval</p>
          <h2>{{ pendingApproval.action.title }}</h2>
          <p>{{ pendingApproval.prompt }}</p>

          <div class="approval-list">
            <label
              v-for="check in pendingApproval.checks"
              :key="check.id"
              class="approval-check"
            >
              <input
                type="checkbox"
                :checked="Boolean(approvalChecks[check.id])"
                @change="toggleApprovalCheck(check.id, ($event.target as HTMLInputElement).checked)"
              />
              <span>{{ check.label }}</span>
            </label>
          </div>

          <label class="approval-field">
            <span>输入确认短语</span>
            <input
              :value="approvalPhrase"
              :placeholder="pendingApproval.requiredPhrase"
              @input="approvalPhrase = ($event.target as HTMLInputElement).value"
            />
          </label>

          <p class="approval-expiry">
            该授权短语两分钟内有效：<strong>{{ pendingApproval.requiredPhrase }}</strong>
          </p>

          <div class="confirm-actions">
            <button type="button" class="panel-chip muted" @click="cancelPendingAction">
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
              :disabled="!canSubmitApproval"
              @click="confirmPendingAction"
            >
              我确认执行
            </button>
          </div>
        </section>
      </div>
    </transition>
  </div>
</template>

<style>
:root {
  color: #eff8fb;
  font-family:
    'Avenir Next',
    'Trebuchet MS',
    'Segoe UI Variable Text',
    sans-serif;
  background: transparent;
}

* {
  box-sizing: border-box;
}

body,
#app {
  width: 100vw;
  height: 100vh;
  margin: 0;
  background: transparent;
}

button,
input,
textarea,
select {
  font: inherit;
}

body {
  overflow: hidden;
}

.settings-window-shell {
  width: 100%;
  height: 100%;
  background: linear-gradient(180deg, #f5fbfc, #e7f1f5);
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
}

.research-window-shell {
  width: 100%;
  height: 100%;
  background: linear-gradient(180deg, #f5fbfc, #e7f1f5);
  overflow-y: auto;
  overflow-x: hidden;
}

.bubble-shell {
  width: 100%;
  height: 100%;
  background: transparent;
  overflow: visible;
  pointer-events: none;
}

.app-shell {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  padding: 0;
  overflow: visible;
}

.pet-stack,
.confirm-shell {
  position: relative;
  z-index: 2;
}

.pet-stack {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  width: 100%;
  height: 100%;
  padding: 0;
}

.confirm-shell {
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 12px;
  background: rgba(4, 15, 24, 0.34);
  backdrop-filter: blur(10px);
}

.command-confirm-strip {
  width: min(100%, 304px);
  padding: 12px 14px;
  border-radius: 22px;
  background: rgba(12, 31, 45, 0.9);
  color: #eff8fb;
  box-shadow:
    0 16px 28px rgba(5, 16, 27, 0.2),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.shell-confirm-strip {
  background: rgba(45, 30, 12, 0.92);
  border: 1px solid rgba(255, 180, 60, 0.3);
}

.shell-command-preview {
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 12px;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.35);
  border-radius: 6px;
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-all;
  color: #ffd080;
}

.task-status-strip {
  width: min(100%, 304px);
  padding: 11px 14px;
  border-radius: 22px;
  background: rgba(240, 248, 251, 0.92);
  color: #17384b;
  box-shadow:
    0 12px 24px rgba(5, 16, 27, 0.12),
    inset 0 1px 0 rgba(255, 255, 255, 0.4);
}

.task-status-strip.success {
  background: rgba(231, 247, 239, 0.94);
}

.task-status-strip.failure {
  background: rgba(255, 236, 233, 0.95);
}

.task-status-strip.muted {
  background: rgba(235, 240, 244, 0.94);
}

.command-confirm-copy {
  display: grid;
  gap: 4px;
}

.command-confirm-copy strong {
  font-size: 14px;
  color: #f3fbff;
}

.command-confirm-copy p,
.command-confirm-hint {
  margin: 0;
  line-height: 1.45;
}

.command-confirm-hint {
  color: rgba(220, 238, 246, 0.78);
  font-size: 12px;
}

.task-status-strip .command-confirm-hint {
  color: #45606f;
}

.task-step-summary {
  color: #27485c;
  font-size: 12px;
}

.command-confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 10px;
}

.settings-confirm-shell {
  background: rgba(6, 18, 28, 0.2);
}

.confirm-actions {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: center;
}

.confirm-panel {
  width: min(88vw, 360px);
  padding: 22px;
  border-radius: 28px;
  background: linear-gradient(180deg, rgba(251, 253, 254, 0.98), rgba(232, 243, 247, 0.98));
  color: #17384b;
  box-shadow:
    0 28px 48px rgba(5, 16, 27, 0.2),
    inset 0 1px 0 rgba(255, 255, 255, 0.78);
}

.confirm-panel h2 {
  margin: 4px 0 0;
  font-size: 20px;
}

.eyebrow {
  margin: 0;
  color: rgba(210, 236, 245, 0.78);
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.eyebrow.dark {
  color: #5b7a88;
}

.panel-chip,
.confirm-button {
  min-height: 34px;
  padding: 0 12px;
  border: none;
  border-radius: 999px;
  cursor: pointer;
}

.panel-chip {
  background: rgba(255, 255, 255, 0.92);
  color: #17384b;
}

.panel-chip.muted {
  background: rgba(17, 45, 63, 0.9);
  color: rgba(241, 250, 255, 0.92);
}

.confirm-panel p {
  margin: 0;
  line-height: 1.5;
}

.approval-list {
  display: grid;
  gap: 10px;
  margin: 16px 0;
}

.approval-check {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 10px 12px;
  border-radius: 16px;
  background: rgba(17, 68, 92, 0.08);
}

.approval-check input {
  margin-top: 2px;
}

.approval-field {
  display: grid;
  gap: 6px;
  margin-top: 10px;
}

.approval-field span {
  color: #335465;
  font-size: 13px;
}

.approval-field input {
  width: 100%;
  border: 1px solid rgba(23, 56, 75, 0.14);
  border-radius: 14px;
  padding: 11px 12px;
}

.approval-expiry {
  margin-top: 10px;
  color: #4e6878;
  font-size: 12px;
}

.confirm-button {
  background: linear-gradient(135deg, #0e7998, #18a07f);
  color: #effbff;
}

.confirm-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.confirm-enter-active,
.confirm-leave-active {
  transition: opacity 0.2s ease;
}

.confirm-enter-from,
.confirm-leave-to {
  opacity: 0;
}

.composer-enter-active,
.composer-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.composer-enter-from,
.composer-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
