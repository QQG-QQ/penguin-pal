/** Local declarative media only. Pack files are validated and copied by the native importer. */
export const motionSlots = ['idle','blink','look','think','walk-left','walk-right','rest','happy','greet','pat','poke','tail','eat','sweep','wake','talk','annoyed','dizzy','land','drag','fall','working','waiting','ended','interrupted','toolError'] as const
export type MotionSlot = typeof motionSlots[number]
export interface MediaSegment {
  poster:string; frames:string[]; video?:string; durationMs?:number
  frameMs:number
}
export interface MediaClip extends MediaSegment {
  id:string; slot:MotionSlot; weight:number; loop:boolean
  enter?:MediaSegment; exit?:MediaSegment
}
export interface MediaPack { schema:number; id:string; name:string; clips:MediaClip[]; directory?:string }
export function motionPackForSlot(preferred:MediaPack|undefined,fallback:MediaPack,slot:string){return preferred?.clips.some(c=>c.slot===slot&&c.weight>0)?preferred:fallback}
/** Exclude the last candidate when an alternative exists, without changing relative weights. */
export function pickMotion(clips:MediaClip[], slot:string, previous:string|undefined, random:number) {
  const candidates=clips.filter(c=>c.slot===slot&&c.weight>0)
  const pool=candidates.length>1?candidates.filter(c=>c.id!==previous):candidates
  const total=pool.reduce((sum,c)=>sum+c.weight,0)
  let cursor=Math.max(0,Math.min(.999999,Number.isFinite(random)?random:0))*total
  for(const item of pool){cursor-=item.weight;if(cursor<0)return item}
  return pool[0]
}
export class MotionPicker {
  private key=''; private selected:MediaClip|undefined; private started=0
  private revision=0
  private last=new Map<string,string>()
  sample(pack:MediaPack|undefined,slot:string,now:number,random:number){
    const key=`${pack?.id??''}:${slot}`
    // Rotate idle variants at a relaxed cadence. Explicit actions play once per request.
    if(key!==this.key||(slot==='idle'&&now-this.started>12000)){
      this.key=key;this.started=now
      this.revision++
      this.selected=pack?pickMotion(pack.clips,slot,this.last.get(key),random):undefined
      if(this.selected)this.last.set(key,this.selected.id)
    }
    const c=this.selected
    if(!c)return undefined
    const index=Math.max(0,Math.floor((now-this.started)/c.frameMs))
    return {clip:c,revision:this.revision,directory:pack?.directory,frame:c.frames.length?c.frames[c.loop?index%c.frames.length:Math.min(index,c.frames.length-1)]:c.poster}
  }
  reset(){this.key='';this.selected=undefined}
}

export interface PointerSample {x:number;y:number;at:number}
export class DragTrail {
  private samples:PointerSample[]=[]
  reset(sample:PointerSample){this.samples=[sample]}
  push(sample:PointerSample){this.samples.push(sample);this.samples=this.samples.filter(p=>sample.at-p.at<=140).slice(-12)}
  velocity(now:number,scale=1){
    const tail=this.samples[this.samples.length-1]
    if(!tail||now-tail.at>100)return {vx:0,vy:0}
    const first=this.samples.find(p=>tail.at-p.at>=35)??this.samples[0]
    const dt=(tail.at-first.at)/1000,limit=2200*scale
    if(dt<=0)return {vx:0,vy:0}
    const clamp=(v:number)=>Math.max(-limit,Math.min(limit,v))
    return {vx:clamp((tail.x-first.x)/dt),vy:clamp((tail.y-first.y)/dt)}
  }
}
/** Analytic critically damped spring; stable even after a delayed frame. */
export function springStep(position:number,velocity:number,target:number,dt:number){
  const time=Math.max(0,Math.min(.05,dt)),omega=32,offset=position-target,aux=velocity+omega*offset,decay=Math.exp(-omega*time)
  return {position:target+(offset+aux*time)*decay,velocity:(velocity-omega*aux*time)*decay}
}

/** Preserve the full builtin video when a manual action outlasts the old fixed timeout. */
export function motionDuration(pack:MediaPack|undefined,slot:string,fallback=3500){
  const clip=pack?.clips.find(c=>c.slot===slot),duration=clip?.durationMs
  const phases=[clip?.enter?.durationMs,clip?.exit?.durationMs].reduce<number>((sum,n)=>sum+(typeof n==='number'&&Number.isFinite(n)&&n>0?n:0),0)
  return typeof duration==='number'&&Number.isFinite(duration)&&duration>0?Math.max(fallback,Math.min(15000,duration+phases+100)):fallback
}
