<script setup lang="ts">
import { ref } from 'vue'
import type { MediaPack } from '../lib/companionMedia'
import type { Activity, CareAction, CompanionState, Preferences } from '../lib/companionCore'
const props = defineProps<{ state: CompanionState; status: string; connection: string; expanded: boolean; packs:MediaPack[]; native:boolean; importing:boolean; systemReduced:boolean; musicStatus:string }>()
const emit = defineEmits<{
  care: [CareAction]; action: [Activity]; codex: []; close: []; hide: []; quit: []; install: []
  preference: [Partial<Preferences>]; expand: [boolean]; snap: []; tools: []; importPack:[]; refreshPacks:[]; packFolder:[]
}>()
const privacy = ref(false),tab=ref<'home'|'actions'|'media'>('home'),category=ref('互动')
function showTab(value:'home'|'actions'|'media'){tab.value=value;emit('expand',value!=='home')}
const actions:{group:string;label:string;activity?:Activity;care?:CareAction}[]=[
  {group:'互动',label:'挥手',activity:'greet'},{group:'互动',label:'开心',activity:'happy'},{group:'互动',label:'摸摸',care:'pat'},{group:'互动',label:'戳戳',care:'poke'},{group:'互动',label:'尾巴',care:'tail'},
  {group:'生活',label:'小鱼',care:'feed'},{group:'生活',label:'休息',care:'rest'},{group:'生活',label:'醒来',activity:'wake'},{group:'生活',label:'扫地',activity:'sweep'},
  {group:'表情',label:'发呆',activity:'think'},{group:'表情',label:'张望',activity:'look'},{group:'表情',label:'眨眼',activity:'blink'},{group:'表情',label:'说话',activity:'talk'},{group:'表情',label:'小脾气',activity:'annoyed'}]
function play(item:typeof actions[number]){if(item.care)emit('care',item.care);else if(item.activity)emit('action',item.activity)}
const patch = (value: Partial<Preferences>) => emit('preference', value)
const toggle = (key: keyof Preferences, e: Event) => patch({ [key]: (e.target as HTMLInputElement).checked })
const movement = (mode: Preferences['movement']) => patch({ movement: mode, wander: mode === 'wander' })
const toggles: { key: keyof Preferences; label: string }[] = [
  { key: 'awareness', label: '本地感知' }, { key: 'topmost', label: '窗口置顶' },
  { key: 'fullscreenHide', label: '全屏隐藏' }, { key: 'hoverPause', label: '悬停暂停' },
  { key: 'reducedMotion', label: '减少动态' }, { key: 'surfaces', label: '窗口落脚' },
  { key: 'gravity', label: '抛掷重力' }, { key: 'passthrough', label: '角色穿透' },
  { key: 'quiet', label: '安静陪伴' }, { key: 'autostart', label: '开机自启' },
  { key: 'edgeDock', label: '贴边停靠' }, { key: 'autoHide', label: '自动半隐藏' },
  { key: 'notifications', label: '任务提醒' },
  { key: 'springDrag', label: '弹性拖拽' }, { key: 'statusAnimations', label: '任务动作' },
]
</script>

