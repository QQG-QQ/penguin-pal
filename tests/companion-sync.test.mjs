import test from 'node:test'
import assert from 'node:assert/strict'
import {mkdtemp,mkdir,readFile,readdir,rm,writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {capture,visibleMessage,redact} from '../src-tauri/resources/companion/conversation-events.mjs'

test('visible messages exclude tools and subagents, redact secrets and deduplicate hook retries',async()=>{
  const user={hook_event_name:'UserPromptSubmit',session_id:'s',turn_id:'t',prompt:'记得 password=example-secret'}
  assert.equal(visibleMessage({...user,agent_id:'child'}),null)
  assert.equal(visibleMessage({...user,hook_event_name:'PostToolUse',tool_response:'private'}),null)
  assert.equal(visibleMessage(user,1).id,visibleMessage(user,2).id)
  assert.notEqual(visibleMessage(user,1).id,visibleMessage({...user,turn_id:'t2'},1).id)
  assert.ok(!visibleMessage(user).text.includes('example-secret'))
  assert.equal(redact('-----BEGIN PRIVATE KEY-----\nprivate\n'), '[凭据已移除]')
  const root=await mkdtemp(join(tmpdir(),'penguin-capture-'))
  try {
    await capture(root,user)
    assert.deepEqual(await readdir(root),[])
    await mkdir(join(root,'sync'));await writeFile(join(root,'sync/local.json'),'{"capture":true}')
    await capture(root,user);await capture(root,user)
    assert.equal((await readdir(join(root,'conversations'))).length,1)
    const files=await readdir(join(root,'conversations'))
    const saved=await readFile(join(root,'conversations',files[0]),'utf8')
    assert.ok(!saved.includes('example-secret'));assert.ok(!saved.includes('tool_response'))
  } finally {await rm(root,{recursive:true,force:true})}
})

test('bounded recall uses data context only when past conversation is relevant',async()=>{
  const root=await mkdtemp(join(tmpdir(),'penguin-recall-'))
  try {
    await mkdir(join(root,'sync'))
    await writeFile(join(root,'sync/local.json'),JSON.stringify({capture:true,server:{username:'private-user'},document:{schema:1,updatedAt:10,pet:{mood:75},
      memories:[{text:'用户喜欢安静',deleted:false},{text:'不要复活',deleted:true}],
      conversations:[{session:'aabbccdd',role:'assistant',text:'昨天约定去散步',at:9}]}}))
    const input={hook_event_name:'UserPromptSubmit',session_id:'s',turn_id:'t',prompt:'刚才聊过什么'}
    const result=await capture(root,input)
    assert.match(result.hookSpecificOutput.additionalContext,/昨天约定去散步/)
    assert.ok(!result.hookSpecificOutput.additionalContext.includes('不要复活'))
    assert.ok(!result.hookSpecificOutput.additionalContext.includes('private-user'))
    assert.equal(await capture(root,{...input,turn_id:'t2',prompt:'编译工程'}),undefined)
  } finally {await rm(root,{recursive:true,force:true})}
})

test('older relevant preferences are recalled ahead of unrelated recent notes',async()=>{
  const root=await mkdtemp(join(tmpdir(),'penguin-relevance-'))
  try {
    await mkdir(join(root,'sync'))
    const memories=[{text:'我喜欢清淡的小鱼',deleted:false},...Array.from({length:20},(_,n)=>({text:'桌面记录 '+n,deleted:false}))]
    await writeFile(join(root,'sync/local.json'),JSON.stringify({capture:true,document:{schema:1,updatedAt:1,pet:{mood:70},memories,conversations:[]}}))
    const result=await capture(root,{hook_event_name:'UserPromptSubmit',session_id:'s',turn_id:'t',prompt:'你还记得我喜欢什么口味的小鱼吗'})
    assert.match(result.hookSpecificOutput.additionalContext,/清淡的小鱼/)
  }finally{await rm(root,{recursive:true,force:true})}
})
