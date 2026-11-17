<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
// Procedurally drawn effects layer. Nothing here uses image assets: emotion
// symbols, status rings, weather and wipes are all canvas paths, so the layer
// costs no download and can react to live state.
type Burst = 'heart' | 'sparkle' | 'anger' | 'sweat' | 'question' | 'note' | 'zzz' | 'dust'
interface Particle { kind: Burst; x: number; y: number; vx: number; vy: number; age: number; life: number; size: number; rot: number; spin: number }
interface Drop { x: number; y: number; v: number; len: number }
interface Flake { x: number; y: number; v: number; sway: number; size: number }

const props = withDefaults(defineProps<{
  paused?: boolean
  reduced?: boolean
  status?: string
  scene?: 'none' | 'rain' | 'snow'
  activity?: string
}>(), { paused: false, reduced: false, status: 'idle', scene: 'none', activity: 'idle' })

const W = 250, H = 250
const cv = ref<HTMLCanvasElement | null>(null)
let ctx: CanvasRenderingContext2D | null = null
let raf = 0, last = 0, alive = false, dpr = 1
const parts: Particle[] = []
const drops: Drop[] = []
const flakes: Flake[] = []
let zzzClock = 0
let wipeAt = -10
let statusShown = 'idle'
let statusAt = 0

function resize() {
  const c = cv.value
  if (!c) return
  dpr = Math.min(2, window.devicePixelRatio || 1)
  c.width = Math.round(W * dpr)
  c.height = Math.round(H * dpr)
  ctx = c.getContext('2d')
  if (ctx) ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
}

function seedWeather() {
  drops.length = 0
  flakes.length = 0
  let s = 20261117
  const rnd = () => (s = (s * 1103515245 + 12345) % 2147483648) / 2147483648
  for (let i = 0; i < 74; i++) drops.push({ x: rnd() * (W + 60) - 30, y: rnd() * H, v: 260 + rnd() * 220, len: 7 + rnd() * 9 })
  for (let i = 0; i < 46; i++) flakes.push({ x: rnd() * W, y: rnd() * H, v: 16 + rnd() * 26, sway: rnd() * 6.28, size: 0.8 + rnd() * 1.6 })
}

function burst(kind: Burst) {
  if (props.reduced) return
  const anchor: Record<Burst, [number, number]> = {
    heart: [125, 50], sparkle: [125, 96], anger: [168, 62],
    sweat: [176, 78], question: [166, 66], note: [96, 70], zzz: [150, 62], dust: [125, 236],
  }
  const [ax, ay] = anchor[kind]
  const n = kind === 'sparkle' ? 7 : kind === 'dust' ? 10 : kind === 'heart' ? 4 : 1
  const slow = kind === 'zzz'
  for (let i = 0; i < n; i++) {
    const spread = kind === 'sparkle' || kind === 'dust' ? 34 : kind === 'heart' ? 28 : 12
    parts.push({
      kind,
      x: ax + (Math.random() - 0.5) * spread,
      y: ay + (Math.random() - 0.5) * (kind === 'dust' ? 6 : 16),
      vx: slow ? 6 + Math.random() * 6 : (Math.random() - 0.5) * (kind === 'dust' ? 60 : 22),
      vy: slow ? -12 - Math.random() * 6 : kind === 'dust' ? -12 - Math.random() * 18 : -18 - Math.random() * 26,
      age: 0,
      life: slow ? 1.7 + Math.random() * 0.4 : kind === 'dust' ? 0.55 : 0.9 + Math.random() * 0.5,
      size: kind === 'dust' ? 1.5 + Math.random() * 2 : kind === 'heart' ? 4 + Math.random() * 2.5 : 3 + Math.random() * 3,
      rot: Math.random() * 6.28,
      spin: (Math.random() - 0.5) * 3,
    })
  }
  if (parts.length > 140) parts.splice(0, parts.length - 140)
}

function wipe() { wipeAt = performance.now() / 1000 }

function heartPath(c: CanvasRenderingContext2D, s: number) {
  c.beginPath()
  c.moveTo(0, s * 0.35)
  c.bezierCurveTo(-s, -s * 0.3, -s * 0.55, -s * 1.05, 0, -s * 0.45)
  c.bezierCurveTo(s * 0.55, -s * 1.05, s, -s * 0.3, 0, s * 0.35)
  c.closePath()
}

function starPath(c: CanvasRenderingContext2D, s: number) {
  c.beginPath()
  for (let i = 0; i < 8; i++) {
    const a = (Math.PI * i) / 4
    const r = i % 2 === 0 ? s : s * 0.26
    const x = Math.cos(a) * r, y = Math.sin(a) * r
    if (i === 0) c.moveTo(x, y); else c.lineTo(x, y)
  }
  c.closePath()
}

