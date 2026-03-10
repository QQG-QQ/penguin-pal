<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type {
  OAuthState,
  ProviderConfigInput,
  ProviderKind
} from '../types/assistant'

const props = defineProps<{
  open: boolean
  draft: ProviderConfigInput
  saving: boolean
  voiceSupported: boolean
  oauthState: OAuthState
  oauthBusy: boolean
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
  oauthStart: [input: ProviderConfigInput]
  oauthComplete: [callbackUrl: string]
  oauthDisconnect: []
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
            <p class="eyebrow">Safety + Provider</p>
            <h2>模型与安全设置</h2>
          </div>
          <button type="button" class="ghost-button" @click="emit('close')">
            关闭
          </button>
        </header>

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

          <div class="field inline-actions">
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
                <p>采用 PKCE 授权码思路，要求你的上游模型网关支持 OAuth bearer token。</p>
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

            <div class="field inline-actions">
              <button type="button" class="ghost-button" @click="clearOAuthToken">
                下次保存时清空内存令牌状态
              </button>
            </div>

            <div v-if="oauthState.pendingAuthUrl" class="oauth-card">
              <label class="field compact">
                <span>授权链接</span>
                <textarea :value="oauthState.pendingAuthUrl" rows="3" readonly />
              </label>

              <div class="inline-actions">
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
          <input v-model.number="localDraft.permissionLevel" type="range" min="0" max="2" step="1" />
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
            允许保留历史对话
          </label>
        </div>

        <div class="release-note">
          <strong>高危操作仍需确认</strong>
          <p>
            当前版本默认把系统控制封在白名单网关后。即使接入真实模型，也不会开放自由命令执行。
          </p>
          <p>
            OpenAI 的 Sign in with ChatGPT 更偏身份接入，不等于任何上游模型 API 都天然支持 OAuth bearer token。
          </p>
          <p v-if="!voiceSupported">
            当前环境未检测到可用的 Web Speech 语音输入，Windows 真机上建议补测 WebView2 语音权限。
          </p>
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
  padding: 14px;
  background: rgba(7, 18, 29, 0.34);
  backdrop-filter: blur(10px);
}

.drawer-panel {
  width: min(100%, 324px);
  max-height: calc(100vh - 28px);
  overflow-y: auto;
  padding: 18px;
  border-radius: 26px;
  background: linear-gradient(180deg, rgba(247, 252, 253, 0.98), rgba(231, 244, 247, 0.98));
  color: #17384b;
  box-shadow: 0 24px 48px rgba(6, 18, 30, 0.2);
}

.drawer-header,
.drawer-footer,
.inline-actions,
.oauth-header,
.oauth-actions {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.drawer-header h2 {
  margin: 4px 0 0;
  font-size: 20px;
}

.eyebrow {
  margin: 0;
  color: #5b7a88;
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
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
  background: rgba(9, 46, 62, 0.06);
}

.oauth-header {
  align-items: flex-start;
}

.oauth-header p,
.oauth-meta p,
.release-note p {
  margin: 6px 0 0;
  line-height: 1.5;
}

.oauth-status {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 0 10px;
  border-radius: 999px;
  background: rgba(17, 68, 92, 0.12);
  color: #19485d;
  font-size: 12px;
}

.oauth-card {
  margin-top: 14px;
  padding: 12px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.72);
}

.copy-feedback {
  color: #4f6c7d;
  font-size: 12px;
}

.oauth-meta {
  margin-top: 12px;
  color: #345465;
  font-size: 12px;
}

.release-note {
  margin-top: 18px;
  padding: 13px;
  border-radius: 18px;
  background: rgba(255, 187, 120, 0.18);
  color: #5f3a12;
}

.ghost-button,
.save-button {
  border: none;
  cursor: pointer;
}

.ghost-button {
  padding: 9px 12px;
  border-radius: 14px;
  background: rgba(18, 56, 74, 0.08);
  color: #17384b;
}

.drawer-footer {
  margin-top: 20px;
}

.save-button {
  width: 100%;
  min-height: 46px;
  border-radius: 18px;
  background: linear-gradient(135deg, #0d7195, #17a58b);
  color: #effbff;
  font-size: 15px;
}

.compact-save {
  margin-top: 10px;
}

.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.22s ease;
}

.drawer-enter-active .drawer-panel,
.drawer-leave-active .drawer-panel {
  transition: transform 0.22s ease;
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}

.drawer-enter-from .drawer-panel,
.drawer-leave-to .drawer-panel {
  transform: translateY(14px);
}
</style>
