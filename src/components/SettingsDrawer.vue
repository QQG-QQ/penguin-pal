<script setup lang="ts">
import { ref, watch } from 'vue'
import ControlPanel from './ControlPanel.vue'
import { presetModelCatalog } from '../lib/modelCatalog'
import type {
  AiConstraintProfile,
  CodexCliStatus,
  DesktopAction,
  ProviderConfigInput,
  ProviderKind,
  ReplyHistoryEntry,
  VisionChannelKind,
  VisionProviderStatus
} from '../types/assistant'

const props = defineProps<{
  section: 'settings' | 'actions'
  draft: ProviderConfigInput
  saving: boolean
  voiceInputAvailable: boolean
  oauthBusy: boolean
  oauthNotice: string
  codexStatus: CodexCliStatus
  currentProviderLabel: string
  visionChannelStatus: VisionProviderStatus
  actions: DesktopAction[]
  permissionLevel: number
  aiConstraints: AiConstraintProfile
  todayReplyHistory: ReplyHistoryEntry[]
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
  sectionChange: [section: 'settings' | 'actions']
  oauthStart: [input: ProviderConfigInput]
  codexRefresh: []
  triggerAction: [action: DesktopAction]
  clearTodayHistory: []
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

const selectedPreset = ref('custom')
const applyingPreset = ref(false)
const isCodexProvider = ref(localDraft.value.kind === 'codexCli')

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
        <input v-model="localDraft.model" type="text" placeholder="例如 gpt-4.1-mini" />
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
            <span class="oauth-status">{{ codexStatus.loggedIn ? '已登录' : '未登录' }}</span>
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
              :disabled="oauthBusy"
              @click="emit('codexRefresh')"
            >
              刷新状态
            </button>
          </div>

          <div class="oauth-meta full-row">
            <p>Codex CLI：{{ codexStatus.installed ? '已安装' : '未安装' }}</p>
            <p>运行时来源：{{ codexStatus.source }}</p>
            <p v-if="codexStatus.version">版本：{{ codexStatus.version }}</p>
            <p v-if="codexStatus.runtimePath">运行时路径：{{ codexStatus.runtimePath }}</p>
            <p v-if="codexStatus.authPath">凭据路径：{{ codexStatus.authPath }}</p>
            <p>当前聊天引擎：{{ currentProviderLabel }}</p>
            <p>{{ codexStatus.message }}</p>
            <p>Codex CLI Provider 会优先使用桌宠自己的私有运行时和私有登录目录，不依赖系统全局安装。</p>
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
        <p>语音输入由电脑是否检测到麦克风决定，不能在这里手动关闭。</p>
        <p>
          {{
            voiceInputAvailable
              ? '已检测到可用麦克风和语音识别环境，主桌宠窗口会默认进入自动语音监听。'
              : '当前未检测到可用麦克风或语音识别环境，主桌宠窗口现阶段只保留文字输入。'
          }}
        </p>
        <p>隐藏到托盘只能通过主桌宠窗口中的输入或语音命令触发。</p>
        <p>高风险桌面动作仍然必须经过一次性人工确认，不会开放自由命令执行。</p>
      </div>

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

@media (max-width: 780px) {
  .settings-surface {
    padding: 18px;
  }

  .panel-grid,
  .oauth-grid,
  .constraint-grid {
    grid-template-columns: 1fr;
  }
}
</style>
