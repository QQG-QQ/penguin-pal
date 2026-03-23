<script setup lang="ts">
import { computed } from 'vue'
import type { ResearchBriefSnapshot, ResearchFundQuote } from '../types/assistant'

const props = defineProps<{
  brief: ResearchBriefSnapshot
  loading: boolean
}>()

const emit = defineEmits<{
  close: []
  refresh: []
}>()

interface ParsedAnalysisSection {
  title: string
  paragraphs: string[]
}

const splitReadableParagraphs = (text: string) => {
  const normalized = text.trim()
  if (!normalized) return []

  const paragraphs = normalized
    .split(/\n{2,}/)
    .map((item) => item.trim())
    .filter(Boolean)

  if (paragraphs.length > 1) {
    return paragraphs
  }

  if (normalized.length < 120) {
    return [normalized]
  }

  const sentences = normalized
    .split(/(?<=[。！？；])/)
    .map((item) => item.trim())
    .filter(Boolean)

  if (sentences.length <= 2) {
    return [normalized]
  }

  const chunks: string[] = []
  for (let index = 0; index < sentences.length; index += 2) {
    chunks.push(sentences.slice(index, index + 2).join(' '))
  }
  return chunks
}

const analysisStatusText = computed(() => {
  if (props.brief.analysisStatus === 'ready') return '已生成'
  if (props.brief.analysisStatus === 'error') return '生成失败'
  if (props.brief.analysisStatus === 'disabled') return '未启用'
  return '暂不可用'
})

const analysisText = computed(() => props.brief.analysisResult?.trim() ?? '')

const parsedAnalysisSections = computed<ParsedAnalysisSection[]>(() => {
  const text = analysisText.value.replace(/\r/g, '')
  if (!text) return []

  const blocks = text
    .split(/(?=【[^】]+】)/)
    .map((item) => item.trim())
    .filter(Boolean)

  if (!blocks.length) {
    return [
      {
        title: '分析结果',
        paragraphs: text
          .split(/\n{2,}/)
          .map((item) => item.trim())
          .filter(Boolean)
      }
    ]
  }

  return blocks.map((block) => {
    const match = block.match(/^【([^】]+)】\s*([\s\S]*)$/)
    if (!match) {
      return {
        title: '分析结果',
        paragraphs: [block]
      }
    }

    return {
      title: match[1].trim(),
      paragraphs: splitReadableParagraphs(match[2])
    }
  })
})

const leadSection = computed(() => parsedAnalysisSections.value[0] ?? null)
const detailSections = computed(() => parsedAnalysisSections.value.slice(1))
const fundQuotes = computed(() => props.brief.fundQuotes ?? [])
const quotedFunds = computed(() =>
  fundQuotes.value.filter((item) => typeof item.changePercent === 'number')
)
const fundChangeScale = computed(() => {
  const max = Math.max(...quotedFunds.value.map((item) => Math.abs(item.changePercent ?? 0)), 0)
  return max > 0 ? max : 1
})

const formatPercent = (value?: number | null) => {
  if (typeof value !== 'number' || Number.isNaN(value)) {
    return '暂无'
  }
  return `${value >= 0 ? '+' : ''}${value.toFixed(2)}%`
}

const fundTrend = (quote: ResearchFundQuote) => {
  const value = quote.changePercent ?? 0
  if (value > 0.01) return 'up'
  if (value < -0.01) return 'down'
  return 'flat'
}

const fundBarStyle = (quote: ResearchFundQuote) => {
  const value = quote.changePercent ?? 0
  const scale = fundChangeScale.value
  const width = `${Math.max(8, (Math.abs(value) / scale) * 50)}%`

  if (value >= 0) {
    return {
      left: '50%',
      width
    }
  }

  return {
    left: `calc(50% - ${width})`,
    width
  }
}

const assetTypeLabel = (quote: ResearchFundQuote) => {
  switch (quote.assetType) {
    case 'stock':
      return '股票'
    case 'etf':
      return 'ETF'
    case 'fund':
      return '基金'
    default:
      return '资产'
  }
}

