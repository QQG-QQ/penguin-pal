import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {createHash} from 'node:crypto'
import ts from 'typescript'
const source=readFileSync(new URL('../src/lib/companionMusic.ts',import.meta.url),'utf8')
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText
const {musicAllowed,musicRate,MusicScene,MusicClock,silentMusic}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'))
const normal={enabled:true,reduced:false,quiet:false,visible:true,hidden:false,codex:'idle',pressed:false,falling:false,panel:false,manual:false,activity:'idle'}

test('music yields to physics, direct interaction and every Codex feedback state',()=>{
 assert.equal(musicAllowed(normal),true)
 for(const codex of ['working','waiting','toolError','ended','interrupted'])assert.equal(musicAllowed({...normal,codex}),false,codex)
 for(const key of ['reduced','quiet','hidden','pressed','falling','panel','manual'])assert.equal(musicAllowed({...normal,[key]:true}),false,key)
 for(const key of ['enabled','visible'])assert.equal(musicAllowed({...normal,[key]:false}),false,key)
 for(const activity of ['land','dizzy'])assert.equal(musicAllowed({...normal,activity}),false)
})
test('rate follows credible tempo with bounded half/double time and invalid input fallback',()=>{
 assert.equal(musicRate(120,1),1);assert.equal(musicRate(150,1),1.25);assert.equal(musicRate(96,1),.8)
 assert.equal(musicRate(240,1),1);assert.equal(musicRate(60,1),1)
 assert.equal(musicRate(180,1),1.25);assert.equal(musicRate(6000/33,1),1.25)
 for(const bpm of [NaN,Infinity,-1,0,999])assert.equal(musicRate(bpm,1),1)
 assert.equal(musicRate(150,.1),1);assert.equal(musicRate(150,NaN),1)
})
test('scene changes only at zero opacity and cancels immediately for a higher priority action',()=>{
 const s=new MusicScene();s.sample(true,0);assert.equal(s.showing,false)
 s.sample(true,70);assert.equal(s.opacity,.5);assert.equal(s.showing,false)
 s.sample(true,140);assert.equal(s.showing,true);assert.equal(s.opacity,0)
 s.sample(true,320);assert.equal(s.opacity,1)
 s.sample(false,330,true);assert.equal(s.showing,false);assert.equal(s.opacity,1)
 s.sample(true,340);s.sample(false,480);assert.equal(s.showing,false)
})
test('clock uses one smooth rate for video and frame fallback; pause never advances motion',()=>{
 const c=new MusicClock();const audio={...silentMusic(),active:true,bpm:150,confidence:.9,level:1,pulse:1,bass:.5,treble:.3}
 for(let n=0;n<120;n++)c.step(audio,1/60)
 assert.ok(c.rate>1.24&&c.rate<=1.25);assert.ok(c.elapsed>2000&&c.elapsed<2500)
 const at=c.elapsed;c.step(audio,.1,true);assert.equal(c.elapsed,at)
 c.step({...audio,pulse:NaN,level:Infinity},.016);assert.ok(Number.isFinite(c.level)&&Number.isFinite(c.pulse))
})
test('DJ source assets are transparent, complete and match their authoring provenance',()=>{
 const root=new URL('../penguin/music-dj/',import.meta.url),meta=JSON.parse(readFileSync(new URL('media.json',root),'utf8'))
 assert.equal(meta.fps,24);assert.equal(meta.frames.length,meta.frameCount);assert.ok(meta.bridgeFrames>0)
 for(const [name,hash] of Object.entries(meta.hashes))assert.equal(createHash('sha256').update(readFileSync(new URL(name,root))).digest('hex'),hash,name)
 for(const name of meta.frames){const p=readFileSync(new URL(name,root));assert.equal(p.readUInt32BE(16),256);assert.equal(p.readUInt32BE(20),256);assert.equal(p[25],6)}
})
