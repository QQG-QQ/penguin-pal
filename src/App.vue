<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import FloatingBubble from './components/FloatingBubble.vue'
import InputBox from './components/InputBox.vue'
import Penguin from './components/Penguin.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import {
  cancelDesktopActionApproval,
  clearConversation,
  closeSettingsWindow,
  confirmDesktopAction,
  getAssistantSnapshot,
  getCodexCliStatus,
  hideAssistantWindow,
  isBubbleWindowView,
  listenForAssistantSnapshot,
  listenForBubbleWindowState,
  listenForSettingsSectionChange,
  openSettingsWindow,
  publishBubbleWindowState,
  publishAssistantSnapshot,
  readWindowView,
  readRequestedSettingsSection,
  requestDesktopAction,
  saveProviderConfig,
  sendChatMessage,
  startCodexCliLogin,
  type SettingsSection
} from './lib/assistant'
import type {
  ActionApprovalRequest,
  AssistantWindowView,
  AiConstraintProfile,
  AssistantSnapshot,
  BubbleWindowState,
  CodexCliStatus,
  DesktopAction,
  PetMode,
  ProviderConfigInput,
  ProviderKind
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

const actionCommandMap: Record<string, string[]> = {
  open_notepad: ['打开记事本', '记事本'],
  open_calculator: ['打开计算器', '计算器'],
  open_downloads: ['打开下载目录', '下载目录', 'downloads'],
  focus_window: ['唤起桌宠', '聚焦桌宠', '显示桌宠'],
  show_window: ['显示主面板', '显示窗口']
}

const PET_WINDOW_COLLAPSED = { width: 248, height: 252 }
const PET_WINDOW_EXPANDED = { width: 312, height: 340 }
const PET_HEAD_ANCHOR_Y = 34
const PET_BODY_BOTTOM_Y = 244

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
  permissionLevel: 2,
  allowedActions: [],
  auditTrail: [],
  audioProfile: {
    inputMode: 'auto-listen',
    outputMode: 'speech-synthesis',
    stages: []
  },
  aiConstraints: emptyConstraints()
})

const emptyCodexStatus = (): CodexCliStatus => ({
  installed: false,
  version: null,
  loggedIn: false,
  authPath: null,
  runtimePath: null,
  source: '未找到',
  message: '尚未检测 Codex CLI 登录状态。'
})

const toDraft = (state: AssistantSnapshot): ProviderConfigInput => ({
  kind: state.provider.kind,
  model: state.provider.model || providerDefaults[state.provider.kind],
  baseUrl: state.provider.baseUrl,
  systemPrompt: state.provider.systemPrompt,
  allowNetwork: state.provider.allowNetwork,
  voiceReply: state.provider.voiceReply,
  retainHistory: state.provider.retainHistory,
  permissionLevel: state.permissionLevel,
  authMode: state.provider.authMode,
  oauthAuthorizeUrl: state.provider.oauth.authorizeUrl,
  oauthTokenUrl: state.provider.oauth.tokenUrl,
  oauthClientId: state.provider.oauth.clientId,
  oauthRedirectUrl: state.provider.oauth.redirectUrl,
  oauthScopes: state.provider.oauth.scopes.join(' '),
  apiKey: '',
  clearApiKey: false,
  clearOAuthToken: false
})

const windowView = ref<AssistantWindowView>(readWindowView())
const snapshot = ref<AssistantSnapshot>(emptySnapshot())
const settingsDraft = ref<ProviderConfigInput>(toDraft(snapshot.value))
const drawerSection = ref<SettingsSection>(readRequestedSettingsSection())
const messageDraft = ref('')
const bubbleText = ref('')
const bubbleWindowState = ref<BubbleWindowState>({
  visible: false,
  text: '',
  anchorX: 0,
  anchorY: 0,
  petBottomY: 0
})
const busy = ref(false)
const savingSettings = ref(false)
const authBusy = ref(false)
const oauthNotice = ref('')
const codexStatus = ref<CodexCliStatus>(emptyCodexStatus())
const pendingApproval = ref<ActionApprovalRequest | null>(null)
const approvalPhrase = ref('')
const approvalChecks = ref<Record<string, boolean>>({})
const listening = ref(false)
const visualMode = ref<PetMode | null>(null)
const microphoneAvailable = ref(false)
const textInputFocused = ref(false)
const composerVisible = ref(false)
const inputBoxRef = ref<{ focusComposer: () => void } | null>(null)

