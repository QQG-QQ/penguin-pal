import test from 'node:test'
import assert from 'node:assert/strict'
import {createServer} from 'node:http'
import {mkdtemp,writeFile,readFile,rm} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {spawn} from 'node:child_process'
import {normalizeSettings,localBase,validateVisual,prepareWorkflow,planVisual,generateImage,imageType,visualContext} from '../src-tauri/resources/companion/visual-engine.mjs'
import {createWechat,trustedWechat,inboundMessages,splitText,parseWechatJson} from '../src-tauri/resources/companion/wechat-engine.mjs'
import {createDecipheriv} from 'node:crypto'
import ts from 'typescript'

const png=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=','base64')
const graph={1:{class_type:'CLIPTextEncode',inputs:{text:'{{prompt}}'}},2:{class_type:'KSampler',inputs:{seed:'{{seed}}'}}}
const cfg=(extra={})=>normalizeSettings({enabled:true,workflow:JSON.stringify(graph),...extra})
const visual={kind:'sticker',description:'a funny cat laughing',caption:'来啦',reuseId:''}
const doc={pet:{mood:92,personality:'playful',moodReason:'private details must not become prompts'}}
const plan=(extra={})=>planVisual({settings:cfg(),document:doc,visual,catalog:[],now:1000000000,...extra})
const json=x=>new Response(JSON.stringify(x),{headers:{'content-type':'application/json'}})
const account={token:'synthetic-token',owner:'test-owner',baseUrl:'https://ilinkai.weixin.qq.com/'}

test('managed low-step workflow generates ordinary memes without a reference',async()=>{
  const source=await readFile(new URL('../src/lib/companionVisual.ts',import.meta.url),'utf8')
  const compiled=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText
  const {managedWorkflow,defaultVisualSettings}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)
  const graph=prepareWorkflow(managedWorkflow(),'funny laughing cat',123)
  assert.equal(graph['1'].inputs.ckpt_name,'DreamShaper8_LCM.safetensors')
  assert.equal(graph['4'].class_type,'EmptyLatentImage')
  assert.equal(graph['5'].inputs.sampler_name,'lcm')
  assert.equal(graph['5'].inputs.steps,6)
  assert.deepEqual(graph['5'].inputs.latent_image,['4',0])
  assert.equal(graph['5'].inputs.denoise,1)
  assert.equal(Object.values(graph).some(x=>x.class_type==='LoadImage'),false)
  assert.doesNotMatch(defaultVisualSettings().character,/penguin|reference|same character/i)
  assert.equal(defaultVisualSettings().character,normalizeSettings().character)
  assert.equal(normalizeSettings({}).managedRuntime,false)
  assert.equal(normalizeSettings({managedRuntime:true}).managedRuntime,true)
  assert.equal(normalizeSettings({baseUrl:'http://localhost:8188'}).baseUrl,'http://localhost:8188')
})

