<script lang="ts">
export interface Account { id: string; label: string; active: boolean }
export interface AccountStatus { canLaunch: boolean; canCapture: boolean; hasLive: boolean; accounts: Account[]; loginNote?: string; loginPending?: boolean }
</script>

<script setup lang="ts">
import { ref, watch } from 'vue'
const props = defineProps<{ status: AccountStatus; client: string; busy: boolean }>()
const emit = defineEmits<{
  login: [device: boolean, label?: string]
  capture: [label: string]
  select: [account: Account]
  remove: [account: Account]
  refreshLogin: []
  cancelLogin: []
}>()
const label = ref('')

watch(() => props.status.loginPending, (pending, _, onCleanup) => {
  if (!pending) return
  const timer = window.setInterval(() => emit('refreshLogin'), 1000)
  onCleanup(() => window.clearInterval(timer))
}, { immediate: true })
</script>

<template>
  <section class="card accounts">
    <div class="account-actions account-heading">
      <h2>官方账号</h2>
      <div v-if="status.canLaunch && client !== 'codex'" class="account-actions">
        <button :disabled="busy" @click="emit('login', false)">发起官方登录</button>
      </div>
    </div>
    <template v-if="status.canCapture">
      <template v-if="client === 'codex'">
        <p>可保存多个 ChatGPT 账号。退出 Codex 后可切换账号。使用官方连接时，在直连配置里选择「切回官方」。</p>
        <form class="account-actions capture" @submit.prevent="emit('login', false, label)">
          <label>账号备注<input v-model.trim="label" required maxlength="64" placeholder="例如 工作账号" :disabled="busy || status.loginPending" /></label>
          <button class="primary" :disabled="busy || status.loginPending || !label">添加官方账号</button>
          <button type="button" :disabled="busy || status.loginPending || !label" @click="emit('login', true, label)">使用设备码</button>
          <button v-if="status.hasLive" type="button" :disabled="busy || status.loginPending || !label" @click="emit('capture', label)">保存当前账号</button>
        </form>
        <div v-if="status.loginNote" class="account-actions login-status" aria-live="polite">
          <span>{{ status.loginNote }}</span>
          <button v-if="status.loginPending" type="button" :disabled="busy" @click="emit('cancelLogin')">取消登录</button>
        </div>
      </template>
      <template v-else>
        <p>{{ status.hasLive ? '检测到本机官方登录。' : '尚未检测到可收录的本机官方登录。' }} 授权完成后刷新，再收录当前账号。</p>
        <form class="account-actions capture" @submit.prevent="emit('capture', label)">
          <label>账号备注<input v-model.trim="label" required maxlength="64" placeholder="例如 工作账号" /></label>
          <button :disabled="busy || !status.hasLive || !label">收录当前账号</button>
        </form>
      </template>
      <div v-for="account in status.accounts" :key="account.id" class="account-row">
        <strong>{{ account.label }}</strong>
        <div class="account-actions">
          <span v-if="account.active" class="active-label">● 当前登录</span>
          <button v-else :disabled="busy || status.loginPending" @click="emit('select', account)">切换账号</button>
          <button :disabled="busy" @click="emit('remove', account)">移除副本</button>
        </div>
      </div>
      <small>登录副本由 Windows 加密，仅供本机切换。切换前请退出对应客户端。</small>
    </template>
    <p v-else-if="status.canLaunch">{{ status.loginNote || '授权和账号切换由原客户端管理。' }}</p>
    <p v-else>此客户端通过自身设置管理认证。</p>
  </section>
</template>

<style scoped>
.accounts{padding:18px 20px;background:#fff;border:1px solid #dbe7ee;border-radius:12px;margin-bottom:14px}
.accounts h2{margin:0}.accounts p{margin:10px 0;color:#526578}
.account-heading,.account-row{justify-content:space-between}
.account-actions,.account-row{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.account-row{border-top:1px solid #e3ebf1;padding:10px 0;overflow-wrap:anywhere}
.capture{justify-content:flex-start;align-items:end;margin:12px 0}
.capture label{display:grid;gap:5px;min-width:0;width:min(100%,255px)}
.login-status{margin:10px 0 14px;color:#526578}
.accounts small{display:block;margin-top:10px;color:#526578}
.active-label{color:#215d7f;font-weight:650}
</style>
