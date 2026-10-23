export interface Preferences {
  wander: boolean; gravity: boolean; surfaces: boolean; awareness: boolean
  reducedMotion: boolean; quiet: boolean; codexEvents: boolean; allowWorkChat: boolean
  movement: 'still' | 'wander' | 'follow'; size: number; rhythm: 'calm' | 'balanced' | 'lively'
  walkSpeed: number; hoverPause: boolean; topmost: boolean; fullscreenHide: boolean
  passthrough: boolean; autostart: boolean; personality: 'gentle' | 'playful' | 'quiet' | 'warm'
  edgeDock: boolean; autoHide: boolean; notifications: boolean
  animationPack:string; videoAnimations:boolean; springDrag:boolean; statusAnimations:boolean
  musicMode:'audio'|'off'
}
export interface CompanionState {
  schema: number; energy: number; hunger: number; affection: number; savedAt: number
  x: number | null; y: number | null; preferences: Preferences
  dock: DockEdge
}
export type CareAction = 'pat' | 'poke' | 'tail' | 'feed' | 'rest'
export type Activity = 'idle' | 'blink' | 'look' | 'think' | 'walk' | 'rest' | 'happy' | 'greet' | 'pat' | 'poke' | 'tail' | 'eat' | 'sweep' | 'wake' | 'talk' | 'annoyed' | 'dizzy' | 'land'
export interface Surface { id: number; left: number; right: number; top: number; bottom: number }
export interface Bounds { left: number; right: number; top: number; bottom: number }
export interface Body { x: number; y: number; vx: number; vy: number; width: number; height: number }
export type DockEdge = 'none' | 'left' | 'right' | 'top'
export const defaultCompanion = (now: number): CompanionState => ({ schema: 1, energy: 80, hunger: 20,
  affection: 65, savedAt: now, x: null, y: null, dock: 'none', preferences: { wander: false, gravity: true,
    surfaces: true, awareness: false, reducedMotion: false, quiet: false, codexEvents: true, allowWorkChat: false,
    movement: 'still', size: 1, rhythm: 'balanced', walkSpeed: 55, hoverPause: true,
    topmost: true, fullscreenHide: true, passthrough: false, autostart: false, personality: 'gentle',
    edgeDock: true, autoHide: true, notifications: true, animationPack:'builtin',videoAnimations:true,springDrag:true,statusAnimations:true,musicMode:'audio' } })
const clamp = (v: number, min = 0, max = 100) => Math.min(max, Math.max(min, v))

const mergedBuiltinPacks = new Set(['penguin-greeting-video','doubao-walk-right-20261021','doubao-walk-pair-20261021','doubao-core-20261021','doubao-gray-20261021'])
export const isMergedBuiltinPack = (id:string) => mergedBuiltinPacks.has(id)
export const canonicalAnimationPack = (id:string) => isMergedBuiltinPack(id)?'builtin':id

