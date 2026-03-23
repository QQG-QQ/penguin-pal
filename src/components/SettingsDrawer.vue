<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import ControlPanel from './ControlPanel.vue'
import { presetModelCatalog } from '../lib/modelCatalog'
import type {
  AppUpdateStatus,
  AiConstraintProfile,
  CodexCliStatus,
  DesktopAction,
  DownloadProgress,
  ManagedMemoryKind,
  MemoryConflictGroup,
  MemoryManagementSnapshot,
  ManagedMemoryRecord,
  ProviderConfigInput,
  ProviderKind,
  ReplyHistoryEntry,
  VoiceInputMode,
  VisionChannelKind,
  VisionProviderStatus,
  WhisperModel,
  WhisperStatus
} from '../types/assistant'

const props = defineProps<{
  section: 'settings' | 'actions'
  draft: ProviderConfigInput
  saving: boolean
  voiceInputAvailable: boolean
  oauthBusy: boolean
  oauthNotice: string
  codexStatus: CodexCliStatus
  appUpdateStatus: AppUpdateStatus
  appUpdateBusy: boolean
  currentProviderLabel: string
  visionChannelStatus: VisionProviderStatus
  actions: DesktopAction[]
  permissionLevel: number
  aiConstraints: AiConstraintProfile
  todayReplyHistory: ReplyHistoryEntry[]
  memoryDashboard: MemoryManagementSnapshot
  memoryBusy: boolean
  whisperStatus: WhisperStatus
  whisperDownloading: boolean
  whisperDownloadProgress: DownloadProgress | null
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
  sectionChange: [section: 'settings' | 'actions']
  oauthStart: [input: ProviderConfigInput]
  codexRelogin: []
  codexRefresh: []
  appUpdateCheck: []
  appUpdateOpen: []
  openResearch: []
  memoryRefresh: []
  memoryDelete: [kind: ManagedMemoryKind, id: string]
  memoryPromote: [id: string]
  memoryResolve: [kind: ManagedMemoryKind, group: string, keepId: string]
  triggerAction: [action: DesktopAction]
  clearTodayHistory: []
  whisperDownload: [model: WhisperModel]
  whisperLoad: [model: WhisperModel]
  whisperUnload: []
  whisperDelete: [model: WhisperModel]
}>()

const cloneDraft = (value: ProviderConfigInput): ProviderConfigInput =>
  JSON.parse(JSON.stringify(value)) as ProviderConfigInput

const localDraft = ref<ProviderConfigInput>(cloneDraft(props.draft))

const providerOptions: Array<{ label: string; value: ProviderKind }> = [
  { label: 'Codex CLI', value: 'codexCli' },
  { label: 'OpenAI', value: 'openAi' },
  { label: 'Anthropic', value: 'anthropic' },
  { label: 'OpenAI-Compatible', value: 'openAiCompatible' },
  { label: 'Mock', value: 'mock' }
]

const visionProviderOptions: Array<{ label: string; value: VisionChannelKind }> = [
  { label: '禁用', value: 'disabled' },
  { label: 'OpenAI', value: 'openAi' },
  { label: 'OpenAI-Compatible', value: 'openAiCompatible' }
]

const presetOptions = presetModelCatalog
const DEFAULT_PUSH_TO_TALK_SHORTCUT = 'CommandOrControl+Alt+Space'
const voiceInputModeOptions: Array<{ label: string; value: VoiceInputMode; summary: string }> = [
  { label: '关闭语音输入', value: 'disabled', summary: '仅保留文字输入，不自动开麦。' },
  { label: '常驻监听', value: 'continuous', summary: '后台短窗循环录音，识别后自动发给桌宠。' },
  { label: '按键说话', value: 'pushToTalk', summary: '按住全局快捷键时录音，松开后转写发送。' }
]

const selectedPreset = ref('custom')
const applyingPreset = ref(false)
const isCodexProvider = ref(localDraft.value.kind === 'codexCli')
const shortcutRecording = ref(false)
const shortcutPreview = ref('')
type MemoryPanelKey = 'stable' | 'candidate' | 'conflicts'
const memoryPanelOpen = ref<Record<MemoryPanelKey, boolean>>({
  stable: true,
  candidate: false,
  conflicts: false
})

const modifierOrder = ['CommandOrControl', 'Alt', 'Shift']

const normalizeResearchList = (value: string) =>
  value
    .split(/\r?\n|,|，/)
    .map((item) => item.trim())
    .filter((item, index, items) => item.length > 0 && items.indexOf(item) === index)

const researchWatchlistText = computed({
  get: () => localDraft.value.research.watchlist.join('\n'),
  set: (value: string) => {
    localDraft.value.research.watchlist = normalizeResearchList(value)
  }
})

const researchFundsText = computed({
  get: () => localDraft.value.research.funds.join('\n'),
  set: (value: string) => {
    localDraft.value.research.funds = normalizeResearchList(value)
  }
})

const researchThemesText = computed({
  get: () => localDraft.value.research.themes.join('\n'),
  set: (value: string) => {
    localDraft.value.research.themes = normalizeResearchList(value)
  }
})

const modifierTokens = (event: KeyboardEvent) => {
  const tokens: string[] = []
  if (event.ctrlKey || event.metaKey) {
    tokens.push('CommandOrControl')
  }
  if (event.altKey) {
    tokens.push('Alt')
  }
  if (event.shiftKey) {
    tokens.push('Shift')
  }
  return tokens
}

const normalizeShortcutKey = (event: KeyboardEvent): string | null => {
  const { key } = event

  if (key === 'Control' || key === 'Meta' || key === 'Alt' || key === 'Shift') {
    return null
  }

  if (key === ' ') {
    return 'Space'
  }

  const aliasMap: Record<string, string> = {
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Escape: 'Esc',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown'
  }

  if (aliasMap[key]) {
    return aliasMap[key]
  }

  if (/^F\d{1,2}$/i.test(key)) {
    return key.toUpperCase()
  }

  if (/^[a-z0-9]$/i.test(key)) {
    return key.toUpperCase()
  }

  return null
}

const currentShortcutDisplay = () =>
  localDraft.value.pushToTalkShortcut?.trim() || DEFAULT_PUSH_TO_TALK_SHORTCUT

const updateShortcutPreview = (event: KeyboardEvent) => {
  const modifiers = modifierTokens(event)
  shortcutPreview.value = modifiers.join('+')
}

const stopShortcutCapture = () => {
  shortcutRecording.value = false
  shortcutPreview.value = ''
  if (typeof window !== 'undefined') {
    window.removeEventListener('keydown', handleShortcutCaptureKeydown, true)
    window.removeEventListener('keyup', handleShortcutCaptureKeyup, true)
    window.removeEventListener('blur', handleShortcutCaptureBlur)
  }
}

const toggleMemoryPanel = (panel: MemoryPanelKey) => {
  memoryPanelOpen.value[panel] = !memoryPanelOpen.value[panel]
}

const commitShortcutCapture = (value: string) => {
  localDraft.value.pushToTalkShortcut = value
  stopShortcutCapture()
}

