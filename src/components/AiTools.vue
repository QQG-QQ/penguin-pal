<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import AiUsage from './AiUsage.vue'
import AiMcpForm from './AiMcpForm.vue'
import AiSkillImport from './AiSkillImport.vue'
import AiAccounts from './AiAccounts.vue'
import type { Account, AccountStatus } from './AiAccounts.vue'
import type { Report } from './AiUsage.vue'

type Client = string
type Page = 'tools' | 'accounts' | 'extensions' | 'projects' | 'sessions' | 'usage' | 'backups'

interface Provider { id: string; name: string; official: boolean; model?: string; host?: string; inUse?: boolean; modelChoice?: boolean; savedDirect?: boolean; installed?: boolean }
interface ClientView {
  id: Client; label: string; config: string; current: Provider | null
  error: string | null; providers: Provider[]; executable?: string | null
  mcpSupported?: boolean; mcpSse?: boolean
  supportsDirect?: boolean; canLaunchAuth?: boolean
  configured: boolean; configuredModel?: string; managedByClient?: boolean; sessionLogs?: boolean
}
interface Skill { id: string; name: string; source: string; enabled: boolean }
interface Profile { id: string; name: string; provider: string }
interface Detail {
  provider: ClientView
  mcp: { name: string; transport: string; enabled: boolean; editable: boolean }[] | null
  mcpError: string | null
  skills: { installed: Skill[]; library: Skill[] }
  prompt: { path: string; text: string } | null
  promptError: string | null
  profiles: Profile[]
}
interface Session { id: string; title: string; client: Client; modified: number; cwd: string }
interface Thread { session: Session; messages: { role: string; text: string }[]; limited: boolean }
interface Confirm {
  title: string
  details: string[]
  button?: string
  execute: () => Promise<void>
}
type Operation =
  | { kind: 'provider' | 'nativeModel' | 'profile'; id: string }
  | { kind: 'mcp'; name: string; definition?: Record<string, unknown>; targets?: string[]; replace?: boolean }
  | { kind: 'importSkills'; ids: string[]; targets: string[] }
  | { kind: 'prompt'; text: string }
  | { kind: 'localSkill' }
  | { kind: 'toggleSkill'; id: string; enabled: boolean }

const pages: { id: Page; label: string }[] = [
  { id: 'tools', label: '工具与模型' },
  { id: 'accounts', label: '授权中心' },
  { id: 'extensions', label: '扩展与提示词' },
  { id: 'projects', label: '项目配置' },
  { id: 'sessions', label: '会话' },
  { id: 'usage', label: '用量' },
  { id: 'backups', label: '配置回滚' },
]

const clients = ref<ClientView[]>([])
const client = ref<Client>('codex')
const page = ref<Page>('tools')
const detail = ref<Detail | null>(null)
const busy = ref(false)
const error = ref('')
const notice = ref('')
const infoNote = ref('')
const confirm = ref<Confirm | null>(null)
const profileName = ref('')
const direct = ref<{ name: string; endpoint: string; model: string; key: string } | null>(null)
const accountStatus = ref<AccountStatus | null>(null)
type McpDefinition = { type: 'stdio' | 'http' | 'sse'; command?: string; url?: string; args?: string[]; cwd?: string }
const mcpEditor = ref<{ name: string; definition?: McpDefinition; preservesPrivateFields?: boolean } | null>(null)
const skillPicker = ref(false)
const prompt = ref('')
const query = ref('')
const history = ref<Session[]>([])
const historyLimited = ref(false)
const thread = ref<Thread | null>(null)
const usage = ref<Report | null>(null)
const usageSince = ref<number | null>(new Date().setHours(0, 0, 0, 0))
const showAllApps = ref(false)
const backups = ref<{ id: string; title: string; files: string[]; createdAt: string }[]>([])
const cli = ref<{ executables: string[]; version: string | null } | null>(null)
const currentApp = computed(() => clients.value.find(item => item.id === client.value))
const visibleClients = computed(() => clients.value.filter(item => item.configured || item.executable || item.id === client.value))
const otherClients = computed(() => clients.value.filter(item => !visibleClients.value.includes(item)))
const mcpApps = computed(() => clients.value.filter(item => item.mcpSupported))
const pageTitle = computed(() => pages.find(item => item.id === page.value)?.label)

function request<T>(action: string, args: Record<string, unknown> = {}) {
  return invoke<T>('companion_ai_tools', { request: { action, ...args } })
}

