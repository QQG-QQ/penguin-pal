export interface MusicSnapshot {
  available:boolean; active:boolean; level:number; bass:number; treble:number
  pulse:number; bpm:number; confidence:number; source:string; issue:string
}
export const silentMusic=():MusicSnapshot=>({available:false,active:false,level:0,bass:0,treble:0,pulse:0,bpm:0,confidence:0,source:'',issue:''})
export interface MusicContext {
  enabled:boolean; reduced:boolean; quiet:boolean; visible:boolean; hidden:boolean
  codex:string; pressed:boolean; falling:boolean; panel:boolean; manual:boolean; activity:string
}
export function musicAllowed(c:MusicContext){
  return c.enabled&&!c.reduced&&!c.quiet&&c.visible&&!c.hidden&&!c.pressed&&!c.falling&&!c.panel&&!c.manual
    &&!['working','waiting','toolError','ended','interrupted'].includes(c.codex)
    &&!['land','dizzy'].includes(c.activity)
}
/** Gain changes never change tempo. Fold extreme BPMs into the video range. */
export function musicRate(bpm:number,confidence:number){
  if(!Number.isFinite(bpm)||!Number.isFinite(confidence)||confidence<.3||bpm<40||bpm>300)return 1
  let rate=bpm/120
  if(rate>=1.8)rate/=2
  if(rate<=.6)rate*=2
  return Math.max(.8,Math.min(1.25,rate))
}
/** Sequential fade: only one penguin is ever visible. Physical/task interruption is immediate. */
export class MusicScene {
  showing=false; opacity=1
  private phase:'steady'|'out'|'in'='steady'; private at=0
  sample(wanted:boolean,now:number,urgent=false){
    if(urgent){this.showing=false;this.opacity=1;this.phase='steady';return}
    if(this.phase==='steady'&&wanted!==this.showing){this.phase='out';this.at=now}
    if(this.phase==='out'){
      this.opacity=Math.max(0,1-(now-this.at)/140)
      if(this.opacity===0){this.showing=wanted;this.phase='in';this.at=now}
    }else if(this.phase==='in'){
      this.opacity=Math.min(1,(now-this.at)/180)
      if(this.opacity===1)this.phase='steady'
    }
  }
}
export class MusicClock {
  rate=1; elapsed=0; pulse=0; level=0; bass=0; treble=0
  step(sample:MusicSnapshot,dt:number,paused=false){
    const delta=Math.max(0,Math.min(.1,Number.isFinite(dt)?dt:0))
    const finite=(v:number)=>Number.isFinite(v)?Math.max(0,Math.min(1,v)):0
    const smooth=1-Math.exp(-delta*8)
    this.rate+=(musicRate(sample.bpm,sample.confidence)-this.rate)*smooth
    if(paused)return
    this.elapsed+=delta*1000*this.rate
    this.pulse=Math.max(finite(sample.pulse),this.pulse*Math.exp(-delta*7))
    for(const key of ['level','bass','treble'] as const)this[key]+=(finite(sample[key])-this[key])*smooth
  }
}
