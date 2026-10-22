import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync,existsSync} from 'node:fs'
import {createHash} from 'node:crypto'
import ts from 'typescript'
const read=name=>readFileSync(new URL('../'+name,import.meta.url),'utf8')
async function module(name){return import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(read(`src/lib/${name}.ts`),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText).toString('base64')}`)}
const core=await module('companionCore'),media=await module('companionMedia'),pack=JSON.parse(read('penguin/companion-v4/motions.json'))
test('merged historical packs migrate to builtin without resetting user preferences',()=>{
 for(const id of ['penguin-greeting-video','doubao-walk-right-20261021','doubao-walk-pair-20261021','doubao-core-20261021','doubao-gray-20261021']){
  const before=core.defaultCompanion(123);Object.assign(before.preferences,{animationPack:id,size:.8,movement:'follow',quiet:true,videoAnimations:false})
  const after=core.recoverNeeds(before,123);assert.equal(after.preferences.animationPack,'builtin');assert.deepEqual({...after.preferences,animationPack:id},before.preferences);assert.equal(before.preferences.animationPack,id)
 }
 const custom=core.defaultCompanion(123);custom.preferences.animationPack='my-custom-pet';assert.equal(core.recoverNeeds(custom,123).preferences.animationPack,'my-custom-pet');assert.equal(core.isMergedBuiltinPack('my-custom-pet'),false)
})
test('long actions finish their builtin video, unknown and invalid durations keep safe defaults',()=>{
 assert.equal(media.motionDuration(pack,'happy'),pack.clips.find(c=>c.slot==='happy').durationMs+100)
 assert.equal(media.motionDuration(pack,'greet'),pack.clips.find(c=>c.slot==='greet').durationMs+100)
 assert.equal(media.motionDuration(pack,'blink'),3500);assert.equal(media.motionDuration(undefined,'greet'),3500)
 assert.equal(media.motionDuration(pack,'land',500),Math.max(500,pack.clips.find(c=>c.slot==='land').durationMs+100))
 assert.equal(media.motionDuration(pack,'look',500),pack.clips.find(c=>c.slot==='look').durationMs+100)
 assert.equal(media.motionDuration(pack,'rest',45000),45000)
 for(const durationMs of [NaN,Infinity,-1,0])assert.equal(media.motionDuration({clips:[{slot:'greet',durationMs}]},'greet'),3500)
 assert.equal(media.motionDuration({clips:[{slot:'greet',durationMs:999999}]},'greet'),15000)
})
test('single builtin provides every activity and Codex alias with real local media',()=>{
 assert.equal(pack.id,'builtin');assert.equal(pack.name,'内置企鹅');assert.equal(new Set(pack.clips.map(c=>c.slot)).size,26)
 for(const slot of media.motionSlots){const c=pack.clips.find(c=>c.slot===slot);assert.ok(c,slot);for(const s of [c,c.enter,c.exit].filter(Boolean)){assert.ok(s.frames.length>0);for(const name of [s.poster,s.video,...s.frames]){assert.match(name,/^[a-z0-9-]+\.(png|webm)$/);assert.ok(existsSync(new URL('../penguin/companion-v4/'+name,import.meta.url)),name)}}}
 for(const [alias,slot] of [['working','think'],['waiting','look'],['ended','happy'],['interrupted','look'],['toolError','annoyed']])assert.equal(pack.clips.find(c=>c.slot===alias).video,pack.clips.find(c=>c.slot===slot).video)
 assert.equal(pack.clips.find(c=>c.slot==='blink').loop,false);assert.equal(pack.clips.find(c=>c.slot==='rest').loop,true)
})

test('seven real-source actions include full-rate enter and exit fallback sequences',()=>{
 for(const slot of ['poke','tail','eat','rest','sweep','talk','annoyed']){
  const c=pack.clips.find(c=>c.slot===slot)
  for(const s of [c,c.enter,c.exit]){assert.ok(s,slot);assert.equal(s.frameMs,1000/24);assert.ok(Math.abs(s.durationMs-s.frames.length*s.frameMs)<.001,slot)}
  assert.equal(c.loop,['tail','rest','sweep','talk'].includes(slot));assert.deepEqual(pack.clips.find(c=>c.slot==='toolError').exit,pack.clips.find(c=>c.slot==='annoyed').exit)
 }
})
test('all static fallbacks use the unified artwork and have valid transparent bounds',()=>{
 const art=JSON.parse(read('penguin/companion-v4/artwork.json'))
 for(const key of ['idle','drag','fall','pat-soft','sleep','eat-bite','wave-0','walk-left-0'])assert.ok(art[key],key)
 for(const frame of Object.values(art)){assert.equal(frame.width,256);assert.equal(frame.height,256);assert.ok(frame.alphaBBox.left<frame.alphaBBox.right);assert.ok(frame.alphaBBox.top<frame.alphaBBox.bottom);assert.ok(existsSync(new URL('../penguin/companion-v4/'+frame.file,import.meta.url)))}
 const provenance=JSON.parse(read('penguin/companion-v4/asset-provenance.json'));for(const [name,sha] of Object.entries(provenance.hashes))assert.equal(createHash('sha256').update(readFileSync(new URL('../penguin/companion-v4/'+name,import.meta.url))).digest('hex'),sha,name)
})
