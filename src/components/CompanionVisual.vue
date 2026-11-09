<script setup lang="ts">
import {computed,onBeforeUnmount,onMounted,ref} from 'vue'
import {invoke} from '@tauri-apps/api/core'
import QRCode from 'qrcode'
import {defaultVisualSettings,starterWorkflow,managedWorkflow,wechatMemeStyle,animeReferenceStyle,type VisualBoard} from '../lib/companionVisual'
const props=defineProps<{native:boolean;board:VisualBoard|null;reference:string}>()
const emit=defineEmits<{changed:[VisualBoard]}>()
const draft=ref(defaultVisualSettings()),busy=ref(false),error=ref(''),info=ref(''),qr=ref(''),loginStatus=ref(''),code=ref('')
const models=ref<string[]>([]),model=ref('')
const tokens={prompt:'{{prompt}}',seed:'{{seed}}',image:'{{image}}'}
let disposed=false,loginTimer:number|undefined,loginGeneration=0
const recovery=computed(()=>{const w=props.board?.wechat;return (w?.processing??0)+(w?.sending??0)+(w?.failed??0)+(w?.uncertain??0)+(w?.imagesUncertain??0)})
const runtimeLabels:Record<string,string>={missing:'尚未安装',installing:'正在安装，请稍候',stopped:'已安装，绘图时自动启动',starting:'后台启动中',running:'后台运行中',unavailable:'当前环境不支持内置安装',busy:'正在检查'}
const loginLabels:Record<string,string>={wait:'请使用微信扫描二维码',scaned:'已扫码，请在微信确认',need_verifycode:'请填写微信显示的配对码',verify_code_blocked:'配对码尝试受限，请稍后重新扫码',expired:'二维码已过期，请重新获取',binded_redirect:'该账号已有绑定，请按微信提示处理',confirmed:'微信已连接'}
async function call(action:string,payload:unknown=null){if(!props.native)throw Error('绘图和微信设置需在桌宠应用中使用');return invoke<any>('companion_chat',{action,payload})}
function accept(value:VisualBoard){if(disposed)return;draft.value={...value.settings};emit('changed',value)}
async function run(work:()=>Promise<void>){if(busy.value)return;busy.value=true;error.value='';info.value='';try{await work()}catch(e){if(!disposed)error.value=String(e)}finally{busy.value=false}}
async function save(){await run(async()=>{if(draft.value.enabled&&draft.value.workflow.includes('{{image}}')&&!draft.value.hasReference)throw Error('请先上传角色参考图，或明确选择使用内置参考图');accept(await call('visualSave',draft.value));info.value='已保存。自动图片会随聊天回复生成；手动可说“给我画张今日心情卡”。'})}
async function useManaged(){draft.value={...draft.value,enabled:true,managedRuntime:true,baseUrl:'http://127.0.0.1:8198',workflow:managedWorkflow(props.board?.runtime?.model),timeoutSeconds:600};await save();if(!error.value)info.value='已启用智能绘图：普通表情自由生成，画桌宠本人或按参考图画时自动读取已上传参考图。后台按需启动，空闲 5 分钟后释放内存。'}
async function check(){await run(async()=>{accept(await call('visualSave',draft.value));const v=await call('visualCheck');models.value=v.models??[];model.value=models.value[0]??'';info.value='本机绘图服务已连接'+(v.devices?.length?' · '+v.devices.map((x:any)=>x.name).join(' / '):'')})}
async function workflow(event:Event){const file=(event.target as HTMLInputElement).files?.[0];if(!file)return;await run(async()=>{if(file.size>2*1024*1024)throw Error('工作流最多 2 MiB');draft.value.workflow=await file.text();info.value='已读取工作流，请保存设置。'})}
async function reference(event:Event){const file=(event.target as HTMLInputElement).files?.[0];if(!file)return;await run(async()=>{if(file.size>8*1024*1024)throw Error('参考图最多 8 MiB');await setReference(await file.arrayBuffer());info.value='已保存角色参考图。内置快速绘图会在画桌宠本人或按参考图画时读取它；普通聊天表情继续自由生成。'})}
async function setReference(bytes:ArrayBuffer){let raw='';for(const x of new Uint8Array(bytes))raw+=String.fromCharCode(x);const result=await call('visualReference',{bytes:btoa(raw)});draft.value.hasReference=result.settings.hasReference;emit('changed',result)}
async function currentReference(){await run(async()=>{const response=await fetch(props.reference);if(!response.ok)throw Error('无法读取当前企鹅参考图');await setReference(await response.arrayBuffer());info.value='已使用内置企鹅参考图。角色一致性取决于工作流和模型。'})}
async function pollLogin(generation:number){
  if(disposed||generation!==loginGeneration)return
  try{
    const result=await call('visualLoginPoll',{verifyCode:code.value.trim()});if(disposed||generation!==loginGeneration)return
    loginStatus.value=result.status
    if(result.status==='confirmed'){qr.value='';code.value='';accept(await call('visualStatus'));return}
    if(['expired','verify_code_blocked','binded_redirect'].includes(result.status))return
    if(result.status==='need_verifycode'&&!code.value.trim())return
    loginTimer=window.setTimeout(()=>{void pollLogin(generation)},1500)
  }catch(e){if(!disposed&&generation===loginGeneration)error.value=String(e)}
}
async function login(){await run(async()=>{const generation=++loginGeneration;window.clearTimeout(loginTimer);const result=await call('visualLogin');if(disposed||generation!==loginGeneration)return;qr.value=await QRCode.toDataURL(result.url,{width:220,margin:2,errorCorrectionLevel:'M'});loginStatus.value='wait';code.value='';void pollLogin(generation)})}
async function disconnect(){++loginGeneration;window.clearTimeout(loginTimer);await run(async()=>{accept(await call('visualDisconnect'));qr.value='';loginStatus.value='';info.value='已清除本机微信授权。已发出的消息无法撤回。'})}
onMounted(()=>{if(props.native)void run(async()=>accept(await call('visualStatus')))})
onBeforeUnmount(()=>{disposed=true;++loginGeneration;window.clearTimeout(loginTimer)})
</script>