let recognition: SpeechRecognition | null = null
let recognitionBuffer = ''
let submitVoiceAfterStop = false
let bubbleTimer: number | null = null
let speechSession = 0
let mediaDevicesCleanup: (() => void) | null = null
let microphonePermissionRequested = false
let snapshotListenerCleanup: (() => void) | null = null
let sectionListenerCleanup: (() => void) | null = null
let bubbleStateListenerCleanup: (() => void) | null = null
let windowMovedCleanup: (() => void) | null = null
let windowResizedCleanup: (() => void) | null = null
let autoListenTimer: number | null = null
let speechPlaybackActive = false

const isSettingsView = computed(() => windowView.value === 'settings')
const isBubbleView = computed(() => windowView.value === 'bubble' || isBubbleWindowView())
const activeMode = computed<PetMode>(() => visualMode.value ?? snapshot.value.mode)
const activeProviderLabel = computed(() => providerLabels[snapshot.value.provider.kind])
const showComposer = computed(
  () => composerVisible.value || textInputFocused.value || Boolean(messageDraft.value.trim())
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

const voiceInputAvailable = computed(
  () => speechRecognitionSupported.value && microphoneAvailable.value
)

const voiceReplySupported = computed(
  () => typeof window !== 'undefined' && 'speechSynthesis' in window
)

const shouldAutoListen = computed(
  () =>
    voiceInputAvailable.value &&
    !isSettingsView.value &&
    !busy.value &&
    !textInputFocused.value &&
    !pendingApproval.value &&
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

const clearBubble = () => {
  clearBubbleTimer()
  bubbleText.value = ''
}

const syncPetWindowFrame = async () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return
  }

  const appWindow = getCurrentWindow()
  const nextSize = showComposer.value ? PET_WINDOW_EXPANDED : PET_WINDOW_COLLAPSED
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
}

const buildBubbleWindowState = async (): Promise<BubbleWindowState> => {
  const text = bubbleText.value.trim()
  if (!text || !isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return {
      visible: false,
      text: '',
      anchorX: 0,
      anchorY: 0,
      petBottomY: 0
    }
  }

  const appWindow = getCurrentWindow()
  const position = await appWindow.outerPosition()
  const size = await appWindow.outerSize()

  return {
    visible: true,
    text,
    anchorX: Math.round(position.x + size.width / 2),
    anchorY: position.y + PET_HEAD_ANCHOR_Y,
    petBottomY: position.y + PET_BODY_BOTTOM_Y
  }
}

const syncBubbleWindow = async () => {
  if (!isTauriDesktop() || isBubbleView.value) {
    return
  }

  const nextState = await buildBubbleWindowState()
  bubbleWindowState.value = nextState
  await publishBubbleWindowState(nextState)
}