const handleShortcutCaptureKeydown = (event: KeyboardEvent) => {
  if (!shortcutRecording.value) {
    return
  }

  event.preventDefault()
  event.stopPropagation()

  if (event.key === 'Escape' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey) {
    stopShortcutCapture()
    return
  }

  if (
    (event.key === 'Backspace' || event.key === 'Delete') &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey
  ) {
    commitShortcutCapture(DEFAULT_PUSH_TO_TALK_SHORTCUT)
    return
  }

  const modifiers = modifierTokens(event)
  const keyToken = normalizeShortcutKey(event)

  if (!keyToken) {
    shortcutPreview.value = modifiers.join('+')
    return
  }

  if (modifiers.length === 0) {
    shortcutPreview.value = `${keyToken}（请至少加一个 Ctrl / Alt / Shift）`
    return
  }

  const combo = [...modifiers, keyToken]
    .filter((token, index, tokens) => tokens.indexOf(token) === index)
    .sort((left, right) => {
      const leftIndex = modifierOrder.indexOf(left)
      const rightIndex = modifierOrder.indexOf(right)
      if (leftIndex === -1 && rightIndex === -1) {
        return 0
      }
      if (leftIndex === -1) {
        return 1
      }
      if (rightIndex === -1) {
        return -1
      }
      return leftIndex - rightIndex
    })
    .join('+')

  commitShortcutCapture(combo)
}

const handleShortcutCaptureKeyup = (event: KeyboardEvent) => {
  if (!shortcutRecording.value) {
    return
  }

  event.preventDefault()
  event.stopPropagation()
  updateShortcutPreview(event)
}

const handleShortcutCaptureBlur = () => {
  stopShortcutCapture()
}

const beginShortcutCapture = () => {
  if (shortcutRecording.value) {
    stopShortcutCapture()
    return
  }

  shortcutRecording.value = true
  shortcutPreview.value = ''

  if (typeof window !== 'undefined') {
    window.addEventListener('keydown', handleShortcutCaptureKeydown, true)
    window.addEventListener('keyup', handleShortcutCaptureKeyup, true)
    window.addEventListener('blur', handleShortcutCaptureBlur)
  }
}

const resetShortcutToDefault = () => {
  localDraft.value.pushToTalkShortcut = DEFAULT_PUSH_TO_TALK_SHORTCUT
  stopShortcutCapture()
}

const applyProviderRules = () => {
  isCodexProvider.value = localDraft.value.kind === 'codexCli'
  localDraft.value.authMode = isCodexProvider.value ? 'oauth' : 'apiKey'
  if (isCodexProvider.value) {
    localDraft.value.baseUrl = null
    localDraft.value.oauthAuthorizeUrl = null
    localDraft.value.oauthTokenUrl = null
    localDraft.value.oauthClientId = null
    localDraft.value.oauthScopes = ''
  }
}

watch(
  () => localDraft.value.kind,
  () => {
    if (!applyingPreset.value) {
      selectedPreset.value = 'custom'
    }
    applyProviderRules()
  },
  { immediate: true }
)

watch(
  () => props.draft,
  (value) => {
    stopShortcutCapture()
    localDraft.value = cloneDraft(value)
    selectedPreset.value = 'custom'
    applyProviderRules()
  },
  { deep: true, immediate: true }
)

const applyPreset = (presetId: string) => {
  applyingPreset.value = true
  selectedPreset.value = presetId
  const preset = presetOptions.find((item) => item.id === presetId)
  if (!preset) {
    applyingPreset.value = false
    return
  }

  localDraft.value.kind = preset.kind
  localDraft.value.model = preset.model
  localDraft.value.baseUrl = preset.baseUrl
  localDraft.value.authMode = preset.authMode
  localDraft.value.oauthAuthorizeUrl = null
  localDraft.value.oauthTokenUrl = null
  localDraft.value.oauthClientId = null
  localDraft.value.oauthScopes = ''
  localDraft.value.clearOAuthToken = true
  applyProviderRules()
  applyingPreset.value = false
}

const clearApiKey = () => {
  localDraft.value.apiKey = ''
  localDraft.value.clearApiKey = true
}

const clearVisionApiKey = () => {
  localDraft.value.visionChannel.apiKey = ''
  localDraft.value.visionChannel.clearApiKey = true
}

const save = () => {
  localDraft.value.pushToTalkShortcut =
    localDraft.value.pushToTalkShortcut?.trim() || DEFAULT_PUSH_TO_TALK_SHORTCUT
  localDraft.value.workspaceRoot = localDraft.value.workspaceRoot?.trim() || null

  if (isCodexProvider.value || localDraft.value.kind === 'mock') {
    localDraft.value.apiKey = ''
    localDraft.value.clearApiKey = true
  }

  if (localDraft.value.apiKey?.trim()) {
    localDraft.value.clearApiKey = false
  }

  if (
    localDraft.value.visionChannel.kind === 'disabled' ||
    !localDraft.value.visionChannel.enabled
  ) {
    localDraft.value.visionChannel.enabled = false
    localDraft.value.visionChannel.baseUrl = null
  } else if (localDraft.value.visionChannel.kind === 'openAi') {
    localDraft.value.visionChannel.baseUrl = null
  }

  if (localDraft.value.visionChannel.apiKey?.trim()) {
    localDraft.value.visionChannel.clearApiKey = false
  }

  emit('save', cloneDraft(localDraft.value))
}

const formatHistoryTime = (timestamp: number) =>
  new Date(timestamp).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })

const formatMemoryTime = (timestamp: number) =>
  new Date(timestamp).toLocaleString([], {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  })

const memoryStatusLabel = (status: ManagedMemoryRecord['status']) => {
  switch (status) {
    case 'active':
      return '活跃'
    case 'archived':
      return '归档'
    case 'deprecated':
      return '废弃'
    case 'conflicted':
      return '冲突'
    default:
      return status
  }
}

const memoryKindLabel = (kind: ManagedMemoryKind) => (kind === 'semantic' ? '语义记忆' : '交互偏好')

const conflictActionLabel = (group: MemoryConflictGroup) =>
  group.memoryType === 'semantic' ? '保留这条事实' : '保留这条偏好'

onBeforeUnmount(() => {
  stopShortcutCapture()
})
</script>

