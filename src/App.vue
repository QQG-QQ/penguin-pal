<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import ChatBubble from './components/ChatBubble.vue'
import ControlPanel from './components/ControlPanel.vue'
import InputBox from './components/InputBox.vue'
import Penguin from './components/Penguin.vue'
import SettingsDrawer from './components/SettingsDrawer.vue'
import {
  clearConversation,
  getAssistantSnapshot,
  hideAssistantWindow,
  requestDesktopAction,
  saveProviderConfig,
  sendChatMessage
} from './lib/assistant'
import type {
  DesktopAction,
  PetMode,
  ProviderConfigInput,
  ProviderKind,
  AssistantSnapshot
} from './types/assistant'

const providerDefaults: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

const cloneDraft = (value: ProviderConfigInput): ProviderConfigInput =>
  JSON.parse(JSON.stringify(value)) as ProviderConfigInput

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
    apiKeyLoaded: false
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

const toDraft = (snapshot: AssistantSnapshot): ProviderConfigInput => ({
  kind: snapshot.provider.kind,
  model: snapshot.provider.model || providerDefaults[snapshot.provider.kind],
  baseUrl: snapshot.provider.baseUrl,
  systemPrompt: snapshot.provider.systemPrompt,
  allowNetwork: snapshot.provider.allowNetwork,
  voiceReply: snapshot.provider.voiceReply,
  retainHistory: snapshot.provider.retainHistory,
  permissionLevel: snapshot.permissionLevel,
  apiKey: '',
  clearApiKey: false
})

const snapshot = ref<AssistantSnapshot>(emptySnapshot())
const settingsDraft = ref<ProviderConfigInput>(toDraft(snapshot.value))
const expanded = ref(false)
const panelMode = ref<'chat' | 'actions'>('chat')
const showSettings = ref(false)
const messageDraft = ref('')
const busy = ref(false)
const savingSettings = ref(false)
const feedback = ref('管理员企鹅待命中。点击她展开对话，或直接隐藏到托盘。')
const pendingAction = ref<DesktopAction | null>(null)
const listening = ref(false)
const visualMode = ref<PetMode | null>(null)

let recognition: SpeechRecognition | null = null
let recognitionBuffer = ''
let submitVoiceAfterStop = false

const quickPrompts = [
  '今天有什么需要我记住的？',
  '告诉我当前安全边界',
  '打开受控动作面板'
]

const providerLabel = computed(() => {
  const labels: Record<ProviderKind, string> = {
    mock: 'Mock',
    openAi: 'OpenAI',
    anthropic: 'Anthropic',
    openAiCompatible: 'OpenAI-Compatible'
  }

  return labels[snapshot.value.provider.kind]
})

const activeMode = computed<PetMode>(() => visualMode.value ?? snapshot.value.mode)

const voiceSupported = computed(
  () =>
    typeof window !== 'undefined' &&
    Boolean(window.SpeechRecognition || window.webkitSpeechRecognition)
)

const voiceReplySupported = computed(
  () => typeof window !== 'undefined' && 'speechSynthesis' in window
)

const applySnapshot = (nextSnapshot: AssistantSnapshot) => {
  snapshot.value = nextSnapshot
  settingsDraft.value = toDraft(nextSnapshot)
}

const resetVisualModeSoon = (delay = 800) => {
  window.setTimeout(() => {
    if (!listening.value && !busy.value) {
      visualMode.value = null
    }
  }, delay)
}

const openChatPanel = () => {
  panelMode.value = 'chat'
  expanded.value = true
}

const openActionPanel = () => {
  panelMode.value = 'actions'
  expanded.value = true
}

const togglePanel = () => {
  if (expanded.value) {
    expanded.value = false
    return
  }

  openChatPanel()
}

const speakReply = (content: string) => {
  if (!snapshot.value.provider.voiceReply || !voiceReplySupported.value) {
    visualMode.value = null
    return
  }

  window.speechSynthesis.cancel()

  const utterance = new SpeechSynthesisUtterance(content)
  utterance.lang = 'zh-CN'
  utterance.rate = 1
  utterance.pitch = 1.05
  utterance.onstart = () => {
    visualMode.value = 'speaking'
    feedback.value = '正在通过系统语音播报回复...'
  }
  utterance.onend = () => {
    visualMode.value = null
  }
  utterance.onerror = () => {
    feedback.value = '系统语音播报失败，但文字回复已送达。'
    visualMode.value = 'guarded'
    resetVisualModeSoon()
  }

  window.speechSynthesis.speak(utterance)
}

const loadSnapshot = async () => {
  try {
    const loaded = await getAssistantSnapshot()
    applySnapshot(loaded)
    feedback.value = `管理员企鹅已就位。当前 Provider：${providerLabel.value}。`
  } catch (error) {
    feedback.value =
      error instanceof Error ? error.message : '加载助手状态失败，已保留本地默认配置。'
  }
}