<template>
  <section class="control-panel" aria-label="企鹅快速控制">
    <header><strong>企鹅</strong><span class="state-tag">{{ status }}</span><button class="close" aria-label="关闭控制面板" @click="emit('close')">×</button></header>
    <nav class="tabs" aria-label="控制分类"><button v-for="(label,key) in {home:'陪伴',actions:'动作',media:'素材'}" :key="key" :class="{selected:tab===key}" @click="showTab(key)">{{label}}</button></nav>
    <template v-if="tab==='home'">
    <div class="quick-actions">
      <button @click="emit('care', 'feed')">投喂</button><button @click="emit('codex')">Codex</button>
      <button @click="emit('action', 'happy')">开心</button><button @click="emit('care', 'rest')">休息</button>
    </div>
    <div class="rule"></div><label class="caption">移动</label>
    <div class="choices">
      <button v-for="(label, mode) in { wander: '散步', follow: '跟随', still: '静止' }" :key="mode"
        :class="{ selected: props.state.preferences.movement === mode }" :aria-pressed="props.state.preferences.movement === mode" @click="movement(mode)">{{ label }}</button>
    </div>
    <div class="panel-footer"><button @click="emit('expand', !expanded)">{{ expanded ? '收起设置' : '更多设置' }}</button><button @click="emit('tools')">工具箱</button><button @click="emit('hide')">隐藏</button></div>
    <div class="rule"></div><label class="caption" for="music-mode">音乐打碟 · 双彩灯</label>
    <select id="music-mode" :value="state.preferences.musicMode" @change="patch({musicMode:($event.target as HTMLSelectElement).value as Preferences['musicMode']})"><option value="auto">自动识别音乐</option><option value="audio">音频跟随（含浏览器）</option><option value="off">关闭</option></select>
    <small role="status">{{musicStatus}}</small><small>关闭面板后开始律动。自动模式识别音乐播放器；浏览器未识别时选音频跟随，此时人声、游戏也可能触发。</small>
    <div v-if="expanded" class="advanced">
      <div class="rule"></div><label class="caption">大小</label>
      <div class="choices"><button v-for="item in [{label:'小',value:.8},{label:'中',value:1},{label:'大',value:1.2}]" :key="item.value" :class="{ selected: state.preferences.size === item.value }" @click="patch({ size: item.value })">{{ item.label }}</button></div>
      <div class="mini-actions"><button @click="emit('action', 'think')">发呆</button><button @click="emit('action', 'sweep')">扫地</button><button @click="emit('care', 'pat')">摸摸</button></div>
      <label class="caption">陪伴节奏</label><div class="choices"><button v-for="(label, value) in { calm: '安静', balanced: '适中', lively: '活泼' }" :class="{ selected: state.preferences.rhythm === value }" @click="patch({ rhythm: value })">{{ label }}</button></div>
      <label class="caption">走路速度</label><div class="choices"><button v-for="(label, value) in { '35': '慢', '55': '适中', '80': '快' }" :class="{ selected: state.preferences.walkSpeed === Number(value) }" @click="patch({ walkSpeed: Number(value) })">{{ label }}</button></div>
      <div class="toggle-grid"><label v-for="item in toggles" :key="item.key"><input type="checkbox" :checked="Boolean(state.preferences[item.key])" @change="toggle(item.key, $event)" /><span>{{ item.label }}</span></label></div>
      <label class="caption">陪伴语气</label><select aria-label="陪伴语气" :value="state.preferences.personality" @change="patch({ personality: ($event.target as HTMLSelectElement).value as Preferences['personality'] })"><option value="gentle">温柔</option><option value="playful">俏皮</option><option value="quiet">少言</option><option value="warm">关心</option></select>
      <div class="rule"></div><label class="bridge-toggle"><input type="checkbox" :checked="state.preferences.codexEvents" @change="toggle('codexEvents', $event)" />跟随 Codex 工作状态</label>
      <button class="install" @click="emit('install')">安装 / 更新 Codex 联动</button><small>{{ connection }}</small>
      <div class="utilities"><button @click="emit('snap')">回到屏幕内</button><button @click="privacy = !privacy">隐私</button><button class="danger" @click="emit('quit')">退出</button></div>
      <p v-if="privacy" class="privacy">聊天、语音与长期记忆由现有 Codex 管理。场景感知仅在本地使用进程类别、空闲和全屏状态；不读标题、不截图。事件桥只保留匿名状态元数据。</p>
    </div>
    </template>
    <div v-else-if="tab==='actions'" class="motion-library">
      <p class="hint">给企鹅换一个动作，任务状态仍由 Codex 提供。</p>
      <div class="choices"><button v-for="group in ['互动','生活','表情']" :key="group" :class="{selected:category===group}" @click="category=group">{{group}}</button></div>
      <div class="action-grid"><button v-for="item in actions.filter(a=>a.group===category)" :key="item.label" @click="play(item)"><span aria-hidden="true">{{item.label==='挥手'?'✦':item.label==='小鱼'?'♡':'·'}}</span>{{item.label}}</button></div>
      <p class="hint">14 个动作均配透明短视频<br/>关闭视频可使用帧图；拖动可贴边停靠。</p>
      <button class="install" @click="emit('codex')">打开 Codex 聊天 ↗</button>
    </div>
    <div v-else class="motion-library">
      <p class="hint">使用自己的企鹅素材。动画包可补充 PNG 逐帧或透明 WebM 动作，缺少的动作沿用内置素材。</p>
      <label class="caption" for="pack-select">当前动画包</label>
      <select id="pack-select" :value="state.preferences.animationPack" @change="patch({animationPack:($event.target as HTMLSelectElement).value})">
        <option value="builtin">内置企鹅</option><option v-for="pack in packs" :key="pack.id" :value="pack.id">{{pack.name}} · {{pack.clips.length}} 个动作</option>
      </select>
      <p v-if="state.preferences.animationPack!=='builtin'&&!packs.some(p=>p.id===state.preferences.animationPack)" class="hint">原动画包暂不可用，当前显示内置企鹅。</p>
      <label class="bridge-toggle media-switch"><input type="checkbox" :checked="state.preferences.videoAnimations" @change="toggle('videoAnimations',$event)"/>播放连续视频</label>
      <small>减少动态或安全模式使用静态预览；隐藏时暂停视频。透明区域点击按预览图判断。</small>
      <small v-if="systemReduced" class="hint">系统减少动态已开启，正常陪伴使用静态预览；连续动画随系统动画设置调整。</small>
      <div class="pack-controls"><button :disabled="!native||importing" @click="emit('importPack')">{{importing?'正在导入…':'导入动画包'}}</button><button :disabled="!native" @click="emit('refreshPacks')">刷新</button><button :disabled="!native" @click="emit('packFolder')">文件夹</button></div>
      <p class="hint">选择素材目录中的 manifest.json；不直接兼容 dsh-pet 配置。清单与所有素材放在同一目录，PNG 画布为 256 × 256。格式和示例随本版说明提供。</p>
    </div>
  </section>