test('image intent is data only and malformed output defaults to no image',()=>{
  assert.equal(validateVisual(null).kind,'none');assert.equal(validateVisual({kind:'command'}).kind,'none')
  assert.equal(validateVisual({...visual,reuseId:'../secret'}).reuseId,'')
  assert.equal(validateVisual({...visual,description:'x'.repeat(9999)}).description.length,600)
  assert.equal(visualContext(cfg(),doc).personality,'playful')
})
test('only explicit enablement permits drawing; personality and mood shape prompts',()=>{
  assert.equal(plan({settings:cfg({enabled:false})}),null)
  assert.equal(plan({settings:cfg({automatic:false})}),null)
  assert.equal(plan({visual:{kind:'none'}}),null)
  const p=plan();assert.match(p.prompt,/playful/);assert.match(p.prompt,/92\/100/)
  assert.ok(!p.prompt.includes('private details'));assert.doesNotMatch(p.prompt,/penguin|reference|same character/i)
  assert.match(plan({settings:cfg({character:'a blue penguin requested by the user'})}).prompt,/blue penguin/)
  assert.equal(plan({document:{pet:{mood:10,personality:'quiet'}}}).emotion,'sad')
  assert.match(plan({document:{pet:{mood:10,personality:'quiet'}}}).prompt,/quiet/)
})
test('explicit requests survive model none, bypass auto limits and retain model scene detail',()=>{
  assert.equal(plan({prompt:'给我画张今日心情卡',visual:{kind:'none'},settings:cfg({automatic:false})}).kind,'mood_card')
  const p=plan({prompt:'帮我画个小剧场',visual:{...visual,kind:'scene',description:'a penguin baking a cake'},catalog:Array.from({length:20},()=>({auto:true,at:999999999}))})
  assert.equal(p.kind,'scene');assert.match(p.prompt,/baking a cake/)
  assert.throws(()=>plan({prompt:'给我画张表情包',settings:cfg({workflow:''})}),/工作流/)
})
test('automatic quotas and cooldown apply to both new and reused pictures',()=>{
  assert.equal(plan({catalog:[{auto:true,at:999999999}]}),null)
  assert.equal(plan({catalog:Array.from({length:4},(_,n)=>({auto:true,at:950000000+n,reused:false}))}),null)
  assert.equal(plan({catalog:Array.from({length:12},(_,n)=>({auto:true,at:950000000+n,reused:true}))}),null)
  const old={...visual,id:'cached',file:'cached.png',emotion:'happy',personality:'playful',at:1}
  const catalog=[old,{file:'other.png',at:2}]
  assert.equal(plan({catalog}).reuseId,'cached')
  assert.equal(plan({catalog,visual:{...visual,reuseId:'invented'}}).reuseId,'cached')
  assert.equal(plan({catalog:[{...old,kind:'scene'}],visual:{...visual,kind:'scene'}}).reuseId,'')
})
test('workflow accepts API graphs, injects only input values and requires a prompt',()=>{
  assert.throws(()=>prepareWorkflow('{','x',1),/JSON/)
  assert.throws(()=>prepareWorkflow(JSON.stringify({nodes:[]}), 'x',1),/API/)
  assert.throws(()=>prepareWorkflow(JSON.stringify({1:{class_type:'X',inputs:{text:'missing'}}}),'x',1),/prompt/)
  const g=prepareWorkflow(JSON.stringify({...graph,3:{class_type:'{{prompt}}',inputs:{image:'{{image}}'}}}),'a $& penguin',42,'pet.png')
  assert.equal(g[1].inputs.text,'a $& penguin');assert.equal(g[2].inputs.seed,42);assert.equal(g[3].class_type,'{{prompt}}');assert.equal(g[3].inputs.image,'pet.png')
  assert.throws(()=>prepareWorkflow(JSON.stringify({...graph,3:{class_type:'LoadImage',inputs:{image:'{{image}}'}}}),'x',1),/参考图/)
  assert.throws(()=>prepareWorkflow(JSON.stringify({...graph,3:{class_type:'LoadImage',inputs:{image:'{{image}}'}},4:{class_type:'LoadImage',inputs:{image:'{{image}}'}}}),'x',1,'pet.png'),/最多/)
})
test('ComfyUI and WeChat reject unexpected hosts, schemes and embedded credentials',()=>{
  for(const url of ['https://example.com','file:///tmp/x','http://127.0.0.1.evil/','http://user:pass@localhost','http://localhost/?key=x'])assert.throws(()=>localBase(url))
  for(const url of ['http://ilinkai.weixin.qq.com','https://weixin.qq.com.evil','https://evil-weixin.qq.com','https://a:secret@wechat.com','https://wechat.com:999/'])assert.throws(()=>trustedWechat(url))
  assert.equal(localBase('http://[::1]:8188').hostname,'[::1]')
  assert.equal(trustedWechat('https://ilinkai.weixin.qq.com').protocol,'https:')
})
async function fakeComfy(work){
  const calls=[];let prompt
  const server=createServer(async(req,res)=>{
    const chunks=[];for await(const chunk of req)chunks.push(chunk)
    const body=Buffer.concat(chunks);calls.push({path:req.url,body:body.toString()})
    res.setHeader('Content-Type','application/json')
    if(req.url==='/upload/image')res.end(JSON.stringify({name:'reference.png',subfolder:'pet'}))
    else if(req.url==='/prompt'){prompt=JSON.parse(body);res.end(JSON.stringify({prompt_id:'test-id'}))}
    else if(req.url==='/history/test-id')res.end(JSON.stringify({'test-id':{outputs:{1:{images:[{filename:'result.png',type:'output'}]}}}}))
    else if(req.url.startsWith('/view?')){res.setHeader('Content-Type','image/png');res.end(png)}
    else{res.statusCode=404;res.end('{}')}
  })
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))
  try{await work('http://127.0.0.1:'+server.address().port,calls,()=>prompt)}finally{await new Promise(resolve=>server.close(resolve))}
}
test('real local HTTP workflow uploads reference, submits API prompt, polls and loads image',async()=>{
  await fakeComfy(async(baseUrl,calls,getPrompt)=>{
    const workflow=JSON.stringify({...graph,3:{class_type:'LoadImage',inputs:{image:'{{image}}'}}})
    const result=await generateImage({settings:cfg({baseUrl,workflow}),prompt:'happy penguin',reference:png.toString('base64')},{pollMs:1})
    assert.equal(result.mediaType,'image/png');assert.equal(result.bytes,png.toString('base64'))
    assert.equal(getPrompt().prompt[3].inputs.image,'pet/reference.png');assert.equal(getPrompt().prompt[1].inputs.text,'happy penguin')
    assert.match(calls[0].body,/penguin-reference.png/);assert.ok(calls.some(x=>x.path.startsWith('/view?')))
  })
})
test('workflow errors, absent images, redirects and oversized responses fail without fabricated pictures',async()=>{
  for(const history of [{status:{status_str:'error'}},{status:{completed:true},outputs:{}}]){
    await assert.rejects(generateImage({settings:cfg(),prompt:'x'},{fetchImpl:async(url)=>String(url).includes('/prompt')?json({prompt_id:'id'}):json({id:history}),pollMs:1}),/工作流|图片/)
  }
  await assert.rejects(generateImage({settings:cfg(),prompt:'x'},{fetchImpl:async()=>new Response('x',{headers:{'content-length':String(3*1024*1024)}})}),/大小/)
  await assert.rejects(generateImage({settings:cfg(),prompt:'x'},{fetchImpl:async(url,options)=>{assert.equal(options.redirect,'error');throw Error('redirect refused')}}),/redirect refused/)
  assert.throws(()=>imageType(Buffer.from('<svg/>')),/只支持/)
  assert.equal(imageType(Buffer.from([255,216,255,0])),'image/jpeg')
  assert.equal(imageType(Buffer.from('RIFFxxxxWEBPxxxx')),'image/webp')
  await assert.rejects(generateImage({settings:cfg(),prompt:'x'},{fetchImpl:async(url)=>String(url).includes('/prompt')?json({prompt_id:'id'}):json({id:{outputs:{1:{images:[{filename:'../private.png',type:'output'}]}}}})}),/无效图片路径/)
})
test('timeout and explicit cancellation clean only the current queued task',async()=>{
  for(const cancel of [false,true]){
    const dir=await mkdtemp(join(tmpdir(),'penguin-art-'));const cancelFile=join(dir,'cancel');let deleted
    try{
      if(cancel)await writeFile(cancelFile,'cancel')
      const fetchImpl=async(url,options)=>{
        if(String(url).endsWith('/queue')){deleted=JSON.parse(options.body).delete;return json({})}
        if(String(url).endsWith('/prompt'))return json({prompt_id:'own-task'})
        return new Promise((resolve,reject)=>{options.signal.addEventListener('abort',()=>reject(options.signal.reason),{once:true})})
      }
      await assert.rejects(generateImage({settings:{...cfg(),timeoutSeconds:cancel?1:0.02},prompt:'x',cancelFile:cancel?cancelFile:undefined},{fetchImpl}),cancel?/取消/:/超时/)
      assert.deepEqual(deleted,['own-task'])
    }finally{await rm(dir,{recursive:true,force:true})}
  }
})
test('WeChat owner filter excludes other users, non-text messages and unsafe IDs',()=>{
  const m={from_user_id:'test-owner',message_type:1,message_id:'safe_1',item_list:[{type:1,text_item:{text:'你好，企鹅🐧'}}],context_token:'context'}
  assert.equal(inboundMessages({msgs:[m]},'test-owner')[0].text,'你好，企鹅🐧')
  for(const bad of [{from_user_id:'stranger'},{message_type:2},{group_id:'group'},{room_id:'room'},{message_id:'../file'},{message_id:Number.MAX_SAFE_INTEGER+1},{item_list:null},{item_list:[{type:2,image_item:{}}]},{item_list:[...m.item_list,{type:2,image_item:{}}]}])assert.equal(inboundMessages({msgs:[{...m,...bad}]},'test-owner').length,0)
  assert.throws(()=>inboundMessages({},''),/绑定/)
  const chunks=splitText('a'.repeat(1799)+'🐧你好');assert.equal(chunks.join(''),'a'.repeat(1799)+'🐧你好');assert.equal(chunks[0].length,1799)
})
test('WeChat uint64 wire IDs retain exact digits for deduplication and text remains unchanged',async()=>{
  const ids=['9223372036854775807','9223372036854775806','18446744073709551615']
  const message=id=>`{"from_user_id":"test-owner","message_type":1,"message_id":${id},"item_list":[{"type":1,"text_item":{"text":"literal \\"message_id\\": 9223372036854775807"}}],"context_token":"ctx"}`
  const response=`{"msgs":[${ids.map(message).join(',')}],"get_updates_buf":"next"}`
  const result=await createWechat({fetchImpl:async()=>new Response(response)}).poll(account,'before')
  assert.deepEqual(result.messages.map(x=>x.id),ids)
  assert.equal(result.messages[0].contextToken,'ctx')
  assert.equal(result.messages[0].text,'literal "message_id": 9223372036854775807')
  assert.deepEqual(result.receive,{received:3,accepted:3,ignored:0,rejected:{}})
  const duplicate=await createWechat({fetchImpl:async()=>new Response(response)}).poll(account)
  assert.equal(duplicate.messages[0].id,result.messages[0].id)
  const m={from_user_id:'test-owner',message_type:1,client_id:'safe-client-1',item_list:[{type:1,text_item:{text:'ok'}}]}
  assert.equal(inboundMessages({msgs:[m]},'test-owner')[0].id,'safe-client-1')
})