async function task(work: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  notice.value = ''
  infoNote.value = ''
  try {
    await work()
  } catch (reason) {
    error.value = String(reason)
  } finally {
    busy.value = false
  }
}

async function overview() {
  const state = await request<{ clients: ClientView[] }>('overview')
  clients.value = state.clients
}

async function loadHistory() {
  const result = await request<{ sessions: Session[]; limited: boolean }>('listSessions', {
    client: client.value, query: query.value,
  })
  history.value = result.sessions
  historyLimited.value = result.limited
  const preferred = history.value.find(item => item.id === thread.value?.session.id) ?? history.value[0]
  thread.value = null
  if (preferred) thread.value = await request<Thread>('readSession', { id: preferred.id })
}

async function loadPage() {
  const app = client.value
  if (page.value === 'sessions') {
    await loadHistory()
  } else if (page.value === 'usage') {
    usage.value = await request<Report>('usage', { client: app, since: usageSince.value })
  } else if (page.value === 'backups') {
    backups.value = await request<typeof backups.value>('listBackups')
  } else if (page.value === 'accounts') {
    accountStatus.value = await request<AccountStatus>('accountStatus', { client: app })
  } else {
    detail.value = await request<Detail>('inspect', { client: app })
    prompt.value = detail.value.prompt?.text ?? ''
  }
}

function focusPage(next: Page) {
  void task(async () => {
    page.value = next
    direct.value = null
    mcpEditor.value = null
    skillPicker.value = false
    await loadPage()
  })
}

function focusClient(next: Client) {
  void task(async () => {
    client.value = next
    direct.value = null
    accountStatus.value = null
    showAllApps.value = false
    query.value = ''
    history.value = []
    thread.value = null
    detail.value = null
    usage.value = null
    cli.value = null
    mcpEditor.value = null
    skillPicker.value = false
    await loadPage()
  })
}

async function prepare(operation: Operation) {
  if (operation.kind === 'provider' || operation.kind === 'nativeModel') {
    const result = await request<{ changed: boolean }>('switch', { client: client.value, operation })
    await overview()
    await loadPage()
    notice.value = result.changed ? '已更新客户端配置。' : '此配置已经生效。'
    return
  }
  const app = client.value
  const preview = await request<{ title: string; files: string[]; revision: string; summary: string[] }>('preview', { client: app, operation })
  if (!preview.files.length) {
    notice.value = '此配置已经生效。'
    return
  }
  confirm.value = {
    title: preview.title,
    details: [...preview.summary, ...(preview.files.length > 7
      ? [...preview.files.slice(0, 6), '共 ' + preview.files.length + ' 个文件']
      : preview.files)],
    button: '备份并应用',
    execute: async () => {
      await request('apply', { client: app, operation, revision: preview.revision })
      await overview()
      await loadPage()
      if (operation.kind === 'mcp') mcpEditor.value = null
      if (operation.kind === 'importSkills') skillPicker.value = false
      notice.value = operation.kind === 'profile'
        ? '配置已应用。正在运行的客户端会话可能需要重开才能读取新配置。'
        : '配置已保存。可在「配置回滚」中撤销。'
    },
  }
}

function apply(operation: Operation) {
  void task(() => prepare(operation))
}

function saveDirect() {
  void task(async () => {
    if (!direct.value) return
    await request<Provider>('saveDirect', { client: client.value, ...direct.value })
    direct.value = null
    await loadPage()
    notice.value = '中转已保存，可以检查连接或切换。'
  })
}

function startLogin(device: boolean) {
  void task(async () => {
    const result = await request<{ message: string }>('startLogin', { client: client.value, device })
    infoNote.value = result.message
    await loadPage()
  })
}

function captureAccount(label: string) {
  void task(async () => {
    accountStatus.value = await request<AccountStatus>('captureAccount', { client: client.value, label })
    notice.value = '当前官方账号已收录。'
  })
}

function selectAccount(account: Account) {
  void task(async () => {
    const app = client.value
    const preview = await request<{ title: string; changed: boolean; revision: string }>('previewAccount', { client: app, id: account.id })
    if (!preview.changed) {
      notice.value = '这个账号已登录。'
      return
    }
    confirm.value = {
      title: preview.title,
      details: ['桌宠会保存当前登录，然后恢复所选账号。切换后重新打开客户端。'],
      button: '切换账号',
      execute: async () => {
        accountStatus.value = await request<AccountStatus>('applyAccount', { client: app, id: account.id, revision: preview.revision })
        notice.value = '账号已切换。可在「配置回滚」中恢复。'
      },
    }
  })
}

