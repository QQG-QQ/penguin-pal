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

const visibleMessages = computed(() => props.messages.slice(-4))
const visibleAudit = computed(() => props.auditTrail.slice(0, 2))

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
        <p class="eyebrow">Pet Chat</p>
        <h2>最近对话</h2>
      </div>
      <button class="close-btn" type="button" @click="$emit('close')">
        收起
      </button>
    </header>

    <div class="status-row">
      <span class="status-pill">{{ modeLabel }}</span>
      <span class="status-pill status-provider">{{ providerLabel }}</span>
      <span class="status-pill">L{{ permissionLevel }}</span>
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
        现在还没有对话记录。输入文字或按住说话，她就会开始回应。
      </div>
    </div>

    <section class="audit-panel">
      <div class="audit-header">
        <h3>安全审计</h3>
        <span>最近 2 条</span>
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
  width: 100%;
  padding: 14px;
  border-radius: 22px;
  background: rgba(255, 255, 255, 0.92);
  color: #153748;
  box-shadow:
    0 14px 26px rgba(8, 22, 34, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.78);
}

.panel-header,
.audit-header,
.audit-topline {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: center;
}

.panel-header h2,
.audit-header h3 {
  margin: 4px 0 0;
  font-size: 18px;
}

.eyebrow {
  margin: 0;
  color: #5c7d8c;
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.close-btn {
  min-height: 32px;
  padding: 0 11px;
  border: none;
  border-radius: 999px;
  background: rgba(20, 48, 64, 0.08);
  color: #17384b;
  cursor: pointer;
}

.status-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 12px 0;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 0 11px;
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
  gap: 8px;
  max-height: 152px;
  overflow-y: auto;
}

.message {
  padding: 10px 12px;
  border-radius: 16px;
}

.message.user {
  margin-left: 18px;
  background: linear-gradient(135deg, #0f7aa5, #1798aa);
  color: #f4fbff;
}

.message.assistant,
.message.system {
  margin-right: 18px;
  background: rgba(15, 54, 77, 0.08);
}

.message-meta {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 5px;
  color: rgba(20, 48, 64, 0.64);
  font-size: 11px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.message p,
.audit-entry p {
  margin: 0;
  line-height: 1.45;
  white-space: pre-wrap;
}

.empty,
.audit-empty {
  padding: 14px;
  border-radius: 16px;
  background: rgba(14, 48, 68, 0.06);
  color: rgba(20, 48, 64, 0.72);
  text-align: center;
}

.audit-panel {
  margin-top: 12px;
  padding: 12px;
  border-radius: 18px;
  background: rgba(17, 38, 48, 0.05);
}

.audit-header {
  margin-bottom: 8px;
  color: rgba(20, 48, 64, 0.74);
  font-size: 12px;
}

.audit-entry {
  padding-top: 10px;
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
