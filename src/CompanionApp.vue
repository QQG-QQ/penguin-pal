<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { cursorPosition, currentMonitor, primaryMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import CompanionPanel from './components/CompanionPanel.vue'
import CompanionSprite from './components/CompanionSprite.vue'
import CompanionDJ from './components/CompanionDJ.vue'
import { MusicClock, MusicScene, musicAllowed, silentMusic, type MusicSnapshot } from './lib/companionMusic'
import djPoster from '../penguin/music-dj/dj.png'
import djMetadata from '../penguin/music-dj/media.json'
import { DragTrail, motionDuration, motionPackForSlot, springStep, type MediaPack } from './lib/companionMedia'
import { MotionSequencer } from './lib/companionSequence'
import CompanionDiagnostics from './components/CompanionDiagnostics.vue'
import CompanionChat from './components/CompanionChat.vue'
import type {ChatView,SyncView} from './lib/companionSync'
import { computePetLayoutMetrics, isPetOpaqueHit, loadPetAlphaMask, type PetAlphaMask } from './lib/petArtwork'
import { builtinMotionPack, companionArtwork } from './lib/companionArtwork'
import { AnimationPlayer, selectClip } from './lib/companionAnimation'
import { care, chooseActivity, codexPresentation, defaultCompanion, followSurface, hitRegion, localLine, recoverNeeds, canonicalAnimationPack, isMergedBuiltinPack,
  stepPhysics, advanceNeeds, dockPosition, nearEdge, frameDelay, shouldIdleRest, relationship, type DockEdge, type Activity, type Body, type Bounds, type CareAction, type CompanionState, type Preferences, type Surface } from './lib/companionCore'

const native = '__TAURI_INTERNALS__' in window
const previewGreeting=ref(new URLSearchParams(window.location.search).get('preview')==='greet')
const previewSuite=ref(false),previewAction=ref<string>(),previewPlan:string[]=[]
let previewTimer:number|undefined
let previewTicks=0
const state = ref(defaultCompanion(Date.now())), activity = ref<Activity>('idle')
const syncView=ref<SyncView|null>(null),chatOpen=ref(false),chatSession=ref('')
let chatTimer:number|undefined
const chatGate=()=>({busy:busy.value||pressed,quiet:state.value.preferences.quiet||menu.value||safeMode.value,fullscreen:fullscreen.value,visible:visible.value&&!dockHidden.value})
function receiveChat(value:ChatView){receiveSync(value.status);chatSession.value=value.session;if(value.reply)talk(value.reply.length>180?value.reply.slice(0,177)+'…':value.reply)}
async function chatTick(){if(!native||disposed||previewGreeting.value)return;try{receiveChat(await invoke<ChatView>('companion_chat',{action:'tick',payload:chatGate()}))}catch{/* Manual chat shows connection errors. */}}
async function openChat(){chatOpen.value=true;tools.value=false;await toggleMenu(true);if(native){try{receiveChat(await invoke<ChatView>('companion_chat',{action:'status',payload:null}))}catch(e){error.value=String(e)}}}
let syncTimer:number|undefined,syncBusy=false
function receiveSync(value:SyncView,apply=false){
  syncView.value=value
  if(value.mode==='server'||apply){
    const pet=value.document.pet
    state.value.energy=pet.energy;state.value.hunger=pet.hunger;state.value.affection=pet.affection
    state.value.preferences.personality=pet.personality
    if(apply)void save()
  }
}
async function syncTick(){
  if(!native||syncBusy||disposed)return
  syncBusy=true
  try{receiveSync(await invoke<SyncView>('companion_sync',{action:syncView.value?.mode==='server'?'sync':'status',payload:{manual:false}}))}
  catch{try{receiveSync(await invoke<SyncView>('companion_sync',{action:'status',payload:null}))}catch{/* Existing pet stays usable; toolbox reports storage errors. */}}
  finally{syncBusy=false}
}
function syncRecord(action:string,payload:unknown){if(native)void invoke<SyncView>('companion_sync',{action,payload}).then(value=>receiveSync(value)).catch(e=>{error.value='陪伴数据保存失败：'+String(e)})}
const menu = ref(false), expanded = ref(false), food = ref(false), message = ref(''), error = ref('')
const tools=ref(false),safeMode=ref(false),dock=ref<DockEdge>('none'),dockHidden=ref(false)
const codexStatus = ref('idle'), lastEvent = ref(0), connected = ref(false), visible = ref(true)
const context = ref('quiet'), fullscreen = ref(false), hovered = ref(false), dragging = ref(false), airborne = ref(false), facing = ref(1)
const img = ref<HTMLElement | null>(null), mask = ref<PetAlphaMask | null>(null), pose = ref('idle')
const stage = ref({x:0,y:0}), panel = ref({x:296,y:8})
const reduce = ref(window.matchMedia('(prefers-reduced-motion: reduce)').matches)
const still = computed(() => (reduce.value&&!previewGreeting.value) || state.value.preferences.reducedMotion || safeMode.value)
const busy = computed(() => ['working','waiting','toolError'].includes(codexStatus.value))
const presentation = computed(() => codexPresentation(codexStatus.value))
const packs=ref<MediaPack[]>([]), importing=ref(false), media=ref<ReturnType<MotionSequencer['sample']>>()
const motionPicker=new MotionSequencer(),trail=new DragTrail(),failedVideos=new Set<string>()
const music=ref<MusicSnapshot>(silentMusic()),musicClock=ref(new MusicClock()),musicScene=ref(new MusicScene()),djFailed=ref(false)
let musicReceived=0,inMusic=false,musicTimer:number|undefined
const musicEnabled=computed(()=>state.value.preferences.musicMode!=='off'&&!still.value&&!state.value.preferences.quiet&&visible.value&&!dockHidden.value&&!hiddenForFullscreen.value)
const musicEligible=()=>musicAllowed({enabled:musicEnabled.value,reduced:still.value,quiet:state.value.preferences.quiet,visible:visible.value,hidden:hiddenForFullscreen.value||dockHidden.value,codex:codexStatus.value,pressed:dragging.value,falling:airborne.value,panel:menu.value||food.value,manual:Date.now()<manualUntil,activity:activity.value})
const musicLabel=computed(()=>state.value.preferences.musicMode==='off'?'音乐打碟已关闭':still.value?'减少动态已开启，音乐打碟暂停':state.value.preferences.quiet?'安静陪伴中，音乐打碟暂停':music.value.issue||(!musicEligible()?'互动或 Codex 状态优先，打碟暂停':music.value.active?`正在跟随${music.value.confidence>=.3?` · ${Math.round(music.value.bpm)} BPM`:''}`:music.value.source||'等待音乐播放'))
const resolveMedia=(file:string)=>media.value?.directory?convertFileSrc(`${media.value.directory}/${file}`):file
const selectedPack=computed(()=>state.value.preferences.animationPack==='builtin'?builtinMotionPack:packs.value.find(p=>p.id===state.value.preferences.animationPack))
const artwork = computed(() => musicScene.value.showing ? {src:djPoster,alt:'企鹅打碟',naturalWidth:256,naturalHeight:256,alphaBBox:{left:djMetadata.alphaBBox[0],top:djMetadata.alphaBBox[1],right:djMetadata.alphaBBox[2],bottom:djMetadata.alphaBBox[3]}} : media.value ? {...companionArtwork('idle'),src:resolveMedia(media.value.frame),alt:`企鹅 ${media.value.clip.id}`} : companionArtwork(pose.value))
const video=computed(()=>{const file=media.value?.clip.video;if(!file||!state.value.preferences.videoAnimations||still.value)return undefined;const url=resolveMedia(file);return failedVideos.has(url)?undefined:url})
let videoNotice=''
function clearVideoNotice(){if(error.value===videoNotice)error.value='';videoNotice=''}
function videoPlaying(_url:string,revision:number){clearVideoNotice();motionPicker.playing(revision,performance.now())}
function videoEnded(revision:number){motionPicker.ended(revision,performance.now())}
function retryMotion(){failedVideos.clear();clearVideoNotice()}
function videoFailed(url:string,reason='load'){
  failedVideos.add(url)
  videoNotice=reason==='blocked'?'视频暂未获准播放，已使用帧图。可再次点击动作重试。':['decode','format','metadata'].includes(reason)?'这个动作的视频无法解码，已使用帧图。可再次点击动作重试或更换素材。':'动作视频未能开始播放，已使用帧图。可再次点击动作重试。'
  error.value=videoNotice
  if(native)void invoke('companion_video_issue',{reason}).catch(()=>{})
}
function imageFailed(){if(media.value){media.value=undefined;motionPicker.reset(true);void preferencesChanged({animationPack:'builtin'});error.value='动画图片无法显示，已回到内置企鹅。'}}
function sendPreview(probe:{width:number;height:number;corner:number;center:number},playing=false,url?:string){
  if(!native||!previewGreeting.value)return
  const v=Array.from(document.querySelectorAll('video')).find(v=>url?v.src===new URL(url,location.href).href:v.style.visibility==='visible')
  if(playing&&!v)return
  void invoke('companion_preview_report',{probe:{...probe,panel:menu.value,action:previewAction.value,playing,reduced:still.value,selected:!!video.value,ready:v?.readyState??0,network:v?.networkState??0,media_error:v?.error?.code??0,paused:v?.paused??true,motion:previewAction.value==='motion'?{ticks:previewTicks,fullscreen:fullscreen.value,document_visible:document.visibilityState==='visible',x:body.x,y:body.y,video_time:v?.currentTime??0}:undefined}}).catch(()=>{})
}
function verifyPreview(probe:{width:number;height:number;corner:number;center:number;playing?:boolean}){sendPreview(probe,probe.playing??false)}
function runPreviewPlan(){
  if(!previewPlan.length)return
  if(previewPlan.includes('motion')){
    previewSuite.value=true;previewAction.value='motion';nextActivity=Date.now()+600000;nextMicro=Date.now()+600000
    const report=()=>{if(disposed)return;sendPreview({width:256,height:256,corner:0,center:0},false);previewTimer=window.setTimeout(report,1000)}
    report();return
  }
  if(previewPlan.includes('music')){previewSuite.value=true;previewAction.value='music';nextActivity=Date.now()+600000;nextMicro=Date.now()+600000;return}
  previewSuite.value=true;let step=0
  const next=()=>{const key=previewPlan[step++];if(!key||disposed)return;previewAction.value=key;facing.value=key==='walk-left'?-1:1;updateActivity(key.startsWith('walk-')?'walk':key as Activity,10000,true);nextActivity=Date.now()+600000;nextMicro=Date.now()+600000;previewTimer=window.setTimeout(next,2200)}
  next()
}
async function refreshPacks(){if(native)try{packs.value=(await invoke<MediaPack[]>('companion_media_list')).filter(p=>!isMergedBuiltinPack(p.id));media.value=undefined;motionPicker.reset(true);failedVideos.clear()}catch(e){error.value=`动画库读取失败：${String(e)}`}}
async function importPack(){if(!native||importing.value)return;importing.value=true;try{const pack=await invoke<MediaPack|null>('companion_media_import');if(pack){await refreshPacks();await preferencesChanged({animationPack:pack.id});talk(`已启用 ${pack.name}。`)}}catch(e){error.value=`动画导入失败：${String(e)}`}finally{importing.value=false}}
async function openPackFolder(){if(native)try{await invoke('companion_media_folder')}catch(e){error.value=String(e)}}
const hiddenForFullscreen = computed(() => fullscreen.value && state.value.preferences.fullscreenHide && !menu.value)
const statusLabel = computed(() => busy.value ? presentation.value.label.replace('Codex ','') : musicScene.value.showing?'音乐打碟':({rest:'休息',walk:'散步',eat:'吃鱼',sweep:'扫地',happy:'开心',pat:'摸摸'} as Record<string,string>)[activity.value] ?? '待机')
const connection = computed(() => !state.value.preferences.codexEvents ? 'Codex 状态已关闭' : lastEvent.value ? '已收到 Codex 状态事件' : connected.value ? '等待事件，首次安装需在 Codex 信任 hooks' : native ? '本地连接中' : '浏览器预览')
const unit = computed(() => state.value.preferences.size)
const stageStyle = computed(() => ({left:`${stage.value.x}px`,top:`${stage.value.y}px`,transform:`scale(${unit.value})`}))
let bounds: Bounds = {left:0,top:0,right:1920,bottom:1080}, workArea = {...bounds}
let surfaces: Surface[] = [], attached: Surface | undefined
let body: Body = {x:200,y:200,vx:0,vy:0,width:160,height:220}, offset = {x:70,y:60}, scale = 1
let windowOrigin = {x:0,y:0}, pointerOffset = {x:0,y:0}, pressed = false, moved = false, heldAt = 0, patAt = 0, heldPat = false
let releasing=false,dragMove:Promise<void>|undefined,dragOrigin={x:0,y:0}
let previousCursor = {x:0,y:0,at:0}, lastFrame = performance.now(), actionUntil = 0, manualUntil=0, nextActivity = Date.now()+12000, nextMicro = Date.now()+5000
let recent: {action:CareAction;at:number}[] = [], inFrame = false, inPoll = false, inHit = false
let ignored:boolean|undefined=false
let initialized = false, saving = false, saveAgain = false, disposed = false, layingOut = false, lastContext = 'quiet', nextContext = 0
const player=new AnimationPlayer()
let maskSource = '', tapAt = 0,dockHome:{x:number;y:number}|undefined,dockHideAt=0,dockMoving=false,nextLine=Date.now()+180000
let motionQuery: MediaQueryList, unlisten: UnlistenFn | undefined
let tickTimer:number|undefined,pollTimer:number|undefined,saveTimer:number|undefined,hitTimer:number|undefined,clearMessageTimer:number|undefined,codexTimer:number|undefined
const talk = (text:string) => {
  if (state.value.preferences.quiet || hiddenForFullscreen.value) return
  message.value = text; window.clearTimeout(clearMessageTimer)
  clearMessageTimer = window.setTimeout(()=>{message.value=''},4800)
}
const updateActivity = (next:Activity,duration=3000,manual=false) => {activity.value=next;actionUntil=Date.now()+motionDuration(motionPackForSlot(selectedPack.value,builtinMotionPack,next),next,duration);if(manual)manualUntil=actionUntil;motionPicker.reset();player.reset()}
function requestAction(next:Activity){retryMotion();updateActivity(next,next==='sweep'?6500:3500,true)}
async function save() {
  if (!initialized || !native) return
  if (saving) {saveAgain=true;return}
  saving=true
  try {state.value.savedAt=Date.now();await invoke('companion_save',{state:JSON.parse(JSON.stringify(state.value))})}
  catch(e) {error.value=`设置保存失败：${String(e)}`}
  finally {saving=false;if(saveAgain&&!disposed){saveAgain=false;void save()}}
}
function interact(action:CareAction) {
  retryMotion()
  const now=Date.now();recent=recent.filter(item=>now-item.at<15000)
  const result=care(state.value,action,recent.filter(item=>item.action===action).length)
  state.value=result.state;recent.push({action,at:now});updateActivity(result.activity,action==='rest'?45000:3000,true)
  talk(result.message);nextActivity=now+20000;void save()
  if(syncView.value?.mode==='server')syncRecord('care',{action})
}
async function openCodex() {
  if (!native){talk('桌面版会打开你现有的 Codex 聊天界面。');return}
  try {await invoke('companion_open_codex');await toggleMenu(false);food.value=false;error.value=''}catch(e){error.value=String(e)}
}
async function hidePet(){await toggleMenu(false);await save();if(native)await getCurrentWindow().hide()}
async function quit(){await save();if(native)await invoke('companion_quit')}
async function installHooks(){if(native)try{error.value=await invoke<string>('companion_install_hooks')}catch(e){error.value=String(e)}}
async function refreshGeometry() {
  if(!native||!img.value)return
  const win=getCurrentWindow(),monitor=await currentMonitor()??await primaryMonitor();scale=await win.scaleFactor()
  if(monitor){const a=monitor.workArea;workArea={left:a.position.x,top:a.position.y,right:a.position.x+a.size.width,bottom:a.position.y+a.size.height}}
  const metrics=computePetLayoutMetrics(img.value.getBoundingClientRect(),companionArtwork('idle'))
  offset={x:metrics.petLeft*scale,y:metrics.petTop*scale}
  body.width=(metrics.petRight-metrics.petLeft)*scale;body.height=(metrics.petBottom-metrics.petTop)*scale
  bounds={left:workArea.left+offset.x,top:workArea.top+offset.y,right:workArea.right-(280*unit.value-metrics.petRight)*scale,bottom:workArea.bottom-(290*unit.value-metrics.petBottom)*scale}
  const pos=await win.outerPosition();windowOrigin={x:pos.x,y:pos.y};body.x=pos.x+offset.x;body.y=pos.y+offset.y
}
async function moveBody() {
  if(!native)return
  const x=Math.round(body.x-offset.x),y=Math.round(body.y-offset.y)
  if(windowOrigin.x!==x||windowOrigin.y!==y){await getCurrentWindow().setPosition(new PhysicalPosition(x,y));windowOrigin={x,y}}
  state.value.x=dockHome?.x??x;state.value.y=dockHome?.y??y
}
async function showDock(hidden:boolean){
  if(!native||dock.value==='none'||!dockHome||dockMoving||menu.value||layingOut)return
  dockMoving=true
  try{
    const pos=dockPosition(dock.value,dockHome,workArea,280*unit.value*scale,290*unit.value*scale,{x:offset.x,y:offset.y,width:body.width,height:body.height},hidden)
    await getCurrentWindow().setPosition(new PhysicalPosition(pos.x,pos.y));windowOrigin=pos;body.x=pos.x+offset.x;body.y=pos.y+offset.y;dockHidden.value=hidden
    state.value.x=dockHome.x;state.value.y=dockHome.y
  }finally{dockMoving=false}
}
async function setDock(edge:DockEdge){
  dock.value=edge;state.value.dock=edge;attached=undefined;airborne.value=false;body.vx=0;body.vy=0
  dockHome=dockPosition(edge,windowOrigin,workArea,280*unit.value*scale,290*unit.value*scale,{x:offset.x,y:offset.y,width:body.width,height:body.height},false)
  await showDock(false);dockHideAt=Date.now()+1600;await save()
}
async function setSafety(value:boolean){
  if(value&&!safeMode.value&&dockMoving)return
  if(value&&!safeMode.value){await showDock(false);dock.value='none';dockHidden.value=false;dockHome=undefined;cancelDrag();attached=undefined;updateActivity('idle')}
  safeMode.value=value
}
async function layoutPanel() {
  if(!native){panel.value={x:285,y:8};return}
  if(layingOut)return;layingOut=true
  try {
    const win=getCurrentWindow(),origin=await win.outerPosition(),base={x:origin.x+stage.value.x*scale,y:origin.y+stage.value.y*scale}
    const w=280*unit.value*scale,h=290*unit.value*scale
    if(!menu.value){
      await win.setPosition(new PhysicalPosition(Math.round(base.x),Math.round(base.y)))
      await win.setSize(new PhysicalSize(Math.round(w),Math.round(h)));stage.value={x:0,y:0};windowOrigin=base
    }else{
      const pw=(tools.value||chatOpen.value?440:280)*scale,ph=Math.min((tools.value||chatOpen.value?740:expanded.value?800:440)*scale,workArea.bottom-workArea.top)
      const px=base.x+w+pw<=workArea.right?base.x+w-12*scale:Math.max(workArea.left,base.x-pw+12*scale)
      const py=Math.max(workArea.top,Math.min(workArea.bottom-ph,base.y))
      const left=Math.min(base.x,px),top=Math.min(base.y,py),right=Math.max(base.x+w,px+pw),bottom=Math.max(base.y+h,py+ph)
      await win.setSize(new PhysicalSize(Math.ceil(right-left),Math.ceil(bottom-top)))
      await win.setPosition(new PhysicalPosition(Math.round(left),Math.round(top)))
      stage.value={x:(base.x-left)/scale,y:(base.y-top)/scale};panel.value={x:(px-left)/scale+8,y:(py-top)/scale+8};windowOrigin={x:left,y:top}
      await win.setIgnoreCursorEvents(false);ignored=false
    }
  }catch(e){error.value=`面板布局失败：${String(e)}`}
  finally{layingOut=false}
}
async function toggleMenu(open=!menu.value){if(open)await showDock(false);menu.value=open;food.value=false;if(!open){tools.value=false;chatOpen.value=false;}await layoutPanel();if(!open){await refreshGeometry();dockHideAt=Date.now()+1600}}
async function openTools(){await save();await showDock(false);tools.value=true;chatOpen.value=false;menu.value=true;await layoutPanel()}
async function closeTools(){tools.value=false;await layoutPanel()}
async function expandPanel(value:boolean){expanded.value=value;await layoutPanel()}
async function preferencesChanged(patch:Partial<Preferences>) {
  if(patch.personality!==undefined)syncRecord('personality',{value:patch.personality})
  if(patch.animationPack!==undefined)patch={...patch,animationPack:canonicalAnimationPack(patch.animationPack)}
  const wasOpen=menu.value,resize=patch.size!==undefined
  if(resize||patch.edgeDock===false){await showDock(false);dock.value='none';dockHome=undefined;dockHidden.value=false;state.value.dock='none'}
  if(resize&&wasOpen)await toggleMenu(false)
  Object.assign(state.value.preferences,patch)
  if(patch.animationPack!==undefined){media.value=undefined;failedVideos.clear();motionPicker.reset(true);player.reset()}
  if(still.value||!state.value.preferences.gravity){airborne.value=false;body.vx=0;body.vy=0}
  if(!state.value.preferences.surfaces)attached=undefined
  if(!state.value.preferences.wander)updateActivity('idle')
  if(state.value.preferences.quiet)message.value=''
  if(patch.autoHide===false)await showDock(false)
  if(native){
    await getCurrentWindow().setAlwaysOnTop(state.value.preferences.topmost)
    if(resize){await getCurrentWindow().setSize(new PhysicalSize(Math.round(280*unit.value*scale),Math.round(290*unit.value*scale)));await refreshGeometry();await snap()}
  }
  await save();if(resize&&wasOpen)await toggleMenu(true)
}
async function snap(){
  if(menu.value)await toggleMenu(false)
  await showDock(false);dock.value='none';dockHidden.value=false;dockHome=undefined;state.value.dock='none';attached=undefined;airborne.value=false
  await refreshGeometry();body.x=Math.max(bounds.left,Math.min(bounds.right-body.width,body.x));body.y=Math.max(bounds.top,Math.min(bounds.bottom-body.height,body.y));await moveBody();await save()
}
async function pointerDown(e:PointerEvent) {
  if(e.button!==0||pressed||releasing||menu.value||!img.value||state.value.preferences.passthrough)return
  if(!isPetOpaqueHit(artwork.value,img.value.getBoundingClientRect(),mask.value,e.clientX,e.clientY))return
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  pressed=true;moved=false;heldPat=false;heldAt=Date.now();patAt=heldAt+600
  try{
    await showDock(false);dock.value='none';state.value.dock='none';dockHome=undefined;dockHidden.value=false;updateActivity('idle')
    if(!pressed)return
    attached=undefined;airborne.value=false;body.vx=0;body.vy=0
    await refreshGeometry();const cursor=native?await cursorPosition():{x:e.screenX,y:e.screenY};if(!pressed)return;pointerOffset={x:cursor.x-body.x,y:cursor.y-body.y};dragOrigin={x:cursor.x,y:cursor.y};previousCursor={...cursor,at:performance.now()};trail.reset(previousCursor);dragging.value=true
  }
  catch{pressed=false;dragging.value=false;error.value='暂时无法拖动窗口。'}
}
async function release(e?:PointerEvent){
  if(!pressed)return;pressed=false;dragging.value=false;releasing=true
  const velocity=trail.velocity(performance.now(),scale)
  try{
  if(!moved&&e&&img.value){
    if(Date.now()-heldAt<400){
      if(Date.now()-tapAt<330){window.clearTimeout(codexTimer);food.value=true;tapAt=0;return}
      tapAt=Date.now();codexTimer=window.setTimeout(()=>{void openCodex()},330)
    }
    if(!heldPat){const m=computePetLayoutMetrics(img.value.getBoundingClientRect(),artwork.value);interact(e.clientX>m.petLeft+(m.petRight-m.petLeft)*.88&&e.clientY>m.petTop+(m.petBottom-m.petTop)*.5?'tail':Date.now()-heldAt>400?'pat':hitRegion(e.clientY,m.petTop,m.petBottom))}
  }else if(moved){
    // Finish any native position write before reading the release origin.
    await dragMove
    if(disposed)return
    Object.assign(body,velocity)
    try{await refreshGeometry()}catch{/* Last valid bounds remain available. */}
    const edge=state.value.preferences.edgeDock&&!safeMode.value?nearEdge(windowOrigin,workArea,280*unit.value*scale,290*unit.value*scale,28*unit.value*scale):'none'
    if(edge!=='none'){await setDock(edge);return}
    airborne.value=state.value.preferences.gravity&&!still.value
    if(!airborne.value){body.vx=0;body.vy=0;updateActivity('idle')}void save()
  }
  }catch{cancelDrag();error.value='暂时无法完成拖放，请重新拖动。'}finally{releasing=false}
}
function cancelDrag(){pressed=false;dragging.value=false;airborne.value=false;body.vx=0;body.vy=0}
async function updateHitTest(){
  if(!native||!initialized||inHit||disposed||!visible.value||!img.value)return;inHit=true
  try{
    // Fullscreen display changes can reposition the native window without moveBody().
    const win=getCurrentWindow(),[c,origin,hitScale]=await Promise.all([cursorPosition(),win.outerPosition(),win.scaleFactor()])
    const x=(c.x-origin.x)/hitScale,y=(c.y-origin.y)/hitScale
    const opaque=isPetOpaqueHit(artwork.value,img.value.getBoundingClientRect(),mask.value,x,y)
    const control=Array.from(document.querySelectorAll<HTMLElement>('.panel-host,.speech,.pet-tools,.food-picker,.notice,.codex-marker')).some(el=>{const r=el.getBoundingClientRect();return x>=r.left&&x<=r.right&&y>=r.top&&y<=r.bottom})
    hovered.value=opaque||control
    if(dock.value!=='none'&&!menu.value&&!pressed&&!hiddenForFullscreen.value){
      if(hovered.value){dockHideAt=Date.now()+1400;if(dockHidden.value){await showDock(false);return}}
      else if(!dockHidden.value&&Date.now()>dockHideAt&&state.value.preferences.autoHide&&!safeMode.value){await showDock(true);return}
    }
    const next=hiddenForFullscreen.value||(!pressed&&!menu.value&&!control&&(!opaque||state.value.preferences.passthrough))
    if(next!==ignored){await getCurrentWindow().setIgnoreCursorEvents(next);ignored=next}
  }catch{/* Keep the last usable cursor mode. */}finally{inHit=false}
}
async function poll(){
  if(!native||inPoll)return;inPoll=true
  try{
    const [data,shown]=await Promise.all([invoke<{safeMode:boolean;bridge:{status:string;lastReceived:number};desktop:{surfaces:Surface[];context:string;fullscreen:boolean;leftButtonDown:boolean;idleSeconds:number}}>('companion_poll'),getCurrentWindow().isVisible()])
    if(disposed)return;const previous=codexStatus.value,wasFullscreen=fullscreen.value,wasVisible=visible.value;connected.value=true;codexStatus.value=data.bridge.status;lastEvent.value=data.bridge.lastReceived;visible.value=shown
    await setSafety(data.safeMode)
    if(previous!==codexStatus.value&&['waiting','ended','interrupted','toolError'].includes(codexStatus.value)&&state.value.preferences.notifications&&!state.value.preferences.quiet){await showDock(false);talk(presentation.value.label);dockHideAt=Date.now()+8000}
    surfaces=data.desktop.surfaces;context.value=data.desktop.context;fullscreen.value=data.desktop.fullscreen
    if(wasFullscreen!==fullscreen.value||wasVisible!==shown){
      ignored=undefined
      if(shown&&!pressed)await refreshGeometry()
      await updateHitTest()
    }
    if(hiddenForFullscreen.value)message.value=''
    if(native&&!previewGreeting.value)void invoke('companion_chat',{action:'gate',payload:chatGate()}).catch(()=>{})
    if(!syncView.value?.document.social?.proactive&&shown&&context.value!==lastContext&&Date.now()>nextContext&&!busy.value&&!pressed&&!menu.value&&(syncView.value?.mode!=='server'||(syncView.value.active&&Date.now()-syncView.value.lastSync<45000))){if(context.value!=='quiet')talk(localLine(state.value.preferences.personality,context.value,Math.random()));nextContext=Date.now()+600000}lastContext=context.value
    if(pressed&&Date.now()-heldAt>1000&&!data.desktop.leftButtonDown)void release()
    if(shouldIdleRest(data.desktop.idleSeconds,Date.now(),actionUntil,busy.value||pressed||menu.value,activity.value))updateActivity('rest',30000)
  }catch{connected.value=false;codexStatus.value='idle';surfaces=[];fullscreen.value=false}finally{inPoll=false}
}
async function musicTick(){
  if(disposed)return
  if(native&&!inMusic){
    inMusic=true
    try{const sample=await invoke<MusicSnapshot>('companion_music_poll',{enabled:musicEnabled.value});if(!disposed){music.value=sample;musicReceived=performance.now()}}
    catch{music.value=silentMusic()}
    finally{inMusic=false}
  }
  if(!disposed)musicTimer=window.setTimeout(()=>{void musicTick()},musicEnabled.value?50:1000)
}
async function frame(){
  if(disposed||inFrame||!initialized)return
  const now=performance.now(),elapsed=(now-lastFrame)/1000,dt=Math.min(.05,elapsed);lastFrame=now;inFrame=true
  if(previewAction.value==='motion')previewTicks++
  try{
    if(syncView.value?.mode!=='server')advanceNeeds(state.value,elapsed,activity.value==='rest')
    const eligible=musicEligible(),wantsMusic=eligible&&music.value.active&&now-musicReceived<500
    const wasMusic=musicScene.value.showing
    musicClock.value.step(music.value,elapsed,!musicScene.value.showing)
    musicScene.value.sample(wantsMusic,now,!eligible)
    if(wasMusic!==musicScene.value.showing){motionPicker.reset(true);player.reset();if(musicScene.value.showing)musicClock.value.elapsed=0}
    const clip=selectClip(activity.value,state.value.preferences.statusAnimations?codexStatus.value:'idle',dragging.value&&moved,airborne.value,facing.value,Date.now()<manualUntil)
    pose.value=player.sample(clip,now,still.value)
    media.value=motionPicker.sample(motionPackForSlot(selectedPack.value,builtinMotionPack,clip.slot??'idle'),clip.slot??'idle',now,Math.random(),{video:state.value.preferences.videoAnimations&&!still.value&&!(media.value?.clip.video&&failedVideos.has(resolveMedia(media.value.clip.video))),paused:!visible.value||dockHidden.value||hiddenForFullscreen.value,immediate:['drag','fall','land','dizzy'].includes(clip.slot??''),reduced:still.value})
    // A requested action gets its full duration after the preceding exit finishes.
    if(media.value?.queued){actionUntil+=elapsed*1000;manualUntil+=elapsed*1000}
    if(still.value&&media.value)media.value={...media.value,frame:media.value.clip.poster}
    if(artwork.value.src!==maskSource){maskSource=artwork.value.src;mask.value=null;const source=maskSource;void loadPetAlphaMask(artwork.value).then(m=>{if(source===maskSource)mask.value=m}).catch(()=>{})}
    if(!visible.value||hiddenForFullscreen.value||layingOut)return
    if(dragging.value&&native){
      const c=await cursorPosition(),elapsed=Math.max(.016,(now-previousCursor.at)/1000)
      if(!pressed||!dragging.value)return
      if(Math.hypot(c.x-dragOrigin.x,c.y-dragOrigin.y)>3*scale)moved=true
      trail.push({...c,at:now})
      if(moved){
        const target={x:c.x-pointerOffset.x,y:c.y-pointerOffset.y}
        if(state.value.preferences.springDrag&&!still.value){const x=springStep(body.x,body.vx,target.x,elapsed),y=springStep(body.y,body.vy,target.y,elapsed);body.x=x.position;body.vx=x.velocity;body.y=y.position;body.vy=y.velocity}
        else{body.x=target.x;body.y=target.y}
        dragMove=moveBody();try{await dragMove}finally{dragMove=undefined}
      }
      else if(Date.now()>patAt){interact('pat');heldPat=true;patAt=Date.now()+650}
      previousCursor={...c,at:now}
    }else if(native&&!releasing&&!menu.value&&!food.value&&dock.value==='none'&&!dockMoving){
      if(attached){const next=surfaces.find(s=>s.id===attached?.id),following=followSurface(body,attached,next,bounds);if(following&&state.value.preferences.surfaces){body=following;attached=next;await moveBody()}else{attached=undefined;airborne.value=state.value.preferences.gravity&&!still.value}}
      if(airborne.value&&!still.value&&state.value.preferences.gravity){
        const result=stepPhysics(body,bounds,state.value.preferences.surfaces?surfaces:[],dt,scale);body=result.body;await moveBody()
        if(result.hard)updateActivity('dizzy',2400)
        if(result.landed){airborne.value=false;attached=surfaces.find(s=>s.id===result.surface);if(activity.value!=='dizzy')updateActivity('land',500);void save()}
      }else if(!musicScene.value.showing&&!wantsMusic&&!busy.value&&!still.value&&!(hovered.value&&state.value.preferences.hoverPause)){
        let direction=activity.value==='walk'?facing.value:0
        if(state.value.preferences.movement==='follow'){
          const c=await cursorPosition(),distance=c.x-(body.x+body.width/2)
          direction=Math.abs(distance)>140*scale?Math.sign(distance):0
          if(direction){
            facing.value=direction
            if(activity.value==='idle')updateActivity('walk',500)
            if(activity.value==='walk')actionUntil=Math.max(actionUntil,Date.now()+500)
          }else if(activity.value==='walk')updateActivity('idle')
        }
        if(direction&&activity.value==='walk'){
          const left=Math.max(bounds.left,attached?attached.left:bounds.left),right=Math.min(bounds.right-body.width,attached?attached.right-body.width:bounds.right-body.width)
          if(left<=right){body.x+=direction*state.value.preferences.walkSpeed*scale*unit.value*dt;if(body.x<=left||body.x>=right){body.x=Math.max(left,Math.min(right,body.x));facing.value*=-1}await moveBody()}
        }
      }
    }
    if(Date.now()>actionUntil&&activity.value!=='idle')updateActivity(activity.value==='rest'?'wake':'idle',activity.value==='rest'?1200:3000)
    if(Date.now()>nextMicro&&!wantsMusic&&!musicScene.value.showing&&activity.value==='idle'&&!pressed&&!menu.value&&!busy.value&&!still.value){updateActivity(Math.random()<.65?'blink':'look',500);nextMicro=Date.now()+4500+Math.random()*8000}
    if(Date.now()>nextActivity&&!wantsMusic&&!musicScene.value.showing&&!pressed&&!menu.value&&!food.value&&!airborne.value&&!(hovered.value&&state.value.preferences.hoverPause)){
      const next=chooseActivity(state.value,Math.random(),busy.value||hiddenForFullscreen.value||still.value||context.value==='video'||dock.value!=='none')
      updateActivity(next,next==='rest'?45000:next==='walk'?5000:next==='sweep'?6500:3000)
      const rhythm=state.value.preferences.rhythm==='calm'?2:state.value.preferences.rhythm==='lively'?.65:1
      nextActivity=Date.now()+(25000+Math.random()*30000)*rhythm
    }
    if(!syncView.value?.document.social?.proactive&&Date.now()>nextLine&&!busy.value&&!pressed&&!menu.value&&!dockHidden.value&&!fullscreen.value&&(syncView.value?.mode!=='server'||(syncView.value.active&&Date.now()-syncView.value.lastSync<45000))){talk(state.value.hunger>75?'小鱼可以留一点给我吗？':localLine(state.value.preferences.personality,context.value,Math.random()));nextLine=Date.now()+300000+Math.random()*600000}
  }catch{cancelDrag();attached=undefined;updateActivity('idle');nextActivity=Date.now()+60000;error.value='移动已暂停，仍可打开 Codex。'}finally{inFrame=false}
}
async function tick(){await frame();if(!disposed)tickTimer=window.setTimeout(()=>{void tick()},frameDelay(!visible.value||hiddenForFullscreen.value||dockHidden.value,dragging.value||airborne.value||activity.value==='walk'||!!attached||musicScene.value.showing,activity.value!=='idle'||busy.value))}
async function pollTick(){await poll();if(!disposed)pollTimer=window.setTimeout(()=>{void pollTick()},visible.value?1000:2500)}
function motionChanged(e:MediaQueryListEvent){reduce.value=e.matches;void preferencesChanged({})}
onMounted(async()=>{
  motionQuery=window.matchMedia('(prefers-reduced-motion: reduce)');motionQuery.addEventListener('change',motionChanged)
  try{
    if(native){
      previewGreeting.value=await invoke<boolean>('companion_preview_mode');previewPlan.push(...await invoke<string[]>('companion_preview_plan'))
      const loaded=await invoke<CompanionState>('companion_load');state.value=recoverNeeds(loaded,Date.now());
      receiveSync(await invoke<SyncView>('companion_sync',{action:'status',payload:null}))
      void syncTick();syncTimer=window.setInterval(()=>{void syncTick()},15000)
      chatTimer=window.setInterval(()=>{void chatTick()},15000)
      if(loaded.preferences.animationPack!==state.value.preferences.animationPack)await invoke('companion_save',{state:JSON.parse(JSON.stringify(state.value))});
      await refreshPacks();const win=getCurrentWindow()
      if(previewGreeting.value)await win.show()
      scale=await win.scaleFactor();await win.setSize(new PhysicalSize(Math.round(280*unit.value*scale),Math.round(290*unit.value*scale)))
      await win.setAlwaysOnTop(state.value.preferences.topmost)
      if(state.value.x!==null&&state.value.y!==null)await win.setPosition(new PhysicalPosition(state.value.x,state.value.y))
      const savedDock=state.value.dock;await snap();state.value.dock=savedDock;unlisten=await listen('companion-panel',()=>{void toggleMenu(true)})
    }
    initialized=true;await poll();if(native&&state.value.dock!=='none'&&state.value.preferences.edgeDock&&!safeMode.value)await setDock(state.value.dock);if(previewGreeting.value&&!previewPlan.some(p=>p==='music'||p==='motion')){updateActivity('greet',600000);await toggleMenu(true)}void tick();void pollTick();void musicTick();runPreviewPlan()
    if(native&&previewGreeting.value&&!previewSuite.value)window.setTimeout(()=>{
      const videos=Array.from(document.querySelectorAll('video')),v=videos.find(v=>v.style.visibility==='visible')??videos.find(v=>v.hasAttribute('src'))??videos[0]
      if(!v)return
      let corner=0,center=0
      if(v.readyState>=2){try{const c=document.createElement('canvas');c.width=256;c.height=256;const ctx=c.getContext('2d');if(ctx){ctx.drawImage(v,0,0,256,256);corner=ctx.getImageData(0,0,1,1).data[3];center=ctx.getImageData(128,128,1,1).data[3]}}catch{/* Diagnostic remains explicitly unverified. */}}
      void invoke('companion_preview_report',{probe:{width:v.videoWidth,height:v.videoHeight,corner,center,panel:menu.value,reduced:still.value,selected:!!video.value,ready:v.readyState,network:v.networkState,media_error:v.error?.code??0,paused:v.paused}}).catch(()=>{})
    },4000)
    saveTimer=window.setInterval(()=>{void save()},30000);hitTimer=window.setInterval(()=>{void updateHitTest()},100)
  }catch(e){error.value=`初始化失败：${String(e)}`}
})
onBeforeUnmount(()=>{disposed=true;if(native)void invoke('companion_music_poll',{enabled:false}).catch(()=>{});cancelDrag();[tickTimer,pollTimer,saveTimer,hitTimer,musicTimer,syncTimer,chatTimer].forEach(t=>window.clearTimeout(t));window.clearTimeout(clearMessageTimer);window.clearTimeout(codexTimer);window.clearTimeout(previewTimer);motionQuery?.removeEventListener('change',motionChanged);unlisten?.()})
</script>

<template>
  <main class="companion" :class="{still, suppressed:hiddenForFullscreen, dormant:!visible||dockHidden||hiddenForFullscreen}" @contextmenu.prevent="toggleMenu()" @keydown.esc="toggleMenu(false)">
    <div class="pet-stage" :style="stageStyle">
      <div v-if="message" class="speech" role="button" tabindex="0" aria-label="打开气泡聊天" @click="openChat" @keydown.enter="openChat" @keydown.space.prevent="openChat"><span>● 企鹅</span><p>{{message}}</p></div>
      <button v-else-if="safeMode||busy||codexStatus==='ended'||codexStatus==='interrupted'" class="codex-marker" @click="safeMode?openTools():openCodex"><i :class="codexStatus"></i>{{safeMode?'安全模式 · 查看诊断':presentation.label}}</button>
      <div class="pet" :class="[activity,{music:musicScene.showing,dragging:dragging&&moved,held:dragging&&!moved,airborne}]" @pointerdown="pointerDown" @pointerup="release" @pointercancel="release()" @lostpointercapture="release()">
        <div ref="img" class="pet-visual" :style="{opacity:musicScene.opacity}"><CompanionDJ v-if="musicScene.showing" :clock="musicClock" :paused="still||!visible||dockHidden||hiddenForFullscreen" :video-enabled="state.preferences.videoAnimations&&!djFailed" :probe="previewGreeting" @verified="verifyPreview" @failed="djFailed=true"/><CompanionSprite v-else :src="artwork.src" :alt="artwork.alt" :video="video" :request="media?.revision" :loop="media?.clip.loop??true" :paused="still||!visible||dockHidden||hiddenForFullscreen" :probe="previewGreeting" @verified="verifyPreview" @playing="videoPlaying" @ended="videoEnded" @failed="videoFailed" @image-failed="imageFailed"/></div>
      </div>
      <div v-if="hovered&&!dockHidden&&!menu&&!pressed&&!state.preferences.passthrough" class="pet-tools"><button @click.stop="openChat" title="和企鹅聊聊">聊聊</button><button @click.stop="openCodex" title="在现有 Codex 中聊天">Codex ↗</button><button @click.stop="toggleMenu(true)" aria-label="打开快速控制">···</button></div>
      <div v-if="food" class="food-picker"><strong>给企鹅一点小鱼</strong><p>饥饿 {{Math.round(state.hunger)}} · 体力 {{Math.round(state.energy)}}</p><button @click="interact('feed');food=false">喂小鱼</button><button @click="food=false">下次再喂</button></div>
    </div>
    <div v-if="menu" class="panel-host" :style="{left:`${panel.x}px`,top:`${panel.y}px`}" @contextmenu.stop.prevent>
      <CompanionChat v-if="chatOpen" :native="native" :view="syncView" :session="chatSession" @close="toggleMenu(false)" @received="receiveChat" @tools="openTools"/>
      <CompanionDiagnostics v-else-if="tools" :native="native" @close="closeTools" @safe="setSafety" @sync="receiveSync"/>
      <CompanionPanel v-else :state="state" :packs="packs" :native="native" :importing="importing" :system-reduced="reduce" @import-pack="importPack" @refresh-packs="refreshPacks" @pack-folder="openPackFolder" :music-status="musicLabel" :status="statusLabel" :connection="`${relationship(state.affection)} · ${connection}`" :expanded="expanded" @close="toggleMenu(false)" @expand="expandPanel" @care="interact" @action="requestAction" @codex="openCodex" @chat="openChat" @preference="preferencesChanged" @hide="hidePet" @quit="quit" @install="installHooks" @snap="snap" @tools="openTools"/>
    </div>
    <p v-if="error" class="notice" role="alert" @click="error=''">{{error}}</p>
  </main>
</template>

<style>
.pet.music .pet-visual{animation:none!important}
:root{font-family:'Segoe UI','Microsoft YaHei',sans-serif;font-size:12px;color:#173c56}*{box-sizing:border-box}body{margin:0;background:transparent;overflow:hidden}.companion{position:relative;width:100vw;height:100vh;user-select:none}.pet-stage{position:absolute;width:280px;height:290px;transform-origin:0 0}.pet{position:absolute;left:15px;top:40px;width:250px;height:250px;touch-action:none;cursor:grab}.pet .pet-visual{width:250px;height:250px;object-fit:contain;pointer-events:none;transform-origin:50% 90%}.pet.idle .pet-visual,.pet.rest .pet-visual{animation:breathe 5s ease-in-out infinite}.pet.held .pet-visual{transform:scale(.97,1.02)}.pet.land .pet-visual{animation:landing .4s ease-out}.pet.dragging{cursor:grabbing}.pet.dragging .pet-visual{transform:rotate(-3deg)}.pet.dizzy .pet-visual{animation:sway .8s ease-in-out infinite}.still .pet .pet-visual{animation:none!important;transform:none!important}
button{font:inherit;cursor:pointer}.speech{position:absolute;top:0;left:16px;right:16px;background:#f3fafff7;border-radius:15px;padding:9px 13px;z-index:3;box-shadow:0 3px 12px #03132122;pointer-events:auto}.speech:after{content:'';position:absolute;bottom:-7px;left:48%;border-left:8px solid transparent;border-right:8px solid transparent;border-top:8px solid #f3faff}.speech span{color:#36a5d1;font-size:10px;font-weight:700}.speech p{margin:4px 0 0;font-size:12px;font-weight:600;line-height:1.5}
.codex-marker{position:absolute;top:10px;left:50%;transform:translateX(-50%);white-space:nowrap;border:1px solid #bdd9e9;border-radius:14px;background:#edf8fff2;color:#194460;font-size:10px;padding:5px 10px;z-index:2}.codex-marker i{display:inline-block;width:6px;height:6px;background:#36a5d1;border-radius:50%;margin-right:5px}.codex-marker i.waiting{background:#dc9d39}.codex-marker i.ended{background:#4aaf8e}.pet-tools{position:absolute;bottom:1px;left:40px;display:flex;gap:6px;z-index:3}.pet-tools button{border:1px solid #8fb4c9;border-radius:12px;background:#eef9ffed;color:#194460;font-size:10px;padding:4px 10px}.panel-host{position:absolute;z-index:8}.notice{position:absolute;bottom:8px;left:8px;max-width:260px;max-height:125px;overflow:auto;margin:0;padding:11px;background:#fff4ddf7;color:#72521c;border:1px solid #d9bd83;border-radius:12px;z-index:20;font-size:11px;cursor:pointer;line-height:1.5}.food-picker{position:absolute;left:15px;right:15px;top:50px;padding:15px;border-radius:15px;background:#0c2c43f7;color:#e7f5ff;z-index:4;box-shadow:0 4px 20px #03132144}.food-picker p{font-size:11px;color:#b1d3e7}.food-picker button{border:0;border-radius:9px;background:#55b9df;color:#062a42;padding:8px 12px;margin-right:6px}.food-picker button:last-child{background:#214b64;color:#e7f5ff}.suppressed{visibility:hidden}
.dormant .pet .pet-visual{animation-play-state:paused}@keyframes landing{0%{transform:scale(1.08,.91)}70%{transform:scale(.98,1.02)}100%{transform:scale(1)}}@keyframes breathe{50%{transform:translateY(-1px)}}@keyframes sway{25%{transform:rotate(-2deg)}75%{transform:rotate(2deg)}}@media(prefers-reduced-motion:reduce){.pet .pet-visual{animation:none!important;transform:none!important}}
</style>
