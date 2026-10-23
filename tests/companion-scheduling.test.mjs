import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'

const script=name=>readFileSync(new URL('../src/'+name,import.meta.url),'utf8')
const transpile=(source,module=ts.ModuleKind.None)=>ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module}}).outputText
const mind=await import('data:text/javascript;base64,'+Buffer.from(transpile(script('lib/companionMind.ts'),ts.ModuleKind.ES2022)).toString('base64'))
const source=script('components/CompanionMind.vue').match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^import[^\n]*\r?$/mg,'')
const view={status:{document:{memories:[]},history:{}},session:'fixture',reply:null,preview:{reason:'环境允许',motive:'陪伴',recall:{memories:[]}}}
function component(invoke){
  const mounted=[],unmounted=[],events=[]
  const deps={...mind,ref:value=>({value}),computed:fn=>({get value(){return fn()}}),watch(){},onMounted:fn=>mounted.push(fn),onBeforeUnmount:fn=>unmounted.push(fn),setInterval:()=>1,clearInterval(){},defineProps:()=>({native:true,view:null,allowWorkChat:false}),defineEmits:()=>((_event,value)=>events.push(value)),invoke}
  const api=new Function('deps',`const {${Object.keys(deps).join(',')}}=deps;${transpile(source)};return {rehearse,outreach,cancelOutreach,refreshRuntime,busy,rehearsing,manualPending,error,preview,progress};`)(deps)
  mounted.forEach(fn=>fn())
  return {...api,events,unmount:()=>unmounted.forEach(fn=>fn())}
}
const idle={active:null,queuedManual:false,queued:0,cancelling:false}
const cases=[
  {id:'preview-while-manual-waits',suite:'chat',feature:'scheduling',tags:['happy','concurrency'],async run(){
    let finish
    const app=component(async(_command,{action})=>action==='runtime'?{active:'learning',queuedManual:true,queued:1,cancelling:false}:action==='outreach'?new Promise(ok=>{finish=ok}):view)
    const waiting=app.outreach()
    await new Promise(setImmediate);await app.refreshRuntime()
    assert.equal(app.busy.value,true);assert.equal(app.manualPending.value,true)
    assert.match(app.progress.value,/后台整理记忆.*排队/)
    await app.rehearse()
    assert.ok(app.preview.value);assert.equal(app.rehearsing.value,false);assert.equal(app.manualPending.value,true)
    finish({...view,reply:'测试气泡'});await waiting
    assert.equal(app.events.filter(e=>e.reply==='测试气泡').length,1);assert.equal(app.busy.value,false)
  }},
  {id:'duplicate-manual-and-cancel',suite:'chat',feature:'scheduling',tags:['boundary','state'],async run(){
    let finish,calls=0,cancelled=0
    const app=component(async(_command,{action})=>{if(action==='runtime')return idle;if(action==='cancelOutreach'){cancelled++;return {ok:true}}if(action==='outreach'){calls++;return new Promise(ok=>{finish=ok})}return view})
    const waiting=app.outreach();await app.outreach();assert.equal(calls,1)
    await app.cancelOutreach();assert.equal(cancelled,1)
    app.unmount();assert.equal(cancelled,2)
    finish({...view,reply:'late result'});await waiting
    assert.equal(app.events.length,0);assert.equal(app.manualPending.value,false)
  }},
  {id:'failure-cleans-ui-for-retry',suite:'chat',feature:'scheduling',tags:['failure','recovery'],async run(){
    let fail=true
    const app=component(async(_command,{action})=>{if(action==='runtime')return idle;if(fail)throw Error('测试超时');return view})
    await app.rehearse();assert.match(app.error.value,/超时/);assert.equal(app.rehearsing.value,false)
    await app.outreach();assert.match(app.error.value,/超时/);assert.equal(app.busy.value,false);assert.equal(app.manualPending.value,false)
    fail=false;await app.rehearse();assert.ok(app.preview.value);assert.equal(app.error.value,'')
  }}
]
for(const item of cases)test(`${item.suite}/${item.feature}/${item.id} [${item.tags.join(',')}]`,item.run)
