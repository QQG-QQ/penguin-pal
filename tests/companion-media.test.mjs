import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {createHash} from 'node:crypto'
import ts from 'typescript'
async function module(name){const source=readFileSync(new URL(`../src/lib/${name}.ts`,import.meta.url),'utf8');return import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText).toString('base64')}`)}
const media=await module('companionMedia'),animation=await module('companionAnimation'),core=await module('companionCore')
const clip=(id,weight=1)=>({id,slot:'idle',weight,frameMs:100,loop:true,poster:'poster.png',frames:['a.png','b.png']})
test('weighted candidates avoid repeats and preserve weight boundaries',()=>{
 const clips=[clip('a',1),clip('b',3),{...clip('other'),slot:'eat'}]
 assert.equal(media.pickMotion(clips,'idle',undefined,0).id,'a');assert.equal(media.pickMotion(clips,'idle',undefined,.25).id,'b')
 assert.equal(media.pickMotion(clips,'idle','b',.99).id,'a');assert.equal(media.pickMotion([clip('a')],'idle','a',.5).id,'a')
 assert.equal(media.pickMotion(clips,'fall',undefined,0),undefined);assert.equal(media.pickMotion(clips,'idle',undefined,NaN).id,'a')
})
test('pack changes, idle rotation, frame loops and one-shot boundaries',()=>{
 const player=new media.MotionPicker(),pack={id:'one',clips:[clip('a'),clip('b')]}
 assert.equal(player.sample(pack,'idle',0,0).frame,'a.png');assert.equal(player.sample(pack,'idle',100,0).frame,'b.png');assert.equal(player.sample(pack,'idle',200,0).frame,'a.png')
 assert.equal(player.sample(pack,'idle',12001,0).clip.id,'b')
 assert.equal(player.sample({...pack,id:'two',clips:[{...clip('new'),loop:false}]},'idle',12002,0).clip.id,'new')
 assert.equal(player.sample({...pack,id:'two'},'idle',12500,0).frame,'b.png');assert.equal(player.sample(undefined,'idle',12600,0),undefined)
 player.reset();assert.equal(player.sample(pack,'idle',13000,0).clip.id,'a')
})
test('native task priorities and new greeting retain builtin fallbacks',()=>{
 assert.equal(animation.selectClip('greet','working',false,false,1).slot,'greet')
 assert.equal(animation.selectClip('idle','waiting',true,false,1).slot,'drag')
 assert.equal(animation.selectClip('idle','toolError',false,false,1).slot,'toolError')
 assert.equal(animation.selectClip('walk','idle',false,false,-1).slot,'walk-left')
 assert.equal(animation.selectClip('greet','idle',false,false,1).frames.length,8)
 const player=new animation.AnimationPlayer(),greet=animation.selectClip('greet','idle',false,false,1)
 assert.equal(player.sample(greet,0,false),'greet');assert.equal(player.sample(greet,160,false),'wave-0');player.reset();assert.equal(player.sample(greet,2000,true),'wave-0')
})
test('sampled throw velocity ignores stationary release, is capped and handles short trails',()=>{
 const trail=new media.DragTrail();assert.deepEqual(trail.velocity(0),{vx:0,vy:0})
 trail.reset({x:0,y:0,at:0});assert.deepEqual(trail.velocity(0),{vx:0,vy:0})
 trail.push({x:50,y:-25,at:50});trail.push({x:100,y:-50,at:100});assert.deepEqual(trail.velocity(100),{vx:1000,vy:-500})
 assert.deepEqual(trail.velocity(201),{vx:0,vy:0});trail.push({x:10000,y:10000,at:120});assert.deepEqual(trail.velocity(120),{vx:2200,vy:2200})
})
test('critical spring converges without oscillation and tolerates delayed frames',()=>{
 let p={position:0,velocity:0}
 for(let i=0;i<60;i++){const next=media.springStep(p.position,p.velocity,100,i===0?2:1/60);assert.ok(next.position>=p.position&&next.position<=100);p=next}
 assert.ok(Math.abs(p.position-100)<.001);assert.equal(media.springStep(10,0,20,-1).position,10)
})
test('old saved preferences acquire new defaults without losing care/movement values',()=>{
 const state=core.defaultCompanion(0);delete state.preferences.animationPack;delete state.preferences.springDrag;state.preferences.quiet=true;state.preferences.movement='follow'
 const recovered=core.recoverNeeds(state,100);assert.equal(recovered.preferences.animationPack,'builtin');assert.equal(recovered.preferences.springDrag,true);assert.equal(recovered.preferences.quiet,true);assert.equal(recovered.preferences.movement,'follow')
})

test('manual menu requests override Codex animations while automatic rest remains interruptible',()=>{
 for(const action of ['greet','happy','pat','poke','tail','eat','rest','wake','sweep','think','look','blink','talk','annoyed']){
  assert.equal(animation.selectClip(action,'working',false,false,1,true).slot,action)
 }
 assert.equal(animation.selectClip('rest','working',false,false,1,false).slot,'working')
 assert.equal(animation.selectClip('sweep','working',true,false,1,true).slot,'drag')
})
test('partial external packs use builtins without applying an external directory to builtin URLs',()=>{
 const builtin={id:'builtin',clips:[{...clip('blink'),slot:'blink'}]},external={id:'external',directory:'owned/packs/external',clips:[{...clip('happy'),slot:'happy'}]}
 const picker=new media.MotionPicker()
 assert.equal(media.motionPackForSlot(external,builtin,'happy'),external)
 assert.equal(picker.sample(external,'happy',0,0).directory,external.directory)
 const fallback=media.motionPackForSlot(external,builtin,'blink')
 assert.equal(fallback,builtin);assert.equal(picker.sample(fallback,'blink',1,0).directory,undefined)
})
test('all 14 menu actions have video assets and new generated frame sets have actual pose changes',()=>{
 const root=new URL('../penguin/companion-v3/',import.meta.url)
 const rows=JSON.parse(readFileSync(new URL('motions.json',root),'utf8'))
 for(const slot of ['happy','pat','poke','tail','eat','rest','wake','sweep','think','look','blink','talk','annoyed']){
  const row=rows.find(c=>c.slot===slot);assert.ok(row,slot);assert.ok(readFileSync(new URL(row.video,root)).length>1000,slot)
  const unique=new Set(row.frames.map(f=>createHash('sha256').update(readFileSync(new URL(f,root))).digest('hex')))
  assert.ok(unique.size>1,slot+' must not be a still-image video')
 }
 assert.ok(readFileSync(new URL('greeting.webm',root)).length>1000)
})
test('idle rest waits for active actions and respects busy/menu interaction boundaries',()=>{
 assert.equal(core.shouldIdleRest(301,2000,3000,false,'greet'),false)
 assert.equal(core.shouldIdleRest(301,3000,3000,false,'greet'),true)
 assert.equal(core.shouldIdleRest(300,5000,0,false,'idle'),false)
 assert.equal(core.shouldIdleRest(301,5000,0,true,'idle'),false)
 assert.equal(core.shouldIdleRest(301,5000,0,false,'rest'),false)
})