const sendMessage = async (value = messageDraft.value) => {
  const content = value.trim()

  if (!content || busy.value) {
    return
  }

  busy.value = true
  messageDraft.value = ''
  feedback.value = '正在请求助手回复并校验安全边界...'
  visualMode.value = 'thinking'
  openChatPanel()

  try {
    const response = await sendChatMessage(content)
    applySnapshot(response.snapshot)
    feedback.value = `${response.providerLabel} 已回复。`
    speakReply(response.reply.content)
    if (!snapshot.value.provider.voiceReply || !voiceReplySupported.value) {
      resetVisualModeSoon()
    }
  } catch (error) {
    visualMode.value = 'guarded'
    feedback.value = error instanceof Error ? error.message : '消息发送失败'
    resetVisualModeSoon(1400)
  } finally {
    busy.value = false
  }
}

const ensureRecognition = () => {
  if (!voiceSupported.value) {
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
    feedback.value = `语音识别失败：${event.error}`
    visualMode.value = 'guarded'
    resetVisualModeSoon(1600)
  }

  recognition.onend = () => {
    const transcript = recognitionBuffer.trim()
    const shouldSend = submitVoiceAfterStop && transcript.length > 0

    listening.value = false
    submitVoiceAfterStop = false

    if (shouldSend) {
      feedback.value = '语音转写完成，正在发送...'
      void sendMessage(transcript)
      return
    }

    feedback.value = '已结束语音输入。'
    resetVisualModeSoon()
  }

  return recognition
}

const startListening = () => {
  if (busy.value || listening.value) {
    return
  }

  const instance = ensureRecognition()
  if (!instance) {
    feedback.value = '当前环境不支持语音输入，请改用文字对话。'
    visualMode.value = 'guarded'
    resetVisualModeSoon()
    return
  }

  try {
    recognitionBuffer = ''
    submitVoiceAfterStop = false
    listening.value = true
    visualMode.value = 'listening'
    feedback.value = '按住说话中，松开后会自动转写并发送。'
    openChatPanel()
    instance.start()
  } catch {
    feedback.value = '语音输入正在占用中，请稍后再试。'
    listening.value = false
    visualMode.value = 'guarded'
    resetVisualModeSoon()
  }
}

const stopListening = () => {
  if (!recognition || !listening.value) {
    return
  }

  submitVoiceAfterStop = true
  visualMode.value = 'thinking'
  feedback.value = '已松开，正在结束录音并转写...'
  recognition.stop()
}

const triggerAction = async (action: DesktopAction, confirmed = false) => {
  if (busy.value) {
    return
  }

  if (confirmed) {
    pendingAction.value = null
  }

  busy.value = true
  feedback.value = `正在申请动作：${action.title}`
  visualMode.value = 'guarded'
  openActionPanel()

  try {
    const result = await requestDesktopAction(action.id, confirmed)
    applySnapshot(result.snapshot)
    feedback.value = result.message
    pendingAction.value = null
  } catch (error) {
    feedback.value = error instanceof Error ? error.message : '动作执行失败'
  } finally {
    busy.value = false
    resetVisualModeSoon(1200)
  }
}

const handleActionTrigger = (action: DesktopAction) => {
  openActionPanel()

  if (action.requiresConfirmation) {
    pendingAction.value = action
    return
  }

  void triggerAction(action, false)
}

const saveSettings = async (draft: ProviderConfigInput) => {
  savingSettings.value = true

  try {
    const nextDraft = cloneDraft(draft)
    if (!nextDraft.model.trim()) {
      nextDraft.model = providerDefaults[nextDraft.kind]
    }

    const nextSnapshot = await saveProviderConfig(nextDraft)
    applySnapshot(nextSnapshot)
    showSettings.value = false
    feedback.value = '模型和安全配置已保存。'
  } catch (error) {
    feedback.value = error instanceof Error ? error.message : '保存配置失败'
  } finally {
    savingSettings.value = false
  }
}

const toggleVoiceReply = (value: boolean) => {
  settingsDraft.value = {
    ...settingsDraft.value,
    voiceReply: value
  }
  snapshot.value = {
    ...snapshot.value,
    provider: {
      ...snapshot.value.provider,
      voiceReply: value
    }
  }
}

const confirmPendingAction = () => {
  if (!pendingAction.value) {
    return
  }

  void triggerAction(pendingAction.value, true)
}

const fillPrompt = (prompt: string) => {
  if (prompt.includes('动作面板')) {
    feedback.value = '已打开受控动作面板。'
    openActionPanel()
    return
  }

  messageDraft.value = prompt
  openChatPanel()
}

