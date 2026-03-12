<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import {
  buildWorkAreaRect,
  chooseBubbleCandidate,
  finalizeBubbleLayout
} from '../lib/petLayout'
import {
  publishBubbleInteractionState,
  publishBubbleLayoutMetrics,
  requestBubbleDismiss
} from '../lib/assistant'
import type { BubbleLayoutMetrics, BubbleWindowState } from '../types/assistant'

const props = defineProps<{
  state: BubbleWindowState
}>()

const bubbleRef = ref<HTMLElement | null>(null)
const bubbleContentRef = ref<HTMLElement | null>(null)
const bubbleMaxWidth = ref(320)
const bubbleMaxHeight = ref(220)
const placement = ref<'above' | 'upper-left' | 'upper-right'>('above')
const tailSide = ref<'bottom' | 'left' | 'right'>('bottom')
const tailOffsetX = ref(56)
const tailOffsetY = ref(48)
const interactionActive = ref(false)
const pinned = ref(false)
const hoverActive = ref(false)
const scrollActive = ref(false)
const activeMessageId = ref(0)
let scrollIdleTimer: number | null = null

const waitForStableLayout = async () => {
  await nextTick()
  await new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => resolve())
    })
  })
}

const clearScrollIdleTimer = () => {
  if (scrollIdleTimer !== null) {
    window.clearTimeout(scrollIdleTimer)
    scrollIdleTimer = null
  }
}

const syncCompositeInteractionState = async () => {
  await syncInteractionState(hoverActive.value || scrollActive.value)
}

const syncInteractionState = async (active: boolean) => {
  if (interactionActive.value === active) {
    return
  }

  interactionActive.value = active
  await publishBubbleInteractionState(active)
}

const measureBubbleLayout = (): BubbleLayoutMetrics | null => {
  const bubble = bubbleRef.value
  const bubbleContent = bubbleContentRef.value
  if (!bubble || !bubbleContent) {
    return null
  }

  const rect = bubble.getBoundingClientRect()
  const scrollHeight = Math.ceil(bubbleContent.scrollHeight)
  const clientHeight = Math.ceil(bubbleContent.clientHeight)

  return {
    messageId: props.state.messageId,
    charCount: props.state.text.trim().length,
    scrollHeight,
    clientHeight,
    contentHeight: Math.ceil(rect.height),
    isScrollable: scrollHeight > clientHeight + 2
  }
}

const handlePointerEnter = () => {
  hoverActive.value = true
  void syncCompositeInteractionState()
}

const handlePointerLeave = () => {
  hoverActive.value = false
  void syncCompositeInteractionState()
}

const handleScrollActivity = () => {
  scrollActive.value = true
  void syncCompositeInteractionState()
  clearScrollIdleTimer()
  scrollIdleTimer = window.setTimeout(() => {
    scrollActive.value = false
    void syncCompositeInteractionState()
  }, 720)
}

const dismissPinnedBubble = async () => {
  await syncInteractionState(true)
  await requestBubbleDismiss(props.state.messageId)
  await syncInteractionState(false)
}

const hideBubbleWindow = async () => {
  clearScrollIdleTimer()
  hoverActive.value = false
  scrollActive.value = false
  pinned.value = false
  activeMessageId.value = 0
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

  if (activeMessageId.value !== props.state.messageId) {
    activeMessageId.value = props.state.messageId
    pinned.value = false
  }

  await waitForStableLayout()

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

  await waitForStableLayout()

  let metrics = measureBubbleLayout()
  if (!metrics) {
    return
  }

  if (!pinned.value && metrics.isScrollable) {
    pinned.value = true
    await waitForStableLayout()
    metrics = measureBubbleLayout()
    if (!metrics) {
      return
    }
  }

  const bubble = bubbleRef.value
  if (!bubble) {
    return
  }

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
  await publishBubbleLayoutMetrics(metrics)
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
  clearScrollIdleTimer()
  void syncInteractionState(false)
})
</script>

<template>
  <div class="bubble-window-shell">
    <div
      v-if="state.visible"
      ref="bubbleRef"
      class="floating-bubble"
      :class="[placement, `tail-${tailSide}`, { 'has-close': pinned }]"
      :style="{
        '--tail-x': `${tailOffsetX}px`,
        '--tail-y': `${tailOffsetY}px`,
        '--bubble-max-width': `${bubbleMaxWidth}px`,
        '--bubble-max-height': `${bubbleMaxHeight}px`
      }"
      @mouseenter="handlePointerEnter"
      @mouseleave="handlePointerLeave"
      @focusin="handlePointerEnter"
      @focusout="handlePointerLeave"
    >
      <button
        v-if="pinned"
        type="button"
        class="bubble-close"
        aria-label="关闭气泡"
        title="关闭气泡"
        @click.stop="dismissPinnedBubble"
        @mouseenter="handlePointerEnter"
      >
        ×
      </button>
      <div
        ref="bubbleContentRef"
        class="bubble-content"
        @wheel.passive="handleScrollActivity"
        @scroll.passive="handleScrollActivity"
      >
        <p>{{ state.text }}</p>
      </div>
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
  overflow: visible;
  pointer-events: auto;
  padding: 12px 16px;
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.98);
  color: #17384b;
  box-shadow: 0 10px 24px rgba(7, 18, 30, 0.1);
}

.floating-bubble.has-close {
  padding-right: 44px;
}

.bubble-content {
  max-height: calc(var(--bubble-max-height, 220px) - 18px);
  overflow-x: hidden;
  overflow-y: auto;
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

.bubble-close {
  position: absolute;
  top: 9px;
  right: 10px;
  z-index: 2;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 999px;
  background: rgba(23, 56, 75, 0.08);
  color: rgba(23, 56, 75, 0.78);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  opacity: 0.62;
  transition:
    opacity 140ms ease,
    background-color 140ms ease,
    color 140ms ease;
}

.bubble-close:hover,
.bubble-close:focus-visible {
  opacity: 1;
  background: rgba(23, 56, 75, 0.16);
  color: #17384b;
  outline: none;
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

.bubble-content p {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.45;
  font-size: 13px;
}
</style>
