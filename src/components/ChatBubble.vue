<script setup lang="ts">
import { computed } from 'vue'
import type { AuditEntry, ChatMessage, PetMode } from '../types/assistant'

const props = defineProps<{
  messages: ChatMessage[]
  mode: PetMode
  providerLabel: string
  permissionLevel: number
  auditTrail: AuditEntry[]
}>()

defineEmits<{
  close: []
}>()

const modeLabel = computed(() => {
  const map: Record<PetMode, string> = {
    idle: '待命',
    listening: '聆听中',
    thinking: '思考中',
    speaking: '回复中',
    guarded: '警戒'
  }

  return map[props.mode]
})

const visibleMessages = computed(() => props.messages.slice(-8))
const visibleAudit = computed(() => props.auditTrail.slice(0, 3))

const formatTime = (value: number) =>
  new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit'
  }).format(value)
</script>

<template>
  <section class="chat-panel">
    <header class="panel-header">
      <div>
        <p class="eyebrow">Phase 2-5 Console</p>
        <h2>对话与审计</h2>
      </div>
      <button class="close-btn" type="button" @click="$emit('close')">
        收起
      </button>
    </header>

    <div class="status-row">
      <span class="status-pill">{{ modeLabel }}</span>
      <span class="status-pill status-provider">{{ providerLabel }}</span>
      <span class="status-pill">L{{ permissionLevel }} 白名单</span>
    </div>

    <div class="messages">
      <article
        v-for="message in visibleMessages"
        :key="message.id"
        :class="['message', message.role]"
      >
        <div class="message-meta">
          <span>{{ message.role === 'user' ? '你' : '企鹅助手' }}</span>
          <time>{{ formatTime(message.createdAt) }}</time>
        </div>
        <p>{{ message.content }}</p>
      </article>

      <div v-if="visibleMessages.length === 0" class="empty">
        现在还没有对话记录。你可以输入文字，或者按住语音键直接和她说话。
      </div>
    </div>

    <section class="audit-panel">
      <div class="audit-header">
        <h3>安全审计</h3>
        <span>最近 3 条</span>
      </div>

      <div v-if="visibleAudit.length === 0" class="audit-empty">
        尚未产生审计记录。
      </div>

      <article
        v-for="entry in visibleAudit"
        :key="entry.id"
        class="audit-entry"
      >
        <div class="audit-topline">
          <strong>{{ entry.action }}</strong>
          <span>{{ entry.outcome }}</span>
        </div>
        <p>{{ entry.detail }}</p>
      </article>
    </section>
  </section>
</template>

<style scoped>
.chat-panel {
  width: min(100%, 360px);
  padding: 18px;
  border-radius: 28px;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.94), rgba(242, 249, 252, 0.98));
  color: #143040;
  box-shadow:
    0 24px 48px rgba(10, 24, 37, 0.16),
    inset 0 1px 0 rgba(255, 255, 255, 0.72);
}

.panel-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: flex-start;
}

.panel-header h2,
.audit-header h3 {
  margin: 4px 0 0;
  font-size: 20px;
}

.eyebrow {
  margin: 0;
  color: #5c7d8c;
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.close-btn {
  padding: 10px 12px;
  border: none;
  border-radius: 14px;
  background: rgba(20, 48, 64, 0.08);
  color: #17384b;
  cursor: pointer;
}

.status-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 16px 0;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  min-height: 30px;
  padding: 0 12px;
  border-radius: 999px;
  background: rgba(17, 87, 122, 0.09);
  color: #17445b;
  font-size: 12px;
}

.status-provider {
  background: rgba(36, 176, 139, 0.12);
}

.messages {
  display: grid;
  gap: 10px;
  max-height: 260px;
  overflow-y: auto;
  padding-right: 4px;
}

.message {
  padding: 12px 14px;
  border-radius: 18px;
}

.message.user {
  margin-left: 28px;
  background: linear-gradient(135deg, #0f7aa5, #1798aa);
  color: #f4fbff;
}

.message.assistant,
.message.system {
  margin-right: 28px;
  background: rgba(15, 54, 77, 0.08);
}

.message-meta {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
  color: rgba(20, 48, 64, 0.64);
  font-size: 11px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.message p,
.audit-entry p {
  margin: 0;
  line-height: 1.5;
  white-space: pre-wrap;
}

.empty,
.audit-empty {
  padding: 18px;
  border-radius: 18px;
  background: rgba(14, 48, 68, 0.06);
  color: rgba(20, 48, 64, 0.72);
  text-align: center;
}

.audit-panel {
  margin-top: 18px;
  padding: 14px;
  border-radius: 20px;
  background: rgba(17, 38, 48, 0.05);
}

.audit-header,
.audit-topline {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: center;
}

.audit-header {
  margin-bottom: 10px;
  color: rgba(20, 48, 64, 0.74);
  font-size: 12px;
}

.audit-entry {
  padding: 12px 0;
  border-top: 1px solid rgba(18, 58, 76, 0.08);
}

.audit-entry:first-of-type {
  border-top: none;
  padding-top: 0;
}

.audit-topline strong {
  font-size: 13px;
}

.audit-topline span {
  color: rgba(20, 48, 64, 0.62);
  font-size: 11px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
</style>
