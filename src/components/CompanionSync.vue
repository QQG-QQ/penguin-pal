<script setup lang="ts">
import {onBeforeUnmount,onMounted,ref} from 'vue'
import {invoke} from '@tauri-apps/api/core'
import type {SyncDocument,SyncServer,SyncView} from '../lib/companionSync'
defineProps<{native:boolean}>()
const emit=defineEmits<{changed:[SyncView,boolean]}>()
const view=ref<SyncView>(),host=ref(''),port=ref(22),username=ref(''),password=ref(''),namespace=ref('my-penguin')
const fingerprint=ref(''),known=ref(false),trusted=ref(false),busy=ref(false),error=ref(''),info=ref(''),memory=ref(''),reason=ref('')
const preview=ref<{memories:number;conversations:number;mood:number}>()
const archiveQuery=ref(''),archiveMessages=ref<SyncDocument['conversations']>([]),archiveOpen=ref(false)
const sourceOf=(text:string)=>[...text.matchAll(/来源：([A-Za-z0-9_-]{1,80})/g)].pop()?.[1]
async function searchArchive(sourceId=''){await run(async()=>{archiveOpen.value=true;archiveMessages.value=(await call('history',{query:archiveQuery.value,sourceId})).messages;if(!archiveMessages.value.length)info.value=sourceId?'本机没有这条历史原文。记忆已保留原话依据、时间和来源；较早原文可在采集电脑查找。':'没有找到匹配的已收录对话。'})}
let timer:number|undefined,disposed=false
const server=():SyncServer=>({host:host.value.trim(),port:Number(port.value),username:username.value.trim(),namespace:namespace.value.trim(),fingerprint:fingerprint.value})
function clearPin(){fingerprint.value='';known.value=false;trusted.value=false}
async function call(action:string,payload:unknown=null){return invoke<any>('companion_sync',{action,payload})}
function receive(value:SyncView,apply=false){if(disposed)return;view.value=value;emit('changed',value,apply)}
async function run(work:()=>Promise<void>){if(busy.value)return;busy.value=true;error.value='';info.value='';try{await work()}catch(e){error.value=String(e)}finally{busy.value=false}}
async function refresh(){try{receive(await call('status'))}catch(e){error.value=String(e)}}
async function probe(){const result=await call('probe',server());fingerprint.value=result.fingerprint;known.value=result.known;trusted.value=result.known}
async function connect(){await run(async()=>{
  if(!fingerprint.value)await probe()
  if(!trusted.value){info.value='请核对下方主机指纹，确认后再次连接。';return}
  receive(await call('configure',{server:server(),password:password.value}),true);password.value=''
  info.value='服务器已连接，后续会自动上传变化并拉取最新状态。'
})}
async function action(name:string,payload:unknown=null,apply=false){await run(async()=>receive(await call(name,payload),apply))}
async function install(){await run(async()=>{info.value=await invoke<string>('companion_install_hooks')})}
async function backfill(){await run(async()=>{const result=await call('backfill');receive(result.status);info.value='已执行一轮扫描；其余历史将在后台分批继续，不受 7 天或单文件大小限制。'})}
async function exportData(){await run(async()=>{const result=await call('export');if(!result.cancelled)info.value='已导出：'+result.path})}
async function previewImport(){await run(async()=>{const result=await call('previewImport');if(!result.cancelled)preview.value=result})}
async function importData(){await action('import',null,true);if(!error.value)preview.value=undefined}
async function addMemory(){if(!memory.value.trim())return;await action('memory',{text:memory.value});if(!error.value)memory.value=''}
async function mood(delta:number){await action('mood',{delta,reason:reason.value});if(!error.value)reason.value=''}
onMounted(async()=>{
  await refresh()
  if(view.value?.server){const s=view.value.server;host.value=s.host;port.value=s.port;username.value=s.username;namespace.value=s.namespace;fingerprint.value=s.fingerprint;trusted.value=true}
  timer=window.setInterval(()=>{if(!busy.value)void refresh()},3000)
})
onBeforeUnmount(()=>{disposed=true;password.value='';window.clearInterval(timer)})
</script>

