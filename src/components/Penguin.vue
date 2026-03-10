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
  subtitle: string
  permissionLevel: number
  expanded: boolean
}>()

const emit = defineEmits<{
  activate: []
  openActions: []
  openSettings: []
  hide: []
}>()

const modeMeta = computed(() => {
  const map: Record<PetMode, { label: string; accent: string }> = {
    idle: { label: '巡航待命', accent: '低打扰陪伴中' },
    listening: { label: '正在聆听', accent: '松开后自动转写' },
    thinking: { label: '任务分析', accent: '先判断风险，再决定答复' },
    speaking: { label: '正在回复', accent: '系统语音播报中' },
    guarded: { label: '安全警戒', accent: '高风险动作默认拦截' }
  }

  return map[props.mode]
})

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
  <section class="pet-shell" :class="[`mode-${mode}`, { expanded }]">
    <div class="pet-glow pet-glow-a" />
    <div class="pet-glow pet-glow-b" />

    <div class="pet-topline">
      <button class="drag-chip" type="button" @mousedown="startDrag">
        拖动
      </button>

      <div class="pet-tools">
        <button class="tool-button" type="button" @click.stop="emit('openActions')">
          动作
        </button>
        <button class="tool-button" type="button" @click.stop="emit('openSettings')">
          设置
        </button>
        <button class="tool-button danger" type="button" @click.stop="emit('hide')">
          隐藏
        </button>
      </div>
    </div>

    <button class="pet-body" type="button" @click="emit('activate')">
      <div class="pet-badges">
        <span class="mode-badge">{{ modeMeta.label }}</span>
        <span class="level-badge">权限 L{{ permissionLevel }}</span>
      </div>

      <div class="penguin-stage" :class="`stage-${mode}`">
        <div class="stage-ring ring-a" />
        <div class="stage-ring ring-b" />
        <div class="stage-platform" />
        <img
          class="penguin-art"
          :class="`motion-${mode}`"
          :src="artwork.src"
          :alt="artwork.alt"
          draggable="false"
        />
      </div>

      <div class="speech-bubble">
        {{ subtitle }}
      </div>

      <div class="pet-footline">
        <span>{{ modeMeta.accent }}</span>
        <span>{{ expanded ? '点击收起面板' : '点击展开面板' }}</span>
      </div>
    </button>
  </section>
</template>

<style scoped>
.pet-shell {
  position: relative;
  width: min(100%, 248px);
  padding: 12px 12px 14px;
  border-radius: 34px;
  overflow: hidden;
  background:
    radial-gradient(circle at top, rgba(245, 253, 255, 0.98), rgba(245, 253, 255, 0) 54%),
    linear-gradient(180deg, rgba(7, 28, 41, 0.96), rgba(10, 24, 38, 0.98));
  box-shadow:
    0 24px 42px rgba(3, 15, 28, 0.34),
    inset 0 1px 0 rgba(255, 255, 255, 0.28);
}

.pet-shell.expanded {
  box-shadow:
    0 28px 50px rgba(3, 15, 28, 0.38),
    inset 0 1px 0 rgba(255, 255, 255, 0.32),
    0 0 0 1px rgba(140, 230, 245, 0.16);
}

.pet-glow {
  position: absolute;
  border-radius: 999px;
  opacity: 0.82;
  filter: blur(18px);
  pointer-events: none;
}

.pet-glow-a {
  width: 132px;
  height: 132px;
  top: 14px;
  left: -28px;
  background: rgba(148, 229, 248, 0.34);
}

.pet-glow-b {
  width: 152px;
  height: 152px;
  right: -28px;
  bottom: -10px;
  background: rgba(255, 173, 102, 0.16);
}

.pet-topline,
.pet-badges,
.pet-footline,
.pet-tools {
  display: flex;
  gap: 8px;
  align-items: center;
}

.pet-topline,
.pet-footline {
  justify-content: space-between;
}

.drag-chip,
.tool-button {
  position: relative;
  z-index: 2;
  border: none;
  border-radius: 999px;
  cursor: pointer;
}

.drag-chip {
  min-height: 28px;
  padding: 0 12px;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(239, 249, 255, 0.76);
  font-size: 11px;
  letter-spacing: 0.08em;
}

.tool-button {
  min-height: 28px;
  padding: 0 10px;
  background: rgba(255, 255, 255, 0.88);
  color: #183c4d;
  font-size: 12px;
}

.tool-button.danger {
  background: rgba(255, 130, 130, 0.22);
  color: #ffd7d7;
}

.pet-body {
  position: relative;
  z-index: 2;
  width: 100%;
  margin-top: 8px;
  border: none;
  background: transparent;
  color: #f4fbff;
  cursor: pointer;
}

.pet-badges {
  justify-content: space-between;
}

.mode-badge,
.level-badge {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  font-size: 11px;
  letter-spacing: 0.04em;
}