// Canvas colour parsing rejects exponent notation, so every interpolated alpha
// goes through this clamp+fix: `${1 - 1e-16}` would otherwise be "1e-16".
function a3(v: number) {
  return Math.max(0, Math.min(1, Number.isFinite(v) ? v : 0)).toFixed(3)
}

function drawParticle(c: CanvasRenderingContext2D, p: Particle) {
  const fade = Math.max(0, 1 - p.age / p.life)
  if (!Number.isFinite(p.x) || !Number.isFinite(p.y) || !Number.isFinite(p.size)) return
  c.save()
  c.translate(p.x, p.y)
  c.rotate(p.rot)
  if (p.kind === 'heart') {
    c.fillStyle = `rgba(255,120,170,${a3(0.85 * fade)})`
    heartPath(c, p.size + 1.5)
    c.fill()
  } else if (p.kind === 'sparkle') {
    c.fillStyle = `rgba(255,246,210,${a3(0.9 * fade)})`
    starPath(c, p.size * (0.6 + Math.sin(p.age * 9) * 0.4) + 1.5)
    c.fill()
  } else if (p.kind === 'anger') {
    c.strokeStyle = `rgba(255,80,80,${a3(0.95 * fade)})`
    c.lineWidth = 2.2
    c.beginPath()
    c.moveTo(-6, -6); c.quadraticCurveTo(0, 0, 6, -6)
    c.moveTo(-6, 2); c.quadraticCurveTo(0, 8, 6, 2)
    c.stroke()
  } else if (p.kind === 'sweat') {
    c.fillStyle = `rgba(160,220,255,${a3(0.9 * fade)})`
    c.beginPath()
    c.moveTo(0, -5); c.quadraticCurveTo(4, 2, 0, 4); c.quadraticCurveTo(-4, 2, 0, -5)
    c.fill()
  } else if (p.kind === 'question') {
    c.fillStyle = `rgba(190,235,255,${a3(0.9 * fade)})`
    c.font = 'bold 13px Consolas, monospace'
    c.fillText('?', -4, 4)
  } else if (p.kind === 'note') {
    c.fillStyle = `rgba(180,240,255,${a3(0.9 * fade)})`
    c.font = 'bold 12px Consolas, monospace'
    c.fillText('♪', -5, 4)
  } else if (p.kind === 'zzz') {
    c.fillStyle = `rgba(185,230,255,${a3(0.85 * fade)})`
    c.font = `bold ${Math.round(9 + p.size)}px Consolas, monospace`
    c.fillText('Z', -4, 4)
  } else {
    c.fillStyle = `rgba(210,228,240,${a3(0.5 * fade)})`
    c.beginPath(); c.arc(0, 0, p.size, 0, 6.283); c.fill()
  }
  c.restore()
}

function drawStatus(c: CanvasRenderingContext2D, t: number) {
  const s = props.status
  const cy = 238
  if (!s || s === 'idle') return
  c.save()
  const fade = Math.min(1, (t - statusAt) / 0.35)
  c.globalAlpha = fade
  if (s === 'working') {
    c.strokeStyle = 'rgba(56,240,255,0.85)'
    c.lineWidth = 1.6
    for (let k = 0; k < 3; k++) {
      const a0 = t * 2.2 + (k * 2 * Math.PI) / 3
      c.beginPath(); c.ellipse(125, cy, 48, 12, 0, a0, a0 + 0.9); c.stroke()
    }
    for (let k = 0; k < 3; k++) {
      const a = t * 2.4 + (k * 2 * Math.PI) / 3
      c.fillStyle = 'rgba(200,250,255,0.9)'
      c.beginPath(); c.arc(125 + Math.cos(a) * 48, cy + Math.sin(a) * 12, 1.8, 0, 6.283); c.fill()
    }
    const prog = (t % 2.4) / 2.4
    c.strokeStyle = 'rgba(56,240,255,0.35)'
    c.lineWidth = 2
    c.beginPath(); c.ellipse(125, cy, 56, 15, 0, -Math.PI / 2, -Math.PI / 2 + prog * 6.283); c.stroke()
  } else if (s === 'waiting') {
    const pulse = 1 + Math.sin(t * 4) * 0.06
    c.strokeStyle = 'rgba(255,205,120,0.8)'
    c.lineWidth = 1.8
    c.beginPath(); c.ellipse(125, cy, 46 * pulse, 12 * pulse, 0, 0, 6.283); c.stroke()
    c.strokeStyle = 'rgba(255,205,120,0.3)'
    c.beginPath(); c.ellipse(125, cy, 54 * pulse, 14 * pulse, 0, 0, 6.283); c.stroke()
  } else if (s === 'toolError' || s === 'interrupted') {
    c.strokeStyle = 'rgba(255,90,90,0.85)'
    c.lineWidth = 1.8
    const j = (Math.sin(t * 30) > 0 ? 1 : 0) * 2
    c.beginPath(); c.ellipse(125 + j, cy, 46, 12, 0, 0, 6.283); c.stroke()
  } else if (s === 'ended') {
    const p = Math.min(1, (t - statusAt) / 1.2)
    c.strokeStyle = `rgba(110,255,190,${a3(0.9 * (1 - p))})`
    c.lineWidth = 2
    c.beginPath(); c.ellipse(125, cy, 46 + p * 22, 12 + p * 6, 0, 0, 6.283); c.stroke()
    c.strokeStyle = `rgba(160,255,210,${a3(1 - p)})`
    c.lineWidth = 2.4
    c.beginPath(); c.moveTo(118, 232); c.lineTo(123, 238); c.lineTo(134, 224); c.stroke()
  }
  c.restore()
}

