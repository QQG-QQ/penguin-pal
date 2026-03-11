<script setup lang="ts">
import { computed, ref } from 'vue'
import guardedArt from '../../penguin/penguin-guarded-cutout.png'
import idleArt from '../../penguin/penguin-idle-cutout.png'
import listeningArt from '../../penguin/penguin-listening-cutout.png'
import speakingArt from '../../penguin/penguin-speaking-cutout.png'
import thinkingArt from '../../penguin/penguin-thinking-cutout.png'
import { startMainWindowDrag } from '../lib/assistant'
import type { PetLayoutMetrics, PetMode } from '../types/assistant'

const props = defineProps<{
  mode: PetMode
}>()

const emit = defineEmits<{
  activate: []
}>()

let pointerStartX = 0
let pointerStartY = 0
let pointerPressed = false
let dragStarted = false
let longPressTimer: number | null = null
const shellRef = ref<HTMLElement | null>(null)

const CLICK_MOVE_TOLERANCE = 6
const LONG_PRESS_DRAG_DELAY_MS = 250

const clearLongPressTimer = () => {
  if (longPressTimer !== null) {
    window.clearTimeout(longPressTimer)
    longPressTimer = null
  }
}

const artwork = computed(() => {
  const map: Record<PetMode, { src: string; alt: string }> = {
    idle: { src: idleArt, alt: '管理员企鹅待机立绘' },
    listening: { src: listeningArt, alt: '管理员企鹅聆听立绘' },
    thinking: { src: thinkingArt, alt: '管理员企鹅思考立绘' },
    speaking: { src: speakingArt, alt: '管理员企鹅回复立绘' },
    guarded: { src: guardedArt, alt: '管理员企鹅警戒立绘' }
  }

  return map[props.mode]
})

const getLayoutMetrics = (): PetLayoutMetrics | null => {
  const shell = shellRef.value
  if (!shell) {
    return null
  }

  const rect = shell.getBoundingClientRect()
  const anchorX = rect.left + rect.width / 2
  const anchorY = rect.top + rect.height * 0.17
  const faceHalfWidth = rect.width * 0.2
  const faceTop = rect.top + rect.height * 0.12
  const faceBottom = rect.top + rect.height * 0.46

  return {
    anchorX,
    anchorY,
    petLeft: rect.left,
    petTop: rect.top,
    petRight: rect.right,
    petBottom: rect.bottom,
    faceLeft: anchorX - faceHalfWidth,
    faceTop,
    faceRight: anchorX + faceHalfWidth,
    faceBottom
  }
}

defineExpose({
  getLayoutMetrics
})

const rememberPointerOrigin = (event: PointerEvent) => {
  pointerStartX = event.clientX
  pointerStartY = event.clientY
  pointerPressed = true
  dragStarted = false
  clearLongPressTimer()
  longPressTimer = window.setTimeout(async () => {
    if (!pointerPressed || dragStarted) {
      return
    }

    dragStarted = true

    try {
      await startMainWindowDrag()
    } catch {
      dragStarted = false
    }
  }, LONG_PRESS_DRAG_DELAY_MS)
}

const activatePet = (event: PointerEvent) => {
  clearLongPressTimer()
  pointerPressed = false

  if (dragStarted) {
    dragStarted = false
    return
  }

  const movedX = Math.abs(event.clientX - pointerStartX)
  const movedY = Math.abs(event.clientY - pointerStartY)

  if (movedX <= CLICK_MOVE_TOLERANCE && movedY <= CLICK_MOVE_TOLERANCE) {
    emit('activate')
  }
}

const cancelPointerInteraction = () => {
  clearLongPressTimer()
  pointerPressed = false
  dragStarted = false
}
</script>

<template>
  <section
    ref="shellRef"
    class="pet-shell"
    :class="`mode-${mode}`"
    @pointerdown.left="rememberPointerOrigin"
    @pointerup.left="activatePet"
    @pointercancel="cancelPointerInteraction"
  >
    <div class="pet-aura aura-a" />
    <div class="pet-aura aura-b" />
    <div class="pet-shadow" />

    <div class="pet-body">
      <img
        class="penguin-art"
        :class="`motion-${mode}`"
        :src="artwork.src"
        :alt="artwork.alt"
        draggable="false"
      />
    </div>
  </section>