.mode-badge {
  background: rgba(116, 219, 242, 0.18);
  color: #c3f5ff;
}

.level-badge {
  background: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.82);
}

.penguin-stage {
  position: relative;
  width: 176px;
  height: 184px;
  margin: 8px auto 2px;
  border-radius: 28px;
  background:
    radial-gradient(circle at 50% 22%, rgba(255, 255, 255, 0.56), rgba(255, 255, 255, 0) 54%),
    linear-gradient(180deg, rgba(242, 250, 252, 0.18), rgba(242, 250, 252, 0.02));
  overflow: hidden;
}

.stage-ring,
.stage-platform {
  position: absolute;
  inset: auto;
  pointer-events: none;
}

.stage-ring {
  border-radius: 999px;
  filter: blur(10px);
}

.ring-a {
  width: 112px;
  height: 112px;
  top: 4px;
  left: 2px;
  background: rgba(128, 218, 242, 0.28);
}

.ring-b {
  width: 116px;
  height: 116px;
  right: 2px;
  top: 22px;
  background: rgba(255, 200, 134, 0.18);
}

.stage-platform {
  width: 122px;
  height: 18px;
  left: 27px;
  bottom: 8px;
  border-radius: 999px;
  background: radial-gradient(circle, rgba(105, 160, 183, 0.28), rgba(105, 160, 183, 0));
}

.penguin-art {
  position: relative;
  z-index: 1;
  display: block;
  width: 176px;
  height: 176px;
  margin: 0 auto;
  user-select: none;
  -webkit-user-drag: none;
  transform-origin: 50% 72%;
  filter: drop-shadow(0 10px 14px rgba(7, 18, 28, 0.14));
}

.speech-bubble {
  min-height: 64px;
  margin-top: 4px;
  padding: 12px 14px;
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.09);
  color: rgba(242, 251, 255, 0.94);
  font-size: 13px;
  line-height: 1.45;
  text-align: left;
}

.pet-footline {
  margin-top: 10px;
  color: rgba(208, 235, 243, 0.72);
  font-size: 11px;
}

.pet-footline span:last-child {
  color: rgba(255, 208, 170, 0.84);
}

.stage-listening {
  background:
    radial-gradient(circle at 50% 22%, rgba(227, 255, 243, 0.52), rgba(255, 255, 255, 0) 54%),
    linear-gradient(180deg, rgba(242, 250, 252, 0.22), rgba(242, 250, 252, 0.03));
}

.stage-thinking {
  background:
    radial-gradient(circle at 50% 22%, rgba(255, 244, 210, 0.48), rgba(255, 255, 255, 0) 54%),
    linear-gradient(180deg, rgba(242, 250, 252, 0.2), rgba(242, 250, 252, 0.03));
}

.stage-speaking {
  background:
    radial-gradient(circle at 50% 22%, rgba(255, 224, 230, 0.46), rgba(255, 255, 255, 0) 54%),
    linear-gradient(180deg, rgba(242, 250, 252, 0.2), rgba(242, 250, 252, 0.03));
}

.stage-guarded {
  background:
    radial-gradient(circle at 50% 22%, rgba(255, 220, 209, 0.44), rgba(255, 255, 255, 0) 54%),
    linear-gradient(180deg, rgba(242, 250, 252, 0.2), rgba(242, 250, 252, 0.03));
}

.motion-idle {
  animation: idleFloat 4.6s ease-in-out infinite;
}

.motion-listening {
  animation: listeningBob 2.9s ease-in-out infinite;
}

.motion-thinking {
  animation: thinkingSway 3.2s ease-in-out infinite;
}

.motion-speaking {
  animation: speakingBounce 1.15s ease-in-out infinite;
}

.motion-guarded {
  animation: guardedAlert 1.8s ease-in-out infinite;
}

.mode-listening {
  box-shadow:
    0 24px 42px rgba(3, 15, 28, 0.34),
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 0 34px rgba(108, 235, 190, 0.16);
}

.mode-thinking {
  box-shadow:
    0 24px 42px rgba(3, 15, 28, 0.34),
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 0 34px rgba(255, 196, 94, 0.18);
}

.mode-speaking {
  box-shadow:
    0 24px 42px rgba(3, 15, 28, 0.34),
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 0 34px rgba(255, 150, 162, 0.16);
}

.mode-guarded {
  box-shadow:
    0 24px 42px rgba(3, 15, 28, 0.34),
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 0 34px rgba(255, 112, 112, 0.2);
}

@keyframes idleFloat {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }

  50% {
    transform: translateY(-6px) scale(1.01);
  }
}

@keyframes listeningBob {
  0%,
  100% {
    transform: translateY(0) rotate(0deg);
  }

  30% {
    transform: translateY(-5px) rotate(-1.2deg);
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
