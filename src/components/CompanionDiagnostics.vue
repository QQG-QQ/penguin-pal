<script setup lang="ts">
import { computed,onBeforeUnmount,onMounted,ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import CompanionSync from './CompanionSync.vue'
import type {SyncView} from '../lib/companionSync'
const props=defineProps<{native:boolean}>()
const emit=defineEmits<{close:[];safe:[boolean];sync:[SyncView,boolean]}>()
interface Health {version:string;safeMode:boolean;crashStreak:number;restoredBackup:boolean;nodeAvailable:boolean;configuredHooks:number;scriptCurrent:boolean;bridge:{status:string;activeSessions:number;lastReceived:number};rejectedEvents:number;dataDirectory:string;legacyFileCount:number;metrics:{workingSetBytes:number|null;privateBytes:number|null;cpuMillis:number|null};logs:{at:number;event:string}[]}
interface LegacyFile{id:string;name:string;kind:string;bytes:number}
interface UpdateInfo{status:string;currentVersion:string;latestVersion:string|null;assetName:string|null;downloadable:boolean;message:string}
const tab=ref<'status'|'data'|'updates'|'sync'>('status'),health=ref<Health|null>(null),files=ref<LegacyFile[]>([])
const selected=ref<string[]>([]),preview=ref(''),info=ref(''),error=ref(''),busy=ref(false),refreshing=ref(false),update=ref<UpdateInfo|null>(null),cpu=ref<number|null>(null)
let timer:number|undefined,disposed=false,lastCpu:{time:number;value:number}|undefined
const age=computed(()=>health.value?.bridge.lastReceived?new Date(health.value.bridge.lastReceived).toLocaleString():'尚未收到；配置存在不代表已获信任')
const mib=(bytes:number|null|undefined)=>bytes==null?'未知':`${(bytes/1048576).toFixed(1)} MB`
const logLabels:Record<string,string>={'startup':'正常启动','startup.safe_mode':'连续异常，安全模式启动','exit.clean':'正常退出','panic':'捕获原生异常','state.recovered_backup':'已从备份恢复','recovery.reset':'已退出安全模式','hooks.installed':'联动配置已安装','legacy.exported':'旧资料已导出','update.checked':'已检查 Companion 更新','update.verified':'更新摘要校验通过','state.save_error':'状态保存失败'}
async function refresh(){
  if(!props.native||refreshing.value)return;refreshing.value=true
  try{
    const value=await invoke<Health>('companion_health');if(disposed)return
    health.value=value;emit('safe',value.safeMode)
    const at=performance.now(),current=value.metrics.cpuMillis
    if(current!==null){if(lastCpu&&at>lastCpu.time)cpu.value=Math.max(0,(current-lastCpu.value)/(at-lastCpu.time)*100/Math.max(1,navigator.hardwareConcurrency||1));lastCpu={time:at,value:current}}
  }catch(e){error.value=String(e)}finally{refreshing.value=false}
}
async function perform(action:()=>Promise<void>){if(busy.value)return;busy.value=true;error.value='';info.value='';try{await action()}catch(e){error.value=String(e)}finally{busy.value=false}}
async function support(action:string){await perform(async()=>{await invoke('companion_support',{action});if(action==='resetRecovery')await refresh()})}
async function install(){await perform(async()=>{info.value=await invoke<string>('companion_install_hooks');await refresh()})}
async function chooseTab(next:typeof tab.value){tab.value=next;preview.value='';error.value='';info.value='';if(next==='data'&&props.native)await perform(async()=>{files.value=(await invoke<{files:LegacyFile[]}>('companion_legacy',{action:'list',ids:[]})).files})}
async function read(file:LegacyFile){await perform(async()=>{preview.value=(await invoke<{content:string}>('companion_legacy',{action:'read',ids:[file.id]})).content})}
async function exportSelected(){await perform(async()=>{const result=await invoke<{path:string}>('companion_legacy',{action:'export',ids:selected.value});info.value=`已导出到本地：${result.path}`})}
async function checkUpdate(){await perform(async()=>{update.value=await invoke<UpdateInfo>('companion_update',{action:'check'})})}
async function download(){await perform(async()=>{const result=await invoke<{path:string}>('companion_update',{action:'download'});info.value=`SHA-256 校验通过：${result.path}。请自行打开更新目录中的文件安装。`})}
onMounted(()=>{void refresh();timer=window.setInterval(()=>{if(tab.value==='status')void refresh()},2000)})
onBeforeUnmount(()=>{disposed=true;preview.value='';window.clearInterval(timer)})
</script>

<template>
  <section class="diagnostics" aria-label="企鹅设置与诊断">
    <header><div><strong>企鹅工具箱</strong><small>Companion {{health?.version??'0.3.0'}}</small></div><button class="back" @click="emit('close')">返回</button></header>
    <nav><button v-for="item in [{id:'status',label:'状态与恢复'},{id:'sync',label:'同步与数据'},{id:'data',label:'旧资料导出'},{id:'updates',label:'软件更新'}] as const" :class="{active:tab===item.id}" @click="chooseTab(item.id)">{{item.label}}</button></nav>
    <p v-if="!native" class="note">浏览器预览。诊断与旧资料操作仅在桌面版可用。</p>
    <div v-if="tab==='status'">
      <div v-if="health?.safeMode" class="warning"><strong>安全模式</strong><p>检测到连续异常退出，已临时关闭自主运动与环境感知。原设置已保留。</p><button :disabled="busy" @click="support('resetRecovery')">退出安全模式</button></div>
      <dl><dt>Codex 事件</dt><dd>{{health?.bridge.status??'待验证'}} · {{health?.bridge.activeSessions??0}} 个活动会话</dd><dt>最近一次事件</dt><dd>{{age}}</dd><dt>Node.js</dt><dd>{{health?.nodeAvailable?'已找到':'未确认'}}</dd><dt>事件配置</dt><dd>{{health?.configuredHooks??0}} 个处理器 · {{health?.scriptCurrent?'脚本为当前版本':'需安装 / 更新联动'}}</dd><dt>状态恢复</dt><dd>{{health?.restoredBackup?'本次已使用备份':'当前状态文件'}} · 连续异常 {{health?.crashStreak??0}} 次</dd><dt>忽略的无效事件</dt><dd>{{health?.rejectedEvents??0}}</dd></dl>
      <div class="actions"><button :disabled="busy||!native" @click="install">安装 / 更新联动</button><button :disabled="refreshing||!native" @click="refresh">刷新诊断</button></div>
      <p class="note">新版或改变的 hooks 需在 Codex 中重新审阅并信任。此处不会修改信任记录。</p>
      <h3>运行情况</h3><div class="metrics"><div><b>{{mib(health?.metrics.workingSetBytes)}}</b><small>工作集</small></div><div><b>{{mib(health?.metrics.privateBytes)}}</b><small>私有提交</small></div><div><b>{{cpu===null?'采样中':`${cpu.toFixed(2)}%`}}</b><small>CPU</small></div></div><p class="note">这里只统计 Rust 宿主，不包含 WebView2 子进程。完整进程树性能记录见交付报告。</p>
      <h3>本地日志</h3><div class="logs"><div v-for="(item,index) in health?.logs.slice(0,30)" :key="`${item.at}-${index}`"><time>{{new Date(item.at).toLocaleTimeString()}}</time><span>{{logLabels[item.event]??item.event}}</span></div><p v-if="!health?.logs.length">暂无日志。</p></div>
      <button :disabled="!native||busy" @click="support('openLogs')">打开日志目录</button><p class="note">日志只记录事件代码与时间，不保存聊天、窗口标题或凭据。</p>
    </div>
    <div v-else-if="tab==='data'">
      <p class="note">只读取原 PenguinPal 的已知记忆和聊天文件。查看与导出不会修改原文件，也不会自动写入 Codex。列表最多显示 100 份归档，仅支持 2 MB 以内的文件。</p>
      <div class="legacy-list"><div v-for="file in files" :key="file.id"><label><input v-model="selected" type="checkbox" :value="file.id" :disabled="!selected.includes(file.id)&&selected.length>=20"/><span><b>{{file.kind}} · {{file.name}}</b><small>{{(file.bytes/1024).toFixed(1)}} KB</small></span></label><button :disabled="busy" @click="read(file)">查看</button></div><p v-if="!files.length">没有找到可查看的旧资料。</p></div>
      <div class="actions"><button :disabled="busy||!selected.length||!native" @click="exportSelected">导出所选（{{selected.length}}）</button><button :disabled="busy||!native" @click="support('openExports')">打开导出目录</button></div>
      <pre v-if="preview" class="preview">{{preview}}</pre>
    </div>
    <CompanionSync v-else-if="tab==='sync'&&native" :native="native" @changed="(value,apply)=>emit('sync',value,apply)"/>
    <div v-else-if="tab==='updates'">
      <p class="note">只检查 PenguinPal Companion 正式发布包，不更新私有 Codex。手动联网检查，下载后必须通过 SHA-256；不会自动运行安装程序。</p>
      <div class="actions"><button :disabled="busy||!native" @click="checkUpdate">检查 Companion 更新</button><button :disabled="busy||!native" @click="support('openReleases')">打开发布页</button></div>
      <div v-if="update" class="update-info"><strong>{{update.message}}</strong><p>当前 {{update.currentVersion}}<template v-if="update.latestVersion"> · 已发布 {{update.latestVersion}}</template></p><small>{{update.assetName}}</small><button v-if="update.downloadable" :disabled="busy" @click="download">下载并校验</button></div>
      <button :disabled="busy||!native" @click="support('openUpdates')">打开更新目录</button>
    </div>
    <p v-if="busy" role="status" class="note">处理中…</p><p v-if="info" role="status" class="feedback">{{info}}</p><p v-if="error" role="alert" class="warning">{{error}}</p>
  </section>
</template>

<style scoped>
.diagnostics{width:424px;max-height:calc(100vh - 16px);overflow:auto;padding:18px;background:#0b2b43fa;border:1px solid #31526b;color:#e6f5ff;border-radius:16px;box-shadow:0 8px 24px #03132155;font-size:12px;line-height:1.6;scrollbar-width:thin;scrollbar-color:#3f6680 transparent}header{display:flex;align-items:center;justify-content:space-between}header strong{font-size:18px}small{display:block;color:#a5c7db;font-size:10px}button{border:0;border-radius:8px;background:#1c4966;color:#edf8ff;padding:8px 11px;font:inherit;cursor:pointer}button:hover{background:#2b6487}button:disabled{opacity:.45;cursor:default}button:focus-visible,input:focus-visible{outline:2px solid #79d6fa;outline-offset:2px}.back{background:transparent}nav{display:grid;grid-template-columns:repeat(3,1fr);gap:6px;margin:16px 0}nav button{font-size:11px;padding:8px 2px}nav .active{background:#59bfe4;color:#072c43}.note{font-size:10px;color:#b1cbdd;line-height:1.8}.warning{background:#5b4127;color:#ffe3b9;padding:12px;border-radius:10px;font-size:11px}.warning p{margin:5px 0}.feedback{background:#194e56;color:#d5f4ed;padding:12px;border-radius:10px;overflow-wrap:anywhere}dl{display:grid;grid-template-columns:110px 1fr;gap:7px 10px}dt{color:#a4c8dc}dd{margin:0;overflow-wrap:anywhere}.actions{display:flex;gap:8px;flex-wrap:wrap;margin:12px 0}h3{font-size:13px;margin:20px 0 9px}.metrics{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}.metrics div{background:#143d58;border-radius:10px;padding:10px}.metrics b{font-size:14px;font-weight:600}.logs{max-height:180px;overflow:auto;font-size:10px;margin-bottom:10px}.logs div{display:flex;gap:10px;padding:4px;border-bottom:1px solid #ffffff0a}.logs time{color:#8eafc4}.legacy-list{max-height:280px;overflow:auto}.legacy-list>div{display:flex;align-items:center;justify-content:space-between;gap:8px;padding:9px 0;border-bottom:1px solid #ffffff15}.legacy-list label{display:flex;align-items:center;gap:8px;min-width:0}.legacy-list b{font-size:11px;font-weight:500;overflow-wrap:anywhere}.legacy-list input{accent-color:#59bfe4}.legacy-list button{font-size:10px}.preview{max-height:250px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;background:#061f31;color:#d0e4f1;padding:12px;border-radius:10px;font-size:10px;user-select:text}.update-info{padding:14px;border-radius:10px;background:#163e56;margin:15px 0}.update-info button{display:block;margin-top:10px}.update-info p{margin:6px 0}
nav{grid-template-columns:repeat(4,1fr)}
</style>