function removeAccount(account: Account) {
  const app = client.value
  confirm.value = {
    title: '移除账号副本「' + account.label + '」？',
    details: ['客户端当前登录仍有效。'],
    button: '移除副本',
    execute: async () => {
      accountStatus.value = await request<AccountStatus>('removeAccount', { client: app, id: account.id })
      notice.value = '账号副本已移除。'
    },
  }
}

function saveMcp(name: string, definition: Record<string, unknown>, targets: string[]) {
  apply({ kind: 'mcp', name, definition, targets, replace: !!mcpEditor.value?.name })
}

function editMcp(name: string) {
  void task(async () => {
    const result = await request<{ definition: McpDefinition; preservesPrivateFields: boolean }>(
      'readMcp', { client: client.value, name },
    )
    mcpEditor.value = { name, ...result }
  })
}

function pickSkill() {
  void task(async () => {
    const result = await request<{ cancelled?: boolean }>('pickSkill')
    if (!result.cancelled) await prepare({ kind: 'localSkill' })
  })
}

function refreshCodexModels() {
  void task(async () => {
    const result = await request<{available: number; usable: number}>('refreshCodexModels')
    infoNote.value = result.usable
      ? `供应商返回 ${result.available} 个模型，已同步 ${result.usable} 个具备 Codex 元数据的模型。重新打开 Codex 后读取新目录。`
      : `供应商返回 ${result.available} 个模型。已保留 Codex 当前目录设置。`
    await loadPage()
  })
}

function check(provider: Provider, useCurrent = false) {
  void task(async () => {
    const result = await request<
      { result: 'clientLogin' | 'noApiKey' } | { result: 'http'; success: boolean; status: number }
    >(
      'testProvider', { client: client.value, id: useCurrent || (provider.inUse && !provider.savedDirect) ? client.value + ':live' : provider.id },
    )
    const name = provider.name
    if (result.result === 'clientLogin') {
      infoNote.value = name + ' 使用客户端账号登录。请在客户端内查看登录状态。'
    } else if (result.result === 'noApiKey') {
      error.value = name + ' 当前配置缺少可用于模型列表检查的 API 凭据。'
    } else if (result.result === 'http') {
      if (result.success) {
        notice.value = name + ' 的模型列表接口可访问。'
      } else {
        error.value = result.status === 401
          ? name + ' 拒绝了本次 API 凭据（HTTP 401）。'
          : name + ' 的模型列表检查失败（HTTP ' + result.status + '）。'
      }
    }
  })
}

function checkCli() {
  void task(async () => {
    cli.value = await request<NonNullable<typeof cli.value>>('checkCli', { client: client.value })
  })
}

function installCli() {
  const app = client.value
  confirm.value = {
    title: '用 npm 安装或更新 ' + (currentApp.value?.label ?? app) + '？',
    details: ['将在新终端打开官方 npm 包的安装流程。如果原工具通过其他方式安装，请先核对路径以免重复安装。'],
    button: '打开安装终端',
    execute: async () => {
      await request('runInstaller', { client: app })
      notice.value = '安装终端已打开，安装结束后可刷新路径和版本。'
    },
  }
}

function saveProfile() {
  void task(async () => {
    await request('saveProfile', { client: client.value, name: profileName.value })
    profileName.value = ''
    await loadPage()
    notice.value = '已保存当前配置。'
  })
}

function removeProfile(item: Profile) {
  const app = client.value
  confirm.value = {
    title: '删除项目配置「' + item.name + '」？',
    details: ['将从项目列表移除该快照。'],
    button: '删除快照',
    execute: async () => {
      await request('removeProfile', { client: app, id: item.id })
      await loadPage()
    },
  }
}

function openThread(item: Session) {
  void task(async () => { thread.value = await request<Thread>('readSession', { id: item.id }) })
}

function threadAction(action: 'resumeSession' | 'exportSession' | 'learnSession', item: Session) {
  const execute = async () => {
    const result = await request<{ cancelled?: boolean }>(action, { id: item.id })
    if (result.cancelled) return
    notice.value = action === 'resumeSession' ? '已启动原客户端。'
      : action === 'exportSession' ? '已导出 Markdown。' : '已整理到陪伴记忆。'
  }
  if (action === 'learnSession') {
    confirm.value = {
      title: '让企鹅整理这段会话？',
      details: ['使用当前陪伴模型整理最多八条近期用户消息，结果会按你的陪伴同步设置保存。'],
      button: '开始整理',
      execute,
    }
  } else {
    void task(execute)
  }
}