const assetTypeIcon = (quote: ResearchFundQuote) => {
  switch (quote.assetType) {
    case 'stock':
      return '📈'
    case 'etf':
      return '🧭'
    case 'fund':
      return '💹'
    default:
      return '📊'
  }
}
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

    <section class="research-overview">
      <article class="research-hero-card">
        <div class="research-analysis-header">
          <div>
            <p class="research-card-eyebrow">AI 自动分析</p>
            <h2>{{ leadSection?.title ?? '分析状态' }}</h2>
            <p v-if="brief.analysisProviderLabel" class="research-provider-line">
              当前由 {{ brief.analysisProviderLabel }} 生成
            </p>
          </div>
          <span class="research-analysis-badge" :data-status="brief.analysisStatus">
            {{ analysisStatusText }}
          </span>
        </div>

        <div v-if="leadSection" class="research-lead-copy">
          <p v-for="paragraph in leadSection.paragraphs" :key="paragraph">{{ paragraph }}</p>
        </div>
        <p v-else-if="brief.analysisNotice" class="research-analysis-notice">
          {{ brief.analysisNotice }}
        </p>
        <p v-else class="research-analysis-notice">
          当前没有可展示的 AI 分析结果。
        </p>
      </article>

      <aside class="research-status-card">
        <p class="research-card-eyebrow">简报状态</p>
        <ul class="research-status-list">
          <li>
            <span>更新状态</span>
            <strong>{{ brief.hasUpdates ? '今日有新变化' : '今日已读' }}</strong>
          </li>
          <li>
            <span>生成时间</span>
            <strong>{{ new Date(props.brief.generatedAt).toLocaleString() }}</strong>
          </li>
          <li v-if="brief.analysisProviderLabel">
            <span>分析来源</span>
            <strong>{{ brief.analysisProviderLabel }}</strong>
          </li>
        </ul>
        <p v-if="brief.updateSummary" class="research-status-note">
          {{ brief.updateSummary }}
        </p>
        <p v-if="brief.analysisStatus !== 'ready' && brief.analysisNotice" class="research-status-error">
          {{ brief.analysisNotice }}
        </p>
      </aside>
    </section>

    <section v-if="fundQuotes.length" class="research-fund-card">
      <div class="research-fund-header">
        <div>
          <p class="research-card-eyebrow">自选资产图表</p>
          <h3>当前自选资产涨跌快照</h3>
          <p>这里会统一显示股票、ETF 和基金的当前涨跌情况，先看波动，再决定今天先拆哪一类逻辑。</p>
        </div>
      </div>

      <div class="research-fund-list">
        <article
          v-for="quote in fundQuotes"
          :key="`${quote.code}-${quote.name}`"
          class="research-fund-row"
          :data-trend="fundTrend(quote)"
        >
          <div class="research-fund-meta">
            <strong>
              <span class="research-asset-icon">{{ assetTypeIcon(quote) }}</span>
              {{ quote.name }}
            </strong>
            <span>
              <span class="research-asset-pill" :data-type="quote.assetType">
                {{ assetTypeLabel(quote) }}
              </span>
              {{ quote.code }}
            </span>
          </div>

          <div class="research-fund-chart">
            <div class="research-fund-track">
              <span class="research-fund-zero"></span>
              <span
                v-if="typeof quote.changePercent === 'number'"
                class="research-fund-fill"
                :style="fundBarStyle(quote)"
              ></span>
            </div>
          </div>

          <div class="research-fund-value">
            <strong>{{ formatPercent(quote.changePercent) }}</strong>
            <span>{{ quote.estimateTime ?? '暂无估值时间' }}</span>
          </div>

          <p v-if="quote.note" class="research-fund-note">{{ quote.note }}</p>
        </article>
      </div>
    </section>

    <section v-if="detailSections.length" class="research-grid">
      <article
        v-for="section in detailSections"
        :key="section.title"
        class="research-card"
      >
        <p class="research-card-eyebrow">分析模块</p>
        <h3>{{ section.title }}</h3>
        <div class="research-section-copy">
          <p v-for="paragraph in section.paragraphs" :key="paragraph">{{ paragraph }}</p>
        </div>
      </article>
    </section>
  </section>
