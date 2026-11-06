// Local-only image workflow client. Model output is data, never a tool or command.
import {randomBytes} from 'node:crypto'
import {readFile} from 'node:fs/promises'
import {realpathSync} from 'node:fs'
import {pathToFileURL} from 'node:url'

export const visualKinds=['none','sticker','mood_card','scene']
export const visualSchema={type:'object',properties:{kind:{type:'string',enum:visualKinds},description:{type:'string'},caption:{type:'string'},reuseId:{type:'string'}},required:['kind','description','caption','reuseId'],additionalProperties:false}
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)
const text=(x,n)=>typeof x==='string'?x.trim().slice(0,n):''
export function validateVisual(value){
  if(!plain(value)||!visualKinds.includes(value.kind))return {kind:'none',description:'',caption:'',reuseId:''}
  return {kind:value.kind,description:text(value.description,600),caption:text(value.caption,60),reuseId:/^[a-zA-Z0-9-]{1,80}$/.test(value.reuseId??'')?value.reuseId:''}
}
export function localBase(value){
  let url;try{url=new URL(value)}catch{throw Error('ComfyUI 地址无效')}
  if(!['http:','https:'].includes(url.protocol)||url.username||url.password||url.search||url.hash||!['127.0.0.1','localhost','[::1]'].includes(url.hostname))throw Error('此版本只连接本机 ComfyUI，请使用 localhost 或 127.0.0.1')
  if(!url.pathname.endsWith('/'))url.pathname+='/'
  return url
}
export function normalizeSettings(value={}){
  const baseUrl=String(value.baseUrl??'http://127.0.0.1:8188').trim()
  localBase(baseUrl)
  const workflow=String(value.workflow??'')
  if(Buffer.byteLength(workflow)>2*1024*1024)throw Error('工作流最多 2 MiB')
  if(workflow.trim())prepareWorkflow(workflow,'测试',1,'reference.png',true)
  const integer=(key,fallback,min,max)=>{const v=value[key]??fallback;if(!Number.isInteger(v)||v<min||v>max)throw Error('请检查频率和超时设置');return v}
  return {enabled:value.enabled===true,automatic:value.automatic!==false,managedRuntime:value.managedRuntime===true,baseUrl,workflow,
    cooldownMinutes:integer('cooldownMinutes',30,1,1440),dailyLimit:integer('dailyLimit',4,1,20),
    timeoutSeconds:integer('timeoutSeconds',180,10,600),wechatEnabled:value.wechatEnabled===true,
    character:text(value.character??'expressive everyday chat reaction meme, freely choose a funny animal, cartoon face or simple everyday subject to match the conversation',600)}
}
export function visualContext(settings,document,catalog=[]){
  return {enabled:settings.enabled,automatic:settings.automatic,personality:document.pet?.personality??'gentle',mood:document.pet?.mood??70,
    catalog:catalog.slice(-20).map(x=>({id:x.id,kind:x.kind,caption:x.caption,emotion:x.emotion})),
    instruction:'根据企鹅当前心情、性格和这轮对话选择图片表达。普通工作问答通常只用文字；俏皮性格可活泼些，安静性格少发表情。合适时用 sticker、mood_card 或 scene。优先从 catalog 挑合适的 reuseId；新图 description 描述普通聊天表情的主体、动作和画面，不要求企鹅或桌宠形象；只有用户明确要求画桌宠时才用桌宠形象，不抄入私密记忆或账号资料。caption 简短自然。没有必要或 enabled=false 时 kind=none。不要承诺已画好，实际图片由后台完成后附上。'}
}
export function explicitVisual(prompt){
  if(!/(给我画|帮我画|画一张|画张|来张|做个.*表情|生成.*(?:图片|配图|表情|心情卡)|画.*(?:表情包|心情卡|小剧场))/.test(prompt??''))return null
  return {kind:/心情卡/.test(prompt)?'mood_card':/小剧场|配图/.test(prompt)?'scene':'sticker',description:text(prompt,600),caption:'',reuseId:''}
}
export function planVisual({settings,document={},visual,prompt='',catalog=[],now=Date.now()}){
  const manual=explicitVisual(prompt),intent=validateVisual(visual)
  const request=manual?{...manual,description:intent.description||manual.description,caption:intent.caption,reuseId:intent.reuseId}:intent
  if(!settings.enabled||request.kind==='none'||(!manual&&!settings.automatic))return null
  const mood=Number(document.pet?.mood??70),emotion=mood>=80?'happy':mood<42?'sad':'calm'
  const personality=document.pet?.personality??'gentle'
  const recent=catalog.filter(x=>x.auto&&now-x.at<86400000)
  const last=recent.at(-1)
  if(!manual&&(recent.length>=settings.dailyLimit*3||(last&&now-last.at<settings.cooldownMinutes*60000)))return null
  const preferred=catalog.find(x=>x.id===request.reuseId&&x.kind===request.kind)
  const cached=preferred??(!manual&&request.kind==='sticker'?catalog.findLast(x=>x.kind==='sticker'&&x.emotion===emotion&&x.personality===personality&&x.file!==catalog.at(-1)?.file):null)
  if(cached)return {...request,reuseId:cached.id,emotion,personality,auto:!manual,prompt:'',mood}
  if(!manual&&recent.filter(x=>!x.reused).length>=settings.dailyLimit)return null
  if(!settings.workflow.trim()){if(manual)throw Error('请先配置本机 ComfyUI 和 API 工作流');return null}
  const date=new Date(now).toLocaleDateString('zh-CN')
  const style={gentle:'gentle, soft gestures, restrained expression',playful:'playful, lively, naturally humorous',quiet:'quiet, subtle expression, simple composition',warm:'warm, friendly, comforting'}[personality]??'friendly and natural'
  const kind={sticker:'chat sticker, single clear gesture, simple background, no text or watermark',mood_card:'daily mood illustration card, clean space for a caption, no text',scene:'one complete miniature story scene, expressive subjects, no text'}[request.kind]
  return {...request,reuseId:'',emotion,personality,auto:!manual,mood,
    prompt:[settings.character,kind,style,'mood '+Math.round(mood)+'/100 ('+emotion+')',
      request.kind==='mood_card'?'date '+date:'',request.description].filter(Boolean).join('. ')}
}
export function prepareWorkflow(raw,prompt,seed,image,validateOnly=false){
  let graph;try{graph=JSON.parse(raw)}catch{throw Error('工作流不是有效 JSON')}
  if(!plain(graph)||!Object.keys(graph).length||Array.isArray(graph.nodes)||!Object.values(graph).every(x=>plain(x)&&typeof x.class_type==='string'&&plain(x.inputs)))throw Error('请导入 ComfyUI 导出的 API 格式工作流')
  let count=0,images=0
  const inject=x=>{
  if(typeof x==='string'){if(x.includes('{{prompt}}')){count++;x=x.replaceAll('{{prompt}}',()=>prompt)}if(x==='{{seed}}')return seed;return x.replaceAll('{{seed}}',String(seed))}
    if(Array.isArray(x))return x.map(inject)
    if(plain(x))return Object.fromEntries(Object.entries(x).map(([k,v])=>[k,inject(v)]))
    return x
  }
  for(const node of Object.values(graph)){
    node.inputs=inject(node.inputs)
    if(node.inputs.image==='{{image}}'){
      images++;if(!image&&!validateOnly)throw Error('这个工作流需要角色参考图，请先上传参考图')
      node.inputs.image=image??'reference.png'
    }
  }
  if(!count)throw Error('请将工作流的正向提示词设为 {{prompt}}')
  if(images>1)throw Error('一个工作流最多使用一个 {{image}} 参考图输入')
  return graph
}
export function imageType(bytes){
  if(bytes.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10])))return 'image/png'
  if(bytes[0]===255&&bytes[1]===216&&bytes[2]===255)return 'image/jpeg'
  if(bytes.subarray(0,4).toString()==='RIFF'&&bytes.subarray(8,12).toString()==='WEBP')return 'image/webp'
  throw Error('只支持 PNG、JPEG 或 WebP 图片')
}
async function bounded(response,max){
  if(Number(response.headers.get('content-length'))>max)throw Error('服务响应超过大小限制')
  const chunks=[];let size=0
  for await(const chunk of response.body??[]){size+=chunk.length;if(size>max)throw Error('服务响应超过大小限制');chunks.push(chunk)}
  return Buffer.concat(chunks)
}
function relativeFile(name,folder=''){
  if(typeof name!=='string'||!name||/[\\/\x00-\x1f]/.test(name)||name==='.'||name==='..'||typeof folder!=='string'||folder.startsWith('/')||folder.includes('\\')||folder.split('/').some(x=>x==='..'||x==='.')||/[\x00-\x1f:]/.test(folder))throw Error('ComfyUI 返回了无效图片路径')
  return folder?folder+'/'+name:name
}
export async function generateImage({settings,prompt,reference,cancelFile},{fetchImpl=fetch,pollMs=500}={}){
  const base=localBase(settings.baseUrl),controller=new AbortController()
  let cancelled=false,promptId=''
  const timer=setTimeout(()=>controller.abort(),settings.timeoutSeconds*1000)
  const cancellation=cancelFile?setInterval(async()=>{try{await readFile(cancelFile);cancelled=true;controller.abort()}catch{}},200):null
  const request=async(path,options={})=>{
    const response=await fetchImpl(new URL(path,base),{...options,signal:controller.signal,redirect:'error'})
    if(!response.ok)throw Error('ComfyUI 请求失败（HTTP '+response.status+'）')
    return response
  }
  const json=async(path,options)=>JSON.parse((await bounded(await request(path,options),2*1024*1024)).toString('utf8'))
  try{
    let input
    if(reference&&settings.workflow.includes('{{image}}')){
      const bytes=Buffer.from(reference,'base64');if(bytes.length>8*1024*1024)throw Error('角色参考图最多 8 MiB')
      const type=imageType(bytes),ext=type.split('/')[1],form=new FormData()
      form.append('image',new Blob([bytes],{type}),'penguin-reference.'+ext);form.append('overwrite','false')
      const result=await json('upload/image',{method:'POST',body:form})
      input=relativeFile(result.name,result.subfolder??'')
    }
    const graph=prepareWorkflow(settings.workflow,prompt,randomBytes(4).readUInt32BE(0),input)
    const submitted=await json('prompt',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({prompt:graph})})
    promptId=submitted.prompt_id
    if(typeof promptId!=='string'||!promptId||promptId.length>100)throw Error('ComfyUI 未返回有效任务标识')
    for(;;){
      const history=await json('history/'+encodeURIComponent(promptId)),entry=history[promptId]
      if(entry?.status?.status_str==='error')throw Error('ComfyUI 工作流执行失败，请检查模型和节点')
      const output=Object.values(entry?.outputs??{}).flatMap(x=>x.images??[]).find(x=>x.type==='output'&&typeof x.filename==='string')
      if(output){
        relativeFile(output.filename,output.subfolder??'')
        const query=new URLSearchParams({filename:output.filename,subfolder:String(output.subfolder??''),type:'output'})
        const bytes=await bounded(await request('view?'+query),8*1024*1024)
        return {bytes:bytes.toString('base64'),mediaType:imageType(bytes),promptId}
      }
      if(entry?.status?.completed)throw Error('工作流已完成，但没有可保存的图片')
      await new Promise(resolve=>setTimeout(resolve,pollMs))
      controller.signal.throwIfAborted()
    }
  }catch(error){
    if(promptId)try{await fetchImpl(new URL('queue',base),{method:'POST',redirect:'error',signal:AbortSignal.timeout(3000),headers:{'content-type':'application/json'},body:JSON.stringify({delete:[promptId]})})}catch{}
    if(controller.signal.aborted)throw Error(cancelled?'已取消绘图；已开始的 ComfyUI 计算可能仍会继续':'本地绘图等待超时，文字回复已保留')
    throw error
  }finally{clearTimeout(timer);if(cancellation)clearInterval(cancellation)}
}
export async function visualAction(request){
  if(request.action==='normalize')return normalizeSettings(request.settings)
  if(request.action==='plan')return planVisual(request)
  if(request.action==='generate')return generateImage(request)
  if(request.action==='check'){
    const response=await fetch(new URL('system_stats',localBase(request.settings.baseUrl)),{signal:AbortSignal.timeout(5000),redirect:'error'})
    if(!response.ok)throw Error('ComfyUI 尚未就绪')
    const value=JSON.parse((await bounded(response,262144)).toString())
    let models=[]
    try{
      const info=await fetch(new URL('object_info/CheckpointLoaderSimple',localBase(request.settings.baseUrl)),{signal:AbortSignal.timeout(5000),redirect:'error'})
      if(info.ok){const data=JSON.parse((await bounded(info,2*1024*1024)).toString());models=data.CheckpointLoaderSimple?.input?.required?.ckpt_name?.[0]??[]}
    }catch{/* Connection check remains useful for custom workflows without checkpoints. */}
    return {ok:true,devices:(value.devices??[]).map(x=>({name:text(x.name,100),type:text(x.type,30)})),models:Array.isArray(models)?models.filter(x=>typeof x==='string').slice(0,200):[]}
  }
  throw Error('未知绘图操作')
}
if(process.argv[1]&&import.meta.url===pathToFileURL(realpathSync(process.argv[1])).href){
  const chunks=[];let size=0;try{
    for await(const chunk of process.stdin){size+=chunk.length;if(size>24*1024*1024)throw Error('请求过大');chunks.push(chunk)}
    const input=Buffer.concat(chunks).toString('utf8')
    process.stdout.write(JSON.stringify(await visualAction(JSON.parse(input))))
  }catch(error){process.stdout.write(JSON.stringify({error:String(error.message??'绘图失败').slice(0,240)}));process.exitCode=1}
}