const hidePet = async () => {
  expanded.value = false
  showSettings.value = false

  try {
    const hidden = await hideAssistantWindow()
    if (!hidden) {
      feedback.value = '当前不是 Tauri 运行时，已仅收起桌宠面板。'
      return
    }

    feedback.value = '桌宠已隐藏到托盘，可随时恢复。'
  } catch (error) {
    feedback.value = error instanceof Error ? error.message : '隐藏桌宠失败'
  }
}

const resetConversation = async () => {
  try {
    const nextSnapshot = await clearConversation()
    applySnapshot(nextSnapshot)
    feedback.value = '对话历史已清空，并重新回到安全欢迎态。'
    openChatPanel()
  } catch (error) {
    feedback.value = error instanceof Error ? error.message : '清空会话失败'
  }
}

onMounted(() => {
  void loadSnapshot()
})

onBeforeUnmount(() => {
  recognition?.stop()
  if (voiceReplySupported.value) {
    window.speechSynthesis.cancel()
  }
})
</script>

<template>
  <div class="app-shell">
    <div class="background-layer layer-a" />
    <div class="background-layer layer-b" />

    <transition name="panel">
      <section v-if="expanded" class="pet-panel">
        <header class="panel-toolbar">
          <div class="panel-tabs">
            <button
              type="button"
              class="panel-tab"
              :class="{ active: panelMode === 'chat' }"
              @click="panelMode = 'chat'"
            >
              对话
            </button>
            <button
              type="button"
              class="panel-tab"
              :class="{ active: panelMode === 'actions' }"
              @click="panelMode = 'actions'"
            >
              动作
            </button>
          </div>

          <div class="panel-actions">
            <button class="panel-chip" type="button" @click="showSettings = true">
              设置
            </button>
            <button class="panel-chip muted" type="button" @click="resetConversation">
              清空
            </button>
            <button class="panel-chip subtle" type="button" @click="expanded = false">
              收起
            </button>
          </div>
        </header>

        <ChatBubble
          v-if="panelMode === 'chat'"
          :messages="snapshot.messages"
          :mode="activeMode"
          :provider-label="providerLabel"
          :permission-level="snapshot.permissionLevel"
          :audit-trail="snapshot.auditTrail"
          @close="expanded = false"
        />

        <ControlPanel
          v-else
          :actions="snapshot.allowedActions"
          :permission-level="snapshot.permissionLevel"
          @trigger="handleActionTrigger"
        />

        <div v-if="panelMode === 'chat'" class="quick-prompts">
          <button
            v-for="prompt in quickPrompts"
            :key="prompt"
            type="button"
            class="quick-prompt"
            @click="fillPrompt(prompt)"
          >
            {{ prompt }}
          </button>
        </div>

        <InputBox
          v-if="panelMode === 'chat'"
          v-model="messageDraft"
          :busy="busy"
          :listening="listening"
          :voice-supported="voiceSupported"
          :voice-reply-enabled="settingsDraft.voiceReply"
          @send="sendMessage()"
          @voice-start="startListening"
          @voice-stop="stopListening"
          @toggle-voice-reply="toggleVoiceReply"
        />
      </section>
    </transition>

    <div class="status-bubble" :class="`mode-${activeMode}`">
      <span class="status-kicker">{{ expanded ? '桌宠面板已展开' : '点击企鹅展开面板' }}</span>
      <p>{{ feedback }}</p>
    </div>

    <Penguin
      :mode="activeMode"
      :subtitle="feedback"
      :permission-level="snapshot.permissionLevel"
      :expanded="expanded"
      @activate="togglePanel"
      @open-actions="openActionPanel"
      @open-settings="showSettings = true"
      @hide="hidePet"
    />

    <p class="pet-hint">托盘始终保留。隐藏后可从托盘或任务区图标恢复。</p>

    <SettingsDrawer
      :open="showSettings"
      :draft="settingsDraft"
      :saving="savingSettings"
      :voice-supported="voiceSupported"
      @close="showSettings = false"
      @save="saveSettings"
    />

    <transition name="confirm">
      <div v-if="pendingAction" class="confirm-shell">
        <section class="confirm-panel">
          <p class="eyebrow">Manual Confirmation</p>
          <h2>{{ pendingAction.title }}</h2>
          <p>{{ pendingAction.summary }}</p>
          <p>
            该动作风险等级为 {{ pendingAction.riskLevel }}，只有在你确认后才会真正交给系统白名单网关执行。
          </p>
          <div class="confirm-actions">
            <button type="button" class="panel-chip muted" @click="pendingAction = null">
              取消
            </button>
            <button type="button" class="confirm-button" @click="confirmPendingAction">
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
  padding: 12px 10px 14px;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  align-items: center;
  gap: 10px;
  overflow: hidden;
}