<template>
  <div class="sync">
    <p class="note">在多台电脑使用相同服务器账号和宠物存储名，可延续同一只企鹅的状态与近期对话。</p>
    <div class="mode"><button :class="{selected:view?.mode==='local'}" :disabled="busy" @click="action('local',null,true)">本地保存</button><span>{{view?.mode==='server'?'服务器自动同步':'数据保存在这台电脑'}}</span></div>
    <form @submit.prevent="connect">
      <label>服务器 IP / 主机名<input v-model="host" required maxlength="253" autocomplete="off" placeholder="服务器地址" @input="clearPin"/></label>
      <div class="row"><label>SSH 端口<input v-model="port" required type="number" min="1" max="65535" @input="clearPin"/></label><label>用户名<input v-model="username" required maxlength="64" autocomplete="username"/></label></div>
      <label>密码<input v-model="password" type="password" maxlength="1024" autocomplete="new-password" placeholder="保存到 Windows 凭据管理器；留空使用已保存密码"/></label>
      <label>宠物存储名<input v-model="namespace" required maxlength="80" pattern="[A-Za-z0-9_-]+" placeholder="两台电脑填写相同名称"/></label>
      <small>服务器需要 SSH、Python 3 及账号目录写权限，无需手动安装同步服务。密码不进入备份和聊天记录。</small>
      <div v-if="fingerprint" class="pin"><code>{{fingerprint}}</code><label><input v-model="trusted" type="checkbox"/>已核对服务器主机指纹<template v-if="known">（匹配已知主机）</template></label></div>
      <div class="buttons"><button type="button" :disabled="busy||!host||!username" @click="run(probe)">检查连接与指纹</button><button :disabled="busy" class="primary">连接并自动同步</button></div>
    </form>
    <p v-if="view" class="status">{{view.message||'尚未连接服务器'}}<br/>待同步 {{view.pending}} 条 · 最近同步 {{view.lastSync?new Date(view.lastSync).toLocaleString():'尚无'}}<br/>宠物 {{view.document.petId.slice(0,8)}} · 版本 {{view.document.revision}}<template v-if="view.mode==='server'"> · {{view.active?'本机负责主动陪伴':'另一台设备负责主动陪伴'}}</template></p>
    <button :disabled="busy||view?.mode!=='server'" @click="action('sync')">立即同步</button>
    <h3>近期对话</h3>
    <label class="check"><input :checked="view?.capture" type="checkbox" :disabled="busy" @change="action('capture',{enabled:($event.target as HTMLInputElement).checked})"/>采集全部本机 Codex 会话，包含项目内外</label>
    <small>开启后自动扫描全部可读取的本机历史与归档，持续收录新增对话，不必逐个选择。原文脱敏片段与整理进度独立保存在本机；近期最多 500 条随服务器同步，长期记忆与依据一起同步。不采集工具输出和推理。网页、云端及远程主机上的聊天须先成为本机可读文件。</small>
    <p v-if="view?.history" class="status">已收录 {{view.history.archived}} 条 · 已整理 {{view.history.processed}} 条用户消息 · 待整理 {{view.history.pending}} 条<br/>已发现 {{view.history.files}} 个文件 · 待扫描 {{view.history.pendingFiles}} 个文件 / {{view.history.pendingDirectories}} 个新目录</p>
    <p v-if="view?.history?.scanError||view?.history?.learningError||view?.history?.fileErrors" class="error">{{view?.history?.scanError||view?.history?.learningError||'部分文件或目录读取失败，后台会继续重试。'}} · 文件错误 {{view?.history?.fileErrors??0}} 个</p>
    <p v-if="view?.history?.oversizedLines||view?.history?.invalidLines" class="error">扫描遇到超长行 {{view.history.oversizedLines??0}} 次 / 损坏行 {{view.history.invalidLines??0}} 次；这些行无法完整收录，请检查源文件。单行上限 1 MiB，单条对话保留脱敏后前 6000 字。</p>
    <p v-if="view?.captureWarning" class="error">对话联动收件队列曾满；自动扫描会从可读取的本机记录补收。</p>
    <div class="buttons"><button :disabled="busy" @click="install">安装 / 更新实时对话联动</button><button :disabled="busy||!view?.capture" @click="backfill">立即扫描下一批历史</button></div>
    <details><summary>已保存 {{view?.document.conversations.length??0}} 条 · 查看最近对话</summary><div class="history"><p v-for="m in view?.document.conversations.slice(-20)" :key="m.id"><b>{{m.role==='user'?'你':'助手'}} · {{new Date(m.at).toLocaleString()}}</b>{{m.text}}</p></div></details>
    <h3>陪伴记忆与心情</h3>
    <textarea v-model="memory" rows="2" maxlength="2000" placeholder="记住一个喜好、约定或共同经历"/>
    <button :disabled="busy||!memory.trim()" @click="addMemory">记下来</button>
    <div class="memories"><div v-for="m in view?.document.memories.filter(m=>!m.deleted).slice(-20)" :key="m.id"><span>{{m.text}}</span><button v-if="sourceOf(m.text)" :disabled="busy" @click="searchArchive(sourceOf(m.text))">查原文</button><button :disabled="busy" @click="action('forget',{id:m.id})">遗忘</button></div></div>
    <details :open="archiveOpen"><summary>查找已收录的历史原文</summary><input v-model="archiveQuery" maxlength="100" placeholder="关键词；留空查看最新 30 条" @keydown.enter.prevent="searchArchive()"/><button :disabled="busy" @click="searchArchive()">查找</button><div class="history"><p v-for="m in archiveMessages" :key="m.id"><b>{{m.role==='user'?'你':'助手'}} · {{m.at?new Date(m.at).toLocaleString():'时间未知'}} · {{m.session.slice(0,8)}}</b>{{m.text}}</p></div></details>
    <p>当前心情 {{Math.round(view?.document.pet.mood??70)}} · {{view?.document.pet.moodReason||'平稳陪伴'}}</p>
    <input v-model="reason" maxlength="400" placeholder="记录这次心情变化的原因"/>
    <div class="buttons"><button :disabled="busy" @click="mood(5)">开心了一点</button><button :disabled="busy" @click="mood(-5)">有点低落</button></div>
    <h3>导入与导出</h3>
    <small>备份包含宠物状态、记忆、心情和近期对话；不包含服务器连接配置、密码或设备布局。导入合并记忆和对话，并恢复心情与养成数值。本地模式恢复备份中的宠物身份，服务器模式保留服务器身份。</small>
    <div class="buttons"><button :disabled="busy" @click="exportData">导出备份</button><button :disabled="busy" @click="previewImport">选择备份导入</button></div>
    <div v-if="preview" class="status">将导入 {{preview.memories}} 条记忆、{{preview.conversations}} 条对话，恢复心情 {{Math.round(preview.mood)}}。<div class="buttons"><button :disabled="busy" @click="importData">确认导入</button><button @click="preview=undefined">取消</button></div></div>
    <p v-if="busy" role="status">正在处理…</p><p v-if="info" role="status" class="status">{{info}}</p><p v-if="error" role="alert" class="error">{{error}}</p>
  </div>
