<script lang="ts">
interface Tokens { input: number; output: number; cached: number }
interface Entry extends Tokens { model: string; provider: string | null; time: number | null; requestCount: number }
export interface Report {
  report: { entries: Entry[]; files: number; errors: number; limited: boolean }
}
</script>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'

const props = defineProps<{ report: Report | null; since: number | null; busy: boolean }>()
const emit = defineEmits<{ range: [since: number | null] }>()
const range = ref(props.since === null ? 'all' : daysAgo(0) === props.since ? 'today' : 'week')
const model = ref('')
const provider = ref('')
const tab = ref<'requests' | 'models' | 'providers'>('requests')
const page = ref(0)
const number = new Intl.NumberFormat('zh-CN')
const pageSize = 30

function daysAgo(days: number) {
  const date = new Date()
  date.setDate(date.getDate() - days)
  return date.setHours(0, 0, 0, 0)
}
function changeRange() {
  emit('range', range.value === 'all' ? null : daysAgo(range.value === 'today' ? 0 : 6))
}
function totalTokens(tokens: Tokens) { return tokens.input + tokens.output }
function label(row: Entry) { return row.provider || '会话日志' }
function when(ms: number | null) { return ms == null ? '—' : new Date(ms).toLocaleString('zh-CN', { hour12: false }) }
type Totals = ReturnType<typeof emptyTotals>
function emptyTotals() { return { requests: 0, input: 0, output: 0, cached: 0 } }
function add(total: Totals, row: Entry) {
  total.requests += row.requestCount
  total.input += row.input
  total.output += row.output
  total.cached += row.cached
}

const entries = computed(() => props.report?.report.entries ?? [])
const models = computed(() => [...new Set(entries.value.map(row => row.model))].sort())
const providers = computed(() => [...new Set(entries.value.map(label))].sort())
const selected = computed(() => entries.value.filter(row => (!model.value || row.model === model.value) && (!provider.value || label(row) === provider.value)))
const total = computed(() => {
  const result = emptyTotals()
  for (const row of selected.value) add(result, row)
  return result
})
const groups = computed(() => {
  const result = new Map<string, Totals>()
  for (const row of selected.value) {
    const name = tab.value === 'providers' ? label(row) : row.model
    if (!result.has(name)) result.set(name, emptyTotals())
    add(result.get(name)!, row)
  }
  return [...result.entries()].sort((a, b) => b[1].requests - a[1].requests)
})
const buckets = computed(() => {
  const result = new Map<number, number>()
  const hourly = range.value === 'today'
  const now = new Date()
  if (hourly) {
    for (let hour = 0; hour <= now.getHours(); hour++) result.set(new Date(daysAgo(0)).setHours(hour), 0)
  } else if (range.value === 'week') {
    for (let day = 6; day >= 0; day--) result.set(daysAgo(day), 0)
  }
  for (const row of selected.value) {
    if (row.time == null) continue
    const date = new Date(row.time)
    const start = hourly ? date.setMinutes(0, 0, 0) : date.setHours(0, 0, 0, 0)
    result.set(start, (result.get(start) ?? 0) + totalTokens(row))
  }
  return [...result.entries()].sort((a, b) => a[0] - b[0]).slice(-31).map(([time, value]) => ({
    value, time,
    label: new Date(time).toLocaleString('zh-CN', hourly ? { hour: '2-digit', minute: '2-digit', hour12: false } : { month: 'numeric', day: 'numeric' }),
  }))
})
const peak = computed(() => Math.max(1, ...buckets.value.map(bucket => bucket.value)))
const pageCount = computed(() => Math.max(1, Math.ceil(selected.value.length / pageSize)))
const rows = computed(() => selected.value.slice(page.value * pageSize, (page.value + 1) * pageSize))
watch([model, provider, () => props.report], () => { page.value = 0 })
watch(models, items => { if (model.value && !items.includes(model.value)) model.value = '' })
watch(providers, items => { if (provider.value && !items.includes(provider.value)) provider.value = '' })
</script>

