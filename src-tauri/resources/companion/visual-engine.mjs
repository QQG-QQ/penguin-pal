// Local-only image workflow client. Model output is data, never a tool or command.
import {randomBytes,createHash} from 'node:crypto'
import {readFile} from 'node:fs/promises'
import {realpathSync} from 'node:fs'
import {pathToFileURL} from 'node:url'

export const visualKinds=['none','sticker','mood_card','scene']
export const visualSchema={type:'object',properties:{kind:{type:'string',enum:visualKinds},description:{type:'string'},caption:{type:'string'},reuseId:{type:'string'},useReference:{type:'boolean'}},required:['kind','description','caption','reuseId','useReference'],additionalProperties:false}
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)
const text=(x,n)=>typeof x==='string'?x.trim().slice(0,n):''
export function validateVisual(value){
  if(!plain(value)||!visualKinds.includes(value.kind))return {kind:'none',description:'',caption:'',reuseId:'',useReference:false}
  return {kind:value.kind,description:text(value.description,600),caption:text(value.caption,60),reuseId:/^[a-zA-Z0-9-]{1,80}$/.test(value.reuseId??'')?value.reuseId:'',useReference:value.useReference===true}
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
  const referenceDenoise=value.referenceDenoise??0.2
  if(!Number.isFinite(referenceDenoise)||referenceDenoise<0.1||referenceDenoise>0.7)throw Error('参考图重绘幅度应在 0.1 到 0.7 之间')
  return {enabled:value.enabled===true,automatic:value.automatic!==false,managedRuntime:value.managedRuntime===true,baseUrl,workflow,
    referenceDenoise,referenceStyle:text(value.referenceStyle,600),referenceNegative:text(value.referenceNegative,600),
    cooldownMinutes:integer('cooldownMinutes',30,1,1440),dailyLimit:integer('dailyLimit',4,1,20),
    timeoutSeconds:integer('timeoutSeconds',180,10,600),wechatEnabled:value.wechatEnabled===true,
    character:text(value.character??'2D chibi anime reaction sticker, crisp dark contour lines, simple flat color fills, cel animation frame, oversized head and tiny body, exaggerated readable comic expression, one animal or cartoon subject, plain white background, no collage, no text or watermark',600)}
}
export function visualContext(settings,document,catalog=[]){
  return {enabled:settings.enabled,automatic:settings.automatic,personality:document.pet?.personality??'gentle',mood:document.pet?.mood??70,
    hasReference:settings.hasReference===true,style:text(settings.character,600),catalog:catalog.slice(-20).map(x=>({id:x.id,kind:x.kind,caption:x.caption,emotion:x.emotion,useReference:x.useReference===true})),
    instruction:'根据企鹅当前心情、性格和这轮对话选择图片表达。普通工作问答通常只用文字；俏皮性格可活泼些，安静性格少发表情。合适时用 sticker、mood_card 或 scene。优先从 catalog 挑合适的 reuseId；新图 description 描述普通聊天表情的主体、动作和画面，不要求企鹅或桌宠形象；只有用户明确要求画桌宠时才用桌宠形象，不抄入私密记忆或账号资料。caption 简短自然。没有必要或 enabled=false 时 kind=none。不要承诺已画好，实际图片由后台完成后附上。'}
}
export function explicitVisual(prompt){
  if(!/(给我画|帮我画|画一张|画张|来张|做个.*表情|生成.*(?:图片|配图|表情|心情卡|一[张个幅]图)|画.*(?:表情包|心情卡|小剧场|桌宠|你自己)|参考图.*(?:画|生成))/.test(prompt??''))return null
  return {kind:/心情卡/.test(prompt)?'mood_card':/小剧场|配图/.test(prompt)?'scene':'sticker',description:text(prompt,600),caption:'',reuseId:''}
}
export function referenceDirective(prompt,petName=''){
  if(/(?:不要|不用|别|无需|不使用|不需要).{0,8}(?:参考图|桌宠|画你|画自己)/.test(prompt))return false
  const escapedName=petName.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')
  if(/参考图|桌宠|画(?:一[张个幅])?(?:你|自己)/.test(prompt)||(escapedName&&new RegExp('画.{0,4}'+escapedName+'|'+escapedName+'的.{0,6}(?:表情|图)').test(prompt)))return true
  return null
}
export function planVisual({settings,document={},visual,prompt='',catalog=[],referenceKey='',now=Date.now()}){
  const manual=explicitVisual(prompt),intent=validateVisual(visual)
  const request=manual?{...manual,description:intent.description||manual.description,caption:intent.caption,reuseId:intent.reuseId}:intent
  if(!settings.enabled||request.kind==='none'||(!manual&&!settings.automatic))return null
  const directive=manual?referenceDirective(prompt,document.brain?.config?.petName??''):null
  const useReference=(directive??intent.useReference)||settings.workflow.includes('{{image}}')
  if(useReference&&!referenceKey){if(manual)throw Error('请先上传桌宠参考图，再让我按参考图画');return null}
  const referenceProfile=useReference?createHash('sha256').update(JSON.stringify([settings.referenceDenoise??0.2,text(settings.referenceStyle,600),text(settings.referenceNegative,600)])).digest('hex'):''
  const styleKey=createHash('sha256').update(JSON.stringify([settings.workflow,useReference?'':settings.character])).digest('hex')
  const compatible=x=>x.styleKey===styleKey&&Boolean(x.useReference)===useReference&&(!useReference||(x.referenceKey===referenceKey&&x.referenceProfile===referenceProfile))
  const mood=Number(document.pet?.mood??70),emotion=mood>=80?'happy':mood<42?'sad':'calm'
  const personality=document.pet?.personality??'gentle'
  const recent=catalog.filter(x=>x.auto&&now-x.at<86400000)
  const last=recent.at(-1)
  if(!manual&&(recent.length>=settings.dailyLimit*3||(last&&now-last.at<settings.cooldownMinutes*60000)))return null
  const preferred=catalog.find(x=>x.id===request.reuseId&&x.kind===request.kind&&compatible(x))
  const cached=preferred??(!manual&&request.kind==='sticker'?catalog.findLast(x=>x.kind==='sticker'&&x.emotion===emotion&&x.personality===personality&&x.file!==catalog.at(-1)?.file&&compatible(x)):null)
  const reference={useReference,referenceKey:useReference?referenceKey:'',referenceProfile,styleKey}
  if(cached)return {...request,...reference,reuseId:cached.id,emotion,personality,auto:!manual,prompt:'',mood}
  if(!manual&&recent.filter(x=>!x.reused).length>=settings.dailyLimit)return null
  if(!settings.workflow.trim()){if(manual)throw Error('请先配置本机 ComfyUI 和 API 工作流');return null}
  const date=new Date(now).toLocaleDateString('zh-CN')
  const style={gentle:'gentle, soft gestures, restrained expression',playful:'playful, lively, naturally humorous',quiet:'quiet, subtle expression, simple composition',warm:'warm, friendly, comforting'}[personality]??'friendly and natural'
  const kind={sticker:'chat sticker, single clear gesture, simple background, no text or watermark',mood_card:'daily mood illustration card, clean space for a caption, no text',scene:'one complete miniature story scene, expressive subjects, no text'}[request.kind]
  referenceWorkflow(settings.workflow,useReference,settings)
  return {...request,...reference,reuseId:'',emotion,personality,auto:!manual,mood,
    prompt:[useReference?'same character as the supplied reference image, preserve the original illustration style, line art, coloring, shading, face, hairstyle, outfit, headset and proportions; subtle change of expression':settings.character,useReference?text(settings.referenceStyle,600):'',kind,style,'mood '+Math.round(mood)+'/100 ('+emotion+')',
      request.kind==='mood_card'?'date '+date:'',request.description].filter(Boolean).join('. ')}
}
// Adapt only the complete known built-in graph; user workflows remain authoritative.
export function referenceWorkflow(raw,useReference=false,options={}){
  if(!useReference)return raw
  let graph;try{graph=JSON.parse(raw)}catch{throw Error('工作流不是有效 JSON')}
  if(Object.values(graph??{}).some(x=>x?.inputs?.image==='{{image}}'))return raw
  const baseNegative='text, watermark, logo, blurry, deformed, extra limbs'
  const builtinNegative=baseNegative+', 3D render, photorealistic, photograph, plastic toy, realistic doll, CGI, glossy vinyl'
  const flatNegative=builtinNegative+', detailed fur, realistic fur, individual hair strands, realistic lighting, volumetric shading, soft edges, detailed texture'
  const negative=graph?.['3']?.inputs?.text,sampler=graph?.['5']?.inputs
  const legacy=sampler?.steps===6&&sampler?.cfg===1.5&&[baseNegative,builtinNegative].includes(negative)
  if(!legacy&&!(sampler?.steps===8&&sampler?.cfg===2&&negative===flatNegative))throw Error('当前自定义工作流不读取参考图；请使用内置快速绘图，或导入包含 {{image}} 的参考图工作流')
  const expected={
    '1':{class_type:'CheckpointLoaderSimple',inputs:{ckpt_name:graph?.['1']?.inputs?.ckpt_name}},
    '2':{class_type:'CLIPTextEncode',inputs:{text:'{{prompt}}',clip:['1',1]}},
    '3':{class_type:'CLIPTextEncode',inputs:{text:negative,clip:['1',1]}},
    '4':{class_type:'EmptyLatentImage',inputs:{width:512,height:512,batch_size:1}},
    '5':{class_type:'KSampler',inputs:{seed:'{{seed}}',steps:legacy?6:8,cfg:legacy?1.5:2,sampler_name:'lcm',scheduler:'normal',denoise:1,model:['1',0],positive:['2',0],negative:['3',0],latent_image:['4',0]}},
    '6':{class_type:'VAEDecode',inputs:{samples:['5',0],vae:['1',2]}},
    '7':{class_type:'SaveImage',inputs:{filename_prefix:'PenguinPal',images:['6',0]}},
  }
  const stable=x=>Array.isArray(x)?x.map(stable):plain(x)?Object.fromEntries(Object.keys(x).sort().map(k=>[k,stable(x[k])])):x
  if(typeof expected['1'].inputs.ckpt_name!=='string'||JSON.stringify(stable(graph))!==JSON.stringify(stable(expected)))throw Error('当前自定义工作流不读取参考图；请使用内置快速绘图，或导入包含 {{image}} 的参考图工作流')
  graph['4']={class_type:'LoadImage',inputs:{image:'{{image}}'}}
  graph['8']={class_type:'EmptyImage',inputs:{width:512,height:512,batch_size:1,color:16777215}}
  graph['9']={class_type:'InvertMask',inputs:{mask:['4',1]}}
  graph['10']={class_type:'ImageCompositeMasked',inputs:{destination:['8',0],source:['4',0],x:0,y:0,resize_source:true,mask:['9',0]}}
  graph['11']={class_type:'VAEEncode',inputs:{pixels:['10',0],vae:['1',2]}}
  const denoise=options.referenceDenoise??0.2
  if(!Number.isFinite(denoise)||denoise<0.1||denoise>0.7)throw Error('参考图重绘幅度应在 0.1 到 0.7 之间')
  Object.assign(graph['5'].inputs,{latent_image:['11',0],denoise,steps:6,cfg:1.5})
  graph['3'].inputs.text=baseNegative+(text(options.referenceNegative,600)?', '+text(options.referenceNegative,600):'')
  return JSON.stringify(graph)
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
export async function generateImage({settings,prompt,reference,useReference=false,cancelFile},{fetchImpl=fetch,pollMs=500}={}){
  const workflow=referenceWorkflow(settings.workflow,useReference,settings)
  if(useReference&&!reference)throw Error('请先上传桌宠参考图')
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
    if(reference&&workflow.includes('{{image}}')){
      const bytes=Buffer.from(reference,'base64');if(bytes.length>8*1024*1024)throw Error('角色参考图最多 8 MiB')
      const type=imageType(bytes),ext=type.split('/')[1],form=new FormData()
      form.append('image',new Blob([bytes],{type}),'penguin-reference.'+ext);form.append('overwrite','false')
      const result=await json('upload/image',{method:'POST',body:form})
      input=relativeFile(result.name,result.subfolder??'')
    }
    const graph=prepareWorkflow(workflow,prompt,randomBytes(4).readUInt32BE(0),input)
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