const revealComposer = async () => {
  if (isSettingsView.value || isBubbleView.value) {
    return
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

const showBubble = (content: string, mode: PetMode = 'speaking', duration = 4200) => {
  const session = ++speechSession
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
  clearBubbleTimer()
  bubbleText.value = content
  visualMode.value = mode
  bubbleTimer = window.setTimeout(() => {
    if (session !== speechSession) {
      return
    }
    bubbleText.value = ''
    resetVisualModeSoon(0)
    scheduleAutoListening()
  }, duration)
}

const speakReply = (content: string) => {
  if (!snapshot.value.provider.voiceReply || !voiceReplySupported.value) {
    showBubble(content, 'speaking')
    return
  }

  const session = ++speechSession
  clearBubbleTimer()
  window.speechSynthesis.cancel()
  clearAutoListenTimer()
  speechPlaybackActive = true

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
    bubbleText.value = ''
    resetVisualModeSoon()
    scheduleAutoListening(320)
  }
  utterance.onerror = () => {
    if (session !== speechSession) {
      return
    }
    speechPlaybackActive = false
    showBubble(content, 'speaking')
    scheduleAutoListening(320)
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

const setPendingApproval = (approvalRequest: ActionApprovalRequest | null | undefined) => {
  if (!approvalRequest) {
    clearPendingApproval()
    return
  }

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

  return saveProviderConfig(nextDraft)
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
    ['关闭动作面板', '收起动作面板'].some((token) => normalized.includes(normalizeCommand(token)))
  ) {
    const closed = await closeDrawer()
    announce(closed ? '动作窗口已经关闭。' : '当前没有打开的动作窗口。', closed ? 'speaking' : 'guarded')
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

const sendMessage = async (value = messageDraft.value) => {
  const content = value.trim()
  if (!content || busy.value || isSettingsView.value) {
    return
  }

  messageDraft.value = ''
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
  if (recognition && listening.value) {
    submitVoiceAfterStop = false
    recognition.stop()
  }
  clearAutoListenTimer()
  clearBubble()

  if (await maybeHandleLocalCommand(content)) {
    scheduleAutoListening(260)
    return
  }

  busy.value = true
  visualMode.value = 'thinking'

  try {
    const response = await sendChatMessage(content)
    applySnapshot(response.snapshot)
    announce(response.reply.content)
  } catch (error) {
    announce(error instanceof Error ? error.message : '消息发送失败', 'guarded')
  } finally {
    busy.value = false
    resetVisualModeSoon(900)
    scheduleAutoListening(320)
  }
}

const ensureRecognition = () => {
  if (!voiceInputAvailable.value) {
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

const handleInputFocus = () => {
  composerVisible.value = true
  textInputFocused.value = true
  clearAutoListenTimer()

  if (recognition && listening.value) {
    submitVoiceAfterStop = false
    recognition.stop()
  }
}

const handleInputBlur = () => {
  textInputFocused.value = false
  if (!messageDraft.value.trim() && !busy.value) {
    composerVisible.value = false
    void syncPetWindowFrame()
  }
  scheduleAutoListening(320)
}

const setupPetWindowListeners = async () => {
  if (!isTauriDesktop() || isSettingsView.value || isBubbleView.value) {
    return
  }

  const appWindow = getCurrentWindow()
  windowMovedCleanup = await appWindow.onMoved(() => {
    void syncBubbleWindow()
  })
  windowResizedCleanup = await appWindow.onResized(() => {
    void syncBubbleWindow()
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
  })

  if (isSettingsView.value) {
    sectionListenerCleanup = await listenForSettingsSectionChange((section) => {
      drawerSection.value = section
    })
  }
}

watch(showComposer, (visible, previousVisible) => {
  if (isSettingsView.value || isBubbleView.value) {
    return
  }

  void syncPetWindowFrame().then(() => syncBubbleWindow())

  if (visible && !previousVisible) {
    void nextTick(() => {
      inputBoxRef.value?.focusComposer()
    })
  }
})

watch(
  () => bubbleText.value,
  () => {
    void syncBubbleWindow()
  }
)

onMounted(() => {
  if (isBubbleView.value) {
    void setupCrossWindowListeners()
    return
  }

  void loadSnapshot()
  void refreshCodexLoginStatus(true)
  void refreshMicrophoneAvailability(!isSettingsView.value).then(() => {
    scheduleAutoListening(420)
  })
  setupMediaDeviceWatcher()
  void setupCrossWindowListeners()
  if (!isSettingsView.value) {
    void syncPetWindowFrame().then(() => syncBubbleWindow())
    void setupPetWindowListeners()
  }
})

onBeforeUnmount(() => {
  recognition?.stop()
  clearBubbleTimer()
  clearAutoListenTimer()
  mediaDevicesCleanup?.()
  snapshotListenerCleanup?.()
  sectionListenerCleanup?.()
  bubbleStateListenerCleanup?.()
  windowMovedCleanup?.()
  windowResizedCleanup?.()
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
      :current-provider-label="activeProviderLabel"
      :actions="snapshot.allowedActions"
      :permission-level="snapshot.permissionLevel"
      :ai-constraints="snapshot.aiConstraints"
      @close="closeDrawer"
      @save="saveSettings"
      @section-change="drawerSection = $event"
      @oauth-start="beginOAuthLogin"
      @codex-refresh="refreshCodexLoginStatus()"
      @trigger-action="handleActionTrigger"
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

  <div v-else-if="isBubbleView" class="bubble-shell">
    <FloatingBubble :state="bubbleWindowState" />
  </div>

  <div v-else class="app-shell">
    <div class="pet-stack">
      <Penguin :mode="activeMode" @activate="revealComposer" />

      <transition name="composer">
        <InputBox
          v-if="showComposer"
          ref="inputBoxRef"
          v-model="messageDraft"
          :busy="busy"
          @send="sendMessage()"
          @focus="handleInputFocus"
          @blur="handleInputBlur"
        />
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
