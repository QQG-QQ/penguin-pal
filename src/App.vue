<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import InputBox from './components/InputBox.vue'
import Penguin from './components/Penguin.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import {
  cancelDesktopActionApproval,
  clearConversation,
  completeOAuthSignIn,
  confirmDesktopAction,
  disconnectOAuthSignIn,
  getAssistantSnapshot,
  hideAssistantWindow,
  requestDesktopAction,
  saveProviderConfig,
  sendChatMessage,
  startOAuthSignIn
} from './lib/assistant'
import type {
  ActionApprovalRequest,
  AssistantSnapshot,
  DesktopAction,
  PetMode,
  ProviderConfigInput,
  ProviderKind
} from './types/assistant'

const providerDefaults: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

const actionCommandMap: Record<string, string[]> = {
  open_notepad: ['打开记事本', '记事本'],
  open_calculator: ['打开计算器', '计算器'],
  open_downloads: ['打开下载目录', '下载目录', 'downloads'],
  focus_window: ['唤起桌宠', '聚焦桌宠', '显示桌宠'],
  show_window: ['显示主面板', '显示窗口']
}

const emptySnapshot = (): AssistantSnapshot => ({
  mode: 'idle',
  messages: [],
  provider: {
    kind: 'mock',
    model: providerDefaults.mock,
    baseUrl: null,
    systemPrompt:
      '你是一只严格遵守白名单规则的管理员企鹅助手，任何桌面动作都必须经过人工确认。',
    allowNetwork: false,
    voiceReply: true,
    retainHistory: true,
    apiKeyLoaded: false,
    authMode: 'apiKey',
    oauth: {
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
    }
  },
  permissionLevel: 1,
  allowedActions: [],
  auditTrail: [],
  audioProfile: {
    inputMode: 'press-to-talk',
    outputMode: 'speech-synthesis',
    stages: []
  }
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

const snapshot = ref<AssistantSnapshot>(emptySnapshot())
const settingsDraft = ref<ProviderConfigInput>(toDraft(snapshot.value))
const showSettings = ref(false)
const drawerSection = ref<'settings' | 'actions'>('settings')
const messageDraft = ref('')
const bubbleText = ref('')
const busy = ref(false)
const savingSettings = ref(false)
const authBusy = ref(false)
const pendingApproval = ref<ActionApprovalRequest | null>(null)
const approvalPhrase = ref('')
const approvalChecks = ref<Record<string, boolean>>({})
const listening = ref(false)
const visualMode = ref<PetMode | null>(null)
const microphoneAvailable = ref(false)

let recognition: SpeechRecognition | null = null
let recognitionBuffer = ''
let submitVoiceAfterStop = false
let bubbleTimer: number | null = null
let speechSession = 0
let mediaDevicesCleanup: (() => void) | null = null
let microphonePermissionRequested = false

const activeMode = computed<PetMode>(() => visualMode.value ?? snapshot.value.mode)

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

const normalizeCommand = (value: string) => value.replace(/\s+/g, '').toLowerCase()

const applySnapshot = (nextSnapshot: AssistantSnapshot) => {
  snapshot.value = nextSnapshot
  settingsDraft.value = toDraft(nextSnapshot)
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
    bubbleText.value = ''
    resetVisualModeSoon()
  }
  utterance.onerror = () => {
    if (session !== speechSession) {
      return
    }
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
  if (!nextDraft.model.trim()) {
    nextDraft.model = providerDefaults[nextDraft.kind]
  }

  const nextSnapshot = await saveProviderConfig(nextDraft)
  applySnapshot(nextSnapshot)
  return nextSnapshot
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
    void refreshMicrophoneAvailability()
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

const openDrawer = (section: 'settings' | 'actions') => {
  drawerSection.value = section
  showSettings.value = true
}

const hidePet = async () => {
  showSettings.value = false

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
    applySnapshot(nextSnapshot)
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
    applySnapshot(result.snapshot)
    showSettings.value = false
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
    ['打开设置', '显示设置', '模型设置', '安全设置', '系统设置', '打开配置', 'oauth设置', 'oauth登录'].some((token) =>
      normalized.includes(normalizeCommand(token))
    )
  ) {
    openDrawer('settings')
    announce('设置已经打开，你可以调整模型、OAuth、安全边界和受控动作。')
    return true
  }

  if (['关闭设置', '收起设置'].some((token) => normalized.includes(normalizeCommand(token)))) {
    showSettings.value = false
    announce('设置已经收起。')
    return true
  }

  if (
    ['打开动作面板', '显示动作面板', '受控动作', '动作面板', '打开动作', '动作设置'].some((token) =>
      normalized.includes(normalizeCommand(token))
    )
  ) {
    openDrawer('actions')
    announce('动作页已经打开。高风险动作仍然需要逐项确认。')
    return true
  }

  if (
    ['关闭动作面板', '收起动作面板'].some((token) => normalized.includes(normalizeCommand(token)))
  ) {
    if (drawerSection.value === 'actions') {
      showSettings.value = false
    }
    announce('动作列表已经收起。')
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
  if (!content || busy.value) {
    return
  }

  messageDraft.value = ''
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
  clearBubble()

  if (await maybeHandleLocalCommand(content)) {
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
  }

  return recognition
}

const startListening = async () => {
  if (busy.value || listening.value) {
    return
  }

  let instance = ensureRecognition()
  if (!instance && speechRecognitionSupported.value) {
    await refreshMicrophoneAvailability(true)
    instance = ensureRecognition()
  }

  if (!instance) {
    announce('当前没有检测到可用麦克风或语音识别环境，请改用文字输入。', 'guarded')
    return
  }

  try {
    recognitionBuffer = ''
    submitVoiceAfterStop = false
    listening.value = true
    visualMode.value = 'listening'
    if (voiceReplySupported.value) {
      window.speechSynthesis.cancel()
    }
    clearBubble()
    instance.start()
  } catch {
    listening.value = false
    announce('语音输入正在占用中，请稍后再试。', 'guarded')
  }
}

const stopListening = () => {
  if (!recognition || !listening.value) {
    return
  }

  submitVoiceAfterStop = true
  visualMode.value = 'thinking'
  recognition.stop()
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
    applySnapshot(result.snapshot)
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
    applySnapshot(nextSnapshot)
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
    await persistSettings(draft)
    announce('设置已经保存。')
  } catch (error) {
    announce(error instanceof Error ? error.message : '保存配置失败', 'guarded')
  } finally {
    savingSettings.value = false
  }
}

const beginOAuthLogin = async (draft: ProviderConfigInput) => {
  authBusy.value = true

  try {
    await persistSettings(draft)
    const result = await startOAuthSignIn()
    applySnapshot(result.snapshot)
    announce(result.message)
    if (result.authorizationUrl && typeof window !== 'undefined') {
      try {
        window.open(result.authorizationUrl, '_blank', 'noopener,noreferrer')
      } catch {
        announce('浏览器没有自动打开，你可以在设置浮层里复制授权链接。', 'guarded')
      }
    }
  } catch (error) {
    announce(error instanceof Error ? error.message : '生成 OAuth 授权链接失败', 'guarded')
  } finally {
    authBusy.value = false
  }
}

const finishOAuthLogin = async (callbackUrl: string) => {
  authBusy.value = true

  try {
    const result = await completeOAuthSignIn(callbackUrl)
    applySnapshot(result.snapshot)
    announce(result.message)
  } catch (error) {
    announce(error instanceof Error ? error.message : '完成 OAuth 登录失败', 'guarded')
  } finally {
    authBusy.value = false
  }
}

const disconnectOAuthLogin = async () => {
  authBusy.value = true

  try {
    const result = await disconnectOAuthSignIn()
    applySnapshot(result.snapshot)
    announce(result.message)
  } catch (error) {
    announce(error instanceof Error ? error.message : '退出 OAuth 登录失败', 'guarded')
  } finally {
    authBusy.value = false
  }
}

onMounted(() => {
  void loadSnapshot()
  void refreshMicrophoneAvailability(true)
  setupMediaDeviceWatcher()
})

onBeforeUnmount(() => {
  recognition?.stop()
  clearBubbleTimer()
  mediaDevicesCleanup?.()
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
})
</script>

<template>
  <div class="app-shell">
    <div class="pet-stack">
      <Penguin :mode="activeMode" :bubble-text="bubbleText" />

      <InputBox
        v-model="messageDraft"
        :busy="busy"
        :listening="listening"
        :voice-supported="voiceInputAvailable"
        @send="sendMessage()"
        @voice-start="startListening"
        @voice-stop="stopListening"
      />
    </div>

    <SettingsDrawer
      :open="showSettings"
      :section="drawerSection"
      :draft="settingsDraft"
      :saving="savingSettings"
      :voice-input-available="voiceInputAvailable"
      :oauth-state="snapshot.provider.oauth"
      :oauth-busy="authBusy"
      :actions="snapshot.allowedActions"
      :permission-level="snapshot.permissionLevel"
      @close="showSettings = false"
      @save="saveSettings"
      @section-change="drawerSection = $event"
      @oauth-start="beginOAuthLogin"
      @oauth-complete="finishOAuthLogin"
      @oauth-disconnect="disconnectOAuthLogin"
      @trigger-action="handleActionTrigger"
    />

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

.app-shell {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  padding: 6px 8px 10px;
  overflow: hidden;
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
  gap: 6px;
  width: min(100%, 284px);
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

.confirm-actions {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: center;
}

.confirm-panel {
  width: min(88vw, 332px);
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
</style>