</template>

<style scoped>
.research-surface {
  min-height: 100vh;
  padding: 28px;
  background:
    radial-gradient(circle at top right, rgba(88, 176, 255, 0.22), transparent 24%),
    radial-gradient(circle at left bottom, rgba(255, 206, 122, 0.16), transparent 22%),
    linear-gradient(180deg, #f7fbff 0%, #eef4ff 46%, #eaf1fb 100%);
  color: #17283d;
  box-sizing: border-box;
}

.research-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  margin-bottom: 20px;
}

.research-card-eyebrow {
  margin: 0 0 8px;
  font-size: 11px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: #6783a3;
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
  flex-wrap: wrap;
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
  transition: transform 160ms ease, box-shadow 160ms ease, background 160ms ease;
}

.ghost-button:hover:not(:disabled) {
  transform: translateY(-1px);
  background: rgba(255, 255, 255, 0.9);
  box-shadow: 0 10px 24px rgba(42, 76, 128, 0.1);
}

.research-overview {
  display: grid;
  grid-template-columns: minmax(0, 2fr) minmax(280px, 0.9fr);
  gap: 16px;
  margin-bottom: 16px;
}

.research-fund-card {
  margin-bottom: 16px;
  padding: 20px;
  border-radius: 22px;
  border: 1px solid rgba(20, 34, 54, 0.08);
  background: rgba(255, 255, 255, 0.9);
  box-shadow: 0 20px 44px rgba(42, 76, 128, 0.09);
}

.research-fund-header h3 {
  margin: 0 0 8px;
  color: #12263c;
  font-size: 21px;
}

.research-fund-header p:last-child {
  margin-bottom: 0;
  color: #5f7996;
}

.research-fund-list {
  display: grid;
  gap: 14px;
  margin-top: 16px;
}

.research-fund-row {
  display: grid;
  grid-template-columns: minmax(160px, 1.1fr) minmax(220px, 1.6fr) minmax(120px, 0.8fr);
  gap: 14px;
  align-items: center;
  padding: 14px 16px;
  border-radius: 18px;
  background: rgba(246, 250, 255, 0.88);
}

.research-fund-meta {
  display: grid;
  gap: 4px;
}

.research-fund-meta strong {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  color: #18304a;
}

.research-fund-meta span,
.research-fund-value span {
  font-size: 12px;
  color: #6d84a0;
}

.research-asset-icon {
  font-size: 16px;
}

.research-asset-pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 42px;
  margin-right: 8px;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  background: rgba(90, 128, 175, 0.12);
  color: #35587f;
}

.research-asset-pill[data-type='stock'] {
  background: rgba(76, 141, 255, 0.14);
  color: #245fbb;
}

.research-asset-pill[data-type='etf'] {
  background: rgba(83, 194, 165, 0.14);
  color: #1c7f66;
}

.research-asset-pill[data-type='fund'] {
  background: rgba(231, 159, 57, 0.14);
  color: #946019;
}

.research-fund-chart {
  width: 100%;
}

.research-fund-track {
  position: relative;
  height: 12px;
  border-radius: 999px;
  background: linear-gradient(90deg, rgba(236, 94, 94, 0.1) 0 50%, rgba(72, 179, 118, 0.1) 50% 100%);
  overflow: hidden;
}

.research-fund-zero {
  position: absolute;
  inset: 0 auto 0 50%;
  width: 1px;
  background: rgba(23, 40, 61, 0.16);
}