<template>
  <section class="settings-surface">
    <header class="surface-header">
      <div>
        <p class="eyebrow">独立设置窗口</p>
        <h1>设置与受控动作</h1>
      </div>
      <button type="button" class="ghost-button" @click="emit('close')">
        关闭窗口
      </button>
    </header>

    <div class="tab-row">
      <button
        type="button"
        class="tab-button"
        :class="{ active: section === 'settings' }"
        @click="emit('sectionChange', 'settings')"
      >
        设置
      </button>
      <button
        type="button"
        class="tab-button"
        :class="{ active: section === 'actions' }"
        @click="emit('sectionChange', 'actions')"
      >
        动作
      </button>
    </div>

    <section v-if="section === 'settings'" class="panel-grid">
      <label class="field full-row">
        <span>快速预设</span>
        <select
          :value="selectedPreset"
          @change="applyPreset(($event.target as HTMLSelectElement).value)"
        >
          <option value="custom">自定义（保持当前）</option>
          <option
            v-for="preset in presetOptions"
            :key="preset.id"
            :value="preset.id"
          >
            {{ preset.label }}
          </option>
        </select>
      </label>

      <label class="field">
        <span>Provider</span>
        <select v-model="localDraft.kind">
          <option
            v-for="option in providerOptions"
            :key="option.value"
            :value="option.value"
          >
            {{ option.label }}
          </option>
        </select>
      </label>

      <label class="field" aria-label="auth-mode">
        <span>认证方式</span>
        <input
          :value="isCodexProvider ? 'Codex CLI OAuth 登录' : 'API Key'"
          type="text"
          readonly
        />
      </label>

      <label class="field full-row">
        <span>Model</span>
        <input
          v-model="localDraft.model"
          type="text"
          :readonly="isCodexProvider"
          :placeholder="isCodexProvider ? 'Codex CLI 模型由私有运行时配置决定' : '例如 gpt-4.1-mini'"
        />
        <small v-if="isCodexProvider" class="field-note">
          Codex CLI 会按桌宠私有运行时自己的配置执行，这里不再回写 <code>.codex/config.toml</code>。
        </small>
      </label>

      <label v-if="!isCodexProvider" class="field full-row">
        <span>Base URL</span>
        <input
          v-model="localDraft.baseUrl"
          type="text"
          placeholder="OpenAI-compatible 可填写自定义网关（本地 Ollama 也走这里）"
        />
      </label>

      <template v-if="!isCodexProvider && localDraft.kind !== 'mock'">
        <label class="field full-row">
          <span>API Key</span>
          <input
            v-model="localDraft.apiKey"
            type="password"
            placeholder="仅保留在当前运行内存，不会持久化"
          />
        </label>

        <div class="field inline-actions full-row compact-actions">
          <button type="button" class="ghost-button" @click="clearApiKey">
            清空当前运行密钥
          </button>
        </div>
      </template>

      <template v-if="isCodexProvider">
        <section class="oauth-shell full-row">
          <div class="oauth-header">
            <div>
              <strong>Codex CLI 登录</strong>
              <p>会在系统终端执行 <code>codex login</code>，完成后即可直接对话。</p>
            </div>
            <span class="oauth-status">{{ codexStatus.statusLabel }}</span>
          </div>

          <div class="oauth-actions">
            <button
              type="button"
              class="ghost-button"
              :disabled="oauthBusy"
              @click="emit('oauthStart', cloneDraft(localDraft))"
            >
              {{ oauthBusy ? '处理中...' : '启动 codex login' }}
            </button>
            <button
              type="button"
              class="ghost-button"
              :disabled="oauthBusy || !codexStatus.installed || !codexStatus.credentialPresent"
              @click="emit('codexRelogin')"
            >
              {{ oauthBusy ? '处理中...' : '重新登录' }}
            </button>
            <button
              type="button"
              class="ghost-button"
              :disabled="oauthBusy"
              @click="emit('codexRefresh')"
            >
              刷新状态
            </button>
          </div>

          <div class="oauth-meta full-row">
            <p>Codex CLI：{{ codexStatus.installed ? '已安装' : '未安装' }}</p>
            <p>认证状态：{{ codexStatus.credentialPresent ? '检测到凭据' : '未检测到凭据' }}</p>
            <p>实际可用性：{{ codexStatus.statusLabel }}</p>
            <p>运行时来源：{{ codexStatus.source }}</p>
            <p v-if="codexStatus.version">版本：{{ codexStatus.version }}</p>
            <p v-if="codexStatus.runtimePath">运行时路径：{{ codexStatus.runtimePath }}</p>
            <p v-if="codexStatus.authPath">凭据路径：{{ codexStatus.authPath }}</p>
            <p>当前聊天引擎：{{ currentProviderLabel }}</p>
            <p>{{ codexStatus.message }}</p>
            <p v-if="codexStatus.reloginRecommended">建议：点击“重新登录”，清理旧私有凭据后重新选择可用的账号 / workspace。</p>
            <p>Codex CLI Provider 会优先使用桌宠自己的私有运行时和私有登录目录，不依赖系统全局安装。</p>
            <p>设置页里的 Model 现在只用于显示当前运行时视图，不会再直接改写 Codex CLI 私有配置。</p>
            <p v-if="oauthNotice">{{ oauthNotice }}</p>
          </div>
        </section>
      </template>

      <label class="field full-row">
        <span>System Prompt</span>
        <textarea
          v-model="localDraft.systemPrompt"
          rows="5"
          placeholder="定义桌宠的人设和安全边界"
        />
      </label>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>视觉副通道</strong>
            <p>主聊天与规划继续走当前 Provider，活动窗口截图会单独送到支持图像输入的副通道做结构化视觉摘要。</p>
          </div>
          <span class="oauth-status">状态：{{ visionChannelStatus.kind }}</span>
        </div>

        <div class="toggle-grid full-row">
          <label class="toggle">
            <input v-model="localDraft.visionChannel.enabled" type="checkbox" />
            启用视觉副通道
          </label>
        </div>

        <div class="oauth-grid">
          <label class="field">
            <span>视觉 Provider</span>
            <select v-model="localDraft.visionChannel.kind">
              <option
                v-for="option in visionProviderOptions"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </label>

          <label class="field">
            <span>视觉 Model</span>
            <input
              v-model="localDraft.visionChannel.model"
              type="text"
              placeholder="例如 gpt-4.1-mini"
            />
          </label>

          <label
            v-if="localDraft.visionChannel.kind === 'openAiCompatible'"
            class="field full-row"
          >
            <span>视觉 Base URL</span>
            <input
              v-model="localDraft.visionChannel.baseUrl"
              type="text"
              placeholder="例如 https://api.openai.com/v1 或兼容网关地址"
            />
          </label>

          <label
            v-if="localDraft.visionChannel.kind !== 'disabled'"
            class="field full-row"
          >
            <span>视觉 API Key</span>
            <input
              v-model="localDraft.visionChannel.apiKey"
              type="password"
              placeholder="仅用于视觉副通道，不影响 Codex 主链"
            />
          </label>

          <div
            v-if="localDraft.visionChannel.kind !== 'disabled'"
            class="field inline-actions full-row compact-actions"
          >
            <button type="button" class="ghost-button" @click="clearVisionApiKey">
              清空视觉副通道密钥
            </button>
          </div>

          <label class="field">
            <span>超时（ms）</span>
            <input v-model.number="localDraft.visionChannel.timeoutMs" type="number" min="1000" />
          </label>

          <label class="field">
            <span>最大图片字节</span>
            <input
              v-model.number="localDraft.visionChannel.maxImageBytes"
              type="number"
              min="65536"
            />
          </label>

          <label class="field">
            <span>最大图片宽度</span>
            <input
              v-model.number="localDraft.visionChannel.maxImageWidth"
              type="number"
              min="320"
            />
          </label>

          <label class="field">
            <span>最大图片高度</span>
            <input
              v-model.number="localDraft.visionChannel.maxImageHeight"
              type="number"
              min="240"
            />
          </label>
        </div>

        <div class="oauth-meta full-row">
          <p>视觉状态：{{ visionChannelStatus.message }}</p>
          <p>当前视觉副通道密钥：{{ localDraft.visionChannel.apiKey?.trim() ? '本次已输入' : '未在当前表单中输入' }}</p>
          <p v-if="localDraft.visionChannel.kind === 'disabled' || !localDraft.visionChannel.enabled">
            当前不会做真正图像分析，只会保留 UIA 和必要时的截图工件。
          </p>
        </div>
      </section>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>Shell Agent 权限</strong>
            <p>控制 AI 通过 Shell 命令操作电脑的权限。启用后 AI 可以执行命令行操作。</p>
          </div>
          <span class="oauth-status">{{ localDraft.shellPermissions.enabled ? '已启用' : '已禁用' }}</span>
        </div>

        <div class="toggle-grid full-row" style="margin-top: 14px;">
          <label class="toggle">
            <input v-model="localDraft.shellPermissions.enabled" type="checkbox" />
            启用 Shell Agent
          </label>
        </div>

        <div v-if="localDraft.shellPermissions.enabled" class="oauth-grid">
          <label class="toggle">
            <input v-model="localDraft.shellPermissions.allowExecute" type="checkbox" />
            基本执行权限
          </label>

          <label class="toggle">
            <input v-model="localDraft.shellPermissions.allowFileModify" type="checkbox" />
            文件修改权限
          </label>

          <label class="toggle">
            <input v-model="localDraft.shellPermissions.allowFileDelete" type="checkbox" />
            文件删除权限
          </label>

          <label class="toggle">
            <input v-model="localDraft.shellPermissions.allowNetwork" type="checkbox" />
            网络访问权限
          </label>

          <label class="toggle">
            <input v-model="localDraft.shellPermissions.allowSystem" type="checkbox" />
            系统操作权限
          </label>

          <label class="field">
            <span>权限有效期（小时，0=永久）</span>
            <input
              v-model.number="localDraft.shellPermissions.durationHours"
              type="number"
              min="0"
              max="720"
            />
          </label>
        </div>

        <div class="oauth-meta full-row">
          <p v-if="!localDraft.shellPermissions.enabled">
            Shell Agent 已禁用。AI 无法执行命令行操作。
          </p>
          <p v-else>
            已启用的权限将在保存后生效。高风险操作仍需用户确认。
          </p>
        </div>
      </section>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>工作区 Agent</strong>
            <p>代码审查、项目分析和受控构建会优先在这个工作区根目录里进行。</p>
          </div>
          <span class="oauth-status">{{ localDraft.workspaceRoot?.trim() ? '已固定' : '自动检测' }}</span>
        </div>

        <div class="oauth-grid">
          <label class="field full-row">
            <span>工作区根目录</span>
            <input
              v-model="localDraft.workspaceRoot"
              type="text"
              placeholder="例如 D:\\projectsnew\\penguin-pal；留空时自动从当前目录向上检测 .git/Cargo.toml/package.json"
            />
          </label>
        </div>

        <div class="oauth-meta full-row">
          <p v-if="localDraft.workspaceRoot?.trim()">
            当前会优先把工作区任务固定到：{{ localDraft.workspaceRoot }}
          </p>
          <p v-else>
            留空时会使用当前进程目录，并向上寻找 .git、Cargo.toml、package.json 等标记作为工作区根。
          </p>
        </div>
      </section>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>本地投研模式</strong>
            <p>在桌宠里启用本地研究模式、每日简报和研究提醒，并把你的投资习惯写入长期记忆。</p>
          </div>
          <span class="oauth-status">{{ localDraft.research.enabled ? '已启用' : '未启用' }}</span>
        </div>

        <div class="toggle-grid full-row" style="margin-top: 14px;">
          <label class="toggle">
            <input v-model="localDraft.research.enabled" type="checkbox" />
            启用本地投研模式
          </label>

          <label class="toggle">
            <input v-model="localDraft.research.startupPopup" type="checkbox" />
            启动时弹出每日简报
          </label>

          <label class="toggle">
            <input v-model="localDraft.research.bubbleAlerts" type="checkbox" />
            用气泡提醒研究新情况
          </label>
        </div>

        <div class="field inline-actions full-row compact-actions">
          <button type="button" class="ghost-button" @click="emit('openResearch')">
            立即打开投研简报
          </button>
        </div>

        <div v-if="localDraft.research.enabled" class="oauth-grid">
          <label class="field">
            <span>股票 / ETF 自选池</span>
            <textarea
              v-model="researchWatchlistText"
              rows="5"
              placeholder="每行一个代码或名称，例如\nSPY\nQQQ\nNVDA"
            />
          </label>

          <label class="field">
            <span>基金观察池</span>
            <textarea
              v-model="researchFundsText"
              rows="5"
              placeholder="每行一个基金代码或名称，例如\n易方达蓝筹精选\n广发纳指100ETF"
            />
          </label>

          <label class="field">
            <span>增强分析主题</span>
            <textarea
              v-model="researchThemesText"
              rows="5"
              placeholder="每行一个主题，例如\n地缘政治\n半导体\n利率"
            />
          </label>

          <label class="field full-row">
            <span>投资习惯备注</span>
            <textarea
              v-model="localDraft.research.habitNotes"
              rows="4"
              placeholder="例如：偏好低回撤基金；单次研究先看财报和现金流；不追高；更关注政策和汇率。"
            />
          </label>

          <label class="field full-row">
            <span>决策框架</span>
            <textarea
              v-model="localDraft.research.decisionFramework"
              rows="5"
              placeholder="把你希望桌宠长期遵循的投研/决策框架写在这里。"
            />
          </label>
        </div>

        <div class="oauth-meta full-row">
          <p v-if="localDraft.research.enabled">
            启用后，桌宠会生成本地投研简报、在启动时可弹出独立研究窗口，并把你的研究习惯同步到长期记忆。
          </p>
          <p v-else>
            当前未启用本地投研模式。保存后，桌宠不会生成每日研究简报，也不会主动弹出投研窗口。
          </p>
          <p>
            这一版先做本地研究模式、每日简报和长期记忆联动；实时行情和新闻抓取后续再接。
          </p>
        </div>
      </section>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>本地 Whisper 语音识别</strong>
            <p>使用本地 Whisper 模型进行语音转写，无需外网。</p>
          </div>
          <span class="oauth-status">{{ whisperStatus.modelLoaded ? '已加载' : '未加载' }}</span>
        </div>

        <div class="whisper-models">
          <article
            v-for="model in whisperStatus.availableModels"
            :key="model.model"
            class="whisper-model-card"
          >
            <div class="whisper-model-info">
              <strong>{{ model.label }}</strong>
              <span
                class="whisper-model-status"
                :class="{
                  downloaded: model.downloaded,
                  loaded: whisperStatus.currentModel === model.model
                }"
              >
                {{
                  whisperStatus.currentModel === model.model
                    ? '已加载'
                    : model.downloaded
                      ? '已下载'
                      : '未下载'
                }}
              </span>
            </div>

            <div
              v-if="whisperDownloading && whisperDownloadProgress?.model === model.model"
              class="whisper-progress"
            >
              <div
                class="whisper-progress-bar"
                :style="{ width: `${whisperDownloadProgress.progressPercent}%` }"
              />
              <span>{{ Math.round(whisperDownloadProgress.progressPercent) }}%</span>
            </div>

            <div class="whisper-model-actions">
              <template v-if="!model.downloaded">
                <button
                  type="button"
                  class="ghost-button"
                  :disabled="whisperDownloading"
                  @click="emit('whisperDownload', model.model)"
                >
                  {{ whisperDownloading && whisperDownloadProgress?.model === model.model ? '下载中...' : '下载' }}
                </button>
              </template>

              <template v-else-if="whisperStatus.currentModel === model.model">
                <button
                  type="button"
                  class="ghost-button"
                  @click="emit('whisperUnload')"
                >
                  卸载
                </button>
              </template>

              <template v-else>
                <button
                  type="button"
                  class="ghost-button"
                  @click="emit('whisperLoad', model.model)"
                >
                  加载
                </button>
                <button
                  type="button"
                  class="ghost-button"
                  @click="emit('whisperDelete', model.model)"
                >
                  删除
                </button>
              </template>
            </div>
          </article>
        </div>

        <div class="oauth-meta full-row">
          <p v-if="whisperStatus.modelLoaded">
            当前已加载 {{ whisperStatus.currentModel }} 模型。保存后会按所选模式使用本地 Whisper 转写。
          </p>
          <p v-else>
            请下载并加载一个 Whisper 模型以启用本地语音识别。推荐使用 Base 模型（142MB）。
          </p>
        </div>
      </section>

      <section class="constraint-shell full-row">
        <div class="constraint-header">
          <div>
            <strong>语音输入方式</strong>
            <p>本地 Whisper 不依赖前台窗口。这里可以切换常驻监听或全局按键说话。</p>
          </div>
        </div>

        <div class="constraint-grid">
          <article class="constraint-panel">
            <h3>输入模式</h3>
            <label class="field full-row">
              <span>模式</span>
              <select v-model="localDraft.voiceInputMode">
                <option
                  v-for="option in voiceInputModeOptions"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </option>
              </select>
            </label>

            <div class="constraint-item">
              <div class="constraint-item-top">
                <strong>{{ voiceInputModeOptions.find((item) => item.value === localDraft.voiceInputMode)?.label }}</strong>
                <span class="constraint-status">{{ localDraft.voiceInputMode }}</span>
              </div>
              <p>{{ voiceInputModeOptions.find((item) => item.value === localDraft.voiceInputMode)?.summary }}</p>
            </div>

            <label
              v-if="localDraft.voiceInputMode === 'pushToTalk'"
              class="field full-row"
            >
              <span>按键说话快捷键</span>
              <div class="shortcut-capture">
                <input
                  :value="shortcutRecording ? (shortcutPreview || '请直接按下组合键') : currentShortcutDisplay()"
                  readonly
                />
                <div class="compact-actions">
                  <button
                    type="button"
                    class="ghost-button"
                    @click="beginShortcutCapture"
                  >
                    {{ shortcutRecording ? '停止录制' : '开始录制' }}
                  </button>
                  <button
                    type="button"
                    class="ghost-button"
                    @click="resetShortcutToDefault"
                  >
                    恢复默认
                  </button>
                </div>
              </div>
              <p class="field-note">
                {{
                  shortcutRecording
                    ? '正在录制。按下组合键即可保存；按 Esc 取消；按 Backspace/Delete 恢复默认。'
                    : `当前快捷键：${currentShortcutDisplay()}`
                }}
              </p>
            </label>
          </article>

          <article class="constraint-panel">
            <h3>当前状态</h3>
            <div class="constraint-item">
              <div class="constraint-item-top">
                <strong>Whisper 模型</strong>
                <span class="constraint-status">{{ whisperStatus.modelLoaded ? '已加载' : '未加载' }}</span>
              </div>
              <p>
                {{
                  whisperStatus.modelLoaded
                    ? `当前模型：${whisperStatus.currentModel}，录音状态：${whisperStatus.recordingState}。`
                    : '请先下载并加载 Whisper 模型，语音输入模式配置才会真正生效。'
                }}
              </p>
            </div>

            <div class="constraint-item">
              <div class="constraint-item-top">
                <strong>设备环境</strong>
                <span class="constraint-status">{{ voiceInputAvailable ? '可用' : '待就绪' }}</span>
              </div>
              <p>
                {{
                  voiceInputAvailable
                    ? '桌宠运行时已经具备本地语音输入条件。'
                    : whisperStatus.inputMessage ||
                      '当前未进入可录音状态。若模型已加载，请先检查麦克风权限、默认输入设备和录音链是否正常。'
                }}
              </p>
            </div>
          </article>
        </div>
      </section>

      <label class="field full-row">
        <span>权限等级</span>
        <input
          v-model.number="localDraft.permissionLevel"
          type="range"
          min="0"
          max="2"
          step="1"
        />
        <strong>L{{ localDraft.permissionLevel }}</strong>
      </label>

      <div class="toggle-grid full-row">
        <label class="toggle">
          <input v-model="localDraft.allowNetwork" type="checkbox" />
          允许外网调用 AI API / OAuth token exchange
        </label>

        <label class="toggle">
          <input v-model="localDraft.launchAtStartup" type="checkbox" />
          开机自启
        </label>

        <label class="toggle">
          <input v-model="localDraft.autoUpdateCodex" type="checkbox" />
          启动时自动更新 Codex
        </label>

        <label class="toggle">
          <input v-model="localDraft.autoCheckAppUpdate" type="checkbox" />
          启动时自动检查软件更新
        </label>

        <label class="toggle">
          <input v-model="localDraft.voiceReply" type="checkbox" />
          启用语音回复
        </label>

        <label class="toggle">
          <input v-model="localDraft.retainHistory" type="checkbox" />
          保留对话上下文
        </label>
      </div>

      <div class="release-note full-row">
        <strong>当前交互约束</strong>
        <p>
          {{
            voiceInputAvailable
              ? '本地 Whisper 语音入口已就绪；常驻监听会在后台短窗循环录音，按键说话会等待全局快捷键。'
              : '当前本地 Whisper 语音入口还未就绪，通常是因为模型尚未加载或主窗口还没开始实际录音。'
          }}
        </p>
        <p>按键说话使用 Tauri 全局快捷键格式，例如：{{ DEFAULT_PUSH_TO_TALK_SHORTCUT }}。</p>
        <p>桌宠会自动记住你上次拖动后的主窗口位置，下次启动时优先在该位置打开。</p>
        <p>关闭“启动时自动更新 Codex”后，桌宠启动时不会自动拉取新版本，但手动更新按钮仍然可用。</p>
        <p>软件更新会检查 GitHub Releases。关闭“启动时自动检查软件更新”后，只会在你手动点按钮时检查。</p>
        <p>隐藏到托盘只能通过主桌宠窗口中的输入或语音命令触发。</p>
        <p>高风险桌面动作仍然必须经过一次性人工确认，不会开放自由命令执行。</p>
      </div>

      <section class="oauth-shell full-row">
        <div class="oauth-header">
          <div>
            <strong>软件更新</strong>
            <p>检查 PenguinPal Assistant 本体是否有新版本，并打开推荐安装包下载页。</p>
          </div>
          <span class="oauth-status">{{ props.appUpdateStatus.updateAvailable ? '有新版本' : '已检查' }}</span>
        </div>

        <div class="oauth-actions">
          <button
            type="button"
            class="ghost-button"
            :disabled="props.appUpdateBusy"
            @click="emit('appUpdateCheck')"
          >
            {{ props.appUpdateBusy ? '检查中...' : '检查软件更新' }}
          </button>
          <button
            type="button"
            class="ghost-button"
            :disabled="props.appUpdateBusy || (!props.appUpdateStatus.downloadUrl && !props.appUpdateStatus.releaseUrl)"
            @click="emit('appUpdateOpen')"
          >
            打开下载页
          </button>
        </div>

        <div class="oauth-meta full-row">
          <p>当前版本：{{ props.appUpdateStatus.currentVersion || '未知' }}</p>
          <p v-if="props.appUpdateStatus.latestVersion">最新版本：{{ props.appUpdateStatus.latestVersion }}</p>
          <p v-if="props.appUpdateStatus.assetName">推荐安装包：{{ props.appUpdateStatus.assetName }}</p>
          <p>{{ props.appUpdateStatus.message }}</p>
        </div>
      </section>

      <section class="constraint-shell full-row">
        <div class="constraint-header">
          <div>
            <strong>{{ aiConstraints.label }}</strong>
            <p>{{ aiConstraints.summary }}</p>
          </div>
          <span class="constraint-version">{{ aiConstraints.version }}</span>
        </div>

        <div class="constraint-grid">
          <article class="constraint-panel">
            <h3>不可覆盖规则</h3>
            <div
              v-for="item in aiConstraints.immutableRules"
              :key="item.id"
              class="constraint-item"
            >
              <div class="constraint-item-top">
                <strong>{{ item.title }}</strong>
                <span class="constraint-status">{{ item.status }}</span>
              </div>
              <p>{{ item.summary }}</p>
            </div>
          </article>

          <article class="constraint-panel">
            <h3>允许能力</h3>
            <div
              v-for="item in aiConstraints.capabilityGates"
              :key="item.id"
              class="constraint-item"
            >
              <div class="constraint-item-top">
                <strong>{{ item.title }}</strong>
                <span class="constraint-status">{{ item.status }}</span>
              </div>
              <p>{{ item.summary }}</p>
            </div>
          </article>

          <article class="constraint-panel">
            <h3>当前运行门禁</h3>
            <div
              v-for="item in aiConstraints.runtimeBoundaries"
              :key="item.id"
              class="constraint-item"
            >
              <div class="constraint-item-top">
                <strong>{{ item.title }}</strong>
                <span class="constraint-status">{{ item.status }}</span>
              </div>
              <p>{{ item.summary }}</p>
            </div>
          </article>
        </div>
      </section>

      <section class="memory-shell full-row">
        <div class="memory-header">
          <div>
            <strong>长期记忆管理</strong>
            <p>这里会区分稳定长期记忆、候选记忆和冲突记忆。显式“记住”会直接进入长期记忆，隐式事实通常需要重复出现后才会提升。</p>
          </div>
          <button
            type="button"
            class="ghost-button"
            :disabled="memoryBusy"
            @click="emit('memoryRefresh')"
          >
            {{ memoryBusy ? '刷新中...' : '刷新记忆' }}
          </button>
        </div>

        <div class="memory-stats">
          <article class="memory-stat-card">
            <strong>{{ memoryDashboard.stats.stableCount }}</strong>
            <span>稳定长期记忆</span>
          </article>
          <article class="memory-stat-card">
            <strong>{{ memoryDashboard.stats.candidateCount }}</strong>
            <span>候选记忆</span>
          </article>
          <article class="memory-stat-card">
            <strong>{{ memoryDashboard.stats.conflictCount }}</strong>
            <span>冲突组</span>
          </article>
          <article class="memory-stat-card">
            <strong>{{ memoryDashboard.stats.semanticCount }}</strong>
            <span>语义总量</span>
          </article>
          <article class="memory-stat-card">
            <strong>{{ memoryDashboard.stats.metaCount }}</strong>
            <span>偏好总量</span>
          </article>
        </div>

        <div class="memory-grid">
          <article class="memory-panel">
            <div class="memory-panel-header">
              <div>
                <h3>稳定长期记忆</h3>
                <p>这些条目当前会参与检索和 prompt 注入。</p>
              </div>
              <button
                type="button"
                class="memory-toggle"
                :aria-expanded="memoryPanelOpen.stable"
                @click="toggleMemoryPanel('stable')"
              >
                <span>{{ memoryPanelOpen.stable ? '收起' : '展开' }}</span>
                <span class="memory-toggle-count">{{ memoryDashboard.stableRecords.length }}</span>
              </button>
            </div>

            <div v-if="memoryPanelOpen.stable" class="memory-scroll-frame">
              <div v-if="!memoryDashboard.stableRecords.length" class="memory-empty">
                还没有稳定长期记忆。
              </div>

              <div v-else class="memory-list">
                <article
                  v-for="record in memoryDashboard.stableRecords"
                  :key="record.id"
                  class="memory-entry"
                >
                  <div class="memory-entry-top">
                    <div>
                      <strong>{{ record.title }}</strong>
                      <p>{{ record.summary }}</p>
                    </div>
                    <span class="constraint-status">{{ memoryKindLabel(record.memoryType) }}</span>
                  </div>
                  <p class="memory-detail">{{ record.detail }}</p>
                  <div class="memory-meta-row">
                    <span>状态：{{ memoryStatusLabel(record.status) }}</span>
                    <span>置信度：{{ Math.round(record.confidence * 100) }}%</span>
                    <span>更新：{{ formatMemoryTime(record.updatedAt) }}</span>
                  </div>
                  <div v-if="record.tags.length" class="memory-tags">
                    <span v-for="tag in record.tags" :key="tag" class="memory-tag">{{ tag }}</span>
                  </div>
                  <div class="compact-actions">
                    <button
                      type="button"
                      class="ghost-button"
                      :disabled="memoryBusy"
                      @click="emit('memoryDelete', record.memoryType, record.id)"
                    >
                      删除
                    </button>
                  </div>
                </article>
              </div>
            </div>
          </article>

          <article class="memory-panel">
            <div class="memory-panel-header">
              <div>
                <h3>候选记忆</h3>
                <p>这类条目通常是对话中推断出的隐式事实，默认不会立即长期生效。</p>
              </div>
              <button
                type="button"
                class="memory-toggle"
                :aria-expanded="memoryPanelOpen.candidate"
                @click="toggleMemoryPanel('candidate')"
              >
                <span>{{ memoryPanelOpen.candidate ? '收起' : '展开' }}</span>
                <span class="memory-toggle-count">{{ memoryDashboard.candidateRecords.length }}</span>
              </button>
            </div>

            <div v-if="memoryPanelOpen.candidate" class="memory-scroll-frame">
              <div v-if="!memoryDashboard.candidateRecords.length" class="memory-empty">
                当前没有候选记忆。
              </div>

              <div v-else class="memory-list">
                <article
                  v-for="record in memoryDashboard.candidateRecords"
                  :key="record.id"
                  class="memory-entry"
                >
                  <div class="memory-entry-top">
                    <div>
                      <strong>{{ record.title }}</strong>
                      <p>{{ record.summary }}</p>
                    </div>
                    <span class="constraint-status">候选</span>
                  </div>
                  <p class="memory-detail">{{ record.detail }}</p>
                  <div class="memory-meta-row">
                    <span>提及次数：{{ record.mentionCount }}</span>
                    <span>置信度：{{ Math.round(record.confidence * 100) }}%</span>
                    <span v-if="record.expiresAt">过期：{{ formatMemoryTime(record.expiresAt) }}</span>
                  </div>
                  <div v-if="record.tags.length" class="memory-tags">
                    <span v-for="tag in record.tags" :key="tag" class="memory-tag">{{ tag }}</span>
                  </div>
                  <div class="compact-actions">
                    <button
                      type="button"
                      class="ghost-button"
                      :disabled="memoryBusy"
                      @click="emit('memoryPromote', record.id)"
                    >
                      提升为长期记忆
                    </button>
                    <button
                      type="button"
                      class="ghost-button"
                      :disabled="memoryBusy"
                      @click="emit('memoryDelete', record.memoryType, record.id)"
                    >
                      删除
                    </button>
                  </div>
                </article>
              </div>
            </div>
          </article>
        </div>

        <div class="memory-panel">
          <div class="memory-panel-header">
            <div>
              <h3>冲突记忆</h3>
              <p>当系统发现同一类事实上下文互相冲突时，会先暂停自动采用，等待你明确选择保留哪一条。</p>
            </div>
            <button
              type="button"
              class="memory-toggle"
              :aria-expanded="memoryPanelOpen.conflicts"
              @click="toggleMemoryPanel('conflicts')"
            >
              <span>{{ memoryPanelOpen.conflicts ? '收起' : '展开' }}</span>
              <span class="memory-toggle-count">{{ memoryDashboard.conflicts.length }}</span>
            </button>
          </div>

          <div v-if="memoryPanelOpen.conflicts" class="memory-scroll-frame memory-scroll-frame-conflicts">
            <div v-if="!memoryDashboard.conflicts.length" class="memory-empty">
              当前没有待处理的冲突记忆。
            </div>

            <div v-else class="memory-conflicts">
              <article
                v-for="group in memoryDashboard.conflicts"
                :key="group.id"
                class="memory-conflict-group"
              >
                <div class="memory-entry-top">
                  <div>
                    <strong>{{ group.title }}</strong>
                    <p>{{ memoryKindLabel(group.memoryType) }} · 冲突组 {{ group.id }}</p>
                  </div>
                  <span class="constraint-status">待裁决</span>
                </div>

                <div class="memory-list">
                  <article
                    v-for="record in group.entries"
                    :key="record.id"
                    class="memory-entry conflicted"
                  >
                    <div class="memory-entry-top">
                      <div>
                        <strong>{{ record.title }}</strong>
                        <p>{{ record.summary }}</p>
                      </div>
                      <span class="constraint-status">{{ memoryStatusLabel(record.status) }}</span>
                    </div>
                    <p class="memory-detail">{{ record.detail }}</p>
                    <div class="memory-meta-row">
                      <span>来源：{{ record.source }}</span>
                      <span>更新：{{ formatMemoryTime(record.updatedAt) }}</span>
                    </div>
                    <div class="compact-actions">
                      <button
                        type="button"
                        class="ghost-button"
                        :disabled="memoryBusy"
                        @click="emit('memoryResolve', group.memoryType, group.id, record.id)"
                      >
                        {{ conflictActionLabel(group) }}
                      </button>
                      <button
                        type="button"
                        class="ghost-button"
                        :disabled="memoryBusy"
                        @click="emit('memoryDelete', record.memoryType, record.id)"
                      >
                        删除这条
                      </button>
                    </div>
                  </article>
                </div>
              </article>
            </div>
          </div>
        </div>
      </section>

      <section class="history-shell full-row">
        <div class="history-header">
          <div>
            <strong>今日回复历史</strong>
            <p>仅展示本地时间今天的问答记录。更早的记录会自动归档到本地文档。</p>
          </div>
          <button type="button" class="ghost-button" @click="emit('clearTodayHistory')">
            清空今日历史
          </button>
        </div>

        <div v-if="!todayReplyHistory.length" class="history-empty">
          今天还没有可展示的回复历史。
        </div>

        <div v-else class="history-list">
          <article
            v-for="entry in todayReplyHistory"
            :key="entry.id"
            class="history-entry"
          >
            <div class="history-entry-top">
              <strong>{{ formatHistoryTime(entry.timestamp) }}</strong>
            </div>
            <p><span>你：</span>{{ entry.userInput }}</p>
            <p><span>企鹅：</span>{{ entry.assistantReply }}</p>
          </article>
        </div>
      </section>

      <footer class="surface-footer full-row">
        <button
          type="button"
          class="save-button"
          :disabled="saving || oauthBusy"
          @click="save"
        >
          {{ saving ? '保存中...' : '保存配置' }}
        </button>
      </footer>
    </section>

    <section v-else class="action-pane">
      <ControlPanel
        :actions="actions"
        :permission-level="permissionLevel"
        @trigger="emit('triggerAction', $event)"
      />
    </section>
  </section>
