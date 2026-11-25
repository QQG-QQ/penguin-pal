<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

type Account = { id: string; label: string; active: boolean }
type Instance = { id: string; name: string; directory: string; running: boolean }
type Relay = { id: string; name: string; endpoint: string }
type View = {
  app: string | null; cli: string | null; signedIn: boolean; credentialError: string | null
  cliRelayMode: boolean
  accounts: Account[]; instances: Instance[]; relays: Relay[]
}

const props = defineProps<{ refresh: number; accountsOnly: boolean }>()
const view = ref<View | null>(null)
const busy = ref(false)
const error = ref('')
const notice = ref('')
const accountLabel = ref('')
const instance = ref<{ id?: string; name: string; directory: string } | null>(null)
const relay = ref<{ id?: string; name: string; endpoint: string; key: string } | null>(null)

function request<T>(operation: string, args: Record<string, unknown> = {}) {
  return invoke<T>('companion_ai_tools', {
    request: { action: 'antigravity', command: { operation, ...args } },
  })
}

async function run(work: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await work()
  } catch (reason) {
    error.value = String(reason)
  } finally {
    busy.value = false
  }
}

async function load() {
  view.value = await request<View>('status')
}

function operate(operation: string, args: Record<string, unknown> = {}, finished?: () => void) {
  void run(async () => {
    const result = await request<{ message?: string }>(operation, args)
    finished?.()
    notice.value = result.message ?? ''
    await load()
  })
}

function pickDirectory() {
  void run(async () => {
    const result = await request<{ directory: string | null }>('pickDirectory')
    if (instance.value && result.directory) instance.value.directory = result.directory
  })
}

function editInstance(item?: Instance) {
  instance.value = item
    ? { id: item.id, name: item.name, directory: item.directory }
    : { name: '', directory: '' }
}

function editRelay(item?: Relay) {
  relay.value = item
    ? { id: item.id, name: item.name, endpoint: item.endpoint, key: '' }
    : { name: '', endpoint: '', key: '' }
}

onMounted(() => void run(load))
watch(() => props.refresh, () => void run(load))
</script>