.research-fund-fill {
  position: absolute;
  top: 0;
  bottom: 0;
  border-radius: 999px;
  background: linear-gradient(90deg, #4ebf85 0%, #2e9d68 100%);
}

.research-fund-row[data-trend='down'] .research-fund-fill {
  background: linear-gradient(90deg, #d86c6c 0%, #c94949 100%);
}

.research-fund-row[data-trend='flat'] .research-fund-fill {
  background: linear-gradient(90deg, #7a97b8 0%, #6283a8 100%);
}

.research-fund-value {
  display: grid;
  justify-items: end;
  gap: 4px;
}

.research-fund-value strong {
  font-size: 16px;
  color: #19324d;
}

.research-fund-note {
  grid-column: 1 / -1;
  margin: 0;
  padding: 10px 12px;
  border-radius: 14px;
  background: rgba(255, 243, 228, 0.72);
  color: #8c5a1f;
  font-size: 13px;
  line-height: 1.6;
}

.research-hero-card,
.research-status-card,
.research-card {
  border-radius: 22px;
  border: 1px solid rgba(20, 34, 54, 0.08);
  background: rgba(255, 255, 255, 0.9);
  box-shadow: 0 20px 44px rgba(42, 76, 128, 0.09);
  backdrop-filter: blur(14px);
}

.research-hero-card {
  padding: 22px 22px 20px;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.95), rgba(245, 250, 255, 0.94)),
    rgba(255, 255, 255, 0.9);
}

.research-status-card {
  padding: 18px;
}

.research-status-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  gap: 12px;
}

.research-status-list li {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
  padding-bottom: 10px;
  border-bottom: 1px solid rgba(20, 34, 54, 0.08);
}

.research-status-list li:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

.research-status-list span {
  color: #63809e;
  font-size: 13px;
}

.research-status-list strong {
  color: #1b314b;
  font-size: 14px;
  text-align: right;
}

.research-status-note,
.research-status-error {
  margin: 14px 0 0;
  padding: 12px 14px;
  border-radius: 16px;
  line-height: 1.6;
}

.research-status-note {
  background: rgba(236, 245, 255, 0.8);
  color: #325f80;
}

.research-status-error {
  background: rgba(255, 239, 239, 0.88);
  color: #9d4848;
}

.research-provider-line {
  margin: 4px 0 0;
  color: #5f7996;
  font-size: 13px;
}

.research-hero-card h2,
.research-card h3 {
  margin: 0 0 10px;
  color: #12263c;
}

.research-hero-card h2 {
  font-size: 28px;
}

.research-card h3 {
  font-size: 19px;
}

.research-card p,
.research-hero-card p {
  margin: 0 0 12px;
  line-height: 1.72;
}

.research-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 16px;
}

.research-analysis-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: flex-start;
}

.research-analysis-badge {
  padding: 6px 10px;
  border-radius: 999px;
  font-size: 12px;
  background: rgba(237, 244, 255, 0.9);
  color: #35617d;
  border: 1px solid rgba(53, 97, 125, 0.08);
}

.research-analysis-badge[data-status='ready'] {
  background: rgba(227, 248, 233, 0.92);
  color: #286246;
}

.research-analysis-badge[data-status='error'] {
  background: rgba(255, 232, 232, 0.92);
  color: #a24646;
}

.research-analysis-notice {
  color: #58789b;
}

.research-lead-copy p:last-child,
.research-section-copy p:last-child {
  margin-bottom: 0;
}

.research-card {
  padding: 18px;
}

.research-section-copy {
  display: grid;
  gap: 10px;
}

.research-section-copy p,
.research-lead-copy p {
  max-width: 62ch;
}

@media (max-width: 720px) {
  .research-surface {
    padding: 18px;
  }

  .research-header {
    flex-direction: column;
  }

  .research-overview {
    grid-template-columns: 1fr;
  }

  .research-fund-row {
    grid-template-columns: 1fr;
  }

  .research-fund-value {
    justify-items: start;
  }

  .research-hero-card h2 {
    font-size: 23px;
  }
}
</style>