</template>

<style scoped>
.pet-shell {
  position: relative;
  width: 100%;
  max-width: 244px;
  height: 248px;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  overflow: visible;
  user-select: none;
  -webkit-user-select: none;
  cursor: grab;
}

.pet-shell:active {
  cursor: grabbing;
}

.pet-aura,
.pet-shadow {
  position: absolute;
  pointer-events: none;
}

.pet-aura {
  border-radius: 999px;
  filter: blur(18px);
  opacity: 0.82;
}

.aura-a {
  width: 136px;
  height: 136px;
  left: 50%;
  top: 86px;
  transform: translateX(-96px);
  background: rgba(134, 221, 245, 0.24);
}

.aura-b {
  width: 120px;
  height: 120px;
  left: 50%;
  top: 98px;
  transform: translateX(10px);
  background: rgba(255, 185, 124, 0.14);
}

.pet-shadow {
  width: 112px;
  height: 16px;
  bottom: 18px;
  left: 50%;
  border-radius: 999px;
  background: radial-gradient(circle, rgba(12, 30, 43, 0.18), rgba(12, 30, 43, 0));
  transform: translateX(-50%);
}

.pet-body {
  position: relative;
  z-index: 1;
  width: min(224px, calc(100% - 10px));
  display: flex;
  justify-content: center;
  align-items: flex-end;
}

.penguin-art {
  display: block;
  width: 100%;
  height: auto;
  transform-origin: 50% 78%;
  filter: drop-shadow(0 12px 18px rgba(8, 20, 31, 0.14));
  user-select: none;
  -webkit-user-drag: none;
  pointer-events: none;
}

.mode-listening .aura-a {
  background: rgba(122, 238, 190, 0.26);
}

.mode-thinking .aura-b {
  background: rgba(255, 205, 120, 0.2);
}

.mode-speaking .aura-a {
  background: rgba(255, 198, 220, 0.24);
}

.mode-guarded .aura-b {
  background: rgba(255, 147, 147, 0.22);
}

.motion-idle {
  animation: idleFloat 4.6s ease-in-out infinite;
}

.motion-listening {
  animation: listeningBob 2.8s ease-in-out infinite;
}

.motion-thinking {
  animation: thinkingSway 3.2s ease-in-out infinite;
}

.motion-speaking {
  animation: speakingBounce 1.2s ease-in-out infinite;
}

.motion-guarded {
  animation: guardedAlert 1.8s ease-in-out infinite;
}

@keyframes idleFloat {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }

  50% {
    transform: translateY(-5px) scale(1.01);
  }
}

@keyframes listeningBob {
  0%,
  100% {
    transform: translateY(0) rotate(0deg);
  }

  30% {
    transform: translateY(-4px) rotate(-1.2deg);
  }

  65% {
    transform: translateY(-2px) rotate(1deg);
  }
}

@keyframes thinkingSway {
  0%,
  100% {
    transform: translateY(0) rotate(0deg);
  }

  25% {
    transform: translateY(-2px) rotate(-1.4deg);
  }

  75% {
    transform: translateY(-4px) rotate(1.3deg);
  }
}

@keyframes speakingBounce {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }

  35% {
    transform: translateY(-6px) scale(1.015);
  }

  70% {
    transform: translateY(-1px) scale(0.996);
  }
}

@keyframes guardedAlert {
  0%,
  100% {
    transform: translateY(0) rotate(0deg);
  }

  20% {
    transform: translateY(-3px) rotate(-1deg);
  }

  40% {
    transform: translateY(-1px) rotate(1deg);
  }

  60% {
    transform: translateY(-4px) rotate(-0.6deg);
  }

  80% {
    transform: translateY(-1px) rotate(0.6deg);
  }
}
</style>
