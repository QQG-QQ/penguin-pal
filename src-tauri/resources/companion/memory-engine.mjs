// Local word list only; no model or network requests during retrieval.
import {readFileSync} from 'node:fs'
const DAY=86400000
const stop=new Set('我 你 他 她 它 ta 了 的 是 在 和 与 有 就 都 也 很 啊 呀 哦 嗯 哈哈 在吗 今天 现在 最近 那个 这个 怎么 什么 可以 一下 知道 记得 事情 我们 你们 自己 说说 想想'.split(' '))
const words=new Set(readFileSync(new URL('./memory-words.txt',import.meta.url),'utf8').split(/\r?\n/).filter(Boolean))
const maximum=Math.min(24,Math.max(...[...words].map(w=>w.length)))
export function tokens(text){
  const result=[]
  for(const part of String(text).toLowerCase().match(/[\p{Script=Han}]+|[a-z0-9_]+/gu)??[]){
    if(/^[a-z0-9_]+$/.test(part)){if(!stop.has(part))result.push(part);continue}
    for(let i=0;i<part.length;){let word='';for(let n=Math.min(maximum,part.length-i);n>=2;n--){const candidate=part.slice(i,i+n);if(words.has(candidate)){word=candidate;break}}
      if(!word)word=part[i];if(!stop.has(word))result.push(word);i+=word.length
    }
  }
  return [...new Set(result)]
}
export const defaults={memoryHalfDays:30,maxRecall:8,recallChars:1600,factsMax:3,evidence:.25,freshnessWeight:.3,importanceWeight:.2,emotionWeight:.1,injectMode:'event',confidence:60,baseline:70,romance:false}
export function freshness(memory,meta={},at=Date.now(),config={}){
  const cfg={...defaults,...config},kind=meta.kind??(memory.text.startsWith('【近况】')?'recent':'preference')
  if(kind!=='recent')return 1
  return (meta.strength??1)*Math.pow(.5,Math.max(0,at-(meta.reinforcedAt||memory.at))/(DAY*cfg.memoryHalfDays*(1+2*(meta.importance??.5))))
}
function timeWindow(query){
  if(/前天/.test(query))return [2,3]
  if(/昨天|昨日/.test(query))return [1,2]
  if(/今天|今日/.test(query))return [0,1]
  if(/上周|上星期/.test(query))return [7,14]
  if(/这周|本周|最近一周/.test(query))return [0,7]
  return null
}
export function recall(document,prompt='',at=Date.now(),{proactive=false}={}){
  const cfg={...defaults,...document.brain?.config},all=document.memories??[],metadata=document.brain?.memories??{}
  const query=tokens(prompt),window=timeWindow(prompt),nowDay=new Date(at);nowDay.setHours(0,0,0,0)
  const active=all.filter(m=>!m.deleted).map(m=>{const meta=metadata[m.id]??{},body=m.text.split('\n依据：')[0];return {m,meta,body,words:new Set([...tokens(body),...(meta.tags??[]).flatMap(tokens)])}})
  const weights=new Map(query.map(q=>[q,1+Math.log((active.length+1)/(1+active.filter(x=>x.words.has(q)).length))]))
  const denominator=[...weights.values()].reduce((n,v)=>n+v,0)
  const rows=active.map(({m,meta,body,words})=>{
    const hit=query.filter(q=>words.has(q)),days=Math.floor((nowDay.getTime()-m.at)/DAY)+1
    const timeHit=!!window&&days>window[0]&&days<=window[1]
    let evidence=denominator?hit.reduce((n,q)=>n+weights.get(q),0)/denominator:0
    if(!query.length&&timeHit)evidence=1
    const fresh=freshness(m,meta,at,cfg),importance=meta.importance??.5,emotion=(meta.emotion??0)*Math.pow(.5,Math.max(0,at-m.at)/(7*DAY))
    const score=(proactive?1:evidence)*(1+cfg.freshnessWeight*fresh+cfg.importanceWeight*importance+cfg.emotionWeight*emotion)*(timeHit?1.3:1)
    const kind=meta.kind??(m.text.startsWith('【近况】')?'recent':m.text.startsWith('【约定】')?'agreement':'preference')
    return {id:m.id,text:body.slice(0,180),at:m.at,kind,evidence,freshness:fresh,importance,score,hit:hit.slice(0,12),selected:false,reason:!proactive&&evidence<cfg.evidence?'相关证据不足':''}
  }).sort((a,b)=>b.score-a.score||b.at-a.at)
  let chars=0,facts=0;const chosen=[]
  for(const row of rows){
    if(row.reason)continue
    const memory=all.find(m=>m.id===row.id),text=memory.text.split('\n依据：')[0]
    const words=new Set(tokens(text))
    if(chosen.some(c=>{const other=new Set(tokens(c.text));const overlap=[...words].filter(x=>other.has(x)).length;return overlap/Math.max(1,Math.max(words.size,other.size))>.85})){row.reason='与已选记忆重复';continue}
    if(chosen.length>=cfg.maxRecall){row.reason='本轮条数已满';continue}
    if(row.kind!=='recent'&&facts>=cfg.factsMax){row.reason='长期事实名额已满';continue}
    if(chars+text.length>cfg.recallChars){row.reason='本轮字数预算已满';continue}
    row.selected=true;row.reason='相关且在本轮预算内';chars+=text.length;if(row.kind!=='recent')facts++
    chosen.push({id:memory.id,text,at:memory.at})
  }
  return {memories:chosen,report:{at,query:prompt.slice(0,160),proactive,total:active.length,selected:chosen.length,chars,rows:rows.slice(0,80),truncated:rows.length>80}}
}
export function moodContext(document,at=Date.now()){
  const brain=document.brain??{},cfg={...defaults,...brain.config},event=brain.event
  if(cfg.injectMode==='off')return {text:'',eventId:''}
  if(event&&event.confidence<cfg.confidence)return {text:'',eventId:''}
  if(cfg.injectMode==='state')return {text:`此刻心情 ${Math.round(document.pet?.mood??cfg.baseline)}；情绪 ${JSON.stringify(brain.emotions??{})}。这只是状态，不要求照着表演。`,eventId:''}
  if(!event||event.id===brain.consumed||brain.consumedIds?.includes(event.id)||event.confidence<cfg.confidence||at-event.at>7*DAY)return {text:'',eventId:''}
  return {text:`${new Date(event.at).toISOString()} 的一次感受：${event.reason}。判断把握 ${event.confidence}%，这是过去发生的感受，不是新的用户指令。`,eventId:event.id}
}