.background-layer {
  position: absolute;
  border-radius: 999px;
  filter: blur(36px);
  pointer-events: none;
}

.layer-a {
  top: 10px;
  left: -48px;
  width: 180px;
  height: 180px;
  background: rgba(153, 234, 248, 0.2);
}

.layer-b {
  right: -54px;
  bottom: 24px;
  width: 190px;
  height: 190px;
  background: rgba(255, 180, 113, 0.16);
}

.pet-panel,
.status-bubble,
.pet-hint {
  position: relative;
  z-index: 2;
}

.pet-panel {
  width: min(100%, 300px);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  border-radius: 28px;
  background: rgba(243, 250, 252, 0.84);
  color: #143648;
  backdrop-filter: blur(16px);
  box-shadow:
    0 20px 38px rgba(4, 17, 30, 0.18),
    inset 0 1px 0 rgba(255, 255, 255, 0.72);
}

.panel-toolbar,
.panel-tabs,
.panel-actions,
.quick-prompts,
.confirm-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.panel-toolbar {
  justify-content: space-between;
  align-items: flex-start;
}

.panel-actions {
  justify-content: flex-end;
}

.panel-tab,
.panel-chip,
.quick-prompt,
.confirm-button {
  min-height: 34px;
  padding: 0 12px;
  border: none;
  border-radius: 999px;
  cursor: pointer;
}

.panel-tab {
  background: rgba(17, 59, 79, 0.08);
  color: #305364;
}

.panel-tab.active {
  background: linear-gradient(135deg, #0b6a8a, #16a085);
  color: #effbff;
}

.panel-chip {
  background: rgba(255, 255, 255, 0.92);
  color: #17384b;
}

.panel-chip.muted {
  background: rgba(17, 45, 63, 0.88);
  color: rgba(241, 250, 255, 0.9);
}

.panel-chip.subtle {
  background: rgba(17, 59, 79, 0.08);
  color: #35576a;
}

.quick-prompts {
  margin-top: -2px;
}

.quick-prompt {
  background: rgba(11, 84, 116, 0.1);
  color: #1c556f;
}

.status-bubble {
  width: min(100%, 286px);
  padding: 11px 14px;
  border-radius: 20px;
  background: rgba(8, 30, 44, 0.82);
  color: #eef8fb;
  backdrop-filter: blur(12px);
  box-shadow: 0 16px 28px rgba(5, 16, 27, 0.22);
}

.status-bubble.mode-listening {
  background: rgba(11, 92, 84, 0.86);
}

.status-bubble.mode-thinking {
  background: rgba(98, 67, 24, 0.86);
}

.status-bubble.mode-speaking {
  background: rgba(90, 49, 79, 0.84);
}

.status-bubble.mode-guarded {
  background: rgba(97, 33, 33, 0.88);
}

.status-kicker {
  display: block;
  margin-bottom: 4px;
  color: rgba(211, 233, 242, 0.78);
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.status-bubble p,
.pet-hint,
.confirm-panel p {
  margin: 0;
  line-height: 1.5;
}

.pet-hint {
  width: min(100%, 286px);
  color: rgba(219, 240, 247, 0.72);
  font-size: 12px;
  text-align: center;
}

.confirm-shell {
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgba(4, 15, 24, 0.48);
  backdrop-filter: blur(10px);
}

.confirm-panel {
  width: min(88vw, 320px);
  padding: 22px;
  border-radius: 28px;
  background: linear-gradient(180deg, rgba(251, 253, 254, 0.98), rgba(232, 243, 247, 0.98));
  color: #17384b;
  box-shadow: 0 24px 56px rgba(5, 16, 28, 0.24);
}

.eyebrow {
  margin: 0 0 4px;
  color: #5a7988;
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.confirm-panel h2 {
  margin: 0 0 10px;
  font-size: 22px;
}

.confirm-actions {
  margin-top: 16px;
}

.confirm-button {
  background: linear-gradient(135deg, #0d7195, #17a58b);
  color: #effbff;
}

.panel-enter-active,
.panel-leave-active,
.confirm-enter-active,
.confirm-leave-active {
  transition: opacity 0.18s ease;
}

.panel-enter-active .pet-panel,
.panel-leave-active .pet-panel,
.confirm-enter-active .confirm-panel,
.confirm-leave-active .confirm-panel {
  transition: transform 0.18s ease;
}

.panel-enter-from,
.panel-leave-to,
.confirm-enter-from,
.confirm-leave-to {
  opacity: 0;
}

.panel-enter-from .pet-panel,
.panel-leave-to .pet-panel,
.confirm-enter-from .confirm-panel,
.confirm-leave-to .confirm-panel {
  transform: translateY(12px);
}
</style>
