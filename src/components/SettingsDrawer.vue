<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ControlPanel from './ControlPanel.vue'
import type {
  DesktopAction,
  OAuthState,
  ProviderConfigInput,
  ProviderKind
} from '../types/assistant'

const props = defineProps<{
  open: boolean
  section: 'settings' | 'actions'
  draft: ProviderConfigInput
  saving: boolean
  voiceInputAvailable: boolean
  oauthState: OAuthState
  oauthBusy: boolean
  actions: DesktopAction[]
  permissionLevel: number
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
  sectionChange: [section: 'settings' | 'actions']
  oauthStart: [input: ProviderConfigInput]
  oauthComplete: [callbackUrl: string]
  oauthDisconnect: []
  triggerAction: [action: DesktopAction]
}>()

const cloneDraft = (value: ProviderConfigInput): ProviderConfigInput =>
  JSON.parse(JSON.stringify(value)) as ProviderConfigInput

const localDraft = ref<ProviderConfigInput>(cloneDraft(props.draft))
const callbackUrl = ref('')
const copyFeedback = ref('')

watch(
  () => props.draft,
  (value) => {
    localDraft.value = cloneDraft(value)
  },
  { deep: true, immediate: true }
)

watch(
  () => props.open,
  (open) => {
    if (!open) {
      callbackUrl.value = ''
      copyFeedback.value = ''
    }
  }
)

const providerOptions: Array<{ label: string; value: ProviderKind }> = [
  { label: 'Mock', value: 'mock' },
  { label: 'OpenAI', value: 'openAi' },
  { label: 'Anthropic', value: 'anthropic' },
  { label: 'OpenAI-Compatible', value: 'openAiCompatible' }
]

const oauthStatusLabel = computed(() => {
  const map = {
    signedOut: '未登录',
    pending: '待完成',
    authorized: '已登录',
    error: '错误'
  }

  return map[props.oauthState.status]
})

const clearApiKey = () => {
  localDraft.value.apiKey = ''
  localDraft.value.clearApiKey = true
}

const clearOAuthToken = () => {
  localDraft.value.clearOAuthToken = true
}

const save = () => {
  if (localDraft.value.apiKey?.trim()) {
    localDraft.value.clearApiKey = false
  }

  emit('save', cloneDraft(localDraft.value))
}

const completeOauth = () => {
  emit('oauthComplete', callbackUrl.value)
}

const copyPendingUrl = async () => {
  copyFeedback.value = ''
  const value = props.oauthState.pendingAuthUrl
  if (!value || typeof navigator === 'undefined' || !navigator.clipboard) {
    return
  }

  try {
    await navigator.clipboard.writeText(value)
    copyFeedback.value = '已复制授权链接'
  } catch {
    copyFeedback.value = '复制失败，请手动复制'
  }
}
</script>

