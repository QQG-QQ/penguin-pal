<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import {
  buildWorkAreaRect,
  chooseBubbleCandidate,
  finalizeBubbleLayout
} from '../lib/petLayout'
import { publishBubbleInteractionState } from '../lib/assistant'
import type { BubbleWindowState } from '../types/assistant'

const props = defineProps<{
  state: BubbleWindowState
}>()

const bubbleRef = ref<HTMLElement | null>(null)
const bubbleMaxWidth = ref(320)
const bubbleMaxHeight = ref(220)
const placement = ref<'above' | 'upper-left' | 'upper-right'>('above')
const tailSide = ref<'bottom' | 'left' | 'right'>('bottom')
const tailOffsetX = ref(56)
const tailOffsetY = ref(48)
const interactionActive = ref(false)

const syncInteractionState = async (active: boolean) => {
  if (interactionActive.value === active) {
    return
  }

  interactionActive.value = active
  await publishBubbleInteractionState(active)
}

const hideBubbleWindow = async () => {
  await syncInteractionState(false)
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
  const workArea = buildWorkAreaRect(
    monitor?.workArea.position ?? { x: 0, y: 0 },
    monitor?.workArea.size ?? monitor?.size ?? {
      width: window.screen.availWidth,
      height: window.screen.availHeight
    }
  )
  const candidate = chooseBubbleCandidate(props.state, workArea)
  bubbleMaxWidth.value = candidate.maxWidth
  bubbleMaxHeight.value = candidate.maxHeight

  await nextTick()

  const measured = bubble.getBoundingClientRect()
  const layout = finalizeBubbleLayout(props.state, workArea, candidate, {
    width: Math.ceil(measured.width),
    height: Math.ceil(measured.height)
  })

  placement.value = layout.placement
  tailSide.value = layout.tailSide
  tailOffsetX.value = layout.tailOffsetX
  tailOffsetY.value = layout.tailOffsetY

  const appWindow = getCurrentWindow()
  await appWindow.setSize(new LogicalSize(Math.ceil(measured.width), Math.ceil(measured.height)))
  await appWindow.setPosition(new PhysicalPosition(layout.left, layout.top))
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
    await appWindow.setIgnoreCursorEvents(false)
  } catch {
    // ignore in unsupported runtimes
  }

  await syncBubbleWindow()
})

onBeforeUnmount(() => {
  void syncInteractionState(false)
})
</script>

<template>
  <div class="bubble-window-shell">
    <div
      v-if="state.visible"
      ref="bubbleRef"
      class="floating-bubble"
      :class="[placement, `tail-${tailSide}`]"
      :style="{
        '--tail-x': `${tailOffsetX}px`,
        '--tail-y': `${tailOffsetY}px`,
        '--bubble-max-width': `${bubbleMaxWidth}px`,
        '--bubble-max-height': `${bubbleMaxHeight}px`
      }"
      @mouseenter="syncInteractionState(true)"
      @mouseleave="syncInteractionState(false)"
      @focusin="syncInteractionState(true)"
      @focusout="syncInteractionState(false)"
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
  max-height: var(--bubble-max-height, 220px);
  overflow-x: hidden;
  overflow-y: auto;
  pointer-events: auto;
  padding: 12px 16px;
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.98);
  color: #17384b;
  box-shadow: 0 10px 24px rgba(7, 18, 30, 0.1);
  overscroll-behavior: contain;
  scrollbar-width: thin;
}

.floating-bubble::after {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  background: rgba(255, 255, 255, 0.98);
  transform: rotate(45deg);
  border-radius: 3px;
}

.floating-bubble.tail-bottom::after {
  left: var(--tail-x, 56px);
  bottom: -7px;
  transform: translateX(-50%) rotate(45deg);
}

.floating-bubble.tail-left::after {
  left: -7px;
  top: var(--tail-y, 48px);
  transform: translateY(-50%) rotate(45deg);
}

.floating-bubble.tail-right::after {
  right: -7px;
  top: var(--tail-y, 48px);
  transform: translateY(-50%) rotate(45deg);
}

.floating-bubble p {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.45;
  font-size: 13px;
}
</style>
