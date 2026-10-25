import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'
const compile=name=>ts.transpileModule(readFileSync(new URL(`../src/lib/${name}.ts`,import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText
const uri=js=>'data:text/javascript;base64,'+Buffer.from(js).toString('base64'),mediaUri=uri(compile('companionMedia'))
const {MotionSequencer}=await import(uri(compile('companionSequence').replace("'./companionMedia'",JSON.stringify(mediaUri)))),{motionDuration}=await import(mediaUri)
const part=id=>({video:id+'.webm',poster:id+'.png',frames:[id+'0.png',id+'1.png'],frameMs:100,durationMs:200})
const clip=(id,loop=true,phases=true)=>({...part(id),id,slot:id,weight:1,loop,...(phases?{enter:part(id+'-in'),exit:part(id+'-out')}:{})})
const pack={id:'builtin',clips:[clip('rest'),clip('talk'),clip('poke',false),clip('idle',true,false),clip('drag',true,false)]}
const sample=(p,slot,now,options={})=>p.sample(pack,slot,now,0,{video:true,...options})

test('a single idle video or PNG loop never reloads at twelve seconds',()=>{
 for(const video of [true,false]){
  const p=new MotionSequencer(),first=sample(p,'idle',0,{video})
  for(const time of [12001,24100,36300]){
   const current=sample(p,'idle',time,{video})
   assert.equal(current.revision,first.revision);assert.equal(current.frame,time%200>=100?'idle1.png':'idle0.png')
  }
 }
})
test('native phase waits for actual end, then body; metadata duration includes all phases',()=>{const p=new MotionSequencer(),a=sample(p,'rest',0);assert.equal(a.phase,'enter');assert.equal(sample(p,'rest',1000).phase,'enter');p.ended(a.revision,1000);assert.equal(sample(p,'rest',1000).phase,'body');assert.equal(motionDuration(pack,'rest',0),700)})
test('ordinary switch completes loop, exits, then starts target without hard cut',()=>{const p=new MotionSequencer();let s=sample(p,'rest',0);p.ended(s.revision,200);s=sample(p,'talk',250);assert.equal(s.clip.slot,'rest');assert.equal(s.phase,'body');assert.equal(s.clip.loop,false);p.ended(s.revision,400);s=sample(p,'talk',400);assert.equal(s.phase,'exit');p.ended(s.revision,600);s=sample(p,'talk',600);assert.equal(s.clip.slot,'talk');assert.equal(s.phase,'enter')})
test('latest queued request wins, including returning to active while exiting',()=>{const p=new MotionSequencer();let s=sample(p,'rest',0);p.ended(s.revision,200);s=sample(p,'talk',250);p.ended(s.revision,400);s=sample(p,'rest',450);assert.equal(s.phase,'exit');p.ended(s.revision,600);s=sample(p,'rest',600);assert.equal(s.clip.slot,'rest');assert.equal(s.phase,'enter')})
test('single-shot body exits automatically and holds neutral without restarting',()=>{const p=new MotionSequencer();let s=sample(p,'poke',0);p.ended(s.revision,200);s=sample(p,'poke',200);p.ended(s.revision,400);s=sample(p,'poke',400);assert.equal(s.phase,'exit');p.ended(s.revision,600);s=sample(p,'poke',600);assert.equal(s.phase,'hold');assert.equal(sample(p,'poke',900).frame,'poke-out1.png')})
test('one-shot perform clips expose hold immediately after each clip length',()=>{const p=new MotionSequencer(),make=(id,durationMs)=>({...part(id),id,slot:'perform',weight:1,loop:false,frames:[id+'.png'],frameMs:durationMs,durationMs}),pack={id:'perform',clips:[make('lion',10000),make('holo',5000)]};let s=p.sample(pack,'perform',0,0,{video:false});assert.equal(s.phase,'body');assert.equal(p.sample(pack,'perform',10000,0,{video:false}).phase,'hold');p.reset(true);s=p.sample(pack,'perform',0,.99,{video:false});assert.equal(s.phase,'body');assert.equal(p.sample(pack,'perform',5000,.99,{video:false}).phase,'hold')})
test('forced performance selection chooses the requested clip',()=>{const p=new MotionSequencer(),pack={id:'perform',clips:[{...clip('lion',false,false),slot:'perform'},{...clip('holo',false,false),slot:'perform'}]};assert.equal(p.sample(pack,'perform',0,0,{video:false,forcedId:'holo'}).clip.id,'holo')})
test('physical state preempts exit and stale ended callbacks are ignored',()=>{const p=new MotionSequencer();const old=sample(p,'rest',0);const current=sample(p,'drag',50,{immediate:true});p.ended(old.revision,100);assert.equal(sample(p,'drag',100).revision,current.revision);assert.equal(sample(p,'drag',100).clip.slot,'drag')})
test('PNG fallback completes current cycle and processes all phases',()=>{const p=new MotionSequencer();let s=sample(p,'rest',0,{video:false});s=sample(p,'rest',200,{video:false});assert.equal(s.phase,'body');s=sample(p,'talk',250,{video:false});assert.equal(s.clip.slot,'rest');s=sample(p,'talk',400,{video:false});assert.equal(s.phase,'exit');s=sample(p,'talk',600,{video:false});assert.equal(s.phase,'enter');assert.equal(s.clip.slot,'talk')})
test('hidden PNG pauses its phase clock and resumes without skipping',()=>{const p=new MotionSequencer();sample(p,'rest',0,{video:false});sample(p,'rest',100,{video:false,paused:true});assert.equal(sample(p,'rest',9000,{video:false,paused:true}).phase,'enter');assert.equal(sample(p,'rest',10000,{video:false}).phase,'enter');assert.equal(sample(p,'rest',10100,{video:false}).phase,'body')})
test('packs without phase metadata switch directly and preserve original fallback behavior',()=>{const p=new MotionSequencer(),old={id:'external',directory:'my-pack',clips:[clip('a',true,false),clip('b',false,false)]};let s=p.sample(old,'a',0,0);assert.equal(s.directory,'my-pack');s=p.sample(old,'b',100,0);assert.equal(s.clip.id,'b');assert.equal(s.phase,'body');assert.equal(p.sample(undefined,'b',200,0),undefined)})
test('reduced motion bypasses phases and uses requested static poster',()=>{const p=new MotionSequencer();sample(p,'rest',0);const s=sample(p,'talk',50,{video:false,reduced:true});assert.equal(s.phase,'body');assert.equal(s.frame,'talk.png');assert.equal(s.clip.slot,'talk')})
test('soft reset replays action via exit; hard reset cancels stale phase ownership',()=>{const p=new MotionSequencer();let s=sample(p,'rest',0);p.ended(s.revision,200);p.reset();s=sample(p,'rest',250);assert.equal(s.clip.loop,false);p.reset(true);const current=sample(p,'talk',300);p.ended(s.revision,400);assert.equal(sample(p,'talk',400).revision,current.revision)})
test('switching a failed video to PNG cannot leave the entry stuck',()=>{const p=new MotionSequencer();sample(p,'rest',0);assert.equal(sample(p,'rest',250,{video:false}).phase,'body')})
test('old play completion cannot restart the current phase clock',()=>{const p=new MotionSequencer();let s=sample(p,'rest',0);p.playing(s.revision,100);p.ended(s.revision,300);s=sample(p,'rest',300,{video:false});p.playing(s.revision-1,10000);assert.equal(sample(p,'rest',500,{video:false}).phase,'body')})

test('queued request remains pending through the prior exit and clears when it starts',()=>{const p=new MotionSequencer();let s=sample(p,'poke',0);p.ended(s.revision,200);s=sample(p,'talk',250);assert.equal(s.queued,true);p.ended(s.revision,400);s=sample(p,'talk',400);assert.equal(s.queued,true);p.ended(s.revision,600);s=sample(p,'talk',600);assert.equal(s.queued,false);assert.equal(s.clip.slot,'talk')})
