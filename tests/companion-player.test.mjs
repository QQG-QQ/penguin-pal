import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'

// Exercise the real component's script with deferred media promises; browser tests
// separately cover Vue rendering and actual decoding. No production test hooks.
function player(){
 const path=process.env.PENGUIN_PLAYER_SOURCE??new URL('../src/components/CompanionSprite.vue',import.meta.url)
 const source=readFileSync(path,'utf8').match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^import .+$/mg,'')
 const js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText
 const props={src:'poster.png',alt:'penguin',video:'first.webm',paused:false,loop:true,request:0}
 const events=[],watchers=[],timers=new Map();let mounted,unmount,id=0,now=0
 const window={setTimeout(fn,delay){timers.set(++id,{fn,at:now+delay});return id},clearTimeout(i){timers.delete(i)}}
 const result=new Function('ref','watch','nextTick','onMounted','onBeforeUnmount','defineProps','defineEmits','window',js+'\nreturn {videos};')(
  value=>({value}), (get,fn)=>watchers.push({get,fn,last:JSON.stringify(get())}),async()=>{},fn=>{mounted=fn},fn=>{unmount=fn},()=>props,()=>((...args)=>events.push(args)),window)
 const makeVideo=()=>({src:'',paused:true,videoWidth:256,videoHeight:256,duration:1,error:null,requests:[],defer:false,
  play(){this.paused=false;if(!this.defer)return Promise.resolve();return new Promise((resolve,reject)=>this.requests.push({resolve,reject}))},
  pause(){this.paused=true;for(const r of this.requests.splice(0))r.reject(new DOMException('Interrupted by pause','AbortError'))},
  load(){},removeAttribute(name){if(name==='src')this.src=''},resolve(){for(const r of this.requests.splice(0))r.resolve()}})
 result.videos.push(makeVideo(),makeVideo())
 const settle=async()=>{for(let i=0;i<8;i++)await Promise.resolve()}
 return {props,events,videos:result.videos,async mount(){mounted();await settle()},async set(patch){Object.assign(props,patch);for(const w of watchers){const next=JSON.stringify(w.get());if(next!==w.last){w.last=next;w.fn()}}await settle()},
  async ready(v=result.videos.find(v=>v.src)){v.onloadedmetadata?.();v.onloadeddata?.();await settle()},
  async advance(ms){now+=ms;for(const [i,t] of [...timers])if(t.at<=now){timers.delete(i);t.fn()}await settle()},
  async dispose(){unmount();await settle()},settle,failures(){return events.filter(e=>e[0]==='failed')}}
}

test('pause while play is pending is not a broken video; unpause resumes',async()=>{
 const p=player();await p.mount();p.videos[0].defer=true;await p.ready()
 await p.set({paused:true});assert.deepEqual(p.failures(),[])
 assert.equal(p.videos[0].src,'first.webm')
 await p.set({paused:false});p.videos[0].resolve();await p.settle()
 assert.equal(p.videos[0].paused,false);assert.deepEqual(p.failures(),[]);await p.dispose()
})
test('late rejected resume from old video cannot reset a newly selected video',async()=>{
 const p=player();await p.mount();await p.ready();await p.set({paused:true})
 p.videos[0].defer=true;await p.set({paused:false});await p.set({video:'second.webm',request:1})
 await p.ready(p.videos[1]);assert.deepEqual(p.failures(),[])
 assert.equal(p.videos[1].src,'second.webm');assert.equal(p.videos[0].src,'');await p.dispose()
})
test('hidden loading does not expire; showing and decoding then plays normally',async()=>{
 const p=player();await p.mount();await p.set({paused:true});await p.advance(9000)
 assert.deepEqual(p.failures(),[]);await p.set({paused:false});await p.ready();assert.equal(p.videos[0].paused,false);await p.dispose()
})
test('genuine decode errors reset both buffers and retain the real failure reason',async()=>{
 const p=player();await p.mount();await p.ready();p.videos[0].error={code:3};p.videos[0].onerror()
 assert.deepEqual(p.failures(),[['failed','first.webm','decode']]);assert.ok(p.videos.every(v=>!v.src));await p.dispose()
})
test('invalid metadata, real startup timeout and policy denial still fail',async()=>{
 const a=player();await a.mount();a.videos[0].videoWidth=1024;await a.ready()
 assert.equal(a.failures()[0]?.[2],'metadata');await a.dispose()
 const b=player();await b.mount();await b.advance(8000);assert.equal(b.failures()[0]?.[2],'timeout');await b.dispose()
 const c=player();await c.mount();c.videos[0].play=()=>Promise.reject(new DOMException('Denied','NotAllowedError'));await c.ready()
 assert.equal(c.failures()[0]?.[2],'blocked');await c.dispose()
})
test('one unexpected abort retries once; teardown cancels pending playback silently',async()=>{
 const p=player();await p.mount();let calls=0
 p.videos[0].play=()=>++calls===1?Promise.reject(new DOMException('Abort','AbortError')):Promise.resolve()
 await p.ready();await p.advance(50);assert.equal(calls,2);assert.deepEqual(p.failures(),[])
 await p.set({request:1});p.videos[1].defer=true;await p.ready(p.videos[1]);await p.dispose();assert.deepEqual(p.failures(),[])
})
test('ended events retain phase ownership and stale buffers cannot complete a new phase',async()=>{
 const p=player();await p.mount();await p.ready();const old=p.videos[0].onended
 await p.set({video:'next.webm',request:7});await p.ready(p.videos[1]);old()
 assert.deepEqual(p.events.filter(e=>e[0]==='ended'),[])
 p.videos[1].onended();assert.deepEqual(p.events.filter(e=>e[0]==='ended'),[['ended',7]])
 await p.dispose()
})
test('a hidden end cannot advance the visible phase',async()=>{
 const p=player();await p.mount();await p.ready();await p.set({paused:true});p.videos[0].onended()
 assert.deepEqual(p.events.filter(e=>e[0]==='ended'),[]);await p.dispose()
})
