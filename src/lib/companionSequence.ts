import {MotionPicker,type MediaPack,type MediaSegment} from './companionMedia'
type Candidate=NonNullable<ReturnType<MotionPicker['sample']>>
interface Playback {video:boolean;paused?:boolean;immediate?:boolean;reduced?:boolean;forcedId?:string}
/** Optional media phases. Old imported packs retain the original direct player. */
export class MotionSequencer {
  private picker=new MotionPicker()
  private active?:Candidate; private pending?:Candidate
  private key=''; private phase:'enter'|'body'|'exit'|'hold'='body'
  private at=0; private previous=0; private revision=0; private started=false
  private suspended?:number
  reset(immediate=false){this.picker.reset();if(immediate){this.active=undefined;this.pending=undefined;this.key='';this.revision++}}
  private identity(c:Candidate){return `${c.directory??''}:${c.clip.id}:${c.clip.slot}:${c.revision}`}
  private begin(c:Candidate,now:number,reduced=false){this.active=c;this.pending=undefined;this.key=this.identity(c);this.setPhase(!reduced&&c.clip.enter?'enter':'body',now)}
  private setPhase(phase:typeof this.phase,now:number){this.phase=phase;this.at=now;this.previous=now;this.started=false;this.revision++}
  private segment():MediaSegment|undefined {const c=this.active?.clip;return this.phase==='enter'?c?.enter:this.phase==='exit'||this.phase==='hold'?c?.exit:c}
  private duration(){const s=this.segment();return Math.max(1,s?.durationMs??(s?.frames.length??1)*(s?.frameMs??100))}
  private finish(now:number){if(this.pending)this.begin(this.pending,now);else this.phase='hold'}
  private advance(now:number){
    if(this.phase==='enter'){this.setPhase('body',now);return}
    if(this.phase==='body'){if(this.active?.clip.exit)this.setPhase('exit',now);else this.finish(now);return}
    if(this.phase==='exit')this.finish(now)
  }
  playing(revision:number,now:number){if(revision===this.revision&&!this.started){this.at=now;this.previous=now;this.started=true}}
  ended(revision:number,now:number){
    if(revision!==this.revision||!this.active||this.phase==='hold')return
    if(this.phase==='body'&&this.active.clip.loop&&!this.pending)return
    this.advance(now)
  }
  sample(pack:MediaPack|undefined,slot:string,now:number,random:number,options:Playback={video:true}){
    if(options.paused){this.suspended??=now;now=this.suspended}
    else if(this.suspended!==undefined){const pause=now-this.suspended;this.at+=pause;this.previous+=pause;this.suspended=undefined}
    const request=this.picker.sample(pack,slot,now,random,options.forcedId)
    if(!request){this.active=undefined;return undefined}
    const changed=this.identity(request)!==this.key
    if(!this.active||(changed&&(options.immediate||options.reduced||!this.active.clip.exit||this.phase==='hold'))){this.begin(request,now,options.reduced)}
    else if(changed)this.pending=request
    else if(this.phase!=='exit')this.pending=undefined
    else if(this.pending)this.pending=request
    if(options.reduced){this.pending=undefined;this.phase='body'}
    const c=this.active!,duration=this.duration(),elapsed=Math.max(0,now-this.at)
    // Native videos advance on their own ended event, including disabling loop to
    // finish the current cycle. The clock path is used only by PNG/WebP fallback.
    const nativeVideo=!!this.active?.clip.video
    if(!(options.video&&nativeVideo)&&!options.paused&&!options.reduced&&this.phase!=='hold'){
      const crossedCycle=this.phase==='body'&&c.clip.loop&&this.pending&&Math.floor(elapsed/duration)>Math.floor(Math.max(0,this.previous-this.at)/duration)
      if(crossedCycle||(elapsed>=duration&&(this.phase!=='body'||!c.clip.loop)))this.advance(now)
    }
    this.previous=now
    const active=this.active!,segment=this.segment()??active.clip,body=this.phase==='body',loop=body&&active.clip.loop&&!this.pending,index=Math.max(0,Math.floor((now-this.at)/segment.frameMs)),frames=segment.frames
    const i=this.phase==='hold'?frames.length-1:body&&active.clip.loop?index%frames.length:Math.min(index,frames.length-1)
    return {clip:{...active.clip,...segment,loop},revision:this.revision,directory:active.directory,frame:options.reduced?active.clip.poster:frames[i]??segment.poster,phase:this.phase,queued:!!this.pending}
  }
}
