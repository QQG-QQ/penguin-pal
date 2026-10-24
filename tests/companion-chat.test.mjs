import test from 'node:test'
import assert from 'node:assert/strict'
import {run,validateResult,contextFor,Rpc} from '../src-tauri/resources/companion/chat-engine.mjs'
import {visibleMessage} from '../src-tauri/resources/companion/conversation-events.mjs'

const source={id:'u1',role:'user',text:'我喜欢清淡的小鱼，谢谢你陪我',at:1}
const output={reply:'清淡的小鱼，记住啦。',topic:'小鱼口味',analyses:[{sourceId:'u1',memories:[{kind:'preference',key:'饮食',text:'喜欢清淡的小鱼',quote:'我喜欢清淡的小鱼'}],delta:2,reason:'受到感谢',quote:'谢谢你陪我'}]}
function mock(value=output,{tool=false,fail=false,early=false}={}){
  const calls=[],listeners=new Set()
  const rpc={calls,listeners,notify(){},close(){this.closed=true},async request(method,params){
    calls.push({method,params})
    if(fail)throw Error('连接失败')
    if(method==='config/read')return {config:{mcp_servers:{privateServer:{token:'never-copy'}}}}
    if(method==='thread/start')return {thread:{id:'ephemeral'}}
    if(method==='thread/list')return {data:[{id:'old',name:'历史聊天'}]}
    if(method==='thread/read')return {thread:{id:'old',updatedAt:10}}
    if(method==='thread/turns/list')return {data:[{startedAt:9,items:[{id:'u',type:'userMessage',content:[{type:'text',text:'我喜欢小鱼'}]},{id:'tool',type:'commandExecution',text:'secret'},{id:'thinking',type:'agentMessage',phase:'commentary',text:'hidden'},{id:'a',type:'agentMessage',phase:'final_answer',text:'好呀'}]}]}
    if(method==='turn/start'){
      const deliver=()=>{for(const cb of listeners){if(tool)cb({method:'item/started',params:{threadId:'ephemeral',item:{type:'commandExecution'}}});else cb({method:'item/completed',params:{threadId:'ephemeral',item:{type:'agentMessage',phase:'final_answer',text:JSON.stringify(value)}}});cb({method:'turn/completed',params:{threadId:'ephemeral',turn:{status:'completed'}}})}}
      if(early){deliver();await new Promise(resolve=>setTimeout(resolve,5))}else queueMicrotask(deliver)
    }
    return {}
  }}
  return rpc
}
test('bubble reply, learning and opener use ephemeral inference and preserve selected model defaults',async()=>{
  for(const action of ['reply','analysis','opener']){
    const rpc=mock(action==='opener'?{...output,analyses:[]}:output)
    await run({action,sources:action==='opener'?[]:[source],document:{},prompt:'小鱼'}, {factory:()=>rpc})
    const start=rpc.calls.find(x=>x.method==='thread/start').params
    assert.equal(start.ephemeral,true);assert.equal(start.approvalPolicy,'never');assert.equal(start.sandbox,'read-only')
    assert.equal(start.model,'gpt-6.1-sol');assert.deepEqual(start.config.mcp_servers,{privateServer:{enabled:false}})
    assert.ok(!JSON.stringify(rpc.calls).includes('never-copy'))
    assert.ok(!rpc.calls.some(x=>x.method==='thread/resume'));assert.equal(rpc.closed,true)
  }
})
test('read-only history import never starts or resumes a thread and excludes tools and commentary',async()=>{
  const rpc=mock();const result=await run({action:'read',threadId:'old'},{factory:()=>rpc})
  assert.deepEqual(result.messages.map(x=>x.role),['user','assistant'])
  assert.equal(result.messages[0].id,visibleMessage({session_id:'old',turn_id:'',hook_event_name:'UserPromptSubmit',prompt:'我喜欢小鱼'}).id)
  assert.ok(rpc.calls.every(x=>!['thread/start','thread/resume','turn/start'].includes(x.method)))
  assert.equal((await run({action:'sessions'},{factory:()=>mock()})).sessions.length,1)
})
test('learning rejects fabricated sources, unsupported facts and mood without evidence',()=>{
  assert.equal(validateResult(output,[source]).analyses[0].delta,2)
  for(const edit of [x=>x.analyses[0].sourceId='missing',x=>x.analyses[0].memories[0].quote='不存在的原话',x=>x.analyses[0].quote='',x=>x.analyses[0].delta=4]){
    const invalid=structuredClone(output);edit(invalid);assert.throws(()=>validateResult(invalid,[source]))
  }
  assert.throws(()=>validateResult({...output,analyses:[output.analyses[0],output.analyses[0]]},[source]))
})
test('recall prioritizes relevant preferences without filling unrelated slots or reviving tombstones',()=>{
  const at=20*86400000
  const memories=[{text:'【喜好】清淡小鱼',at:1},{text:'【近况】过期事情',at:1},{text:'已遗忘',at,deleted:true},...Array.from({length:20},(_,n)=>({text:'普通记录'+n,at}))]
  const result=contextFor({memories},'小鱼',at)
  assert.match(result.memories[0].text,/小鱼/);assert.ok(!JSON.stringify(result).includes('已遗忘'));assert.ok(!JSON.stringify(result).includes('过期事情'));assert.equal(result.memories.length,1)
})
test('tool attempts, early notification failures and connection errors stop generation',async()=>{
  await assert.rejects(run({action:'reply',sources:[source]},{factory:()=>mock(output,{tool:true,early:true})}),/调用工具/)
  await assert.rejects(run({action:'reply'},{factory:()=>mock(output,{fail:true})}),/连接失败/)
})
test('RPC timeout terminates a silent process and rejects pending requests',async()=>{
  const rpc=new Rpc(process.execPath,['-e','setInterval(()=>{},1000)'],{timeout:40})
  await assert.rejects(rpc.request('initialize'),/超时/);assert.equal(rpc.closed,true)
})
test('background history requires every queued source to be acknowledged, including empty findings',async()=>{
  await assert.rejects(run({action:'analysis',sources:[source]},{factory:()=>mock({...output,analyses:[]})}),/未完整整理/)
  const empty={...output,analyses:[{sourceId:source.id,memories:[],delta:0,reason:'',quote:''}]}
  assert.equal((await run({action:'analysis',sources:[source]},{factory:()=>mock(empty)})).analyses.length,1)
})
