<script setup lang="ts">
import {computed,nextTick,onMounted,ref} from 'vue'
import {invoke} from '@tauri-apps/api/core'
import type {ChatView,SyncView} from '../lib/companionSync'
import CompanionMind from './CompanionMind.vue'
const props=defineProps<{native:boolean;view:SyncView|null;session:string;allowWorkChat:boolean}>()
const emit=defineEmits<{close:[];received:[ChatView];tools:[];workChat:[boolean]}>()
const text=ref(''),busy=ref(false),error=ref(''),info=ref(''),history=ref<HTMLElement>()
const sessions=ref<{id:string;name:string}[]>([]),selected=ref('')
const llm=ref({provider:'codex',model:'',baseUrl:''})
async function loadLlm(){try{const value=await call('llmGet');llm.value={provider:value.provider,model:value.model,baseUrl:value.baseUrl}}catch{/* 保持默认显示 */}}
async function switchProvider(provider:string){await run(async()=>{const value=await call('llmSet',{provider,model:llm.value.model,baseUrl:llm.value.baseUrl});llm.value={provider:value.provider,model:value.model,baseUrl:value.baseUrl};info.value=value.provider==='deepseek'?'已切换到 DeepSeek（'+(value.model||'deepseek-chat')+'），聊天与记忆整理都走 DeepSeek。':'已切换到 Codex CLI，需要已登录的 Codex 桌面应用。'})}
onMounted(()=>{void loadLlm()})
const messages=computed(()=>props.view?.document.conversations.filter(m=>m.session===props.session)??[])
const enabled=computed(()=>props.view?.document.social?.enabled??true)
const proactive=computed(()=>props.view?.document.social?.proactive??false)
async function call(action:string,payload:unknown=null){return invoke<any>('companion_chat',{action,payload})}
async function run(work:()=>Promise<void>){if(busy.value)return;busy.value=true;error.value='';info.value='';try{await work();await nextTick();history.value?.scrollTo({top:history.value.scrollHeight})}catch(e){error.value=String(e)}finally{busy.value=false}}
async function send(){if(!text.value.trim())return;await run(async()=>{const value:ChatView=await call('send',{text:text.value});emit('received',value);text.value=''})}
async function settings(learn:boolean,opener:boolean){await run(async()=>emit('received',await call('settings',{enabled:learn,proactive:opener})))}
async function learn(){await run(async()=>{emit('received',await call('learn'));info.value='已处理下一批历史消息，剩余内容会在后台继续整理；只保存有原话依据的内容。'})}
async function list(){await run(async()=>{sessions.value=(await call('sessions')).sessions;if(!sessions.value.length)info.value='暂未找到可读取的聊天记录。'})}
async function read(){if(!selected.value)return;await run(async()=>{emit('received',await call('read',{threadId:selected.value}));info.value='已带入选定聊天的近期内容，可以整理回忆或直接聊聊。'})}
</script>

