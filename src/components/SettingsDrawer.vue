<script setup lang="ts">
import { ref, watch } from 'vue'
import ControlPanel from './ControlPanel.vue'
import type {
  AiConstraintProfile,
  CodexCliStatus,
  DesktopAction,
  ProviderConfigInput,
  ProviderKind
} from '../types/assistant'

const props = defineProps<{
  section: 'settings' | 'actions'
  draft: ProviderConfigInput
  saving: boolean
  voiceInputAvailable: boolean
  oauthBusy: boolean
  oauthNotice: string
  codexStatus: CodexCliStatus
  actions: DesktopAction[]
  permissionLevel: number
  aiConstraints: AiConstraintProfile
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
  sectionChange: [section: 'settings' | 'actions']
  oauthStart: [input: ProviderConfigInput]
  codexRefresh: []
  triggerAction: [action: DesktopAction]
}>()

const cloneDraft = (value: ProviderConfigInput): ProviderConfigInput =>
  JSON.parse(JSON.stringify(value)) as ProviderConfigInput

const localDraft = ref<ProviderConfigInput>(cloneDraft(props.draft))

watch(
  () => props.draft,
  (value) => {
    localDraft.value = cloneDraft(value)
  },
  { deep: true, immediate: true }
)

const providerOptions: Array<{ label: string; value: ProviderKind }> = [
  { label: 'Mock', value: 'mock' },
  { label: 'OpenAI', value: 'openAi' },
  { label: 'Anthropic', value: 'anthropic' },
  { label: 'OpenAI-Compatible', value: 'openAiCompatible' }
]

const clearApiKey = () => {
  localDraft.value.apiKey = ''
  localDraft.value.clearApiKey = true
}

const save = () => {
  if (localDraft.value.apiKey?.trim()) {
    localDraft.value.clearApiKey = false
  }

  emit('save', cloneDraft(localDraft.value))
}
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

      <label class="field">
        <span>认证方式</span>
        <select v-model="localDraft.authMode">
          <option value="apiKey">API Key</option>
          <option value="oauth">OAuth (PKCE)</option>
        </select>
      </label>

      <label class="field full-row">
        <span>Model</span>
        <input v-model="localDraft.model" type="text" placeholder="例如 gpt-4.1-mini" />
      </label>

      <label class="field full-row">
        <span>Base URL</span>
        <input
          v-model="localDraft.baseUrl"
          type="text"
          placeholder="OpenAI-compatible 可填写自定义网关"
        />
      </label>

      <template v-if="localDraft.authMode === 'apiKey'">
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

      <template v-else>
        <section class="oauth-shell full-row">
          <div class="oauth-header">
            <div>
              <strong>Codex CLI 登录</strong>
              <p>会在系统终端执行 <code>codex login</code>，登录完成后回到这里刷新状态。</p>
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
            <p v-if="codexStatus.version">版本：{{ codexStatus.version }}</p>
            <p v-if="codexStatus.authPath">凭据路径：{{ codexStatus.authPath }}</p>
            <p>{{ codexStatus.message }}</p>
            <p>通用 OAuth 字段对官方 Codex 登录不生效，保持为空即可。</p>
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
