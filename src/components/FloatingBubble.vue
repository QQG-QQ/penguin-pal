<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'
import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import type { BubbleWindowState } from '../types/assistant'

const props = defineProps<{
  state: BubbleWindowState
}>()

const MAX_WIDTH = 320
const SCREEN_MARGIN = 12
const GAP = 10

const bubbleRef = ref<HTMLElement | null>(null)
const placement = ref<'above' | 'below'>('above')
const tailOffset = ref(56)

const hideBubbleWindow = async () => {
  try {
    await getCurrentWindow().hide()
  } catch {
    // ignore hide errors during teardown or non-tauri fallback
  }
}

const syncBubbleWindow = async () => {
  if (!props.state.visible || !props.state.text.trim()) {
    await hideBubbleWindow()
    return
  }

  await nextTick()

  const bubble = bubbleRef.value
  if (!bubble) {
    return
  }

  const monitor = await currentMonitor()
  const workAreaPosition = monitor?.workArea.position ?? { x: 0, y: 0 }
  const workAreaSize = monitor?.workArea.size ?? monitor?.size ?? {
    width: window.screen.availWidth,
    height: window.screen.availHeight
  }

  const rect = bubble.getBoundingClientRect()
  const width = Math.ceil(rect.width)
  const height = Math.ceil(rect.height)
  const minLeft = workAreaPosition.x + SCREEN_MARGIN
  const maxLeft = workAreaPosition.x + workAreaSize.width - width - SCREEN_MARGIN

  let left = Math.round(props.state.anchorX - width / 2)
  left = Math.max(minLeft, Math.min(left, maxLeft))

  const topAbove = Math.round(props.state.anchorY - height - GAP)
  const minTop = workAreaPosition.y + SCREEN_MARGIN
  const maxTop = workAreaPosition.y + workAreaSize.height - height - SCREEN_MARGIN

  let top = topAbove
  let nextPlacement: 'above' | 'below' = 'above'

  if (topAbove < minTop) {
    nextPlacement = 'below'
    top = Math.round(props.state.petBottomY + GAP)
  }

  top = Math.max(minTop, Math.min(top, maxTop))
  placement.value = nextPlacement
  tailOffset.value = Math.max(24, Math.min(width - 24, props.state.anchorX - left))

  const appWindow = getCurrentWindow()
  await appWindow.setSize(new LogicalSize(width, height))
  await appWindow.setPosition(new PhysicalPosition(left, top))
  await appWindow.show()
}

watch(
  () => props.state,
  () => {
    void syncBubbleWindow()
  },
  { deep: true, immediate: true }
)

onMounted(async () => {
  const appWindow = getCurrentWindow()

  try {
    await appWindow.setIgnoreCursorEvents(true)
  } catch {
    // ignore in unsupported runtimes
  }

  await syncBubbleWindow()
})
</script>

<template>
  <div class="bubble-window-shell">
    <div
      v-if="state.visible"
      ref="bubbleRef"
      class="floating-bubble"
      :class="placement"
      :style="{ '--tail-x': `${tailOffset}px`, '--bubble-max-width': `${MAX_WIDTH}px` }"
    >
      <p>{{ state.text }}</p>
    </div>
  </div>
</template>

<style scoped>
.bubble-window-shell {
  width: max-content;
  max-width: var(--bubble-max-width, 320px);
  background: transparent;
  pointer-events: none;
}

.floating-bubble {
  position: relative;
  display: block;
  width: max-content;
  max-width: var(--bubble-max-width, 320px);
  padding: 12px 16px;
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.98);
  color: #17384b;
  box-shadow: 0 10px 24px rgba(7, 18, 30, 0.1);
}

.floating-bubble::after {
  content: '';
  position: absolute;
  left: var(--tail-x, 56px);
  width: 14px;
  height: 14px;
  background: rgba(255, 255, 255, 0.98);
  transform: translateX(-50%) rotate(45deg);
  border-radius: 3px;
}

.floating-bubble.above::after {
  bottom: -7px;
}

.floating-bubble.below::after {
  top: -7px;
}

.floating-bubble p {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.45;
  font-size: 13px;
}
</style>
