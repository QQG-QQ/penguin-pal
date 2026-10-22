import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'

async function module(name){const source=readFileSync(new URL(`../src/lib/${name}.ts`,import.meta.url),'utf8');return import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText).toString('base64')}`)}
const core=await module('companionCore'),media=await module('companionMedia'),animation=await module('companionAnimation')
const mediaJs=ts.transpileModule(readFileSync(new URL('../src/lib/companionMedia.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText,mediaUrl='data:text/javascript;base64,'+Buffer.from(mediaJs).toString('base64')
const sequenceJs=ts.transpileModule(readFileSync(new URL('../src/lib/companionSequence.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText.replace("'./companionMedia'",JSON.stringify(mediaUrl)),sequence=await import('data:text/javascript;base64,'+Buffer.from(sequenceJs).toString('base64'))
// Run the production component handlers and frame loop with deterministic native IPC.
// Pointer capture delivery and Vue rendering are additionally checked in the browser.
function app(preferences={}){
 const component=readFileSync(process.env.PENGUIN_COMPANION_SOURCE??new URL('../src/CompanionApp.vue',import.meta.url),'utf8')
 const cancelled=component.match(/@pointercancel="([^"]+)"/)[1]
 const source=component.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^import[\s\S]*?from ['"][^'"]+['"]\r?$/mg,'')
 const js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText
 let clock=0,held=true,defer=false,completeMove,moveStarted,failMove=false
 const pos={x:420,y:160},cursor={x:520,y:260},writes=[],timers=[]
 const win={scaleFactor:async()=>1,outerPosition:async()=>({...pos}),isVisible:async()=>true,
  async setPosition(next){if(failMove)throw Error('position unavailable');if(defer){moveStarted?.();await new Promise(ok=>{completeMove=ok})}Object.assign(pos,next);writes.push({...pos})}}
 const invoke=async command=>{if(command==='companion_poll')return{safeMode:false,bridge:{status:'idle',lastReceived:0},desktop:{surfaces:[],context:'quiet',fullscreen:false,leftButtonDown:held,idleSeconds:0}}}
 const window={__TAURI_INTERNALS__:{},location:{search:''},matchMedia:()=>({matches:false}),clearTimeout(){},setTimeout(fn){timers.push(fn);return timers.length}}
 const metrics={petLeft:0,petTop:0,petRight:160,petBottom:220}
 const deps={...core,...media,...animation,...sequence,window,document:{querySelectorAll:()=>[]},performance:{now:()=>clock},ref:value=>({value}),computed:fn=>({get value(){return fn()}}),onMounted(){},onBeforeUnmount(){},
  invoke,listen:async()=>()=>{},convertFileSrc:p=>p,cursorPosition:async()=>({...cursor}),currentMonitor:async()=>({workArea:{position:{x:0,y:0},size:{width:1920,height:1040}}}),primaryMonitor:async()=>null,getCurrentWindow:()=>win,
  PhysicalPosition:class{constructor(x,y){this.x=x;this.y=y}},PhysicalSize:class{},computePetLayoutMetrics:()=>metrics,isPetOpaqueHit:()=>true,loadPetAlphaMask:async()=>null,
  builtinMotionPack:{id:'builtin',clips:[]},companionArtwork:()=>({src:'poster.png'})}
 const run=new Function('deps',`const {${Object.keys(deps).join(',')}}=deps;\n${js}\ninitialized=true;img.value={getBoundingClientRect:()=>({left:0,top:0,width:160,height:220})};return {state,activity,airborne,dragging,pointerDown,release,frame,poll,error,cancel:()=>{return ${cancelled.endsWith('()')?cancelled:cancelled+'()'}},age(){heldAt=Date.now()-2000},getBody:()=>({...body})};`)(deps)
 Object.assign(run.state.value.preferences,{gravity:true,edgeDock:false,springDrag:false,wander:false,movement:'still',...preferences})
 const press=()=>run.pointerDown({button:0,pointerId:1,clientX:80,clientY:100,currentTarget:{setPointerCapture(){}}})
 const frame=async(ms=33)=>{clock+=ms;await run.frame()}
 return {...run,pos,writes,timers,press,frame,async move(x=14,y=-4,ms=40){cursor.x+=x;cursor.y+=y;await frame(ms)},setHeld(value){held=value},deferMove(){defer=true;return new Promise(ok=>{moveStarted=ok})},finishMove(){defer=false;completeMove()},failMove(){failMove=true}}
}
async function drag(a){await a.press();for(let i=0;i<6;i++)await a.move()}
test('normal release retains throw velocity and enters gravity',async()=>{const a=app();await drag(a);await a.release();assert.equal(a.airborne.value,true);assert.ok(a.getBody().vx>0);const y=a.pos.y;for(let i=0;i<15;i++)await a.frame();assert.ok(a.pos.y>y)})
test('capture cancellation and duplicate release preserve falling instead of clearing velocity',async()=>{const a=app();await drag(a);await a.cancel();const before=a.getBody();await a.release();assert.equal(a.airborne.value,true);assert.equal(a.dragging.value,false);assert.equal(a.getBody().vx,before.vx)})
test('small cursor steps accumulate into a drag',async()=>{const a=app();await a.press();for(let i=0;i<20;i++)await a.move(1,0);assert.ok(a.pos.x>430);await a.release();assert.equal(a.airborne.value,true)})
test('disabled gravity and reduced motion allow drag but stop after release',async()=>{for(const preferences of [{gravity:false},{reducedMotion:true}]){const a=app(preferences);await drag(a);await a.release();const pos={...a.pos};await a.frame();assert.equal(a.airborne.value,false);assert.deepEqual(a.pos,pos);assert.equal(a.getBody().vx,0)}})
test('lost mouse-up polling releases a moved pet',async()=>{const a=app();await drag(a);a.setHeld(false);a.age();await a.poll();for(let i=0;i<8;i++)await Promise.resolve();assert.equal(a.dragging.value,false);assert.equal(a.airborne.value,true)})
test('cancellation before moving does not throw or launch Codex',async()=>{const a=app();await a.press();await a.release();await a.frame();assert.equal(a.airborne.value,false);assert.equal(a.dragging.value,false);assert.equal(a.timers.length,0)})
test('release waits for the last position write before reading its origin',async()=>{const a=app();await a.press();const started=a.deferMove();const move=a.move();await started;let finished=false;const release=a.release().then(()=>{finished=true});await Promise.resolve();assert.equal(finished,false);a.finishMove();await move;await release;assert.equal(a.airborne.value,true);assert.equal(a.getBody().x,a.pos.x)})
test('a failed native move cancels motion and reports a recoverable error',async()=>{const a=app();await a.press();a.failMove();await a.move();assert.equal(a.dragging.value,false);assert.equal(a.airborne.value,false);assert.ok(a.error.value.includes('移动已暂停'))})