<template>
  <transition name="drawer">
    <aside v-if="open" class="drawer-shell">
      <div class="drawer-panel">
        <header class="drawer-header">
          <div>
            <p class="eyebrow">通过输入框或语音唤出</p>
            <h2>设置与受控动作</h2>
          </div>
          <button type="button" class="ghost-button" @click="emit('close')">
            收起
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

        <section v-if="section === 'settings'" class="panel-section">
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

          <label class="field">
            <span>Model</span>
            <input v-model="localDraft.model" type="text" placeholder="例如 gpt-4.1-mini" />
          </label>

          <label class="field">
            <span>Base URL</span>
            <input
              v-model="localDraft.baseUrl"
              type="text"
              placeholder="OpenAI-compatible 可填写自定义网关"
            />
          </label>

          <template v-if="localDraft.authMode === 'apiKey'">
            <label class="field">
              <span>API Key</span>
              <input
                v-model="localDraft.apiKey"
                type="password"
                placeholder="仅保留在当前运行内存，不会持久化"
              />
            </label>

            <div class="field inline-actions compact-actions">
              <button type="button" class="ghost-button" @click="clearApiKey">
                清空当前运行密钥
              </button>
            </div>
          </template>

          <template v-else>
            <section class="oauth-shell">
              <div class="oauth-header">
                <div>
                  <strong>OAuth 登录</strong>
                  <p>桌面端走 PKCE，只有允许联网时才会发起授权和换取令牌。</p>
                </div>
                <span class="oauth-status">{{ oauthStatusLabel }}</span>
              </div>

              <label class="field">
                <span>Client ID</span>
                <input
                  v-model="localDraft.oauthClientId"
                  type="text"
                  placeholder="OAuth public client id"
                />
              </label>

              <label class="field">
                <span>Authorize URL</span>
                <input
                  v-model="localDraft.oauthAuthorizeUrl"
                  type="text"
                  placeholder="https://provider.example.com/oauth/authorize"
                />
              </label>

              <label class="field">
                <span>Token URL</span>
                <input
                  v-model="localDraft.oauthTokenUrl"
                  type="text"
                  placeholder="https://provider.example.com/oauth/token"
                />
              </label>

              <label class="field">
                <span>Redirect URL</span>
                <input
                  v-model="localDraft.oauthRedirectUrl"
                  type="text"
                  placeholder="http://127.0.0.1:8976/oauth/callback"
                />
              </label>

              <label class="field">
                <span>Scopes</span>
                <input
                  v-model="localDraft.oauthScopes"
                  type="text"
                  placeholder="openid profile email"
                />
              </label>

              <div class="oauth-actions">
                <button
                  type="button"
                  class="ghost-button"
                  :disabled="oauthBusy"
                  @click="emit('oauthStart', cloneDraft(localDraft))"
                >
                  {{ oauthBusy ? '处理中...' : '生成授权链接' }}
                </button>
                <button
                  type="button"
                  class="ghost-button"
                  :disabled="oauthBusy || (!oauthState.accessTokenLoaded && !oauthState.pendingAuthUrl)"
                  @click="emit('oauthDisconnect')"
                >
                  退出登录
                </button>
              </div>

              <div class="field inline-actions compact-actions">
                <button type="button" class="ghost-button" @click="clearOAuthToken">
                  下次保存时清空内存令牌状态
                </button>
              </div>

              <div v-if="oauthState.pendingAuthUrl" class="oauth-card">
                <label class="field compact">
                  <span>授权链接</span>
                  <textarea :value="oauthState.pendingAuthUrl" rows="3" readonly />
                </label>

                <div class="inline-actions compact-actions">
                  <button type="button" class="ghost-button" @click="copyPendingUrl">
                    复制授权链接
                  </button>
                  <span class="copy-feedback">{{ copyFeedback }}</span>
                </div>

                <label class="field compact">
                  <span>浏览器回调地址</span>
                  <textarea
                    v-model="callbackUrl"
                    rows="2"
                    placeholder="完成授权后，把浏览器地址栏最后完整 URL 粘贴到这里"
                  />
                </label>

                <button
                  type="button"
                  class="save-button compact-save"
                  :disabled="oauthBusy || !callbackUrl.trim()"
                  @click="completeOauth"
                >
                  {{ oauthBusy ? '处理中...' : '完成 OAuth 登录' }}
                </button>
              </div>

              <div class="oauth-meta">
                <p>令牌状态：{{ oauthState.accessTokenLoaded ? '已加载到内存' : '未加载' }}</p>
                <p v-if="oauthState.accountHint">当前账号：{{ oauthState.accountHint }}</p>
                <p v-if="oauthState.lastError">最近错误：{{ oauthState.lastError }}</p>
              </div>
            </section>
          </template>

          <label class="field">
            <span>System Prompt</span>
            <textarea
              v-model="localDraft.systemPrompt"
              rows="4"
              placeholder="定义桌宠的人设和安全边界"
            />
          </label>

          <label class="field">
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

          <div class="toggle-grid">
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

          <div class="release-note">
            <strong>当前交互约束</strong>
            <p>语音输入由电脑是否检测到麦克风决定，不能在这里手动关闭。</p>
            <p>
              {{
                voiceInputAvailable
                  ? '已检测到可用麦克风和语音识别环境，按住主界面左侧按钮即可讲话。'
                  : '当前未检测到可用麦克风或语音识别环境，现阶段只能用文字输入。'
              }}
            </p>
            <p>隐藏到托盘只能通过输入或语音命令触发，恢复依赖系统托盘双击。</p>
            <p>高风险桌面动作仍然必须经过一次性人工确认，不会开放自由命令执行。</p>
          </div>

          <footer class="drawer-footer">
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

        <section v-else class="panel-section action-section">
          <ControlPanel
            :actions="actions"
            :permission-level="permissionLevel"
            @trigger="emit('triggerAction', $event)"
          />
        </section>
      </div>
    </aside>
  </transition>
</template>

<style scoped>
.drawer-shell {
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 12px;
  background: rgba(7, 18, 29, 0.28);
  backdrop-filter: blur(10px);
}

.drawer-panel {
  width: min(100%, 320px);
  max-height: calc(100vh - 24px);
  overflow-y: auto;
  padding: 16px;
  border-radius: 24px;
  background: linear-gradient(180deg, rgba(247, 252, 253, 0.98), rgba(231, 244, 247, 0.98));
  color: #17384b;
  box-shadow: 0 24px 48px rgba(6, 18, 30, 0.2);
}

.drawer-header,
.drawer-footer,
.inline-actions,
.oauth-header,
.oauth-actions,
.tab-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.drawer-header h2 {
  margin: 4px 0 0;
  font-size: 18px;
}

.eyebrow {
  margin: 0;
  color: #5b7a88;
  font-size: 11px;
  letter-spacing: 0.08em;
}

.tab-row {
  margin-top: 14px;
}

.tab-button {
  flex: 1;
  min-height: 36px;
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

.panel-section {
  margin-top: 14px;
}

.field {
  display: grid;
  gap: 8px;
  margin-top: 14px;
}

.field.compact {
  margin-top: 10px;
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
  background: rgba(255, 255, 255, 0.82);
  color: #17384b;
  font-size: 14px;
  outline: none;
}

textarea {
  resize: vertical;
}

.toggle-grid {
  display: grid;
  gap: 9px;
  margin-top: 16px;
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
  margin-top: 16px;
  padding: 14px;
  border-radius: 18px;
  background: rgba(17, 59, 79, 0.06);
}

.oauth-header {
  align-items: flex-start;
}

.oauth-header p,
.oauth-meta p,
.release-note p {
  margin: 6px 0 0;
  line-height: 1.5;
  font-size: 12px;
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
}

.oauth-card {
  margin-top: 14px;
  padding: 12px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.62);
}

.copy-feedback {
  font-size: 12px;
  color: #426171;
}

.oauth-meta {
  margin-top: 12px;
}

.release-note {
  margin-top: 16px;
  padding: 14px;
  border-radius: 18px;
  background: rgba(12, 89, 116, 0.08);
}

.release-note strong {
  font-size: 13px;
}

.drawer-footer {
  margin-top: 18px;
}

.save-button,
.ghost-button {
  min-height: 36px;
  padding: 0 14px;
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

.action-section {
  padding-top: 4px;
}

.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.18s ease;
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}
</style>
