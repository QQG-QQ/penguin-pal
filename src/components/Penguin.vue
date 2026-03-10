<script setup lang="ts">
import { computed } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import guardedArt from '../../penguin/penguin-guarded-cutout.png'
import idleArt from '../../penguin/penguin-idle-cutout.png'
import listeningArt from '../../penguin/penguin-listening-cutout.png'
import speakingArt from '../../penguin/penguin-speaking-cutout.png'
import thinkingArt from '../../penguin/penguin-thinking-cutout.png'
import type { PetMode } from '../types/assistant'

const props = defineProps<{
  mode: PetMode
  bubbleText: string
}>()

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

const startDrag = async () => {
  try {
    await getCurrentWindow().startDragging()
  } catch {
    console.info('Dragging not available in browser preview')
  }
}
</script>

<template>
  <section class="pet-shell" :class="`mode-${mode}`" @pointerdown.left.prevent="startDrag">
    <transition name="bubble">
      <div v-if="bubbleText" class="speech-bubble">
        <p>{{ bubbleText }}</p>
      </div>
    </transition>

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
  width: 220px;
  height: 276px;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  user-select: none;
  -webkit-user-select: none;
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
  width: 132px;
  height: 132px;
  left: 14px;
  top: 92px;
  background: rgba(134, 221, 245, 0.24);
}

.aura-b {
  width: 120px;
  height: 120px;
  right: 8px;
  top: 104px;
  background: rgba(255, 185, 124, 0.14);
}

.pet-shadow {
  width: 112px;
  height: 16px;
  bottom: 18px;
  border-radius: 999px;
  background: radial-gradient(circle, rgba(12, 30, 43, 0.18), rgba(12, 30, 43, 0));
}

.pet-body {
  position: relative;
  z-index: 1;
  width: 212px;
  display: flex;
  justify-content: center;
  align-items: flex-end;
}

.penguin-art {
  display: block;
  width: 212px;
  height: 212px;
  transform-origin: 50% 78%;
  filter: drop-shadow(0 12px 18px rgba(8, 20, 31, 0.14));
  user-select: none;
  -webkit-user-drag: none;
}

.speech-bubble {
  position: absolute;
  top: 8px;
  left: 50%;
  z-index: 3;
  width: min(206px, calc(100% - 10px));
  padding: 11px 13px;
  border-radius: 19px;
  background: rgba(255, 255, 255, 0.96);
  color: #183949;
  box-shadow:
    0 16px 30px rgba(6, 18, 30, 0.16),
    inset 0 1px 0 rgba(255, 255, 255, 0.86);
  transform: translateX(-50%);
  pointer-events: none;
}

.speech-bubble::after {
  content: '';
  position: absolute;
  left: 50%;
  bottom: -10px;
  width: 18px;
  height: 18px;
  background: rgba(255, 255, 255, 0.96);
  transform: translateX(-50%) rotate(45deg);
  border-radius: 4px;
}

.speech-bubble p {
  margin: 0;
  position: relative;
  z-index: 1;
  font-size: 13px;
  line-height: 1.5;
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

.bubble-enter-active,
.bubble-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.bubble-enter-from,
.bubble-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(10px);
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