<template>
  <div class="usage">
    <div class="filters">
      <select v-model="range" aria-label="用量时间范围" :disabled="busy" @change="changeRange">
        <option value="today">今天</option><option value="week">最近 7 天</option><option value="all">全部已扫描</option>
      </select>
      <select v-model="provider" aria-label="筛选供应商">
        <option value="">全部供应商</option><option v-for="name in providers" :key="name">{{ name }}</option>
      </select>
      <select v-model="model" aria-label="筛选模型">
        <option value="">全部模型</option><option v-for="name in models" :key="name">{{ name }}</option>
      </select>
    </div>
    <p v-if="!report && busy" role="status">正在统计本机会话日志…</p>
    <template v-if="report">
      <section class="overview" aria-label="用量概览">
        <div><span>总消耗 Token</span><strong>{{ number.format(totalTokens(total)) }}</strong><small>输入（含缓存）＋输出</small></div>
        <div><span>请求数</span><strong>{{ number.format(total.requests) }}</strong></div>
        <div><span>新增输入与输出</span><strong>{{ number.format(total.input - total.cached + total.output) }}</strong><small>Token</small></div>
        <div><span>输入缓存命中率</span><strong>{{ total.input ? (100 * total.cached / total.input).toFixed(1) + '%' : '—' }}</strong></div>
      </section>
      <section v-if="selected.length && buckets.some(bucket => bucket.value > 0)" class="chart">
        <h2>总 Token 趋势 <small>{{ range === 'today' ? '按小时' : range === 'all' ? '最近 31 个记录日' : '按天' }}</small></h2>
        <div class="bars" :style="{ gridTemplateColumns: 'repeat(' + buckets.length + ', minmax(0, 1fr))' }">
          <div v-for="bucket in buckets" :key="bucket.time" class="bar-column" :title="bucket.label + ' · ' + number.format(bucket.value) + ' Token'">
            <div class="track"><div class="bar" :style="{ height: (100 * bucket.value / peak) + '%' }"></div></div>
            <small>{{ bucket.label }}</small>
          </div>
        </div>
      </section>
      <section class="records">
        <div class="tabs" role="group" aria-label="用量明细类型">
          <button :aria-pressed="tab === 'requests'" @click="tab = 'requests'">请求明细</button>
          <button :aria-pressed="tab === 'providers'" @click="tab = 'providers'">供应商汇总</button>
          <button :aria-pressed="tab === 'models'" @click="tab = 'models'">模型汇总</button>
        </div>
        <p v-if="!selected.length" class="empty">此范围内暂无可统计的请求。</p>
        <div v-else class="table-scroll">
          <table v-if="tab === 'requests'">
            <thead><tr><th>时间</th><th>供应商</th><th>模型</th><th>新增输入</th><th>输出</th><th>缓存命中</th><th>总 Token</th></tr></thead>
            <tbody><tr v-for="(row, index) in rows" :key="index">
              <td>{{ when(row.time) }}</td><td>{{ label(row) }}</td><td>{{ row.model }}<small v-if="row.requestCount > 1"> · {{ row.requestCount }} 次</small></td>
              <td>{{ number.format(row.input - row.cached) }}</td><td>{{ number.format(row.output) }}</td><td>{{ number.format(row.cached) }}</td>
              <td>{{ number.format(totalTokens(row)) }}</td>
            </tr></tbody>
          </table>
          <table v-else>
            <thead><tr><th>{{ tab === 'models' ? '模型' : '供应商' }}</th><th>请求数</th><th>新增输入</th><th>输出</th><th>缓存命中</th><th>总 Token</th></tr></thead>
            <tbody><tr v-for="[name, row] in groups" :key="name">
              <td>{{ name }}</td><td>{{ number.format(row.requests) }}</td><td>{{ number.format(row.input - row.cached) }}</td>
              <td>{{ number.format(row.output) }}</td><td>{{ number.format(row.cached) }}</td><td>{{ number.format(totalTokens(row)) }}</td>
            </tr></tbody>
          </table>
        </div>
        <div v-if="tab === 'requests' && pageCount > 1" class="pager">
          <button :disabled="page === 0" @click="page--">上一页</button><span>{{ page + 1 }} / {{ pageCount }}</span><button :disabled="page + 1 === pageCount" @click="page++">下一页</button>
        </div>
      </section>
      <p class="coverage">本机会话日志 · 扫描 {{ report.report.files }} 个文件<span v-if="report.report.errors"> · {{ report.report.errors }} 个文件读取失败</span><span v-if="report.report.limited"> · 已达扫描上限</span></p>
    </template>
  </div>
</template>

<style scoped>
.usage{--ink:#173e52;--subtle:#526578;display:grid;gap:12px}
.filters,.tabs,.pager{display:flex;flex-wrap:wrap;gap:8px;align-items:center}
select,button{font:inherit;color:var(--ink);border:1px solid #cadce8;border-radius:7px;background:#fff;padding:7px 9px}
select{max-width:100%}
button{cursor:pointer}
button:disabled{opacity:.55;cursor:default}
.overview,.chart,.records{background:white;border:1px solid #d9e3ec;border-radius:12px}
.overview{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));background:#edf6fb}
@media(max-width:850px){.overview{grid-template-columns:repeat(2,minmax(0,1fr))}.overview>div:nth-child(n+3){border-top:1px solid #d9e6ed}}
.overview>div{min-width:0;display:grid;align-content:start;gap:3px;padding:15px 14px}
.overview>div+div{border-left:1px solid #d9e6ed}
.overview strong{font-size:clamp(17px,2vw,25px);font-variant-numeric:tabular-nums;overflow-wrap:anywhere}
.overview span,.overview small,.coverage,.chart small,.empty{color:var(--subtle)}
.chart,.records{padding:14px}
.chart h2{font-size:15px;margin:0 0 13px}
.chart h2 small{margin-left:5px;font-weight:400}
.bars{display:grid;gap:4px}
.bar-column{min-width:0;text-align:center;font-variant-numeric:tabular-nums}
.bar-column small{display:block;margin-top:4px;font-size:9px;overflow:hidden;text-overflow:clip;white-space:nowrap}
.track{height:112px;border-bottom:1px solid #bfd2df;display:flex;align-items:end;background:#f5f9fc}
.bar{width:100%;border-radius:4px 4px 0 0;background:#3286a5}
.tabs button[aria-pressed=true]{background:#e2f1f8;border-color:#3f829e}
.table-scroll{overflow:auto;margin-top:10px}
table{border-collapse:collapse;width:100%;text-align:left;font-size:12px;font-variant-numeric:tabular-nums}
th,td{padding:8px 5px;border-bottom:1px solid #e5edf3;white-space:nowrap}
.pager{justify-content:flex-end;margin-top:10px}
.coverage,.empty{margin:0}
.empty{padding:14px 0}
</style>
