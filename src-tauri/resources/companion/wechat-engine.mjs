// iLink wire format adapted from xmanrui/dsh-im (MIT; see wechat-LICENSE.txt).
// Only the QR-authorized owner's private text and generated gallery images are exposed.
import {createCipheriv,createHash,randomBytes,randomUUID} from 'node:crypto'
import {pathToFileURL} from 'node:url'
import {realpathSync} from 'node:fs'
const ROOT='https://ilinkai.weixin.qq.com/'
const CDN='novac2c.cdn.weixin.qq.com'
export function trustedWechat(value){
  let url;try{url=new URL(value)}catch{throw Error('微信返回的地址无效')}
  const host=url.hostname.toLowerCase()
  if(url.protocol!=='https:'||url.username||url.password||url.search||url.hash||(url.port&&url.port!=='443')||!['weixin.qq.com','wechat.com'].some(x=>host===x||host.endsWith('.'+x)))throw Error('拒绝访问未经验证的微信地址')
  if(!url.pathname.endsWith('/'))url.pathname+='/'
  return url
}
function qrUrl(value){
  let url;try{url=new URL(value)}catch{throw Error('微信没有返回有效扫码链接')}
  const base=new URL(url);base.search='';base.hash=''
  trustedWechat(base)
  return url.toString()
}
const info=()=>({channel_version:'2.4.6',bot_agent:'PenguinPal/0.9.20'})
const headers=auth=>({'content-type':'application/json','iLink-App-Id':'bot','iLink-App-ClientVersion':String((2<<16)|(4<<8)|6),
  AuthorizationType:'ilink_bot_token','X-WECHAT-UIN':Buffer.from(String(randomBytes(4).readUInt32BE(0))).toString('base64'),
  ...(auth?.token?{Authorization:'Bearer '+auth.token}:{})})
