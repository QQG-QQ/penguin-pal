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
      <p class="eyebrow">Action Gate</p>
      <h2>受控动作</h2>
      <p>
        AI 只能建议动作，真正执行必须走白名单。高风险动作一律要求人工确认。
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
      当前权限等级：L{{ permissionLevel }}。桌宠不会开放自由脚本执行或未确认的高危系统操作。
    </div>
  </section>
</template>

<style scoped>
.control-panel {
  width: 100%;
  padding: 14px;
  border-radius: 22px;
  background: linear-gradient(180deg, rgba(7, 28, 41, 0.96), rgba(10, 24, 38, 0.98));
  color: #ebf7fb;
  box-shadow:
    0 16px 32px rgba(5, 14, 24, 0.2),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
}

.panel-copy h2 {
  margin: 4px 0 8px;
  font-size: 18px;
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
  gap: 8px;
  margin: 14px 0;
}

.action-card {
  padding: 12px;
  border: 1px solid rgba(186, 233, 241, 0.08);
  border-radius: 18px;
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
  gap: 10px;
  align-items: center;
}

.action-topline strong {
  font-size: 14px;
}

.action-topline span,
.action-foot span {
  color: rgba(215, 238, 245, 0.72);
  font-size: 11px;
}

.action-summary {
  display: block;
  margin: 6px 0 8px;
  color: rgba(235, 247, 251, 0.84);
  line-height: 1.45;
  font-size: 13px;
}

.security-note {
  padding: 12px 14px;
  border-radius: 16px;
  background: rgba(255, 177, 83, 0.12);
  color: #ffd9a4;
  font-size: 12px;
  line-height: 1.5;
}
</style>