function drawWeather(c: CanvasRenderingContext2D, dt: number) {
  if (props.scene === 'none') return
  c.save()
  if (props.scene === 'rain') {
    c.strokeStyle = 'rgba(170,215,255,0.34)'
    c.lineWidth = 1
    for (const d of drops) {
      d.y += d.v * dt
      d.x -= d.v * 0.22 * dt
      if (d.y > H + 10) { d.y = -10; d.x = Math.random() * (W + 60) - 30 }
      if (d.x < -20) d.x += W + 40
      c.beginPath(); c.moveTo(d.x, d.y); c.lineTo(d.x - d.len * 0.22, d.y + d.len); c.stroke()
    }
  } else {
    for (const f of flakes) {
      f.y += f.v * dt
      f.sway += dt * 1.6
      f.x += Math.sin(f.sway) * 8 * dt
      if (f.y > H + 4) { f.y = -4; f.x = Math.random() * W }
      c.fillStyle = 'rgba(230,245,255,0.55)'
      c.beginPath(); c.arc(f.x, f.y, f.size, 0, 6.283); c.fill()
    }
  }
  c.restore()
}

function drawWipe(c: CanvasRenderingContext2D, t: number) {
  const p = (t - wipeAt) / 0.28
  if (p < 0 || p > 1) return
  c.save()
  const x = -60 + p * (W + 120)
  const g = c.createLinearGradient(x - 26, 0, x + 26, 0)
  g.addColorStop(0, 'rgba(56,240,255,0)')
  g.addColorStop(0.5, `rgba(56,240,255,${0.55 * (1 - p)})`)
  g.addColorStop(1, 'rgba(56,240,255,0)')
  c.fillStyle = g
  c.beginPath()
  c.moveTo(x - 14, 0); c.lineTo(x + 14, 0); c.lineTo(x - 14, H); c.lineTo(x - 42, H)
  c.closePath(); c.fill()
  c.restore()
}

function loop(now: number) {
  if (!alive) return
  raf = requestAnimationFrame(loop)
  const c = ctx
  if (!c) return
  const t = now / 1000
  const dt = Math.min(0.05, last ? (now - last) / 1000 : 0.016) || 0.016
  last = now
  if (props.paused) return
  if (props.status !== statusShown) { statusShown = props.status ?? 'idle'; statusAt = t }
  c.clearRect(0, 0, W, H)
  drawWeather(c, dt)
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i]
    p.age += dt
    if (p.age >= p.life) { parts.splice(i, 1); continue }
    p.x += p.vx * dt
    p.y += p.vy * dt
    p.vy += (p.kind === 'dust' ? 26 : 4) * dt
    p.rot += p.spin * dt
  }
  // resting Zzz, one every ~0.75s
  if (props.activity === 'rest' && !props.reduced) {
    zzzClock += dt
    if (zzzClock > 0.75) { zzzClock = 0; burst('zzz') }
  } else zzzClock = 0
  for (const p of parts) drawParticle(c, p)
  drawStatus(c, t)
  drawWipe(c, t)
}

onMounted(() => {
  resize()
  seedWeather()
  window.addEventListener('resize', resize)
  alive = true
  raf = requestAnimationFrame(loop)
})
onBeforeUnmount(() => {
  alive = false
  window.removeEventListener('resize', resize)
  cancelAnimationFrame(raf)
})

defineExpose({ burst, wipe, resize })
</script>

<template>
  <canvas ref="cv" class="fx" aria-hidden="true" />
</template>

<style scoped>
.fx{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;overflow:visible}
</style>