async function readJson(response){
  if(!response.ok)throw Error('微信服务请求失败（HTTP '+response.status+'）')
  const chunks=[];let size=0
  for await(const chunk of response.body??[]){size+=chunk.length;if(size>2*1024*1024)throw Error('微信响应超过大小限制');chunks.push(Buffer.from(chunk))}
  const body=Buffer.concat(chunks).toString('utf8')
  return parseWechatJson(body)
}
export function parseWechatJson(body,parse=JSON.parse){
  let value,precisionUnavailable=false;try{value=parse(body,(key,value,context)=>{
    // iLink IDs are uint64. Preserve the original token before Number rounds it.
    if(['message_id','msg_id'].includes(key)&&typeof value==='number'&&!Number.isSafeInteger(value)){
      if(!context?.source)precisionUnavailable=true
      if(/^\d{1,20}$/.test(context?.source??''))return context.source
    }
    return value
  })}catch{throw Error('微信返回了无法解析的数据')}
  if(precisionUnavailable)throw Error('微信收信需要 Node.js 22.17.1 或更新版本，请升级 Node.js 后重试')
  if([value.ret,value.errcode].some(x=>x!==undefined&&x!==null&&String(x)!=='0'))throw Error('微信服务拒绝请求，请检查授权状态')
  return value
}
export function splitText(value){
  const result=[];let rest=String(value??'').trim()
  while(rest){let end=Math.min(rest.length,1800);if(end<rest.length&&/[\uD800-\uDBFF]/.test(rest[end-1]))end--;result.push(rest.slice(0,end));rest=rest.slice(end)}
  return result
}
function inboundBatch(value,owner){
  if(!owner)throw Error('微信尚未绑定主人')
  const received=Array.isArray(value.msgs)?value.msgs:[],messages=[],rejected={}
  const reject=reason=>{rejected[reason]=(rejected[reason]??0)+1}
  for(const message of received.slice(0,100)){
    if(!message||message.from_user_id!==owner){reject('owner');continue}
    if(message.message_type!==1){reject('type');continue}
    if(message.group_id||message.room_id||message.group_chat_id||message.chat_type==='group'){reject('group');continue}
    const id=message.message_id??message.msg_id??message.client_id
    if((typeof id==='number'&&!Number.isSafeInteger(id))||!/^[a-zA-Z0-9_-]{1,100}$/.test(String(id??''))){reject('id');continue}
    if(!Array.isArray(message.item_list)||!message.item_list.every(x=>x?.type===1&&typeof x.text_item?.text==='string')){reject('nonText');continue}
    const text=message.item_list.map(x=>x.text_item.text).join('\n').trim()
    if(!text){reject('empty');continue}
    messages.push({id:String(id),text:text.slice(0,3000),contextToken:String(message.context_token??'').slice(0,4096)})
  }
  if(received.length>100)reject('overflow')
  return {messages,receive:{received:received.length,accepted:messages.length,ignored:received.length-messages.length,rejected}}
}
export function inboundMessages(value,owner){return inboundBatch(value,owner).messages}
export function createWechat({fetchImpl=fetch}={}){
  const api=async(auth,path,body,timeout=15000)=>{
    const base=trustedWechat(auth?.baseUrl??ROOT)
    return readJson(await fetchImpl(new URL(path,base),{method:body===undefined?'GET':'POST',headers:headers(auth),
      ...(body===undefined?{}:{body:JSON.stringify({...body,base_info:info()})}),redirect:'error',signal:AbortSignal.timeout(timeout)}))
  }
  const sendItem=async(auth,contextToken,item,clientId)=>api(auth,'ilink/bot/sendmessage',{msg:{from_user_id:'',to_user_id:auth.owner,
    client_id:clientId,message_type:2,message_state:2,item_list:[item],...(contextToken?{context_token:contextToken}:{})}})
  return {
    async login(){
      const value=await api(null,'ilink/bot/get_bot_qrcode?bot_type=3',{local_token_list:[]})
      if(typeof value.qrcode!=='string'||value.qrcode.length>4096)throw Error('微信没有返回有效二维码')
      return {qrcode:value.qrcode,url:qrUrl(value.qrcode_img_content),baseUrl:ROOT,createdAt:Date.now()}
    },
    async loginStatus(login,verifyCode=''){
      if(!login?.qrcode||Date.now()-login.createdAt>300000)throw Error('二维码已过期，请重新获取')
      if(verifyCode&&!/^[0-9]{4,12}$/.test(verifyCode))throw Error('请输入有效的微信配对码')
      const value=await api({baseUrl:login.baseUrl},'ilink/bot/get_qrcode_status?qrcode='+encodeURIComponent(login.qrcode)+(verifyCode?'&verify_code='+encodeURIComponent(verifyCode):''),undefined,35000)
      if(value.status==='confirmed'){
        if(![value.bot_token,value.ilink_bot_id,value.ilink_user_id].every(x=>typeof x==='string'&&x.length>0&&x.length<8192))throw Error('微信返回的授权不完整')
        return {status:'confirmed',auth:{token:value.bot_token,account:value.ilink_bot_id,owner:value.ilink_user_id,
          baseUrl:trustedWechat(value.baseurl??login.baseUrl).toString(),session:randomUUID()}}
      }
      if(value.status==='scaned_but_redirect'){
        const host=String(value.redirect_host??'')
        return {status:'scaned',baseUrl:trustedWechat(host.includes('://')?host:'https://'+host).toString()}
      }
      if(!['wait','scaned','need_verifycode','verify_code_blocked','expired','binded_redirect'].includes(value.status))throw Error('无法识别微信扫码状态')
      return {status:value.status}
    },
    async poll(auth,cursor=''){
      let value
      try{value=await api(auth,'ilink/bot/getupdates',{get_updates_buf:cursor},35000)}
      catch(error){if(error.name==='TimeoutError')return {messages:[],cursor};throw error}
      return {...inboundBatch(value,auth.owner),cursor:String(value.get_updates_buf??cursor).slice(0,100000)}
    },
    async text(auth,message,contextToken=''){
      for(const chunk of splitText(message))await sendItem(auth,contextToken,{type:1,text_item:{text:chunk}},'penguin-'+randomUUID())
      return {ok:true}
    },
    async image(auth,bytes,contextToken=''){
      if(!Buffer.isBuffer(bytes)||!bytes.length||bytes.length>8*1024*1024)throw Error('微信图片大小无效')
      const key=randomBytes(16),fileKey=randomBytes(16).toString('hex'),cipher=createCipheriv('aes-128-ecb',key,null)
      const encrypted=Buffer.concat([cipher.update(bytes),cipher.final()])
      const upload=await api(auth,'ilink/bot/getuploadurl',{filekey:fileKey,media_type:1,to_user_id:auth.owner,rawsize:bytes.length,
        rawfilemd5:createHash('md5').update(bytes).digest('hex'),filesize:encrypted.length,no_need_thumb:true,aeskey:key.toString('hex')})
      const target=upload.upload_full_url?new URL(upload.upload_full_url):new URL('https://'+CDN+'/c2c/upload')
      if(!upload.upload_full_url){if(!upload.upload_param)throw Error('微信没有返回上传地址');target.searchParams.set('encrypted_query_param',upload.upload_param);target.searchParams.set('filekey',fileKey)}
      if(target.protocol!=='https:'||target.hostname!==CDN||target.pathname!=='/c2c/upload'||target.username||target.password||(target.port&&target.port!=='443'))throw Error('拒绝访问未经验证的图片上传地址')
      const response=await fetchImpl(target,{method:'POST',body:encrypted,headers:{'content-type':'application/octet-stream'},
        redirect:'error',signal:AbortSignal.timeout(60000)})
      const param=response.headers.get('x-encrypted-param')
      if(!response.ok||!param||param.length>16384)throw Error('微信图片上传失败')
      await response.body?.cancel()
      await sendItem(auth,contextToken,{type:2,image_item:{media:{encrypt_query_param:param,aes_key:Buffer.from(key.toString('hex')).toString('base64'),encrypt_type:1},mid_size:encrypted.length}},'penguin-'+randomUUID())
      return {ok:true}
    }
  }
}
export async function wechatAction(request,options){
  const client=createWechat(options)
  if(request.action==='login')return client.login()
  if(request.action==='loginStatus')return client.loginStatus(request.login,request.verifyCode)
  if(!request.auth?.owner||!request.auth?.token)throw Error('请先扫码绑定微信')
  if(request.action==='poll')return client.poll(request.auth,request.cursor)
  if(request.action==='text')return client.text(request.auth,request.text,request.contextToken)
  if(request.action==='image')return client.image(request.auth,Buffer.from(request.bytes??'','base64'),request.contextToken)
  throw Error('未知微信操作')
}
if(process.argv[1]&&import.meta.url===pathToFileURL(realpathSync(process.argv[1])).href){
  const chunks=[];let size=0;try{
    for await(const chunk of process.stdin){size+=chunk.length;if(size>16*1024*1024)throw Error('微信请求过大');chunks.push(chunk)}
    const input=Buffer.concat(chunks).toString('utf8')
    process.stdout.write(JSON.stringify(await wechatAction(JSON.parse(input))))
  }catch(error){process.stdout.write(JSON.stringify({error:error?.name==='TimeoutError'?'微信请求超时，发送结果未确认，不会自动重发':String(error?.message??'微信连接失败').slice(0,200)}));process.exitCode=1}
}
