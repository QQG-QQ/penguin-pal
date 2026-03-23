<script setup lang="ts">
import type { ResearchBriefSnapshot } from '../types/assistant'

const props = defineProps<{
  brief: ResearchBriefSnapshot
  loading: boolean
}>()

const emit = defineEmits<{
  close: []
  refresh: []
}>()
</script>

<template>
  <section class="research-surface">
    <header class="research-header">
      <div>
        <p class="research-eyebrow">本地投研模式</p>
        <h1>{{ brief.title }}</h1>
        <p>{{ brief.summary }}</p>
        <p class="research-meta">
          生成时间：{{ new Date(props.brief.generatedAt).toLocaleString() }}
          <span v-if="brief.updateSummary"> · {{ brief.updateSummary }}</span>
        </p>
      </div>
      <div class="research-actions">
        <span class="research-badge" :data-state="brief.hasUpdates ? 'fresh' : 'steady'">
          {{ brief.hasUpdates ? '今日有新变化' : '今日已读' }}
        </span>
        <button type="button" class="ghost-button" :disabled="loading" @click="emit('refresh')">
          {{ loading ? '生成中...' : '刷新简报' }}
        </button>
        <button type="button" class="ghost-button" @click="emit('close')">关闭窗口</button>
      </div>
    </header>

    <section v-if="brief.alerts.length" class="research-alerts">
      <article
        v-for="alert in brief.alerts"
        :key="alert.id"
        class="research-alert"
        :data-severity="alert.severity"
      >
        <strong>{{ alert.title }}</strong>
        <p>{{ alert.summary }}</p>
      </article>
    </section>

    <section class="research-grid">
      <article v-for="section in brief.sections" :key="section.title" class="research-card">
        <h2>{{ section.title }}</h2>
        <p>{{ section.summary }}</p>
        <ul>
          <li v-for="bullet in section.bullets" :key="bullet">{{ bullet }}</li>
        </ul>
      </article>
    </section>

    <section v-if="brief.memoryHints.length" class="research-memory">
      <h2>长期记忆已加载</h2>
      <ul>
        <li v-for="item in brief.memoryHints" :key="item">{{ item }}</li>
      </ul>
    </section>
  </section>
</template>

<style scoped>
.research-surface {
  min-height: 100vh;
  padding: 24px;
  background:
    radial-gradient(circle at top right, rgba(88, 176, 255, 0.18), transparent 28%),
    linear-gradient(180deg, #f8fbff 0%, #eff5ff 100%);
  color: #142236;
  box-sizing: border-box;
}

.research-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  margin-bottom: 20px;
}

.research-eyebrow {
  margin: 0 0 6px;
  font-size: 12px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: #58789b;
}

.research-header h1 {
  margin: 0 0 8px;
  font-size: 28px;
}

.research-header p {
  margin: 0;
  max-width: 720px;
  line-height: 1.6;
}

.research-meta {
  margin-top: 8px !important;
  font-size: 13px;
  color: #58789b;
}

.research-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.research-badge {
  padding: 8px 12px;
  border-radius: 999px;
  font-size: 12px;
  background: rgba(255, 255, 255, 0.82);
  border: 1px solid rgba(20, 34, 54, 0.12);
}

.research-badge[data-state='fresh'] {
  color: #9a4d00;
  background: rgba(255, 241, 222, 0.92);
  border-color: rgba(201, 136, 39, 0.26);
}

.research-badge[data-state='steady'] {
  color: #2f6178;
  background: rgba(236, 247, 255, 0.92);
  border-color: rgba(76, 142, 183, 0.2);
}

.ghost-button {
  border: 1px solid rgba(20, 34, 54, 0.18);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.72);
  padding: 10px 16px;
  cursor: pointer;
}

.research-alerts {
  display: grid;
  gap: 10px;
  margin-bottom: 18px;
}

.research-alert {
  padding: 14px 16px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.88);
  border: 1px solid rgba(20, 34, 54, 0.08);
}

.research-alert[data-severity='watch'] {
  border-color: rgba(233, 151, 43, 0.28);
  background: rgba(255, 245, 223, 0.88);
}

.research-alert[data-severity='urgent'] {
  border-color: rgba(205, 79, 79, 0.28);
  background: rgba(255, 235, 235, 0.88);
}

.research-alert strong,
.research-card h2,
.research-memory h2 {
  display: block;
  margin-bottom: 8px;
}

.research-alert p,
.research-card p {
  margin: 0 0 10px;
  line-height: 1.6;
}

.research-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 14px;
}

.research-card,
.research-memory {
  padding: 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.9);
  border: 1px solid rgba(20, 34, 54, 0.08);
  box-shadow: 0 18px 38px rgba(42, 76, 128, 0.08);
}

.research-card ul,
.research-memory ul {
  margin: 0;
  padding-left: 18px;
  line-height: 1.7;
}

.research-memory {
  margin-top: 16px;
}

@media (max-width: 720px) {
  .research-surface {
    padding: 18px;
  }

  .research-header {
    flex-direction: column;
  }
}
</style>