</template>

<style scoped>
.settings-surface {
  width: 100%;
  min-height: 100%;
  padding: 24px;
  background: linear-gradient(180deg, #f5fbfc, #e7f1f5);
  color: #17384b;
}

.surface-header,
.surface-footer,
.inline-actions,
.oauth-header,
.oauth-actions,
.tab-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.surface-header {
  align-items: flex-start;
}

.surface-header h1 {
  margin: 4px 0 0;
  font-size: 26px;
}

.eyebrow {
  margin: 0;
  color: #5b7a88;
  font-size: 12px;
  letter-spacing: 0.08em;
}

.tab-row {
  margin-top: 18px;
}

.tab-button {
  flex: 1;
  min-height: 40px;
  border: none;
  border-radius: 999px;
  background: rgba(17, 59, 79, 0.08);
  color: #33596b;
  cursor: pointer;
}

.tab-button.active {
  background: linear-gradient(135deg, #0b6a8a, #16a085);
  color: #effbff;
}

.panel-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  margin-top: 18px;
}

.field {
  display: grid;
  gap: 8px;
}

.field.compact {
  margin-top: 0;
}

.full-row {
  grid-column: 1 / -1;
}

.field span {
  font-size: 13px;
  color: #365667;
}

.field-note {
  color: #4c6674;
  font-size: 12px;
  line-height: 1.4;
}

input,
select,
textarea {
  width: 100%;
  border: 1px solid rgba(23, 56, 75, 0.12);
  border-radius: 14px;
  padding: 11px 13px;
  background: rgba(255, 255, 255, 0.9);
  color: #17384b;
  font-size: 14px;
  outline: none;
}

textarea {
  resize: vertical;
}

.toggle-grid {
  display: grid;
  gap: 10px;
}

.toggle {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 11px 13px;
  border-radius: 16px;
  background: rgba(17, 68, 92, 0.08);
  color: #17384b;
  font-size: 13px;
}

.toggle input {
  width: auto;
  margin: 0;
}

.oauth-shell {
  padding: 16px;
  border-radius: 20px;
  background: rgba(17, 59, 79, 0.06);
}

.oauth-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  margin-top: 14px;
}