<template>
  <details class="visual-settings"><summary>表情包、心情卡与微信</summary>
    <small>让企鹅结合性格、心情和当前聊天选择普通聊天表情包，不要求画成桌宠。文字先回复，图片完成后附在同一条消息下。</small>
    <p class="hint">桌宠内置绘图：{{runtimeLabels[board?.runtime?.state??'missing']??board?.runtime?.state}}<small v-if="board?.runtime?.state==='installing'">安装进度 {{board.runtime.install?.progress??0}}%。组件与模型约占数 GB，不会安装到共享源码仓库。</small><small v-if="board?.runtime?.root">组件目录：{{board.runtime.root}}</small></p>
    <div class="actions"><button :disabled="busy||!native||board?.runtime?.state==='installing'" @click="run(async()=>accept(await call('visualRuntimeInstall')))">{{board?.runtime?.installed?'修复绘图组件':'安装内置绘图组件与模型'}}</button><button :disabled="busy||!native||!board?.runtime?.installed" @click="useManaged">使用内置快速绘图</button><button v-if="['running','starting'].includes(board?.runtime?.state??'')" :disabled="busy||board?.job.state==='drawing'" @click="run(async()=>accept(await call('visualRuntimeStop')))">停止后台并释放内存</button></div>
    <p v-if="board?.runtime?.error||board?.runtime?.install?.state==='failed'" class="warning">{{board.runtime.error||board.runtime.install?.message}}</p>
    <label><input v-model="draft.enabled" type="checkbox" :disabled="busy||!native"/>启用本机绘图</label>
    <label><input v-model="draft.automatic" type="checkbox" :disabled="busy||!native"/>聊天时由企鹅决定是否发表情</label>
    <small>自动图片默认每 30 分钟最多一次，每 24 小时最多生成 4 张新图；合适的旧表情可复用。主动叫它画图不占自动额度。</small>
    <label><input v-model="draft.managedRuntime" type="checkbox" :disabled="busy||!native||!board?.runtime?.installed" @change="draft.baseUrl=draft.managedRuntime?'http://127.0.0.1:8198':'http://127.0.0.1:8188'"/>由桌宠按需管理绘图后台</label>
    <label v-if="!draft.managedRuntime" class="field">外部本机 ComfyUI 地址<input v-model="draft.baseUrl" :disabled="busy||!native" placeholder="http://127.0.0.1:8188"/></label>
    <div class="actions"><button :disabled="busy||!native" @click="check">检测本机服务</button><button :disabled="busy||!native" @click="save">保存设置</button></div>
    <p v-if="!draft.hasWorkflow&&!draft.workflow" class="hint">先安装内置绘图组件，再点击「使用内置快速绘图」。普通表情无需参考图；画桌宠本人请先上传参考图。也可在高级设置中连接自己的本机 ComfyUI。</p>
    <details><summary>绘图风格与高级设置</summary>
      <small>可导入 ComfyUI 的 API 格式 JSON：正向提示词用 {{tokens.prompt}}，种子用 {{tokens.seed}}；角色参考图节点的 image 输入用 {{tokens.image}}。只导入可信工作流。</small>
      <label class="field">导入工作流<input type="file" accept=".json,application/json" :disabled="busy||!native" @change="workflow"/></label>
      <small>{{draft.workflow?'工作流已载入':'工作流未配置'}}</small>
      <div v-if="models.length"><label class="field">已安装模型<select v-model="model"><option v-for="item in models" :key="item">{{item}}</option></select></label><button :disabled="busy" @click="draft.workflow=starterWorkflow(model);info='已生成基础工作流，请保存。'">使用基础 SD 1.x 工作流</button><small>基础工作流不使用参考图；请选兼容的 SD 1.x 模型。其他模型或角色参考需要自定义工作流。</small></div>
      <label class="field">图片风格（可选）<textarea v-model="draft.character" maxlength="600" rows="3" :disabled="busy||!native"/></label>
      <button :disabled="busy||!native" @click="draft.character=wechatMemeStyle;info='已选择二维动漫表情风格，请保存设置。'">使用二维动漫表情风格</button><small>普通表情用二维动漫画风，可选动物或Q版卡通题材，按对话表达呆萌、嫌弃、震惊等情绪，每张一个主体，不用立体或照片；可继续修改上面的风格文字。不会把收藏截图当作角色参考图。</small>
      <div class="actions"><button :disabled="busy||!native" @click="currentReference">使用内置企鹅参考图</button><span>{{draft.hasReference?'已设置参考图':'未设置参考图'}}</span></div>
      <label class="field">上传桌宠角色参考图（画本人时使用）<input type="file" accept="image/png,image/jpeg,image/webp" :disabled="busy||!native" @change="reference"/></label>
      <button :disabled="busy||!native" @click="Object.assign(draft,animeReferenceStyle);info='已选择二维动漫参考风格，请保存设置。'">保留二维动漫参考画风</button>
      <label class="field">本人图的画风补充<textarea v-model="draft.referenceStyle" maxlength="600" rows="2" :disabled="busy||!native"/></label>
      <label class="field">本人图避免的画风<textarea v-model="draft.referenceNegative" maxlength="600" rows="2" :disabled="busy||!native"/></label>
      <label class="field">参考图重绘幅度（内置流程）<input :value="draft.referenceDenoise??0.2" type="number" min="0.1" max="0.7" step="0.05" :disabled="busy||!native" @input="draft.referenceDenoise=Number(($event.target as HTMLInputElement).value)"/></label><small>默认 0.2，优先保留原画风；提高后动作变化更大，也更容易变形或换画风。自定义工作流的采样和负面提示参数保持原样。</small>
      <div class="numbers"><label class="field">自动冷却（分钟）<input v-model.number="draft.cooldownMinutes" type="number" min="1" max="1440"/></label><label class="field">每日新图上限<input v-model.number="draft.dailyLimit" type="number" min="1" max="20"/></label></div>
      <label class="field">等待上限（秒）<input v-model.number="draft.timeoutSeconds" type="number" min="10" max="600"/></label>
      <small>CPU 出图可能较慢，首次绘图还需载入模型。文字会先回复；组件与模型由桌宠单独管理，空闲后释放内存，不占单个 EXE。</small>
    </details>
    <div class="actions"><button :disabled="busy||!native" @click="run(async()=>{await call('visualOpenFolder')})">打开本机图库</button><button v-if="board?.job.state==='drawing'" :disabled="busy" @click="run(async()=>emit('changed',await call('visualCancel')))">取消这次绘图</button></div>
    <details><summary>微信私聊</summary>
      <small>扫码后只回应绑定主人的文字消息，沿用桌宠的记忆、心情和性格。启用绘图后可回传生成图片。二维码在本机生成，授权凭据受当前 Windows 用户保护。</small>
      <p v-if="board?.wechat.connected" class="hint">已绑定 {{board.wechat.account}}<small>待处理 {{board.wechat.queued??0}} 条</small></p>
      <small v-if="board?.wechat.receive?.polledAt">最近收信检查 {{new Date(board.wechat.receive.polledAt).toLocaleTimeString()}} · 收到 {{board.wechat.receive.received??0}} 条，接收 {{board.wechat.receive.accepted??0}} 条</small>
      <p v-if="recovery" class="warning">有 {{recovery}} 条记录未完成或发送结果待确认。进行中的回复请稍候；重启后仍未完成的不会自动重发，请核对微信实际消息。</p>
      <p v-if="board?.wechat.error" class="warning">{{board.wechat.error}}</p>
      <div class="actions"><button :disabled="busy||!native||!!qr" @click="login">{{board?.wechat.connected?'重新扫码':'扫码连接微信'}}</button><button v-if="board?.wechat.connected||qr" :disabled="busy" @click="disconnect">断开并清除授权</button></div>
      <div v-if="qr" class="qr"><img :src="qr" alt="微信绑定二维码"/><small>{{loginLabels[loginStatus]||loginStatus}}</small></div>
      <div v-if="loginStatus==='need_verifycode'"><label class="field">微信配对码<input v-model="code" inputmode="numeric" maxlength="12" autocomplete="off"/></label><button :disabled="!code.trim()" @click="pollLogin(loginGeneration)">提交配对码</button></div>
      <button v-if="qr&&['expired','verify_code_blocked','binded_redirect'].includes(loginStatus)" :disabled="busy" @click="login">重新获取二维码</button>
    </details>
    <p v-if="error" class="warning" role="alert">{{error}}</p><p v-if="info" class="hint" role="status">{{info}}</p>
  </details>
</template>
<style scoped>
.visual-settings{font-size:11px;line-height:1.7}summary{cursor:pointer;font-weight:600}small{display:block;color:#53758a;line-height:1.6;margin:6px 0}.field{display:flex;flex-direction:column;gap:4px}label{display:flex;gap:5px;margin:9px 0}input:not([type=checkbox]),textarea,select{width:100%;font:inherit;padding:8px;border:1px solid #b3cfde;border-radius:9px;background:white;color:#173c56}input[type=file]{font-size:10px}button{border:1px solid #b3cfde;border-radius:10px;background:white;color:#234b63;padding:7px 10px}button:disabled{opacity:.5}.actions{display:flex;align-items:center;gap:6px;margin:10px 0;flex-wrap:wrap}.numbers{display:flex;gap:10px}.numbers label{flex:1;min-width:0}.hint,.warning{margin:8px 0;padding:9px;border-radius:9px;overflow-wrap:anywhere}.hint{background:#e2f4ef;color:#286650}.warning{background:#fff0e5;color:#8d4a28}details details{margin-top:10px}.qr{text-align:center}.qr img{width:200px;max-width:100%;border-radius:10px}textarea{resize:vertical}
</style>
