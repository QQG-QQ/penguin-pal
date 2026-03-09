<script setup lang="ts">
import type { DesktopAction } from '../types/assistant'

defineProps<{
  actions: DesktopAction[]
  permissionLevel: number
}>()

const emit = defineEmits<{
  trigger: [action: DesktopAction]
}>()
</script>

<template>
  <section class="control-panel">
    <div class="panel-copy">
      <p class="eyebrow">Phase 4 Security Gate</p>
      <h2>白名单动作</h2>
      <p>
        AI 只能建议动作，真正执行必须走这个白名单面板。高风险动作一律要求人工确认。
      </p>
    </div>

    <div class="action-grid">
      <button
        v-for="action in actions"
        :key="action.id"
        class="action-card"
        type="button"
        :disabled="!action.enabled"
        @click="emit('trigger', action)"
      >
        <span class="action-topline">
          <strong>{{ action.title }}</strong>
          <span>风险 {{ action.riskLevel }}</span>
        </span>
        <span class="action-summary">{{ action.summary }}</span>
        <span class="action-foot">
          <span>最低权限 L{{ action.minimumLevel }}</span>
          <span>{{ action.requiresConfirmation ? '需确认' : '可直接执行' }}</span>
        </span>
      </button>
    </div>

    <div class="security-note">
      当前权限等级：L{{ permissionLevel }}。L3 保留给未来自动化能力，当前版本不会开放自由脚本执行。
    </div>
  </section>
</template>

<style scoped>
.control-panel {
  width: min(100%, 360px);
  padding: 18px;
  border-radius: 28px;
  background:
    linear-gradient(180deg, rgba(6, 23, 35, 0.95), rgba(11, 30, 44, 0.98));
  color: #ebf7fb;
  box-shadow:
    0 20px 40px rgba(5, 14, 24, 0.24),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
}

.panel-copy h2 {
  margin: 4px 0 8px;
  font-size: 20px;
}

.panel-copy p {
  margin: 0;
  color: rgba(235, 247, 251, 0.76);
  line-height: 1.5;
}

.eyebrow {
  margin: 0;
  color: rgba(191, 235, 245, 0.68);
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.action-grid {
  display: grid;
  gap: 10px;
  margin: 16px 0;
}

.action-card {
  padding: 14px;
  border: 1px solid rgba(186, 233, 241, 0.08);
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.06);
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.action-card:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.action-topline,
.action-foot {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.action-topline strong {
  font-size: 15px;
}

.action-topline span,
.action-foot span {
  color: rgba(215, 238, 245, 0.7);
  font-size: 12px;
}

.action-summary {
  display: block;
  margin: 8px 0 10px;
  color: rgba(235, 247, 251, 0.84);
  line-height: 1.5;
}

.security-note {
  padding: 12px 14px;
  border-radius: 18px;
  background: rgba(255, 177, 83, 0.12);
  color: #ffd9a4;
  font-size: 13px;
  line-height: 1.5;
}
</style>