export function recoverNeeds(state: CompanionState, now: number): CompanionState {
  const hours = clamp((now - state.savedAt) / 3600000, 0, 24)
  return { ...state, dock: state.dock ?? 'none', preferences: { ...defaultCompanion(now).preferences, ...state.preferences, musicMode:state.preferences.musicMode==='off'?'off':'audio', animationPack:canonicalAnimationPack(state.preferences.animationPack??'builtin') }, energy: clamp(state.energy + hours * 2), hunger: clamp(state.hunger + hours * 2),
    affection: clamp(state.affection - hours / 4), savedAt: now }
}
export function care(state: CompanionState, action: CareAction, recentCount = 0) {
  const next = { ...state }
  let activity: Activity = 'happy', message = '嗯，摸摸收到啦。'
  if (action === 'feed') {
    if (recentCount >= 3 || state.hunger < 8) return { state, activity: 'look' as Activity, message: '已经吃饱啦，留一点下次吃吧。' }
    next.hunger = clamp(state.hunger - 18); next.energy = clamp(state.energy + 5)
    next.affection = clamp(state.affection + 2); message = '小鱼真香，谢谢你。'; activity = 'eat'
  } else if (action === 'poke') {
    activity = recentCount >= 3 ? 'annoyed' : 'poke'
    message = recentCount >= 3 ? '别一直戳啦，轻轻摸摸就好。' : '咦，找我有什么事？'
  } else if (action === 'tail') {
    activity = 'tail'; message = '欸，尾巴也被你发现了。'
  } else if (action === 'rest') {
    activity = 'rest'; message = '我安静休息一会儿，你忙你的。'
  } else {
    activity = 'pat'
    next.affection = clamp(state.affection + (recentCount >= 4 ? 0 : 1))
    message = recentCount >= 4 ? '好啦好啦，我就在这里。' : message
  }
  return { state: next, activity, message }
}
export function chooseActivity(state: CompanionState, random: number, busy: boolean): Activity {
  if (busy || state.preferences.quiet || state.preferences.reducedMotion) return 'idle'
  if (state.energy < 30) return 'rest'
  if (state.preferences.wander && random < 0.25) return 'walk'
  if (random > .88) return 'sweep'
  return random < 0.65 ? 'look' : 'think'
}
export function stepPhysics(body: Body, bounds: Bounds, surfaces: Surface[], delta: number, scale = 1) {
  const dt = clamp(delta, 0, 0.05), b = { ...body }
  if (!Object.values(b).every(Number.isFinite) || b.width <= 0 || b.height <= 0) throw new Error('Invalid body')
  b.vx = clamp(b.vx, -2200 * scale, 2200 * scale)
  b.vy = clamp(b.vy + 1600 * scale * dt, -2200 * scale, 2200 * scale)
  b.x += b.vx * dt; b.y += b.vy * dt
  const maxX = Math.max(bounds.left, bounds.right - b.width)
  if (b.x < bounds.left || b.x > maxX) { b.x = clamp(b.x, bounds.left, maxX); b.vx *= -0.45 }
  if (b.y < bounds.top) { b.y = bounds.top; b.vy = Math.max(0, -b.vy * 0.35) }
  const center = b.x + b.width / 2
  const platform = surfaces.filter(s => center >= s.left + 8 && center <= s.right - 8
    && s.top >= bounds.top && s.top <= bounds.bottom
    && body.y + body.height <= s.top + 2 * scale && b.y + b.height >= s.top && b.vy >= 0)
    .sort((a, c) => a.top - c.top)[0]
  const floor = platform?.top ?? bounds.bottom
  let landed = false, hard = false
  if (b.y + b.height >= floor && b.vy >= 0) {
    const impact = b.vy
    b.y = Math.max(bounds.top, floor - b.height); b.vx *= 0.65
    b.vy = impact > 650 * scale ? -impact * 0.2 : 0
    landed = b.vy === 0; hard = impact > 950 * scale
    if (Math.abs(b.vx) < 30 * scale) b.vx = 0
  }
  return { body: b, landed, hard, surface: platform?.id ?? null }
}
export function followSurface(body: Body, previous: Surface, next: Surface | undefined, bounds: Bounds) {
  if (!next || next.top < bounds.top + body.height || next.top > bounds.bottom) return null
  const left = Math.max(bounds.left, next.left - body.width / 2 + 10)
  const right = Math.min(bounds.right - body.width, next.right - body.width / 2 - 10)
  if (left > right) return null
  const x = clamp(body.x + next.left - previous.left, left, right)
  return { ...body, x, y: Math.max(bounds.top, next.top - body.height), vx: 0, vy: 0 }
}
export function nearEdge(origin: {x:number;y:number}, area: Bounds, width:number, height:number, threshold=28): DockEdge {
  const choices: [DockEdge,number][] = [['left',Math.abs(origin.x-area.left)],['right',Math.abs(origin.x+width-area.right)],['top',Math.abs(origin.y-area.top)]]
  const selected=choices.sort((a,b)=>a[1]-b[1])[0]
  if(origin.y+height<area.top||origin.y>area.bottom)return 'none'
  return selected[1]<=threshold?selected[0]:'none'
}
export function dockPosition(edge:DockEdge,home:{x:number;y:number},area:Bounds,width:number,height:number,footprint:{x:number;y:number;width:number;height:number},hidden:boolean){
  let x=clamp(home.x,area.left,Math.max(area.left,area.right-width)),y=clamp(home.y,area.top,Math.max(area.top,area.bottom-height))
  if(edge==='left')x=hidden?area.left-footprint.x-footprint.width*.55:area.left
  if(edge==='right')x=hidden?area.right-footprint.x-footprint.width*.45:Math.max(area.left,area.right-width)
  if(edge==='top')y=hidden?area.top-footprint.y-footprint.height*.6:area.top
  return {x:Math.round(x),y:Math.round(y)}
}
export function frameDelay(hidden:boolean,physics:boolean,animated:boolean){return hidden?1000:physics?33:animated?66:200}
export function shouldIdleRest(idleSeconds:number,now:number,actionUntil:number,blocked:boolean,activity:Activity){return idleSeconds>300&&now>=actionUntil&&!blocked&&activity!=='rest'}
export function advanceNeeds(state:CompanionState,seconds:number,resting:boolean){
  const dt=clamp(seconds,0,60)
  state.energy=clamp(state.energy+(resting?.15:-1/180)*dt)
  state.hunger=clamp(state.hunger+dt/240)
}
export function relationship(affection:number){return affection<35?'初识':affection<80?'熟悉':'亲近'}
export function hitRegion(localY: number, top: number, bottom: number): 'pat' | 'poke' {
  return localY < top + (bottom - top) * 0.48 ? 'pat' : 'poke'
}
export function localLine(personality: Preferences['personality'], context: string, random = 0) {
  const lines = {
    gentle: ['我在这里，慢慢来。', '要不要伸个懒腰，再继续呀？', '今天也一起把事情做好。','我把桌面这一角收拾好啦。','不用着急，我们一步一步来。','窗外有风，桌面上有企鹅。','休息时叫我一起呀。','小鱼留着慢慢吃。'],
    playful: ['企鹅值班中！', '小鱼雷达已开启。', '任务加油，我负责可爱。','收到，桌面巡逻开始。','这一小块地盘交给我。','今天的鱼也很值得期待。','尾巴没有偷偷晃，是风。','扫地完毕，企鹅申请摸摸。'],
    quiet: ['嗯，我在。', '安静陪你。', '慢慢来。','休息一下也好。','我就在旁边。','这里很舒服。','不着急。','忙完再见。'],
    warm: ['记得喝口水，照顾好自己。', '忙完这一小步，就休息一下吧。', '你忙你的，我陪着你。','眼睛休息一下，看看远处。','坐久了也起来活动活动呀。','今天已经走过不少小步骤了。','晚一点再继续也可以。','希望这一角能让你放松一点。'],
  }
  if (context === 'video') return '一起安静看一会儿。'
  if(context==='coding')return ['这一段代码，我安静陪你写。','先走完眼前这一步。','需要休息时，企鹅在旁边。'][Math.floor(clamp(random,0,.999)*3)]
  if(context==='document')return ['资料慢慢整理，思路会清楚的。','我帮你守住这一角的安静。','读完这一页，也让眼睛休息一下。'][Math.floor(clamp(random,0,.999)*3)]
  return lines[personality][Math.floor(clamp(random, 0, .999) * lines[personality].length)]
}
export function codexPresentation(status: string) {
  switch (status) {
    case 'working': return { mode: 'thinking' as const, label: 'Codex 正在工作' }
    case 'waiting': return { mode: 'listening' as const, label: 'Codex 等待你的确认' }
    case 'ended': return { mode: 'speaking' as const, label: 'Codex 本轮已结束' }
    case 'interrupted': return { mode: 'idle' as const, label: 'Codex 已中断' }
    case 'toolError': return {mode:'guarded' as const,label:'Codex 工具出现错误'}
    default: return { mode: 'idle' as const, label: '打开 Codex 聊天' }
  }
}
