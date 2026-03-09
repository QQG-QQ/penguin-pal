<script setup lang="ts">
import { ref, watch } from 'vue'
import type { ProviderConfigInput, ProviderKind } from '../types/assistant'

const props = defineProps<{
  open: boolean
  draft: ProviderConfigInput
  saving: boolean
  voiceSupported: boolean
}>()

const emit = defineEmits<{
  close: []
  save: [input: ProviderConfigInput]
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
  <transition name="drawer">
    <aside v-if="open" class="drawer-shell">
      <div class="drawer-panel">
        <header class="drawer-header">
          <div>
            <p class="eyebrow">Phase 3 + 5 Setup</p>
            <h2>模型与发布设置</h2>
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

        <label class="field">
          <span>System Prompt</span>
          <textarea
            v-model="localDraft.systemPrompt"
            rows="5"
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
            允许外网调用 AI API
          </label>

          <label class="toggle">
            <input v-model="localDraft.voiceReply" type="checkbox" />
            启用语音回复
          </label>

          <label class="toggle">
            <input v-model="localDraft.retainHistory" type="checkbox" />
            允许保存历史对话
          </label>
        </div>

        <div class="release-note">
          <strong>Windows 发布建议</strong>
          <p>
            当前版本默认把高风险自动化封在白名单网关后。即使接入真实模型，也不会开放自由命令执行。
          </p>
          <p v-if="!voiceSupported">
            当前环境未检测到可用的 Web Speech 语音输入，Windows 安装后建议补测 WebView2 语音权限。
          </p>
        </div>

        <footer class="drawer-footer">
          <button
            type="button"
            class="save-button"
            :disabled="saving"
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
  display: flex;
  justify-content: flex-end;
  background: rgba(7, 18, 29, 0.3);
  backdrop-filter: blur(8px);
}

.drawer-panel {
  width: min(100%, 360px);
  height: 100%;
  overflow-y: auto;
  padding: 20px;
  background: linear-gradient(180deg, rgba(247, 252, 253, 0.98), rgba(231, 244, 247, 0.98));
  color: #17384b;
  box-shadow: -24px 0 48px rgba(6, 18, 30, 0.16);
}

.drawer-header,
.drawer-footer,
.inline-actions {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.drawer-header h2 {
  margin: 4px 0 0;
  font-size: 22px;
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
  margin-top: 16px;
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
  border-radius: 16px;
  padding: 12px 14px;
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
  gap: 10px;
  margin-top: 18px;
}

.toggle {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 12px 14px;
  border-radius: 16px;
  background: rgba(17, 68, 92, 0.08);
  color: #17384b;
  font-size: 14px;
}

.toggle input {
  width: auto;
  margin: 0;
}

.release-note {
  margin-top: 20px;
  padding: 14px;
  border-radius: 18px;
  background: rgba(255, 187, 120, 0.18);
  color: #5f3a12;
}

.release-note p {
  margin: 8px 0 0;
  line-height: 1.55;
}

.ghost-button,
.save-button {
  border: none;
  cursor: pointer;
}

.ghost-button {
  padding: 10px 12px;
  border-radius: 14px;
  background: rgba(18, 56, 74, 0.08);
  color: #17384b;
}

.drawer-footer {
  margin-top: 24px;
}

.save-button {
  width: 100%;
  min-height: 48px;
  border-radius: 18px;
  background: linear-gradient(135deg, #0d7195, #17a58b);
  color: #effbff;
  font-size: 15px;
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
  transform: translateX(20px);
}
</style>