<template>
  <p v-if="error" class="banner error" role="alert">{{ error }}</p>
  <p v-else-if="notice" class="banner" role="status">{{ notice }}</p>
  <template v-if="view">
    <section v-if="!accountsOnly" class="card highlighted">
      <div class="spread">
        <div>
          <h2>Antigravity App</h2>
          <p class="muted installation">{{ view.app || '未找到 Antigravity App' }}</p>
        </div>
        <button :disabled="busy || !view.app" @click="operate('openApp')">打开 App</button>
      </div>
      <p class="muted installation">Antigravity CLI · {{ view.cli || '未安装' }}</p>
      <p v-if="view.cliRelayMode" class="muted">CLI 已切到中转模式，点「用官方账号启动 CLI」可恢复官方登录。</p>
      <div class="actions">
        <button v-if="!view.cli" :disabled="busy" @click="operate('installGuide')">官方 CLI 安装说明 ↗</button>
        <button v-else :disabled="busy" @click="operate('startOfficialCli')">用官方账号启动 CLI</button>
      </div>
    </section>

    <section class="card">
      <div class="spread">
        <h2>Google 官方账号</h2>
        <div class="actions">
          <button :disabled="busy" @click="operate('import')">导入 JSON</button>
          <button class="primary" :disabled="busy || !view.app" @click="operate('startLogin')">打开官方 OAuth 登录</button>
        </div>
      </div>
      <p v-if="view.credentialError" class="banner error">{{ view.credentialError }}</p>
      <p v-else class="muted">{{ view.signedIn ? '已检测到本机 Google 凭据。' : '在 App 完成 Google 登录，然后点刷新。' }}</p>
      <form v-if="view.signedIn" class="actions capture"
        @submit.prevent="operate('capture', { label: accountLabel }, () => accountLabel = '')">
        <label>账号名称 <input v-model="accountLabel" maxlength="120" required placeholder="例如工作账号" /></label>
        <button :disabled="busy || !accountLabel.trim()">收录当前账号</button>
      </form>
      <div v-for="item in view.accounts" :key="item.id" class="list-row">
        <div><strong>{{ item.label }}</strong><small v-if="item.active">当前共享凭据</small></div>
        <div class="actions">
          <button :disabled="busy || item.active" @click="operate('switch', { id: item.id })">切换账号</button>
          <button :disabled="busy" @click="operate('removeAccount', { id: item.id })">移除记录</button>
        </div>
      </div>
      <p v-if="view.accounts.length" class="muted">切号前请退出 App、IDE 和 CLI，再重新打开 App 核对账号。</p>
    </section>

    <section v-if="!accountsOnly" class="card">
      <div class="spread">
        <div><h2>App 实例</h2><p class="muted">实例可以分别指定工作目录，登录由 Windows 用户共享。</p></div>
        <button :disabled="busy" @click="editInstance()">新建实例</button>
      </div>
      <form v-if="instance" class="connection-editor"
        @submit.prevent="operate('saveInstance', instance, () => instance = null)">
        <label>名称 <input v-model="instance.name" required maxlength="120" /></label>
        <label>工作目录 <input v-model="instance.directory" required /></label>
        <div class="actions">
          <button type="button" :disabled="busy" @click="pickDirectory">选择目录</button>
          <button type="button" :disabled="busy" @click="instance = null">取消</button>
          <button class="primary" :disabled="busy">保存</button>
        </div>
      </form>
      <div v-for="item in view.instances" :key="item.id" class="list-row">
        <div><strong>{{ item.name }}</strong><small>{{ item.directory }}<span v-if="item.running"> · 运行中</span></small></div>
        <div class="actions">
          <button v-if="item.running" :disabled="busy" @click="operate('stopInstance', { id: item.id })">关闭</button>
          <button v-else :disabled="busy || !view.app" @click="operate('startInstance', { id: item.id })">启动</button>
          <button :disabled="busy || item.running" @click="editInstance(item)">编辑</button>
          <button :disabled="busy || item.running" @click="operate('removeInstance', { id: item.id })">移除记录</button>
        </div>
      </div>
    </section>

    <section v-if="!accountsOnly" class="card">
      <div class="spread">
        <div><h2>CLI 中转</h2><p class="muted">填写 Gemini 兼容地址。通过桌宠启动 agy 后，在终端选择模型。</p></div>
        <button :disabled="busy" @click="editRelay()">添加中转</button>
      </div>
      <form v-if="relay" class="connection-editor"
        @submit.prevent="operate('saveRelay', relay, () => relay = null)">
        <label>名称 <input v-model="relay.name" required maxlength="120" /></label>
        <label>Gemini 兼容 Base URL <input v-model="relay.endpoint" type="url" required placeholder="https://gemini.example.com" /></label>
        <label>API Key <input v-model="relay.key" type="password" :required="!relay.id" autocomplete="off"
          :placeholder="relay.id ? '地址所属服务器不变时可留空' : ''" /></label>
        <div class="actions">
          <button type="button" :disabled="busy" @click="relay = null">取消</button>
          <button class="primary" :disabled="busy">保存</button>
        </div>
      </form>
      <div v-for="item in view.relays" :key="item.id" class="list-row">
        <div><strong>{{ item.name }}</strong><small>{{ item.endpoint }}</small></div>
        <div class="actions">
          <button :disabled="busy || !view.cli" @click="operate('startRelay', { id: item.id })">用中转启动</button>
          <button :disabled="busy" @click="editRelay(item)">编辑</button>
          <button :disabled="busy" @click="operate('removeRelay', { id: item.id })">移除</button>
        </div>
      </div>
    </section>
  </template>
</template>

<style scoped>
.installation { overflow-wrap: anywhere; }
.capture { align-items: end; margin-bottom: 12px; }
.capture label { flex: 1 0 160px; }
.connection-editor { display: grid; gap: 10px; margin-bottom: 12px; }
.actions { flex-wrap: wrap; }
</style>
