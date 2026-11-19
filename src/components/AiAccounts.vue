<script lang="ts">
export interface Account { id: string; label: string; active: boolean }
export interface AccountStatus { canLaunch: boolean; canCapture: boolean; hasLive: boolean; accounts: Account[]; loginNote?: string }
</script>

<script setup lang="ts">
import { ref } from 'vue'
defineProps<{ status: AccountStatus; client: string; busy: boolean }>()
const emit = defineEmits<{ login: [device: boolean]; capture: [label: string]; select: [account: Account]; remove: [account: Account] }>()
const label = ref('')
</script>

<template>
  <section class="card accounts">
    <div class="account-actions account-heading">
      <h2>官方账号</h2>
      <div v-if="status.canLaunch" class="account-actions">
        <button :disabled="busy" @click="emit('login', false)">发起官方登录</button>
        <button v-if="client === 'codex'" :disabled="busy" @click="emit('login', true)">使用设备码</button>
      </div>
    </div>
    <template v-if="status.canCapture">
      <p>{{ status.hasLive ? '检测到本机官方登录。' : '尚未检测到可收录的本机官方登录。' }} 授权完成后刷新，再收录当前账号。</p>
      <form class="account-actions capture" @submit.prevent="emit('capture', label)">
        <label>账号备注<input v-model.trim="label" required maxlength="64" placeholder="例如 工作账号" /></label>
        <button :disabled="busy || !status.hasLive || !label">收录当前账号</button>
      </form>
      <div v-for="account in status.accounts" :key="account.id" class="account-row">
        <strong>{{ account.label }}</strong>
        <div class="account-actions">
          <span v-if="account.active" class="active-label">● 当前登录</span>
          <button v-else :disabled="busy" @click="emit('select', account)">切换账号</button>
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
.accounts small{display:block;margin-top:10px;color:#526578}
.active-label{color:#215d7f;font-weight:650}
</style>
