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
const expanded = ref(true)
const showSettings = ref(false)
const messageDraft = ref('')
const busy = ref(false)
const savingSettings = ref(false)
const feedback = ref('PenguinPal 正在加载桌宠控制台...')
const pendingAction = ref<DesktopAction | null>(null)
const listening = ref(false)
const visualMode = ref<PetMode | null>(null)

let recognition: SpeechRecognition | null = null
let recognitionBuffer = ''
let submitVoiceAfterStop = false

const quickPrompts = [
  '帮我整理今天的待办',
  '告诉我当前安全策略',
  '如何严格接入 AI API？'
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
    feedback.value = `桌宠已就绪，当前 Provider：${providerLabel.value}。`
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
  expanded.value = true

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
  messageDraft.value = prompt
}

const resetConversation = async () => {
  try {
    const nextSnapshot = await clearConversation()
    applySnapshot(nextSnapshot)
    feedback.value = '对话历史已清空，并重新回到安全欢迎态。'
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

    <header class="command-bar">
      <button class="command-chip" type="button" @click="expanded = !expanded">
        {{ expanded ? '折叠面板' : '展开面板' }}
      </button>
      <button class="command-chip" type="button" @click="showSettings = true">
        设置
      </button>
      <button class="command-chip muted" type="button" @click="resetConversation">
        清空会话
      </button>
    </header>

    <main class="workspace">
      <section v-if="expanded" class="console-stack">
        <ChatBubble
          :messages="snapshot.messages"
          :mode="activeMode"
          :provider-label="providerLabel"
          :permission-level="snapshot.permissionLevel"
          :audit-trail="snapshot.auditTrail"
          @close="expanded = false"
        />

        <ControlPanel
          :actions="snapshot.allowedActions"
          :permission-level="snapshot.permissionLevel"
          @trigger="handleActionTrigger"
        />

        <div class="quick-prompts">
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
      </section>

      <section class="pet-stack">
        <Penguin
          :mode="activeMode"
          :subtitle="feedback"
          :permission-level="snapshot.permissionLevel"
          @activate="expanded = !expanded"
        />

        <InputBox
          v-if="expanded"
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
    </main>

    <footer class="release-footer">
      <span>Windows 桌宠优先模式</span>
      <span>{{ snapshot.audioProfile.inputMode }} / {{ snapshot.audioProfile.outputMode }}</span>
      <span>严格白名单 · L{{ snapshot.permissionLevel }}</span>
    </footer>

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
            <button type="button" class="command-chip muted" @click="pendingAction = null">
              取消
            </button>
            <button
              type="button"
              class="confirm-button"
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
  color: #effbff;
  font-family:
    "Avenir Next",
    "Trebuchet MS",
    "Segoe UI Variable Text",
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
  padding: 16px 14px 20px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  overflow: hidden;
}

.background-layer {
  position: absolute;
  inset: auto;
  border-radius: 999px;
  filter: blur(42px);
  pointer-events: none;
}

.layer-a {
  top: 48px;
  left: -40px;
  width: 240px;
  height: 240px;
  background: rgba(152, 232, 255, 0.22);
}

.layer-b {
  right: -50px;
  bottom: 100px;
  width: 250px;
  height: 250px;
  background: rgba(142, 255, 214, 0.18);
}

.command-bar,
.release-footer,
.quick-prompts,
.confirm-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.command-bar,
.release-footer {
  position: relative;
  z-index: 2;
}

.command-chip,
.quick-prompt,
.confirm-button {
  min-height: 38px;
  padding: 0 14px;
  border: none;
  border-radius: 999px;
  cursor: pointer;
}

.command-chip {
  background: rgba(244, 251, 255, 0.9);
  color: #16384b;
}

.command-chip.muted {
  background: rgba(10, 33, 50, 0.84);
  color: rgba(241, 251, 255, 0.88);
}

.workspace {
  position: relative;
  z-index: 2;
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: flex-end;
  gap: 14px;
  min-height: 0;
}

.console-stack,
.pet-stack {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
}

.quick-prompts {
  width: min(100%, 360px);
}

.quick-prompt {
  background: rgba(244, 251, 255, 0.16);
  color: #effbff;
  backdrop-filter: blur(12px);
}

.release-footer {
  justify-content: space-between;
  padding: 0 4px;
  color: rgba(231, 246, 251, 0.76);
  font-size: 12px;
  line-height: 1.5;
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
  width: min(88vw, 360px);
  padding: 24px;
  border-radius: 28px;
  background: linear-gradient(180deg, rgba(251, 253, 254, 0.98), rgba(232, 243, 247, 0.98));
  color: #17384b;
  box-shadow: 0 24px 56px rgba(5, 16, 28, 0.24);
}

.confirm-panel h2 {
  margin: 4px 0 8px;
  font-size: 24px;
}

.confirm-panel p {
  margin: 0 0 12px;
  line-height: 1.6;
}

.confirm-button {
  background: linear-gradient(135deg, #0d7195, #17a58b);
  color: #effbff;
}

.confirm-enter-active,
.confirm-leave-active {
  transition: opacity 0.18s ease;
}

.confirm-enter-from,
.confirm-leave-to {
  opacity: 0;
}

@media (max-width: 480px) {
  .app-shell {
    padding: 12px 10px 16px;
  }

  .release-footer {
    justify-content: flex-start;
  }
}
</style>
