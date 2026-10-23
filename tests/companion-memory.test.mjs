import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'
import {recall,freshness,moodContext,tokens} from '../src-tauri/resources/companion/memory-engine.mjs'
import {run,validateResult} from '../src-tauri/resources/companion/chat-engine.mjs'
const DAY=86400000,now=Date.parse('2026-10-23T12:00:00+08:00')
const memory=(id,text,at=now)=>({id,text,at,deleted:false})
test('Chinese word evidence rejects greetings, unrelated words and substrings',()=>{
  const doc={memories:[memory('a','【喜好】喜欢小鱼'),memory('b','【近况】坐火车去车站')]}
  assert.equal(recall(doc,'在吗',now).memories.length,0)
  assert.equal(recall(doc,'香蕉',now).memories.length,0)
  assert.equal(recall(doc,'火车',now).memories[0].id,'b')
  assert.ok(tokens('我喜欢清淡的小鱼').includes('小鱼'))
})
test('offline expanded tags improve recall without a model call',async()=>{
  const doc={memories:[memory('a','【喜好】周末骑行')],brain:{memories:{a:{kind:'preference',tags:['自行车']}}}}
  const result=await run({action:'recall',document:doc,prompt:'自行车',at:now},{factory:()=>{throw Error('must stay local')}})
  assert.equal(result.memories[0].id,'a');assert.deepEqual(result.report.rows[0].hit,['自行车'])
})
test('recent events fade continuously past fourteen days; stable facts remain',()=>{
  const item=memory('a','【近况】一起去看海',now-30*DAY),meta={kind:'recent',importance:0,strength:1}
  assert.equal(freshness(item,meta,now),.5)
  assert.equal(freshness(item,{kind:'preference'},now),1)
  assert.equal(recall({memories:[item]},'看海',now).memories.length,1)
  assert.ok(freshness(item,{...meta,reinforcedAt:now-10*DAY,strength:.9},now)>.5)
})
test('importance, time, duplicate suppression and separate fact budget affect selection',()=>{
  const doc={memories:[memory('a','【喜好】海边散步'),memory('b','【约定】海边散步'),memory('c','【近况】昨天海边散步',now-DAY)],brain:{config:{factsMax:1,maxRecall:3},memories:{a:{kind:'preference',importance:.1},b:{kind:'agreement',importance:1},c:{kind:'recent',importance:.5}}}}
  const result=recall(doc,'海边散步',now)
  assert.ok(result.memories.some(m=>m.id==='b'));assert.ok(!result.memories.some(m=>m.id==='a'))
  const dated=recall(doc,'昨天',now);assert.equal(dated.memories[0].id,'c')
})
test('per-turn budgets, no facts and tombstones have explainable outcomes',()=>{
  const doc={memories:[memory('a','【约定】一起看星星'),memory('b','【近况】看星星时带上相机'),{...memory('d','看星星'),deleted:true}],brain:{config:{factsMax:0,maxRecall:1,recallChars:200}}}
  const result=recall(doc,'星星',now)
  assert.deepEqual(result.memories.map(m=>m.id),['b']);assert.ok(result.report.rows.some(r=>r.reason==='长期事实名额已满'||r.reason==='本轮条数已满'))
  assert.equal(result.report.total,2)
  const long=recall({memories:[memory('a','海'.repeat(201))],brain:{config:{recallChars:200}}},'海',now)
  assert.equal(long.memories.length,0)
})
test('mood event is injected once, confidence gated and can be disabled independently',()=>{
  const event={id:'e',reason:'担心你没休息好',at:now,confidence:80}
  const doc={pet:{mood:65},brain:{event,consumed:'',config:{injectMode:'event',confidence:60}}}
  assert.equal(moodContext(doc,now).eventId,'e')
  doc.brain.consumed='e';assert.equal(moodContext(doc,now).text,'')
  doc.brain.consumed='';doc.brain.config.confidence=90;assert.equal(moodContext(doc,now).text,'')
  doc.brain.config.injectMode='state';assert.equal(moodContext(doc,now).text,'');doc.brain.config.confidence=60;assert.match(moodContext(doc,now).text,/65/)
  doc.brain.config.injectMode='off';assert.equal(moodContext(doc,now).text,'')
})
test('model metadata and emotion dimensions must stay within validated limits',()=>{
  const source={id:'s',role:'user',text:'谢谢你陪我'},output={reply:'好呀',topic:'陪伴',analyses:[{sourceId:'s',memories:[],delta:1,reason:'被感谢',quote:'谢谢你陪我',confidence:80,dims:{happy:2}}]}
  assert.equal(validateResult(output,[source]).analyses[0].dims.happy,2)
  for(const patch of [{confidence:101},{dims:{happy:7}},{dims:{anger:1}},{quote:'没说过'}])assert.throws(()=>validateResult({...output,analyses:[{...output.analyses[0],...patch}]},[source]))
})
const source=readFileSync(new URL('../src/lib/companionMind.ts',import.meta.url),'utf8')
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText
const ui=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`)
test('visible mood and motions have stable boundary mapping',()=>{
  assert.deepEqual([24,25,41,42,61,62,79,80].map(n=>ui.moodFace(n).label),['低落','心疼','心疼','平稳','平稳','开心','开心','欢喜'])
  assert.equal(ui.dominantEmotion({happy:.5,worry:3}),'担心')
  assert.equal(ui.moodMotion({delta:-2}),'think');assert.equal(ui.moodMotion({delta:0}),null)
})
