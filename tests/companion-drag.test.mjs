import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'

async function module(name){const source=readFileSync(new URL(`../src/lib/${name}.ts`,import.meta.url),'utf8');return import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText).toString('base64')}`)}
const core=await module('companionCore'),media=await module('companionMedia'),animation=await module('companionAnimation')
const musicModule=await module('companionMusic')
const mediaJs=ts.transpileModule(readFileSync(new URL('../src/lib/companionMedia.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText,mediaUrl='data:text/javascript;base64,'+Buffer.from(mediaJs).toString('base64')
const sequenceJs=ts.transpileModule(readFileSync(new URL('../src/lib/companionSequence.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText.replace("'./companionMedia'",JSON.stringify(mediaUrl)),sequence=await import('data:text/javascript;base64,'+Buffer.from(sequenceJs).toString('base64'))
// Run the production component handlers and frame loop with deterministic native IPC.
// Pointer capture delivery and Vue rendering are additionally checked in the browser.
const builtinPack={id:'builtin',clips:JSON.parse(readFileSync(new URL('../penguin/companion-v4/motions.json',import.meta.url),'utf8')).clips}
function app(preferences={},pack={id:'builtin',clips:[]},systemReduced=false){
 const component=readFileSync(process.env.PENGUIN_COMPANION_SOURCE??new URL('../src/CompanionApp.vue',import.meta.url),'utf8')
 const cancelled=component.match(/@pointercancel="([^"]+)"/)[1]
 const source=component.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^import[\s\S]*?from ['"][^'"]+['"]\r?$/mg,'')
 const js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText
 let clock=0,held=true,defer=false,completeMove,moveStarted,failMove=false,nativeFullscreen=false,windowShown=true,nativeIgnored=false
 const pos={x:420,y:160},cursor={x:520,y:260},writes=[],timers=[]
 const win={scaleFactor:async()=>1,outerPosition:async()=>({...pos}),isVisible:async()=>windowShown,setIgnoreCursorEvents:async value=>{nativeIgnored=value},setAlwaysOnTop:async()=>{},
  async setPosition(next){if(failMove)throw Error('position unavailable');if(defer){moveStarted?.();await new Promise(ok=>{completeMove=ok})}Object.assign(pos,next);writes.push({...pos})}}
 const invoke=async command=>{if(command==='companion_poll')return{safeMode:false,bridge:{status:'idle',lastReceived:0},desktop:{surfaces:[],context:'quiet',fullscreen:nativeFullscreen,leftButtonDown:held,idleSeconds:0}}}
 const window={__TAURI_INTERNALS__:{},location:{search:''},matchMedia:()=>({matches:systemReduced}),clearTimeout(){},setTimeout(fn){timers.push(fn);return timers.length}}
 const metrics={petLeft:0,petTop:0,petRight:160,petBottom:220}
 const deps={...core,...media,...animation,...sequence,...musicModule,djPoster:'dj.png',djMetadata:{alphaBBox:[47,25,203,242]},window,document:{querySelectorAll:()=>[]},Math:Object.assign(Object.create(Math),{random:()=>.1}),Date:class extends Date{static now(){return 1720000000000+clock}},performance:{now:()=>clock},ref:value=>({value}),computed:fn=>({get value(){return fn()}}),onMounted(){},onBeforeUnmount(){},
  invoke,listen:async()=>()=>{},convertFileSrc:p=>p,cursorPosition:async()=>({...cursor}),currentMonitor:async()=>({workArea:{position:{x:0,y:0},size:{width:1920,height:1040}}}),primaryMonitor:async()=>null,getCurrentWindow:()=>win,
  PhysicalPosition:class{constructor(x,y){this.x=x;this.y=y}},PhysicalSize:class{},computePetLayoutMetrics:()=>metrics,isPetOpaqueHit:(_a,_r,_m,x,y)=>x>=0&&x<=160&&y>=0&&y<=220,loadPetAlphaMask:async()=>null,
  builtinMotionPack:pack,companionArtwork:()=>({src:'poster.png'})}
 const run=new Function('deps',`const {${Object.keys(deps).join(',')}}=deps;\n${js}\ninitialized=true;img.value={getBoundingClientRect:()=>({left:0,top:0,width:160,height:220})};return {state,activity,airborne,dragging,pointerDown,release,frame,poll,error,musicScene,codexStatus,media,pose,facing,still,musicEnabled,video,videoFailed,setSafety,preferencesChanged,motionChanged,requestAction,refreshGeometry,updateHitTest,setSurfaces(value){surfaces=value},setFullscreen(value){fullscreen.value=value},setMusic(sample){music.value=sample;musicReceived=performance.now()},cancel:()=>{return ${cancelled.endsWith('()')?cancelled:cancelled+'()'}},age(){heldAt=Date.now()-2000},getBody:()=>({...body}),getBounds:()=>({...bounds})};`)(deps)
 Object.assign(run.state.value.preferences,{gravity:true,edgeDock:false,springDrag:false,wander:false,movement:'still',...preferences})
 const press=()=>run.pointerDown({button:0,pointerId:1,clientX:80,clientY:100,currentTarget:{setPointerCapture(){}}})
 const frame=async(ms=33)=>{clock+=ms;await run.frame()}
 return {...run,pos,writes,timers,press,frame,setDesktop(fullscreen,shown=true){nativeFullscreen=fullscreen;windowShown=shown},cursorIgnored:()=>nativeIgnored,resetNativeCursorMode(value){nativeIgnored=value},setCursor(x,y=cursor.y){cursor.x=x;cursor.y=y},async move(x=14,y=-4,ms=40){cursor.x+=x;cursor.y+=y;await frame(ms)},setHeld(value){held=value},deferMove(){defer=true;return new Promise(ok=>{moveStarted=ok})},finishMove(){defer=false;completeMove()},failMove(){failMove=true}}
}
async function drag(a){await a.press();for(let i=0;i<6;i++)await a.move()}
test('normal release retains throw velocity and enters gravity',async()=>{const a=app();await drag(a);await a.release();assert.equal(a.airborne.value,true);assert.ok(a.getBody().vx>0);const y=a.pos.y;for(let i=0;i<15;i++)await a.frame();assert.ok(a.pos.y>y)})
test('system reduced motion keeps manual throws, animations and following under app controls',async()=>{
 const a=app({springDrag:true},builtinPack,true)
 await drag(a);await a.release()
 assert.equal(a.still.value,false);assert.equal(a.airborne.value,true);assert.ok(a.getBody().vx>0)
 assert.equal(a.musicEnabled.value,true);assert.ok(a.video.value)
 const y=a.pos.y;for(let i=0;i<15;i++)await a.frame();assert.ok(a.pos.y>y)
 for(let i=0;i<120;i++)await a.frame()
 assert.equal(a.airborne.value,false);assert.equal(a.getBody().vy,0);assert.equal(a.getBody().y+a.getBody().height,a.getBounds().bottom)
 const b=app({movement:'follow',hoverPause:false},builtinPack,true)
 await b.refreshGeometry();b.setCursor(1800);for(let i=0;i<20;i++)await b.frame();assert.ok(b.pos.x>420)
})
test('system animation changes during a throw do not freeze the pet in midair',async()=>{
 const a=app();await drag(a);await a.release();a.motionChanged({matches:true})
 assert.equal(a.still.value,false);assert.equal(a.airborne.value,true)
 const y=a.pos.y;for(let i=0;i<15;i++)await a.frame();assert.ok(a.pos.y>y)
})
test('explicit gravity off, app reduced motion and safe mode still stop manual throws',async()=>{
 for(const systemReduced of [false,true])for(const block of ['gravity','reducedMotion','safe']){
  const a=app(block==='gravity'?{gravity:false}:block==='reducedMotion'?{reducedMotion:true}:{},builtinPack,systemReduced)
  if(block==='safe')await a.setSafety(true)
  await drag(a);await a.release();const pos={...a.pos};await a.frame()
  assert.equal(a.airborne.value,false);assert.deepEqual(a.pos,pos);assert.equal(a.getBody().vx,0)
 }
})
test('app motion controls cancel an in-flight throw even when system motion is reduced',async()=>{
 for(const patch of [{gravity:false},{reducedMotion:true}]){
  const a=app({},builtinPack,true);await drag(a);await a.release();assert.equal(a.airborne.value,true)
  await a.preferencesChanged(patch);const pos={...a.pos};await a.frame()
  assert.equal(a.airborne.value,false);assert.equal(a.getBody().vx,0);assert.deepEqual(a.pos,pos)
 }
})
test('a disappearing window continues gravity with system reduced motion enabled',async()=>{
 const a=app({},builtinPack,true);a.setSurfaces([{id:1,left:0,right:1920,top:700}])
 await drag(a);await a.release();for(let i=0;i<120;i++)await a.frame()
 assert.equal(a.getBody().y+a.getBody().height,700);assert.equal(a.airborne.value,false)
 a.setSurfaces([]);for(let i=0;i<120;i++)await a.frame()
 assert.equal(a.getBody().y+a.getBody().height,a.getBounds().bottom);assert.equal(a.airborne.value,false)
})
test('all fourteen panel actions retain video selection and advancing PNG playback when Windows animations are off',async()=>{
 for(const action of ['greet','happy','pat','poke','tail','eat','rest','wake','sweep','think','look','blink','talk','annoyed'])for(const videoAnimations of [true,false]){
  const a=app({videoAnimations,musicMode:'off'},builtinPack,true)
  a.requestAction(action);await a.frame()
  assert.equal(a.media.value.clip.slot,action,action)
  assert.equal(!!a.video.value,videoAnimations,`${action}: video setting`)
  const frames=new Set([a.media.value.frame])
  for(let i=0;i<36;i++){await a.frame();frames.add(a.media.value.frame)}
  assert.ok(frames.size>1,`${action}: animated frames must advance`)
  assert.equal(a.still.value,false);assert.equal(a.error.value,'')
 }
})
test('system motion preference changes leave an active action and its playback request intact',async()=>{
 const a=app({videoAnimations:false,musicMode:'off'},builtinPack)
 a.requestAction('greet');await a.frame();const revision=a.media.value.revision,frames=new Set([a.media.value.frame])
 for(const matches of [true,false,true]){
  a.motionChanged({matches});await a.frame(450);frames.add(a.media.value.frame)
  assert.equal(a.activity.value,'greet');assert.equal(a.media.value.clip.slot,'greet');assert.equal(a.media.value.revision,revision);assert.equal(a.still.value,false)
 }
 assert.ok(frames.size>1)
})
test('app reduced motion and safe mode pause actions and music and allow playback after recovery',async()=>{
 for(const systemReduced of [false,true])for(const control of ['reducedMotion','safe']){
  const a=app({videoAnimations:false},builtinPack,systemReduced)
  if(control==='safe')await a.setSafety(true);else await a.preferencesChanged({reducedMotion:true})
  a.requestAction('greet');await a.frame();const poster=a.media.value.frame
  for(let i=0;i<10;i++)await a.frame(100)
  assert.equal(a.still.value,true);assert.equal(a.video.value,undefined);assert.equal(a.musicEnabled.value,false);assert.equal(a.media.value.frame,poster)
  if(control==='safe')await a.setSafety(false);else await a.preferencesChanged({reducedMotion:false})
  await a.preferencesChanged({videoAnimations:true});a.requestAction('greet');await a.frame()
  assert.equal(a.still.value,false);assert.ok(a.video.value);assert.equal(a.musicEnabled.value,true)
 }
 const b=app({},builtinPack,true),audio={...musicModule.silentMusic(),active:true,bpm:120,confidence:1,level:.5}
 b.setMusic(audio);await b.frame();await b.frame(150);assert.equal(b.musicScene.value.showing,true)
 await b.preferencesChanged({reducedMotion:true});await b.frame();assert.equal(b.musicScene.value.showing,false)
})
test('a video failure keeps animated PNG fallback and a manual retry restores video with Windows animations off',async()=>{
 const a=app({},builtinPack,true);a.requestAction('greet');await a.frame()
 const url=a.video.value;assert.ok(url);a.videoFailed(url,'decode');await a.frame()
 assert.equal(a.video.value,undefined);assert.ok(a.error.value.includes('帧图'))
 const frames=new Set();for(let i=0;i<20;i++){await a.frame(60);frames.add(a.media.value.frame)}
 assert.ok(frames.size>1)
 a.requestAction('greet');await a.frame();assert.equal(a.video.value,url);assert.equal(a.error.value,'')
})
test('capture cancellation and duplicate release preserve falling instead of clearing velocity',async()=>{const a=app();await drag(a);await a.cancel();const before=a.getBody();await a.release();assert.equal(a.airborne.value,true);assert.equal(a.dragging.value,false);assert.equal(a.getBody().vx,before.vx)})
test('small cursor steps accumulate into a drag',async()=>{const a=app();await a.press();for(let i=0;i<20;i++)await a.move(1,0);assert.ok(a.pos.x>430);await a.release();assert.equal(a.airborne.value,true)})
test('disabled gravity and reduced motion allow drag but stop after release',async()=>{for(const preferences of [{gravity:false},{reducedMotion:true}]){const a=app(preferences);await drag(a);await a.release();const pos={...a.pos};await a.frame();assert.equal(a.airborne.value,false);assert.deepEqual(a.pos,pos);assert.equal(a.getBody().vx,0)}})
test('lost mouse-up polling releases a moved pet',async()=>{const a=app();await drag(a);a.setHeld(false);a.age();await a.poll();for(let i=0;i<8;i++)await Promise.resolve();assert.equal(a.dragging.value,false);assert.equal(a.airborne.value,true)})
test('cancellation before moving does not throw or launch Codex',async()=>{const a=app();await a.press();await a.release();await a.frame();assert.equal(a.airborne.value,false);assert.equal(a.dragging.value,false);assert.equal(a.timers.length,0)})
test('release waits for the last position write before reading its origin',async()=>{const a=app();await a.press();const started=a.deferMove();const move=a.move();await started;let finished=false;const release=a.release().then(()=>{finished=true});await Promise.resolve();assert.equal(finished,false);a.finishMove();await move;await release;assert.equal(a.airborne.value,true);assert.equal(a.getBody().x,a.pos.x)})
test('a failed native move cancels motion and reports a recoverable error',async()=>{const a=app();await a.press();a.failMove();await a.move();assert.equal(a.dragging.value,false);assert.equal(a.airborne.value,false);assert.ok(a.error.value.includes('移动已暂停'))})

test('active music immediately yields to dragging, throwing and Codex working feedback',async()=>{
 const a=app(),audio={...musicModule.silentMusic(),active:true,bpm:120,confidence:1,level:.5}
 a.setMusic(audio);await a.frame();await a.frame(150);assert.equal(a.musicScene.value.showing,true)
 await drag(a);assert.equal(a.musicScene.value.showing,false);await a.release();assert.equal(a.airborne.value,true)
 const b=app();b.setMusic(audio);await b.frame();await b.frame(150);assert.equal(b.musicScene.value.showing,true)
 b.codexStatus.value='working';await b.frame();assert.equal(b.musicScene.value.showing,false);assert.equal(b.error.value,'')
})

test('following keeps one walk playback request alive across whole video and PNG cycles',async()=>{
 for(const videoAnimations of [true,false]){
  const a=app({movement:'follow',hoverPause:false,videoAnimations,musicMode:'off'},builtinPack)
  a.setCursor(1800);await a.frame();await a.frame()
  assert.equal(a.media.value.clip.slot,'walk-right')
  const revision=a.media.value.revision,frames=new Set([a.media.value.frame]),x=a.pos.x
  for(let i=0;i<125;i++){await a.frame();frames.add(a.media.value.frame);assert.equal(a.media.value.revision,revision,`video=${videoAnimations}, frame=${i}`)}
  assert.ok(frames.size>=6,'walk frames must advance instead of repeatedly restarting')
  assert.ok(a.pos.x>x+180);assert.equal(a.activity.value,'walk');assert.equal(a.error.value,'')
 }
})

test('following turns once, stops near cursor and resumes without stealing manual or Codex feedback',async()=>{
 const a=app({movement:'follow',hoverPause:false,musicMode:'off'},builtinPack)
 a.setCursor(1800);await a.frame();await a.frame();const right=a.media.value.revision
 a.setCursor(-300);await a.frame();await a.frame();assert.equal(a.media.value.clip.slot,'walk-left');assert.ok(a.media.value.revision>right)
 const left=a.media.value.revision;for(let i=0;i<8;i++){await a.frame();assert.equal(a.media.value.revision,left)}
 a.setCursor(a.getBody().x+80);await a.frame();await a.frame();assert.equal(a.activity.value,'idle');const stopped=a.pos.x
 await a.frame();assert.equal(a.pos.x,stopped)
 a.requestAction('greet');a.setCursor(1800);await a.frame();assert.equal(a.activity.value,'greet');assert.equal(a.pos.x,stopped)
 const b=app({movement:'follow',hoverPause:false,musicMode:'off'},builtinPack);b.setCursor(1800);b.codexStatus.value='working';await b.frame();assert.equal(b.pos.x,420)
})

test('visible fullscreen follows and wanders; fullscreen hiding pauses and then resumes',async()=>{
 for(const movement of ['follow','wander']){
  const a=app({movement,fullscreenHide:false,hoverPause:false,musicMode:'off',videoAnimations:false},builtinPack)
  if(movement==='follow')a.setCursor(1800);else a.requestAction('walk')
  a.setFullscreen(true);await a.frame();await a.frame();const x=a.pos.x,frame=a.media.value.frame
  for(let i=0;i<15;i++)await a.frame()
  assert.ok(a.pos.x>x,`${movement} must move in visible fullscreen`);assert.notEqual(a.media.value.frame,frame)
  a.state.value.preferences.fullscreenHide=true;await a.frame();const hiddenX=a.pos.x,hiddenFrame=a.media.value.frame
  for(let i=0;i<8;i++)await a.frame();assert.equal(a.pos.x,hiddenX);assert.equal(a.media.value.frame,hiddenFrame)
  a.setFullscreen(false);for(let i=0;i<8;i++)await a.frame();assert.ok(a.pos.x>hiddenX);assert.equal(a.error.value,'')
 }
})

test('fullscreen does not prevent a scheduled autonomous walk from starting',async()=>{
 const a=app({movement:'wander',wander:true,fullscreenHide:false,hoverPause:false,musicMode:'off'},builtinPack)
 a.setFullscreen(true);await a.frame(12001);assert.equal(a.activity.value,'walk')
 await a.frame();assert.equal(a.media.value.clip.slot,'walk-right');const x=a.pos.x
 for(let i=0;i<5;i++)await a.frame();assert.ok(a.pos.x>x)
})

test('leaving fullscreen resyncs a relocated window and restores mouse interaction',async()=>{
 const a=app({fullscreenHide:true})
 await a.refreshGeometry();await a.updateHitTest();assert.equal(a.cursorIgnored(),false)
 a.setDesktop(true);await a.poll();await a.updateHitTest();assert.equal(a.cursorIgnored(),true)
 a.pos.x=900;a.pos.y=500;a.setCursor(980,600);a.setDesktop(false)
 await a.poll();await a.updateHitTest();assert.equal(a.cursorIgnored(),false);assert.equal(a.getBody().x,900)
 await a.press();assert.equal(a.dragging.value,true);await a.release()
})

test('fullscreen exit reapplies mouse mode even when Windows changed its cached native state',async()=>{
 const a=app({fullscreenHide:false});await a.refreshGeometry();await a.updateHitTest()
 a.setDesktop(true);await a.poll();await a.updateHitTest();assert.equal(a.cursorIgnored(),false)
 a.resetNativeCursorMode(true);a.setDesktop(false);await a.poll();await a.updateHitTest();assert.equal(a.cursorIgnored(),false)
 a.state.value.preferences.passthrough=true;await a.updateHitTest();assert.equal(a.cursorIgnored(),true)
})
