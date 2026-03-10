<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web'
import { getCurrentWindow } from '@tauri-apps/api/window'
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

const container = ref<HTMLDivElement>()
let animation: AnimationItem | null = null

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

      <div ref="container" class="penguin-container" />

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
  width: min(100%, 256px);
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

.penguin-container {
  width: 172px;
  height: 172px;
  margin: 4px auto 0;
  user-select: none;
  -webkit-user-drag: none;
}

.speech-bubble {
  min-height: 64px;
  margin-top: -4px;
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
</style>