.oauth-header {
  align-items: flex-start;
}

.oauth-header p,
.oauth-meta p,
.release-note p,
.constraint-header p,
.constraint-item p {
  margin: 6px 0 0;
  line-height: 1.5;
  font-size: 12px;
}

.constraint-shell {
  padding: 18px;
  border-radius: 22px;
  background: rgba(12, 42, 57, 0.07);
}

.history-shell {
  padding: 18px;
  border-radius: 22px;
  background: rgba(255, 255, 255, 0.72);
  display: grid;
  gap: 14px;
}

.memory-shell {
  padding: 18px;
  border-radius: 22px;
  background: rgba(255, 255, 255, 0.78);
  display: grid;
  gap: 16px;
}

.memory-header,
.memory-panel-header,
.memory-meta-row,
.memory-stats {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: flex-start;
}

.memory-header p,
.memory-panel-header p,
.memory-detail {
  margin: 6px 0 0;
  line-height: 1.5;
  font-size: 12px;
}

.memory-stats {
  flex-wrap: wrap;
}

.memory-stat-card {
  min-width: 120px;
  padding: 14px;
  border-radius: 18px;
  background: rgba(17, 68, 92, 0.06);
  display: grid;
  gap: 4px;
}

.memory-stat-card strong {
  font-size: 22px;
}