function selectUsageRange(since: number | null) {
  void task(async () => {
    usageSince.value = since
    await loadPage()
  })
}

function restore(item: { id: string; title: string }) {
  confirm.value = {
    title: '恢复「' + item.title + '」？',
    details: ['恢复前会检查目标文件。检测到较新的外部改动时会停止。'],
    button: '恢复配置',
    execute: async () => {
      await request('rollback', { id: item.id })
      await overview()
      await loadPage()
      notice.value = '已恢复配置。'
    },
  }
}

function accept() {
  const pending = confirm.value
  if (!pending) return
  confirm.value = null
  void task(pending.execute)
}

function refresh() {
  void task(async () => {
    await overview()
    await loadPage()
  })
}

onMounted(refresh)
</script>

<template>
  <div class="app-shell" @keydown.esc="confirm = null">
    <aside class="sidebar">
      <div class="brand"><span>PenguinPal</span><strong>AI 工具</strong></div>
      <nav aria-label="AI 工具页面">
        <button v-for="item in pages" :key="item.id" :aria-current="page === item.id ? 'page' : undefined"
          :disabled="busy" @click="focusPage(item.id)">{{ item.label }}</button>
      </nav>
      <div class="sidebar-footer">客户端直连</div>
    </aside>

    <main class="workspace">
      <header class="workspace-header">
        <h1>{{ pageTitle }}</h1>
        <button class="quiet" :disabled="busy" @click="refresh">{{ busy ? '处理中…' : '刷新' }}</button>
      </header>

      <div v-if="page !== 'backups'" class="clients" aria-label="选择客户端">
        <button v-for="item in visibleClients" :key="item.id" :disabled="busy"
          :aria-pressed="client === item.id" @click="focusClient(item.id)">
          <i :class="{ installed: item.executable || item.configured }"></i>{{ item.label }}
        </button>
        <button v-if="otherClients.length" :disabled="busy" :aria-expanded="showAllApps" @click="showAllApps = !showAllApps">{{ showAllApps ? '收起客户端' : '更多客户端' }}</button>
        <div v-if="showAllApps" class="more-clients">
          <button v-for="item in otherClients" :key="item.id" :disabled="busy" @click="focusClient(item.id)">{{ item.label }}</button>
        </div>
      </div>
      <p v-if="error" class="banner error" role="alert">{{ error }}</p>
      <p v-else-if="infoNote" class="banner neutral" role="status">{{ infoNote }}</p>
      <p v-else-if="notice" class="banner" role="status">{{ notice }}</p>

      <template v-if="page === 'tools' && detail">
        <section class="card highlighted">
          <div class="spread">
            <div>
              <p class="caption">{{ detail.provider.label }}{{ detail.provider.current ? ' · ' + detail.provider.current.name : '' }}</p>
              <h2>{{ detail.provider.current?.model || detail.provider.configuredModel || (detail.provider.configured ? '客户端默认模型' : '本机暂无客户端配置') }}</h2>
              <span v-if="detail.provider.providers.some(item => item.inUse)" class="active-label"><i aria-hidden="true"></i>{{ detail.provider.current?.name }} · 使用中</span>
              <p v-if="detail.provider.current?.host">{{ detail.provider.current.host }}</p>
            </div>
            <div class="actions">
              <button v-if="client === 'codex' && detail.provider.current && !detail.provider.current.official" :disabled="busy" @click="refreshCodexModels">刷新模型目录</button>
              <button v-if="detail.provider.current && !detail.provider.managedByClient" :disabled="busy" @click="check(detail.provider.current, true)">检查当前连接</button>
            </div>
          </div>
          <p v-if="detail.provider.error" class="danger-text">{{ detail.provider.error }}</p>
          <dl class="paths">
            <dt>配置文件</dt><dd>{{ detail.provider.config }}</dd>
            <dt v-if="!detail.provider.managedByClient">可执行文件</dt><dd v-if="!detail.provider.managedByClient">{{ currentApp?.executable || '未找到，请检查安装和 PATH' }}</dd>
          </dl>
          <p v-if="detail.provider.managedByClient && !detail.provider.providers.some(row => row.modelChoice)" class="muted">连接与默认模型由客户端设置管理。</p>
          <div v-if="currentApp?.canLaunchAuth || !detail.provider.managedByClient" class="actions tool-actions">
            <button v-if="currentApp?.canLaunchAuth" :disabled="busy" @click="focusPage('accounts')">管理官方账号</button>
            <template v-if="!detail.provider.managedByClient">
              <button :disabled="busy" @click="checkCli">检查版本与路径</button>
              <button :disabled="busy" @click="installCli">安装或升级</button>
              <button class="quiet" :disabled="busy" @click="task(async () => { await request('openInstallGuide', { client }) })">官方安装说明 ↗</button>
            </template>
          </div>
          <div v-if="cli">
            <p>{{ cli.version || '未找到可执行版本' }}</p>
            <p v-if="cli.executables.length > 1" class="muted">发现多个可执行入口，首项优先。</p>
            <p v-for="path in cli.executables" :key="path" class="path">{{ path }}</p>
          </div>
        </section>
        <section v-if="currentApp?.supportsDirect || !detail.provider.managedByClient || detail.provider.providers.length" class="card">
          <div class="spread"><h2>直连配置</h2><button v-if="currentApp?.supportsDirect" :disabled="busy" @click="direct = { name: '', endpoint: '', model: '', key: '' }">新增中转</button></div>
          <form v-if="direct" class="editor direct-form" @submit.prevent="saveDirect">
            <label>显示名称<input v-model.trim="direct.name" maxlength="80" required /></label>
            <label>模型 ID<input v-model.trim="direct.model" maxlength="120" required /></label>
            <label>API 地址<input v-model.trim="direct.endpoint" type="url" required placeholder="https://example.com/v1" /></label>
            <label>API Key<input v-model.trim="direct.key" type="password" maxlength="8192" autocomplete="off" required /></label>
            <small class="muted">{{ client === 'codex' || client === 'grokbuild' ? '使用 OpenAI Responses API。' : client === 'claude' ? '使用 Anthropic Messages API。' : client === 'gemini' ? '使用 Gemini 原生 API。' : '使用 OpenAI 兼容 Chat Completions API。' }}</small>
            <div class="actions"><button type="button" :disabled="busy" @click="direct = null">取消</button><button class="primary" :disabled="busy">保存中转</button></div>
          </form>
          <div v-for="item in detail.provider.providers" :key="item.id" class="provider-row" :class="{ 'in-use': item.inUse }">
            <div><strong>{{ item.name }}</strong><small>{{ [(item.inUse || item.modelChoice || ['workbuddy', 'omp'].includes(client)) ? item.model : '', item.host].filter(Boolean).join(' · ') || (item.official ? '使用客户端的官方登录' : '沿用客户端默认连接') }}</small></div>
            <div v-if="!detail.provider.managedByClient || item.modelChoice || item.inUse || item.savedDirect" class="actions">
              <span v-if="item.inUse" class="active-label" title="已写入当前客户端配置"><i aria-hidden="true"></i>使用中</span>
              <span v-if="item.installed" class="muted">已写入模型列表</span>
              <button v-if="!detail.provider.managedByClient || item.savedDirect" :disabled="busy" @click="check(item, item.inUse && !item.savedDirect)">检查</button>
              <button v-if="!item.inUse && !item.installed" class="primary" :disabled="busy" @click="apply({ kind: item.modelChoice ? 'nativeModel' : 'provider', id: item.id })">{{ item.official ? '切回官方' : ['workbuddy', 'omp'].includes(client) ? '写入模型' : '切换' }}</button>
            </div>
          </div>
        </section>
      </template>

      <AiAccounts v-if="page === 'accounts' && accountStatus" :key="client" :status="accountStatus" :client="client" :busy="busy"
        @login="startLogin" @capture="captureAccount" @select="selectAccount" @remove="removeAccount" />

      <section v-if="page === 'projects' && detail?.provider.managedByClient" class="card">
        <p>{{ detail.provider.label }} 的这项配置请在客户端内管理。</p>
      </section>
      <template v-if="page === 'extensions' && detail">
        <section class="card">
          <div class="spread">
            <h2>MCP</h2><button v-if="currentApp?.mcpSupported" :disabled="busy" @click="mcpEditor = { name: '' }">添加 MCP</button>
          </div>
          <p v-if="!currentApp?.mcpSupported" class="muted">此客户端的 MCP 由客户端自身管理。</p>
          <p v-else-if="detail.mcpError" class="danger-text">{{ detail.mcpError }}</p>
          <p v-else-if="!detail.mcp?.length" class="muted">当前客户端没有已配置的 MCP。</p>
          <div v-for="item in detail.mcp" :key="item.name" class="list-row">
            <div><strong>{{ item.name }}</strong><small>{{ item.transport }} · {{ item.enabled ? '已启用' : '已停用' }}</small></div>
            <div v-if="item.editable" class="actions">
              <button :disabled="busy" @click="editMcp(item.name)">编辑</button>
              <button class="danger-text" :disabled="busy" @click="apply({ kind: 'mcp', name: item.name })">移除</button>
            </div>
            <small v-else>由连接器管理</small>
          </div>
          <AiMcpForm v-if="mcpEditor" :key="mcpEditor.name" :client="client" :apps="mcpApps" :initial-name="mcpEditor.name"
            :initial="mcpEditor.definition" :keeps-private="mcpEditor.preservesPrivateFields" :busy="busy"
            @preview="saveMcp" @cancel="mcpEditor = null" />
        </section>
        <section class="card">
          <div class="spread">
            <h2>Skills</h2><div class="actions">
              <button :disabled="busy" @click="skillPicker = !skillPicker">导入本机已有 Skill</button>
              <button :disabled="busy" @click="pickSkill">选择 Skill 目录</button>
            </div>
          </div>
          <AiSkillImport v-if="skillPicker" :skills="detail.skills.library" :apps="clients" :client="client" :busy="busy"
            @preview="(ids, targets) => apply({ kind: 'importSkills', ids, targets })" @cancel="skillPicker = false" />
          <p v-if="!detail.skills.installed.length" class="muted">这个客户端的 Skills 目录中暂无可管理的项目。</p>
          <div v-for="skill in detail.skills.installed" :key="skill.id" class="list-row">
            <div><strong>{{ skill.name }}</strong><small>{{ skill.enabled ? '已启用' : '已停用' }}</small></div>
            <button :disabled="busy" @click="apply({ kind: 'toggleSkill', id: skill.id, enabled: !skill.enabled })">{{ skill.enabled ? '停用' : '启用' }}</button>
          </div>
        </section>
        <section v-if="detail.prompt || detail.promptError" class="card">
          <h2>提示词</h2>
          <p v-if="detail.promptError" class="danger-text">{{ detail.promptError }}</p>
          <form v-else-if="detail.prompt" class="editor" @submit.prevent="apply({ kind: 'prompt', text: prompt })">
            <label>桌宠管理的 Markdown 区块<textarea v-model="prompt" :disabled="busy" rows="6"></textarea></label>
            <p class="muted path">{{ detail.prompt.path }}</p>
            <button class="primary" :disabled="busy">预览保存</button>
          </form>
        </section>
      </template>

      <template v-if="page === 'projects' && detail && !detail.provider.managedByClient">
        <section class="card">
          <h2>保存当前工具配置</h2>
          <p class="muted">保存当前供应商、MCP、桌宠管理的提示词和已知 Skills 的启用状态。</p>
          <form class="inline-form" @submit.prevent="saveProfile">
            <input v-model="profileName" :disabled="busy" required maxlength="80" aria-label="项目配置名称" placeholder="项目名称" />
            <button class="primary" :disabled="busy">保存快照</button>
          </form>
        </section>
        <section class="card">
          <h2>项目快照</h2>
          <p v-if="!detail.profiles.length" class="muted">暂无项目快照。</p>
          <div v-for="item in detail.profiles" :key="item.id" class="list-row">
            <strong>{{ item.name }}</strong>
            <div class="actions">
              <button :disabled="busy" @click="removeProfile(item)">删除</button>
              <button class="primary" :disabled="busy" @click="apply({ kind: 'profile', id: item.id })">预览应用</button>
            </div>
          </div>
        </section>
      </template>

      <template v-if="page === 'sessions'">
        <form v-if="currentApp?.sessionLogs !== false" class="inline-form search" @submit.prevent="task(loadHistory)">
          <input v-model="query" :disabled="busy" aria-label="搜索会话" placeholder="项目路径、首条消息或会话 ID" />
          <button class="primary" :disabled="busy">搜索</button>
        </form>
        <p v-if="historyLimited" class="muted">已达到扫描或显示上限，当前为最近会话的搜索结果。</p>
        <section v-if="currentApp?.sessionLogs === false" class="card">这个客户端的会话读取尚未接入。</section>
        <div v-else class="session-browser" :class="{ 'has-history': history.length }">
          <section class="card session-list" aria-label="会话列表">
            <p v-if="!history.length" class="muted">未找到符合条件的本机会话。</p>
            <button v-for="item in history" :key="item.id" :disabled="busy" :aria-pressed="thread?.session.id === item.id" @click="openThread(item)">
              <strong>{{ item.title }}</strong><small :title="item.cwd">{{ item.cwd }}</small><time>{{ new Date(item.modified).toLocaleString() }}</time>
            </button>
          </section>
          <section v-if="thread" class="card session-detail">
            <h2>{{ thread.session.title }}</h2>
            <div class="actions thread-actions">
              <button v-if="!currentApp?.managedByClient" class="primary" :disabled="busy" @click="threadAction('resumeSession', thread.session)">继续会话</button>
              <button :disabled="busy" @click="threadAction('exportSession', thread.session)">导出 Markdown</button>
              <button :disabled="busy" @click="threadAction('learnSession', thread.session)">交给企鹅整理</button>
            </div>
            <p v-if="thread.limited" class="muted">此处显示会话预览。</p>
            <p v-if="!thread.messages.length" class="muted">这段会话暂无可显示的对话消息。</p>
            <article v-for="(message, index) in thread.messages" :key="index" class="message">
              <strong>{{ message.role === 'user' ? '用户' : '助手' }}</strong><p>{{ message.text }}</p>
            </article>
          </section>
        </div>
      </template>

      <AiUsage v-if="page === 'usage'" :key="client" :report="usage" :since="usageSince" :busy="busy"
        @range="selectUsageRange" />

      <template v-if="page === 'backups'">
        <section class="card">
          <h2>配置回滚</h2>
          <p v-if="!backups.length" class="muted">从桌宠应用配置后，回滚记录会显示在这里。</p>
          <div v-for="item in backups" :key="item.id" class="list-row">
            <div><strong>{{ item.title }}</strong><small>{{ new Date(Number(item.id.slice(0, 14))).toLocaleString() }}</small></div>
            <button :disabled="busy" @click="restore(item)">恢复</button>
          </div>
        </section>
      </template>
    </main>

    <div v-if="confirm" class="overlay" @click.self="confirm = null">
      <section class="confirm card" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
        <h2 id="confirm-title">{{ confirm.title }}</h2>
        <ul><li v-for="line in confirm.details" :key="line">{{ line }}</li></ul>
        <div class="actions"><button autofocus @click="confirm = null">取消</button><button class="primary" @click="accept">{{ confirm.button || '确认' }}</button></div>
      </section>
    </div>
  </div>
