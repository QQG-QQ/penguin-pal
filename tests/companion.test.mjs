import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import ts from 'typescript'
import { sanitize, emit } from '../src-tauri/resources/companion/codex-events.mjs'
const source = readFileSync(new URL('../src/lib/companionCore.ts', import.meta.url), 'utf8')
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText
const core = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)
const animationSource = readFileSync(new URL('../src/lib/companionAnimation.ts', import.meta.url), 'utf8')
const animationCode = ts.transpileModule(animationSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText
const animation = await import(`data:text/javascript;base64,${Buffer.from(animationCode).toString('base64')}`)
const bounds = { left: 0, top: 0, right: 1000, bottom: 800 }
const body = { x: 100, y: 100, vx: 0, vy: 0, width: 100, height: 100 }

// Controlled scenario registry: fixed probes, no model-generated actions or shell commands.
export const cases = [
  { id: 'care-feed', suite: 'companion', feature: 'care', tags: ['happy'], run() {
    const state = core.defaultCompanion(0), result = core.care(state, 'feed')
    assert.equal(result.state.hunger, 2); assert.equal(state.hunger, 20)
    assert.equal(core.care(result.state, 'feed', 4).state.hunger, 2)
  } },
  { id: 'care-boundaries', suite: 'companion', feature: 'care', tags: ['boundary'], run() {
    assert.equal(core.care({ ...core.defaultCompanion(0), affection: 100 }, 'pat').state.affection, 100)
    assert.equal(core.care(core.defaultCompanion(0), 'poke', 3).activity, 'annoyed')
    assert.equal(core.hitRegion(20, 0, 100), 'pat'); assert.equal(core.hitRegion(80, 0, 100), 'poke')
  } },
  { id: 'needs-offline', suite: 'companion', feature: 'recovery', tags: ['transition'], run() {
    const state = core.defaultCompanion(100000)
    assert.equal(core.recoverNeeds(state, 0).hunger, 20)
    assert.equal(core.recoverNeeds(state, 1e12).hunger, 68)
  } },
  { id: 'activity-priority', suite: 'companion', feature: 'scheduler', tags: ['transition'], run() {
    const state = core.defaultCompanion(0); state.preferences.wander = true
    assert.equal(core.chooseActivity(state, 0.1, false), 'walk')
    assert.equal(core.chooseActivity(state, 0.1, true), 'idle')
    state.preferences.quiet = true; assert.equal(core.chooseActivity(state, 0.1, false), 'idle')
  } },
  { id: 'reduced-motion', suite: 'companion', feature: 'accessibility', tags: ['boundary'], run() {
    const state = core.defaultCompanion(0); state.preferences.reducedMotion = true
    assert.equal(core.chooseActivity(state, 0.01, false), 'idle')
  } },
  { id: 'physics-floor', suite: 'companion', feature: 'physics', tags: ['happy'], run() {
    let current = { ...body }, result
    for (let n = 0; n < 400; n++) { result = core.stepPhysics(current, bounds, [], 0.033); current = result.body }
    assert.equal(result.landed, true); assert.equal(current.y, 700); assert.equal(current.vy, 0)
  } },
  { id: 'physics-wall-top', suite: 'companion', feature: 'physics', tags: ['boundary'], run() {
    let result = core.stepPhysics({ ...body, x: 899, vx: 2200, y: 0, vy: -2200 }, bounds, [], 10)
    assert.equal(result.body.x, 900); assert.ok(result.body.vx < 0)
    assert.equal(result.body.y, 0); assert.ok(result.body.vy > 0)
    assert.throws(() => core.stepPhysics({ ...body, x: NaN }, bounds, [], .02))
  } },
  { id: 'physics-platform', suite: 'companion', feature: 'surfaces', tags: ['transition'], run() {
    const platform = { id: 1, left: 50, right: 500, top: 250, bottom: 600 }
    const result = core.stepPhysics({ ...body, y: 145, vy: 200 }, bounds, [platform], .05)
    assert.equal(result.surface, 1); assert.equal(result.body.y, 150)
    assert.equal(core.stepPhysics({ ...body, y: 300, vy: 50 }, bounds, [platform], .05).surface, null)
    const next = core.followSurface(result.body, platform, { ...platform, left: 70, top: 280 }, bounds)
    assert.equal(next.x, result.body.x + 20); assert.equal(next.y, 180)
    assert.equal(core.followSurface(result.body, platform, undefined, bounds), null)
    assert.equal(core.followSurface(result.body, platform, { ...platform, left: 1200, right: 1400 }, bounds), null)
    assert.equal(core.followSurface(result.body, platform, { ...platform, top: 20 }, bounds), null)
  } },
  { id: 'codex-presentation', suite: 'companion', feature: 'codex', tags: ['transition'], run() {
    assert.equal(core.codexPresentation('waiting').mode, 'listening')
    assert.equal(core.codexPresentation('ended').label, 'Codex 本轮已结束')
    assert.equal(core.codexPresentation('unknown').mode, 'idle')
  } },
  { id:'dock-negative-monitor-and-reveal',suite:'companion',feature:'dock',tags:['boundary','transition'],run(){
    const area={left:-1920,right:0,top:0,bottom:1080},home={x:-1910,y:300},foot={x:50,y:60,width:170,height:220}
    assert.equal(core.nearEdge(home,area,280,290),'left')
    assert.equal(core.nearEdge({x:-700,y:300},area,280,290),'none')
    const shown=core.dockPosition('left',home,area,280,290,foot,false),hidden=core.dockPosition('left',home,area,280,290,foot,true)
    assert.equal(shown.x,-1920);assert.ok(hidden.x+foot.x<area.left);assert.ok(hidden.x+foot.x+foot.width>area.left)
    const right=core.dockPosition('right',home,area,280,290,foot,true);assert.ok(right.x+foot.x<area.right);assert.ok(right.x+foot.x+foot.width>area.right)
    const top=core.dockPosition('top',home,area,280,290,foot,true);assert.ok(top.y+foot.y<area.top);assert.ok(top.y+foot.y+foot.height>area.top)
    assert.equal(core.dockPosition('right',home,area,280,290,foot,false).x,-280)
  }},
  {id:'animation-enter-exit-interruption',suite:'companion',feature:'animation',tags:['transition'],run(){
    const player=new animation.AnimationPlayer(),walk=animation.selectClip('walk','idle',false,false,-1),idle=animation.selectClip('idle','idle',false,false,1)
    assert.equal(player.sample(walk,0,false),'idle');assert.equal(player.phase,'enter')
    player.sample(walk,400,false);assert.equal(player.phase,'body')
    player.sample(idle,600,false);assert.equal(player.phase,'exit')
    const drag=animation.selectClip('idle','idle',true,false,1);assert.equal(player.sample(drag,650,false),'drag');assert.equal(player.phase,'body')
    assert.equal(player.sample(walk,700,true),'walk-left-0');assert.equal(player.phase,'body')
  }},
  {id:'idle-rate-does-not-slow-needs',suite:'companion',feature:'performance',tags:['boundary'],run(){
    const fast=core.defaultCompanion(0),slow=core.defaultCompanion(0)
    for(let i=0;i<100;i++)core.advanceNeeds(fast,.01,false)
    core.advanceNeeds(slow,1,false);assert.ok(Math.abs(fast.energy-slow.energy)<.00001);assert.ok(Math.abs(fast.hunger-slow.hunger)<.00001)
    assert.equal(core.frameDelay(false,false,false),200);assert.equal(core.frameDelay(false,true,false),33);assert.equal(core.frameDelay(true,true,true),1000)
    assert.equal(core.relationship(90),'亲近')
  }},
  {id:'tool-issue-only-explicit-metadata',suite:'bridge',feature:'metadata',tags:['security','failure'],run(){
    const payload={hook_event_name:'PostToolUse',session_id:'s',turn_id:'t',tool_response:{isError:true,content:'SECRET'}}
    const result=sanitize(payload,123);assert.equal(result.issue,true);assert.ok(!JSON.stringify(result).includes('SECRET'))
    assert.equal(sanitize({...payload,tool_response:'error text'},123).issue,undefined)
    assert.equal(sanitize({...payload,hook_event_name:'Stop'},123).issue,undefined)
  }},
  {id:'concurrent-event-queue-bounded',suite:'bridge',feature:'queue',tags:['boundary'],async run(){
    const root=await mkdtemp(join(tmpdir(),'penguin-burst-'))
    try{await writeFile(join(root,'bridge-enabled.json'),'true');await Promise.all(Array.from({length:150},(_,i)=>emit(root,{hook_event_name:'PostToolUse',session_id:'s',turn_id:String(i)})));assert.ok((await readdir(join(root,'events'))).filter(n=>n.endsWith('.json')).length<=128)}
    finally{await rm(root,{recursive:true,force:true})}
  }},
  { id: 'animation-priority-and-frames', suite: 'companion', feature: 'animation', tags: ['transition','boundary'], run() {
    const manifest = JSON.parse(readFileSync(new URL('../penguin/companion-v2/manifest.json', import.meta.url), 'utf8'))
    assert.equal(Object.keys(manifest).length, 40)
    for (const action of ['idle','blink','look','think','happy','greet','pat','poke','tail','eat','sweep','rest','wake','talk','annoyed','dizzy','land']) {
      const clip = animation.selectClip(action,'idle',false,false,1)
      for (const frame of clip.frames) {
        assert.ok(manifest[frame], `${action}/${frame} is missing`)
        const png = readFileSync(new URL(`../penguin/companion-v2/${frame}.png`, import.meta.url))
        assert.equal(png.readUInt32BE(16),256); assert.equal(png.readUInt32BE(20),256)
      }
    }
    const left = animation.selectClip('walk','idle',false,false,-1), right = animation.selectClip('walk','idle',false,false,1)
    assert.equal(left.frames.length,4); assert.equal(right.frames.length,4)
    assert.ok(left.frames.every(key=>!right.frames.includes(key)))
    assert.equal(animation.frameAt(left,170,false),'walk-left-1');assert.equal(animation.frameAt(left,170,true),'walk-left-0')
    assert.equal(animation.selectClip('walk','working',true,false,1).frames[0],'drag')
    assert.equal(animation.selectClip('walk','working',false,true,1).frames[0],'fall')
    assert.equal(animation.selectClip('walk','working',false,false,1).frames[0],'think')
  } },
  { id: 'hook-privacy', suite: 'bridge', feature: 'metadata', tags: ['security'], run() {
    const result = sanitize({ hook_event_name: 'Stop', session_id: 'private-session', turn_id: 'turn', prompt: 'SECRET', transcript_path: 'private-path' }, 123)
    assert.deepEqual(Object.keys(result).sort(), ['at', 'event', 'session', 'turn'])
    assert.ok(!JSON.stringify(result).includes('private')); assert.ok(!JSON.stringify(result).includes('SECRET'))
    assert.equal(sanitize({ hook_event_name: 'Execute', session_id: 'x' }), null)
    assert.equal(sanitize({ hook_event_name: 'Stop', session_id: 'x', agent_id: 'child' }), null)
  } },
  { id: 'hook-queue-toggle', suite: 'bridge', feature: 'queue', tags: ['failure', 'boundary'], async run() {
    const root = await mkdtemp(join(tmpdir(), 'penguin-hooks-'))
    try {
      await writeFile(join(root, 'bridge-enabled.json'), 'true')
      for (let i = 0; i < 132; i++) await emit(root, { hook_event_name: 'Stop', session_id: 'a', turn_id: String(i) })
      assert.equal((await readdir(join(root, 'events'))).length, 128)
      await writeFile(join(root, 'bridge-enabled.json'), 'false')
      const before = await readdir(join(root, 'events'))
      await emit(root, { hook_event_name: 'UserPromptSubmit', session_id: 'a' })
      assert.deepEqual(await readdir(join(root, 'events')), before)
      const payload = JSON.parse(await readFile(join(root, 'events', before[0]), 'utf8'))
      assert.equal(payload.event, 'Stop')
    } finally { await rm(root, { recursive: true, force: true }) }
  } },
  { id: 'hook-windows-transport', suite: 'bridge', feature: 'transport', tags: ['happy', 'security'], async run() {
    if (process.platform !== 'win32') return
    const root = await mkdtemp(join(tmpdir(), "penguin hook ' "))
    try {
      await writeFile(join(root, 'bridge-enabled.json'), 'true')
      const literal = value => `'${value.replaceAll("'", "''")}'`
      const script = fileURLToPath(new URL('../src-tauri/resources/companion/codex-events.mjs', import.meta.url))
      const command = `& ${literal(process.execPath)} ${literal(script)} ${literal(root)}`
      const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-EncodedCommand', Buffer.from(command, 'utf16le').toString('base64')], {
        input: JSON.stringify({ hook_event_name: 'PermissionRequest', session_id: 'private', turn_id: 'test', prompt: 'DO NOT SAVE' }), encoding: 'utf8', timeout: 2800, windowsHide: true,
      })
      assert.ifError(result.error); assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout, ''); assert.equal(result.stderr, '')
      const files = await readdir(join(root, 'events')); assert.equal(files.length, 1)
      const payload = JSON.parse(await readFile(join(root, 'events', files[0]), 'utf8'))
      assert.equal(payload.event, 'PermissionRequest'); assert.ok(!JSON.stringify(payload).includes('DO NOT SAVE'))
    } finally { await rm(root, { recursive: true, force: true }) }
  } },
]
const selected = cases.filter(item => (!process.env.PENGUIN_TEST_SUITE || item.suite === process.env.PENGUIN_TEST_SUITE)
  && (!process.env.PENGUIN_TEST_FEATURE || item.feature === process.env.PENGUIN_TEST_FEATURE)
  && (!process.env.PENGUIN_TEST_TAG || item.tags.includes(process.env.PENGUIN_TEST_TAG)))
if (!selected.length) throw new Error('No registered scenario matched the selection')
for (const item of selected) test(`${item.suite}/${item.feature}/${item.id}`, item.run)