<template>
  <section class="bubble-chat" aria-label="和企鹅聊聊">
    <header><div><strong>和企鹅聊聊</strong><small> · {{llm.provider==='deepseek'?'DeepSeek '+(llm.model||'deepseek-chat'):'Codex CLI'}}</small><select class="provider" :value="llm.provider" :disabled="busy||!native" @change="switchProvider(($event.target as HTMLSelectElement).value)" aria-label="切换模型来源"><option value="codex">Codex</option><option value="deepseek">DeepSeek</option></select><small>{{view?.mode==='server'?'对话与陪伴状态自动同步':'对话与陪伴状态保存在本机'}}</small></div><button aria-label="关闭聊天" @click="emit('close')">×</button></header>
    <p class="mood">心情 {{Math.round(view?.document.pet.mood??70)}} · {{view?.document.pet.moodReason||'平稳陪伴'}}<small>心情随互动温和变化，之后慢慢恢复平稳。</small></p>
    <div ref="history" class="history" aria-live="polite">
      <p v-if="!messages.length" class="empty">想聊什么都可以。也可以告诉我你的喜好，或我们的小约定。</p>
      <article v-for="m in messages.slice(-60)" :key="m.id" :class="m.role"><b>{{m.role==='user'?'你':'企鹅'}}</b><p>{{m.text}}</p></article>
    </div>
    <form @submit.prevent="send"><textarea v-model="text" aria-label="聊天内容" placeholder="和企鹅说句话…" rows="3" maxlength="3000" :disabled="busy||!native" @keydown.ctrl.enter.prevent="send"/><div class="send"><small>Ctrl + Enter 发送</small><button class="primary" :disabled="busy||!native||!text.trim()">{{busy?'正在想…':'发送'}}</button></div></form>
    <p v-if="error" class="error" role="alert">{{error}}<small>已保存的消息会保留，输入内容仍可编辑。</small></p><p v-if="info" class="info" role="status">{{info}}</p>
    <details><summary>记忆与主动陪伴</summary>
      <label><input type="checkbox" :checked="enabled" :disabled="busy||!native" @change="settings(($event.target as HTMLInputElement).checked,proactive)"/>自动从所有可读取聊天记住喜好、约定和近况</label>
      <small>{{llm.provider==='deepseek'?'启动后整理宠物自己记录下来的对话，持续分批补记忆；不会扫描 Codex 会话。历史只补记忆，气泡实时互动才改变心情。采集开关在同步设置中。':'启动后自动扫描本机 Codex 项目内外及归档会话，持续分批整理。历史只补记忆，气泡实时互动才改变心情。采集开关在同步设置中。'}}</small>
      <p v-if="view?.history" class="info" role="status">已收录 {{view.history.archived}} 条 · 已整理 {{view.history.processed}} 条用户消息 · 待整理 {{view.history.pending}} 条<small>已发现 {{view.history.files}} 个文件，待扫描 {{view.history.pendingFiles}} 个；待发现目录 {{view.history.pendingDirectories}} 个</small></p>
      <p v-if="view?.history?.learningError||view?.history?.scanError||view?.history?.fileErrors" class="error" role="alert">{{view?.history?.learningError||view?.history?.scanError||'部分对话文件暂不可读，后台会重试。'}}<small>已收录的原文与整理进度保留；可在记忆 / 同步中检查并重试。</small></p>
      <label><input type="checkbox" :checked="proactive" :disabled="busy||!native" @change="settings(enabled,($event.target as HTMLInputElement).checked)"/>空闲时主动找我聊聊</label>
      <small>空闲且未忙碌时搭话；当前冷却 {{view?.document.brain?.config.cooldownMinutes??45}} 分钟，每天最多 {{view?.document.brain?.config.dailyLimit??4}} 次。不因未回应减频或暂停。服务器离线时暂停主动搭话。</small>
      <p v-if="view?.history?.outreachError" class="error" role="alert">主动陪伴暂未完成：{{view.history.outreachError}}</p>
      <CompanionMind :view="view" :native="native" :allow-work-chat="allowWorkChat" @work-chat="emit('workChat',$event)" @received="emit('received',$event)"/>
      <div class="actions"><button :disabled="busy||!native||!enabled" @click="learn">立即整理下一批</button><button @click="emit('tools')">记忆 / 同步 / 备份</button></div>
      <details><summary>指定一个聊天重点回顾（可选）</summary><small>全部本机会话已自动纳入扫描；也可单独带入一个聊天的近期内容。</small><button :disabled="busy||!native" @click="list">选择历史聊天</button><select v-if="sessions.length" v-model="selected" aria-label="选择历史聊天"><option value="" disabled>请选择</option><option v-for="s in sessions" :key="s.id" :value="s.id">{{s.name}}</option></select><button v-if="sessions.length" :disabled="busy||!selected" @click="read">带入近期内容</button></details>
    </details>
  </section>
</template>
<style scoped>
.bubble-chat{width:420px;max-height:calc(100vh - 20px);height:710px;display:flex;flex-direction:column;gap:10px;padding:18px;border:1px solid #b5d5e6;border-radius:20px;background:#f0f9fffa;box-shadow:0 8px 28px #0b304533;user-select:text;overflow:auto}header{display:flex;justify-content:space-between;align-items:center}header strong{font-size:17px}small{display:block;color:#53758a;font-size:11px;line-height:1.6}button{border:1px solid #b3cfde;border-radius:10px;background:#fff;color:#234b63;padding:7px 11px}header button{font-size:18px;padding:2px 9px}.mood{margin:0;padding:9px 11px;background:#e2f1fa;border-radius:12px;font-size:11px;line-height:1.6;overflow-wrap:anywhere}.history{min-height:120px;flex:1;overflow:auto;padding:2px 6px}.empty{color:#638298;line-height:1.8;padding:20px 10px}article{max-width:92%;border-radius:13px;padding:10px 12px;margin-bottom:10px;background:#fff;border:1px solid #d7e6ef}article.user{margin-left:auto;background:#dceffb}article b{font-size:10px;color:#4a7891}article p{white-space:pre-wrap;overflow-wrap:anywhere;margin:4px 0 0;line-height:1.7}textarea,select{font:inherit;width:100%;border:1px solid #b3cfde;background:#fff;border-radius:11px;padding:10px;resize:vertical;color:#173c56}textarea{min-height:64px;max-height:140px}.send{display:flex;justify-content:space-between;align-items:center;margin-top:6px}.primary{background:#237da4;color:#fff;border-color:#237da4}button:disabled{opacity:.5;cursor:default}details{font-size:11px;line-height:1.7}summary{cursor:pointer;font-weight:600}label{display:flex;align-items:flex-start;gap:5px;margin:8px 0}.actions{display:flex;gap:6px;margin:9px 0}details details{padding-top:6px}details small{margin:5px 0}.error,.info{margin:0;padding:8px 10px;border-radius:10px;line-height:1.6;overflow-wrap:anywhere}.error{background:#fff0e5;color:#8d4a28}.info{background:#e2f4ef;color:#286650}
</style>
