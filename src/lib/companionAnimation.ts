import type { Activity } from './companionCore'
export interface Clip { frames: string[]; frameMs: number; loop: boolean; enter?:string[]; exit?:string[]; priority?:number; slot?:string }
const clip = (frames: string[], frameMs = 180, loop = true,options:Partial<Clip>={}): Clip => ({ frames, frameMs, loop,...options })
export function selectClip(activity: Activity, codex: string, dragging: boolean, falling: boolean, facing: number,preferActivity=false): Clip {
  const manual=['land','dizzy','pat','poke','tail','eat','happy','greet','annoyed'].includes(activity)||(preferActivity&&activity!=='idle'&&activity!=='walk')
  const selected=selectBase(activity,manual?'idle':codex,dragging,falling,facing)
  const slot=dragging?'drag':falling?'fall':manual?activity:['working','waiting','toolError','ended','interrupted'].includes(codex)?codex:activity==='walk'?`walk-${facing<0?'left':'right'}`:activity
  return {...selected,slot}
}
function selectBase(activity: Activity, codex: string, dragging: boolean, falling: boolean, facing: number): Clip {
  if (dragging) return clip(['drag'],180,true,{priority:100})
  if (falling) return clip(['fall'],180,true,{priority:100})
  if (activity === 'land') return clip(['land','idle'],180,false)
  if (activity === 'dizzy') return clip(['dizzy'])
  if (activity === 'pat') return clip(['pat-soft','pat-closed','pat-soft'],160,false,{priority:70})
  if (activity === 'poke') return clip(['poke','idle'],220,false,{priority:70})
  if (activity === 'tail') return clip(['tail','look-right'],400,false)
  if (activity === 'eat') return clip(['eat-hold','eat-bite'],420)
  if (activity === 'happy') return clip(['happy','greet'],450)
  if (activity === 'greet') return clip(Array.from({length:8},(_,i)=>`wave-${i}`),150,false,{priority:70,enter:['greet'],exit:['idle']})
  if (activity === 'annoyed') return clip(['annoyed'])
  if (codex === 'toolError') return clip(['annoyed','think'],800,true,{priority:90})
  if (codex === 'working') return clip(['think','blink-half','think'],900,true,{priority:50})
  if (codex === 'waiting') return clip(['look-left','look-right'],1300,true,{priority:80})
  if (codex === 'ended') return clip(['greet','happy'],600)
  if (activity === 'walk') {
    const direction=facing<0?'left':'right'
    return clip(Array.from({length:4},(_,i)=>`walk-${direction}-${i}`),170,true,{enter:['idle',`walk-${direction}-0`],exit:[`walk-${direction}-3`,'idle'],priority:10})
  }
  if (activity === 'rest') return clip(['sleep'])
  if (activity === 'wake') return clip(['wake','idle'],650,false)
  if (activity === 'blink') return clip(['blink-half','blink-closed','blink-half','idle'],85,false)
  if (activity === 'look') return clip(['look-left','look-right','idle'],700,false)
  if (activity === 'think') return clip(['think'])
  if (activity === 'talk') return clip(['talk','greet'],450)
  if (activity === 'sweep') return clip(['sweep-left','sweep-right'],450)
  return clip(['idle'])
}
export class AnimationPlayer {
  private current:Clip=clip(['idle'])
  private requested=''
  private pending:Clip|undefined
  private at=0
  phase:'enter'|'body'|'exit'='body'
  reset(){this.requested='';this.pending=undefined}
  private begin(next:Clip,now:number,reduced:boolean){this.current=next;this.at=now;this.phase=!reduced&&next.enter?.length?'enter':'body';this.pending=undefined}
  sample(next:Clip,now:number,reduced:boolean){
    const signature=next.frames.join(',')+':'+(next.priority??0)
    if(signature!==this.requested){
      this.requested=signature
      if(!reduced&&(next.priority??0)<50&&this.current.exit?.length){this.pending=next;this.phase='exit';this.at=now}
      else this.begin(next,now,reduced)
    }
    if(reduced){if(this.pending)this.begin(this.pending,now,true);this.phase='body';return this.current.frames[0]}
    const frames=this.phase==='enter'?this.current.enter:this.phase==='exit'?this.current.exit:undefined
    if(frames&&now-this.at>=frames.length*this.current.frameMs){
      if(this.phase==='exit'&&this.pending)this.begin(this.pending,now,false)
      else{this.phase='body';this.at=now}
    }
    const phaseFrames=this.phase==='enter'?this.current.enter:this.phase==='exit'?this.current.exit:undefined
    if(phaseFrames)return phaseFrames[Math.min(phaseFrames.length-1,Math.max(0,Math.floor((now-this.at)/this.current.frameMs)))]
    return frameAt(this.current,now-this.at,false)
  }
}
export function frameAt(clip: Clip, elapsed: number, reduced: boolean) {
  const index = reduced ? 0 : Math.max(0,Math.floor(elapsed/clip.frameMs))
  return clip.frames[clip.loop ? index % clip.frames.length : Math.min(index,clip.frames.length-1)]
}