</template>

<style scoped>
.tabs{display:flex;gap:5px;margin-bottom:14px;padding:4px;background:#061e303d;border-radius:12px}.tabs button{flex:1;background:none;color:#a9cde2}.tabs .selected{background:#245b7a;color:#e7f5ff}.hint{font-size:11px;color:#a9cde2;line-height:1.8;margin:10px 0 14px}.action-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:9px;margin:16px 0}.action-grid button{display:flex;flex-direction:column;align-items:center;gap:8px;padding:16px 5px;background:#153c55}.action-grid span{color:#80d7f6;font-size:23px;line-height:1}.pack-controls{display:flex;gap:6px;margin:16px 0}.pack-controls button:first-child{flex:1;background:#55b9df;color:#06283c}button:disabled{opacity:.45;cursor:default}.media-switch{margin:15px 0 9px}.control-panel{width:264px;padding:14px;color:#e7f5ff;background:#0c2c43f7;border:1px solid #24495f;border-radius:16px;box-shadow:0 8px 24px #03132155;font-size:12px;line-height:1.4;max-height:calc(100vh - 16px);overflow:auto;scrollbar-width:thin;scrollbar-color:#326078 transparent}
header{display:flex;gap:8px;align-items:center;margin-bottom:13px}header strong{font-size:18px;letter-spacing:1px}.state-tag{font-size:10px;background:#174763;color:#c7e7f6;border-radius:8px;padding:5px 8px}.close{margin-left:auto;font-size:22px;padding:0 3px;background:none}
button{border:0;border-radius:9px;background:#17435e;color:#e7f5ff;font:inherit;font-weight:600;padding:9px 6px;cursor:pointer}button:hover{background:#245b7a}button:focus-visible{outline:2px solid #80d7f6;outline-offset:2px}.selected{background:#55b9df;color:#072b40}.selected:hover{background:#7cd4f3}
.quick-actions{display:grid;grid-template-columns:repeat(4,1fr);gap:6px}.quick-actions button{padding:10px 2px}.rule{height:1px;background:#b8dcf026;margin:12px 0}.caption{display:block;font-size:11px;font-weight:600;color:#a9cde2;margin:8px 0 7px}.choices{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}.choices button{background:#153c55}.choices .selected{background:#55b9df;color:#06283c}.panel-footer{display:flex;margin:9px 0 -4px;gap:8px}.panel-footer button{flex:1;background:none;color:#a9cde2;font-size:11px}.mini-actions{display:flex;gap:8px;margin-top:11px}
.toggle-grid{display:grid;grid-template-columns:1fr 1fr;gap:9px 4px;margin:14px 0}.toggle-grid label{display:flex;gap:6px;align-items:center;white-space:nowrap;font-size:11px}.toggle-grid input{appearance:none;width:26px;height:15px;background:#264d63;border-radius:20px;margin:0;cursor:pointer;position:relative}.toggle-grid input:before{content:'';position:absolute;top:3px;left:3px;width:9px;height:9px;border-radius:50%;background:#c2d5df;transition:transform .15s}.toggle-grid input:checked{background:#55b9df}.toggle-grid input:checked:before{transform:translateX(11px);background:white}.toggle-grid input:focus-visible{outline:2px solid white}
select{width:100%;background:#153c55;color:#e7f5ff;border:1px solid #315b73;border-radius:8px;padding:7px;font:inherit}.bridge-toggle{display:flex;gap:5px;align-items:center;font-size:11px}.bridge-toggle input{accent-color:#55b9df}.install{width:100%;margin:9px 0 6px;padding:8px;font-size:11px}small{display:block;color:#a9cde2;font-size:10px;line-height:1.6}.utilities{display:flex;gap:6px;margin-top:12px}.utilities button{background:#def0fc;color:#0b3049;flex:1;font-size:10px;padding:9px 3px}.utilities .danger{color:white;background:#86465c}.privacy{font-size:10px;line-height:1.8;color:#c5dce8;margin-bottom:0}
</style>