test('older Node reports precision incompatibility before advancing the receive cursor',()=>{
  const raw='{"msgs":[{"message_id":9223372036854775807}]}'
  const oldParse=(body,revive)=>JSON.parse(body,(key,value)=>revive(key,value))
  assert.throws(()=>parseWechatJson(raw,oldParse),/Node.js 22.17.1/)
  assert.equal(parseWechatJson('{"msgs":[]}',oldParse).msgs.length,0)
})

test('WeChat receive counters exclude strangers, groups and media without exposing content',async()=>{
  const m={from_user_id:'test-owner',message_type:1,message_id:'a',item_list:[{type:1,text_item:{text:'private'}}]}
  const result=await createWechat({fetchImpl:async()=>json({msgs:[m,{...m,from_user_id:'stranger'},{...m,room_id:'group'},{...m,item_list:[{type:2}]}]})}).poll(account)
  assert.deepEqual(result.receive,{received:4,accepted:1,ignored:3,rejected:{owner:1,group:1,nonText:1}})
  assert.doesNotMatch(JSON.stringify(result.receive),/private|test-owner|stranger/)
})

test('WeChat QR credentials are complete and only approved redirect hosts are followed',async()=>{
  const client=createWechat({fetchImpl:async(url)=>String(url).includes('get_bot_qrcode')?json({qrcode:'login-id',qrcode_img_content:'https://ilinkai.weixin.qq.com/login?code=test'}):json({status:'confirmed',bot_token:'token',ilink_bot_id:'bot',ilink_user_id:'owner'})})
  const login=await client.login();const result=await client.loginStatus(login)
  assert.equal(result.auth.owner,'owner');assert.equal(result.status,'confirmed');assert.ok(result.auth.session)
  await assert.rejects(createWechat({fetchImpl:async()=>json({status:'confirmed',bot_token:'token'})}).loginStatus(login),/授权不完整/)
  await assert.rejects(createWechat({fetchImpl:async()=>json({status:'scaned_but_redirect',redirect_host:'attacker.example'})}).loginStatus(login),/未经验证/)
  await assert.rejects(client.loginStatus({...login,createdAt:0}),/过期/)
})
test('WeChat streaming JSON preserves split Chinese UTF-8 and sends only to QR owner',async()=>{
  const bytes=Buffer.from(JSON.stringify({msgs:[{from_user_id:'test-owner',message_type:1,message_id:'1',item_list:[{type:1,text_item:{text:'中文企鹅🐧'}}]}]}))
  let sent
  const client=createWechat({fetchImpl:async(url,options)=>{
    assert.equal(options.redirect,'error')
    if(String(url).includes('getupdates'))return new Response(new ReadableStream({start(c){for(const b of bytes)c.enqueue(Uint8Array.of(b));c.close()}}))
    sent=JSON.parse(options.body);return json({ret:0})
  }})
  assert.equal((await client.poll(account)).messages[0].text,'中文企鹅🐧')
  await client.text(account,'你好，企鹅🐧','context')
  assert.equal(sent.msg.to_user_id,'test-owner');assert.equal(sent.msg.context_token,'context');assert.equal(sent.msg.item_list[0].text_item.text,'你好，企鹅🐧')
})
test('WeChat image encryption round-trips and never sends arbitrary upload URLs',async()=>{
  let key,raw,encrypted,sent
  const client=createWechat({fetchImpl:async(url,options)=>{
    if(String(url).includes('getuploadurl')){raw=JSON.parse(options.body);key=Buffer.from(raw.aeskey,'hex');return json({upload_param:'upload-token'})}
    if(new URL(url).hostname==='novac2c.cdn.weixin.qq.com'){encrypted=options.body;return new Response('',{headers:{'x-encrypted-param':'download-param'}})}
    sent=JSON.parse(options.body);return json({ret:0})
  }})
  await client.image(account,png,'ctx')
  const decrypt=createDecipheriv('aes-128-ecb',key,null);assert.deepEqual(Buffer.concat([decrypt.update(encrypted),decrypt.final()]),png)
  const item=sent.msg.item_list[0].image_item;assert.equal(Buffer.from(item.media.aes_key,'base64').toString(),key.toString('hex'));assert.equal(item.mid_size,encrypted.length);assert.equal(raw.rawsize,png.length)
  let calls=0
  await assert.rejects(createWechat({fetchImpl:async()=>{calls++;return json({upload_full_url:'https://evil.example/upload'})}}).image(account,png),/未经验证/)
  assert.equal(calls,1)
})
test('ambiguous WeChat delivery is attempted once and never automatically retried',async()=>{
  let calls=0
  const client=createWechat({fetchImpl:async()=>{calls++;throw Error('network failed after write')}})
  await assert.rejects(client.text(account,'one reply'),/network failed/);assert.equal(calls,1)
})
test('CLI stdin preserves UTF-8 across writes',async()=>{
  const child=spawn(process.execPath,['src-tauri/resources/companion/visual-engine.mjs'],{stdio:['pipe','pipe','pipe']})
  const data=Buffer.from(JSON.stringify({action:'normalize',settings:{character:'企鹅的心情🐧'}}));const chunks=[]
  child.stdout.on('data',x=>chunks.push(x));for(const byte of data)child.stdin.write(Buffer.of(byte));child.stdin.end()
  const code=await new Promise(resolve=>child.on('close',resolve));assert.equal(code,0)
  assert.equal(JSON.parse(Buffer.concat(chunks)).character,'企鹅的心情🐧')
})