.memory-stat-card span {
  color: #476775;
  font-size: 12px;
}

.memory-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.memory-panel {
  padding: 16px;
  border-radius: 20px;
  background: rgba(17, 59, 79, 0.06);
  display: grid;
  gap: 14px;
}

.memory-panel h3 {
  margin: 0;
  font-size: 15px;
}

.memory-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: 1px solid rgba(23, 56, 75, 0.12);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.9);
  color: #17384b;
  min-height: 34px;
  padding: 0 12px;
  cursor: pointer;
  white-space: nowrap;
}

.memory-toggle-count {
  display: inline-flex;
  min-width: 22px;
  min-height: 22px;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  background: rgba(11, 106, 138, 0.1);
  color: #0b6988;
  font-size: 12px;
}

.memory-empty {
  padding: 16px;
  border-radius: 16px;
  background: rgba(17, 68, 92, 0.06);
  color: #476775;
  font-size: 13px;
}

.memory-scroll-frame {
  max-height: 360px;
  overflow: auto;
  padding-right: 4px;
}

.memory-scroll-frame-conflicts {
  max-height: 420px;
}

.memory-list,
.memory-conflicts {
  display: grid;
  gap: 10px;
}

.memory-entry {
  padding: 14px 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.82);
  display: grid;
  gap: 8px;
}