</template>

<style>
:root{font-family:"Segoe UI","Microsoft YaHei UI",sans-serif;color:#1c3042;background:#f2f5f8;font-size:14px}
*{box-sizing:border-box}
body{margin:0}
button,input,textarea{font:inherit}
button{border:1px solid #d0dde8;border-radius:8px;background:#fff;color:#233f54;padding:8px 12px;cursor:pointer;white-space:nowrap}
button:hover:not(:disabled){border-color:#397496;background:#eff7fb}
button:disabled{opacity:.52;cursor:default}
button:focus-visible,input:focus-visible,textarea:focus-visible,summary:focus-visible{outline:2px solid #2e7d9f;outline-offset:2px}
button.primary{background:#215a75;color:white;border-color:#215a75}
button.primary:hover:not(:disabled){background:#164760}
button.quiet{background:transparent}
input:not([type=checkbox]):not([type=radio]),textarea{width:100%;border:1px solid #c6d7e1;border-radius:8px;background:#fff;color:#1c3042;padding:9px 10px}
input[type=checkbox],input[type=radio]{width:16px;height:16px;margin:0;flex:0 0 16px;accent-color:#24576d}
textarea{min-height:74px;resize:vertical}
p{margin:0 0 8px;line-height:1.55}
h1,h2{margin:0}
h1{font-size:25px;letter-spacing:-.02em}
h2{font-size:17px}
small{display:block;color:#526579;overflow-wrap:anywhere;margin-top:4px}
.app-shell{display:grid;grid-template-columns:180px minmax(0,1fr);min-height:100vh}
.sidebar{position:sticky;top:0;height:100vh;background:#172d40;color:#deebf4;padding:24px 14px;display:flex;flex-direction:column}
.brand{padding:0 11px 24px;display:grid;gap:5px}
.brand span{font-size:12px;color:#aac6d6;letter-spacing:.06em}
.brand strong{font-size:20px}
.sidebar nav{display:grid;gap:5px}
.sidebar nav button{width:100%;text-align:left;background:transparent;border-color:transparent;color:inherit;padding:10px 11px}
.sidebar nav button[aria-current=page],.sidebar nav button:hover:not(:disabled){background:#294a61;color:#fff;border-color:#345971}
.sidebar-footer{margin-top:auto;color:#b4cbd8;padding:12px 11px 0;font-size:12px}
.workspace{margin:0 auto;width:100%;max-width:1180px;padding:25px clamp(17px,3vw,34px)}
.workspace-header,.spread,.list-row,.actions,.inline-form{display:flex;align-items:center;gap:12px}
.workspace-header,.spread,.list-row{justify-content:space-between}
.workspace-header{margin-bottom:19px}
.clients{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:18px}
.clients button[aria-pressed=true],.session-list button[aria-pressed=true]{border-color:#3e7f9d;background:#e4f1f8}
.clients i{display:inline-block;width:7px;height:7px;border-radius:50%;background:#8797a4;margin-right:7px;vertical-align:2px}
.clients i.installed{background:#27845a}
.more-clients{display:flex;flex-wrap:wrap;gap:8px;flex-basis:100%;padding:9px;background:#e7eff5;border-radius:8px}
.card{padding:18px 20px;background:#fff;border:1px solid #d9e3ec;border-radius:12px;margin:0 0 14px}
.card h2{margin-bottom:8px}
.highlighted{background:#eaf4fa;border-color:#cfe2ef}
.highlighted h2{font-size:22px}
.caption,.muted,.path,.paths dt,.paths dd{color:#526578}
.path,.paths dd{overflow-wrap:anywhere}
.paths{display:grid;grid-template-columns:max-content 1fr;gap:6px 12px;margin:14px 0}
.paths dd{margin:0}
.paths dt{font-size:13px}
.list-row{border-top:1px solid #e3ebf1;padding:11px 0}
.list-row:first-of-type{border-top:0}
.list-row > div:first-child{min-width:0;overflow-wrap:anywhere}
.provider-row{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:12px;border:1px solid #dce6ed;border-radius:9px;margin:8px 0}
.provider-row.in-use{background:#edf7ff;border-color:#6a9fbd}
.provider-row > div:first-child{min-width:0;overflow-wrap:anywhere}
.active-label{display:inline-flex;gap:6px;align-items:center;color:#215d7f;font-weight:650;white-space:nowrap}
.active-label i{display:inline-block;width:7px;height:7px;border-radius:50%;background:#2877a4}
.actions{gap:7px;flex-wrap:wrap;justify-content:flex-end}
.tool-actions{justify-content:flex-start;margin:10px 0}
.editor{display:grid;gap:10px;margin-top:14px;border-top:1px solid #e3ebf1;padding-top:14px}
.editor label{display:grid;gap:5px;color:#3f5668}
.direct-form{grid-template-columns:repeat(2,minmax(0,1fr));margin-bottom:14px}
.direct-form>.actions{justify-content:flex-end}
.editor button{justify-self:start}
.inline-form input{flex:1}
.search{margin-bottom:13px}
.session-browser{display:grid;gap:12px}
.session-browser.has-history{grid-template-columns:minmax(170px, 30%) minmax(0, 1fr);height:max(350px, calc(100vh - 194px))}
.session-browser.has-history > .card{min-height:0;margin-bottom:0}
.session-list{padding:10px;overflow:auto}
.session-list button{display:block;width:100%;white-space:normal;overflow-wrap:anywhere;text-align:left;margin-bottom:6px}
.session-list strong{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;overflow:hidden}
.session-list small{font-size:11px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.session-list time{color:#526578;font-size:11px;display:block;margin-top:3px}
.session-detail{min-width:0;overflow:auto}
.thread-actions{justify-content:flex-start;margin:12px 0}
.message{border-top:1px solid #dce7ed;padding:11px 0}
.message p{white-space:pre-wrap;overflow-wrap:anywhere;margin-top:6px}
.banner{padding:11px 13px;border:1px solid #c6e0d7;border-radius:8px;background:#e7f5ef;color:#174838;margin:0 0 14px}
.banner.error{background:#fff1ef;border-color:#f1cfca;color:#862e26}
.banner.neutral{background:#edf4f9;border-color:#cbdde9;color:#20485f}
.danger-text{color:#962e29}
.overlay{position:fixed;inset:0;display:grid;place-items:center;padding:24px;background:#122b43a8}
.confirm{width:min(580px,100%);max-height:85vh;overflow:auto}
.confirm ul{padding-left:22px;overflow-wrap:anywhere}
.confirm li{margin:7px 0}
summary{cursor:pointer;color:#286080;padding:9px 0}
@media(max-width:800px){.app-shell{grid-template-columns:140px minmax(0,1fr)}.session-browser.has-history{grid-template-columns:minmax(135px, 34%) minmax(0,1fr)}}
</style>