</template>
<style scoped>
.sync{font-size:12px;line-height:1.7}.sync label{display:block;margin:9px 0;color:#c6e2f1}.sync input:not([type=checkbox]),textarea{box-sizing:border-box;width:100%;padding:9px;background:#092335;color:#eef8ff;border:1px solid #3a657e;border-radius:7px;font:inherit}.row,.buttons,.mode{display:flex;gap:8px}.row label{flex:1;min-width:0}.buttons{margin:12px 0;flex-wrap:wrap}.mode{align-items:center;margin-bottom:16px}.mode span,small,.note{font-size:10px;color:#adcede}button{border:0;border-radius:7px;padding:8px 10px;background:#24516d;color:#eff9ff;cursor:pointer;font:inherit}button:disabled{opacity:.45;cursor:default}.primary,.selected{background:#67c9eb;color:#092a3d}h3{font-size:13px;margin:20px 0 8px}.status,.pin{padding:10px;background:#163d52;border-radius:8px;overflow-wrap:anywhere}.pin code{font-size:10px;user-select:text}.check{display:flex!important;gap:6px;align-items:center}input[type=checkbox]{accent-color:#67c9eb}.error{background:#67402e;color:#ffe3c8;padding:10px;border-radius:8px}.history{max-height:250px;overflow:auto;user-select:text}.history p{white-space:pre-wrap;overflow-wrap:anywhere;border-bottom:1px solid #436075;padding:8px 0}.history b{display:block;color:#8bcceb;font-size:10px}.memories>div{display:flex;gap:8px;align-items:center;padding:8px 0;border-bottom:1px solid #436075}.memories span{flex:1;overflow-wrap:anywhere}.memories button{font-size:10px}summary{cursor:pointer;color:#a1d6ed}
</style>