.memory-entry.conflicted {
  background: rgba(255, 244, 226, 0.94);
}

.memory-entry-top {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: flex-start;
}

.memory-entry-top p {
  margin: 6px 0 0;
  line-height: 1.5;
  font-size: 12px;
}

.memory-detail {
  color: #476775;
}

.memory-meta-row {
  flex-wrap: wrap;
  color: #4a6a78;
  font-size: 12px;
}

.memory-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.memory-tag {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  background: rgba(11, 106, 138, 0.1);
  color: #0b6988;
  font-size: 12px;
}

.memory-conflict-group {
  display: grid;
  gap: 12px;
  padding: 14px;
  border-radius: 18px;
  background: rgba(17, 68, 92, 0.06);
}

.history-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: flex-start;
}

.history-header p {
  margin: 6px 0 0;
  line-height: 1.5;
  font-size: 12px;
}

.history-empty {
  padding: 18px;
  border-radius: 18px;
  background: rgba(17, 68, 92, 0.06);
  color: #476775;
  font-size: 13px;
}

.history-list {
  display: grid;
  gap: 10px;
  max-height: 320px;
  overflow-y: auto;
  padding-right: 4px;
}

.history-entry {
  padding: 14px 16px;
  border-radius: 18px;
  background: rgba(17, 68, 92, 0.06);
  display: grid;
  gap: 8px;
}

