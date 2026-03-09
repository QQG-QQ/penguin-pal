<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { PetMode } from '../types/assistant'

const props = defineProps<{
  mode: PetMode
  subtitle: string
  permissionLevel: number
}>()

const emit = defineEmits<{
  activate: []
}>()

const container = ref<HTMLDivElement>()
let animation: AnimationItem | null = null

const modeMeta = computed(() => {
  const map: Record<PetMode, { label: string; accent: string }> = {
    idle: { label: '巡航待命', accent: '稳定观察中' },
    listening: { label: '正在聆听', accent: '松开后自动转写' },
    thinking: { label: '深度思考', accent: '整理上下文与安全边界' },
    speaking: { label: '语音回复', accent: '通过系统语音播报中' },
    guarded: { label: '警戒模式', accent: '危险动作默认拒绝' }
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

onMounted(() => {
  if (!container.value) {
    return
  }

  animation = lottie.loadAnimation({
    container: container.value,
    renderer: 'svg',
    loop: true,
    autoplay: true,
    path: '/animations/penguin-idle.json'
  })
})

onBeforeUnmount(() => {
  animation?.destroy()
  animation = null
})
</script>

<template>
  <section class="penguin-stage" :class="`mode-${mode}`">
    <div class="aurora aurora-a" />
    <div class="aurora aurora-b" />
    <button class="drag-pill" type="button" @mousedown="startDrag">
      拖动桌宠
    </button>
    <button class="stage-button" type="button" @click="emit('activate')">
      <div class="badge-row">
        <span class="mode-badge">{{ modeMeta.label }}</span>
        <span class="level-badge">权限 L{{ permissionLevel }}</span>
      </div>
      <div ref="container" class="penguin-container" />
      <div class="subtitle">{{ subtitle }}</div>
      <div class="accent-line">{{ modeMeta.accent }}</div>
    </button>
  </section>
</template>

<style scoped>
.penguin-stage {
  position: relative;
  width: min(100%, 360px);
  padding: 18px 16px 20px;
  border-radius: 36px;
  overflow: hidden;
  background:
    radial-gradient(circle at top, rgba(239, 250, 255, 0.95), rgba(239, 250, 255, 0) 54%),
    linear-gradient(180deg, rgba(8, 31, 46, 0.94), rgba(7, 20, 34, 0.98));
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3);
}

.aurora {
  position: absolute;
  border-radius: 999px;
  opacity: 0.8;
  filter: blur(18px);
  pointer-events: none;
}

.aurora-a {
  width: 150px;
  height: 150px;
  top: 10px;
  left: -20px;
  background: rgba(143, 227, 255, 0.42);
}

.aurora-b {
  width: 170px;
  height: 170px;
  right: -30px;
  bottom: 0;
  background: rgba(176, 255, 216, 0.22);
}

.drag-pill {
  position: relative;
  z-index: 2;
  margin: 0 auto 12px;
  display: block;
  padding: 7px 14px;
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(237, 248, 255, 0.72);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  cursor: grab;
}

.drag-pill:active {
  cursor: grabbing;
}

.stage-button {
  position: relative;
  z-index: 2;
  width: 100%;
  border: none;
  background: transparent;
  color: #f4fbff;
  cursor: pointer;
}

.badge-row {
  display: flex;
  justify-content: space-between;
  gap: 10px;
}

.mode-badge,
.level-badge {
  display: inline-flex;
  align-items: center;
  min-height: 32px;
  padding: 0 12px;
  border-radius: 999px;
  font-size: 12px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.mode-badge {
  background: rgba(127, 228, 255, 0.2);
  color: #bff6ff;
}

.level-badge {
  background: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.82);
}

.penguin-container {
  width: 220px;
  height: 220px;
  margin: 2px auto 6px;
  user-select: none;
  -webkit-user-drag: none;
}

.subtitle {
  max-width: 260px;
  margin: 0 auto;
  padding: 10px 14px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(244, 251, 255, 0.92);
  font-size: 13px;
  line-height: 1.5;
}

.accent-line {
  margin-top: 10px;
  color: rgba(207, 236, 244, 0.74);
  font-size: 12px;
  letter-spacing: 0.04em;
}

.mode-idle {
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 0 0 1px rgba(112, 210, 232, 0.12);
}

.mode-listening {
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 0 0 1px rgba(108, 235, 190, 0.2),
    0 0 38px rgba(108, 235, 190, 0.18);
}

.mode-thinking {
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 0 0 1px rgba(255, 206, 127, 0.22),
    0 0 42px rgba(255, 196, 94, 0.16);
}

.mode-speaking {
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 0 0 1px rgba(255, 150, 162, 0.18),
    0 0 38px rgba(255, 150, 162, 0.16);
}

.mode-guarded {
  box-shadow:
    0 28px 60px rgba(3, 15, 28, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 0 0 1px rgba(255, 112, 112, 0.28),
    0 0 40px rgba(255, 112, 112, 0.18);
}
</style>