.history-entry-top {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: center;
}

.history-entry p {
  margin: 0;
  line-height: 1.6;
  font-size: 13px;
  color: #234554;
}

.history-entry span {
  color: #4a6a78;
  font-weight: 600;
}

.constraint-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: flex-start;
}

.constraint-version,
.constraint-status {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 26px;
  padding: 0 10px;
  border-radius: 999px;
  background: rgba(11, 106, 138, 0.12);
  color: #0b6988;
  font-size: 12px;
  white-space: nowrap;
}

.constraint-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-top: 14px;
}

.constraint-panel {
  padding: 14px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.74);
}

.constraint-panel h3 {
  margin: 0;
  font-size: 15px;
}

.constraint-item + .constraint-item {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid rgba(23, 56, 75, 0.08);
}

.constraint-item-top {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: flex-start;
}

.oauth-status {
  padding: 6px 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.78);
  font-size: 12px;
}

.oauth-actions,
.compact-actions {
  flex-wrap: wrap;
  margin-top: 14px;
}

.oauth-card {
  margin-top: 14px;
  padding: 12px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.66);
}

.copy-feedback {
  font-size: 12px;
  color: #426171;
}

.oauth-meta {
  margin-top: 12px;
}

.release-note {
  padding: 14px;
  border-radius: 18px;
  background: rgba(12, 89, 116, 0.08);
}

.release-note strong {
  font-size: 13px;
}

.surface-footer {
  margin-top: 4px;
}

.save-button,
.ghost-button {
  min-height: 38px;
  padding: 0 16px;
  border: none;
  border-radius: 999px;
  cursor: pointer;
}

.save-button {
  background: linear-gradient(135deg, #0b6a8a, #16a085);
  color: #effbff;
}

.ghost-button {
  background: rgba(17, 59, 79, 0.09);
  color: #20475a;
}

.compact-save {
  margin-top: 12px;
}

.action-pane {
  margin-top: 18px;
}

.whisper-models {
  display: grid;
  gap: 10px;
  margin-top: 14px;
}

.whisper-model-card {
  padding: 14px 16px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.72);
  display: grid;
  gap: 10px;
}

.whisper-model-info {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: center;
}

.whisper-model-status {
  padding: 4px 10px;
  border-radius: 999px;
  background: rgba(17, 68, 92, 0.08);
  color: #476775;
  font-size: 12px;
}

.whisper-model-status.downloaded {
  background: rgba(22, 160, 133, 0.12);
  color: #0d7a64;
}

.whisper-model-status.loaded {
  background: rgba(11, 106, 138, 0.15);
  color: #0b6988;
}

.whisper-model-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.shortcut-capture {
  display: grid;
  gap: 8px;
}

.whisper-progress {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 24px;
  background: rgba(17, 68, 92, 0.08);
  border-radius: 999px;
  overflow: hidden;
  padding-right: 10px;
}

.whisper-progress-bar {
  height: 100%;
  background: linear-gradient(90deg, #0b6a8a, #16a085);
  border-radius: 999px;
  transition: width 0.2s ease;
}

.whisper-progress span {
  font-size: 12px;
  color: #365667;
  white-space: nowrap;
}

@media (max-width: 780px) {
  .settings-surface {
    padding: 18px;
  }

  .panel-grid,
  .oauth-grid,
  .constraint-grid,
  .memory-grid {
    grid-template-columns: 1fr;
  }
}
</style>
