import type { BubbleWindowState } from '../types/assistant'

export interface WorkAreaRect {
  left: number
  top: number
  right: number
  bottom: number
  width: number
  height: number
}

export interface BubbleCandidate {
  placement: 'above' | 'upper-left' | 'upper-right'
  maxWidth: number
  maxHeight: number
}

export interface BubbleLayoutResult extends BubbleCandidate {
  left: number
  top: number
  tailSide: 'bottom' | 'left' | 'right'
  tailOffsetX: number
  tailOffsetY: number
}

const SCREEN_MARGIN = 12
const ABOVE_GAP = 14
const SIDE_GAP = 16
const MIN_BUBBLE_WIDTH = 164
const MIN_ABOVE_HEIGHT = 76
const MAX_BUBBLE_WIDTH = 360
const MAX_BUBBLE_HEIGHT = 240
const TAIL_PADDING = 24

const clamp = (value: number, min: number, max: number) => {
  if (max < min) {
    return min
  }

  return Math.min(Math.max(value, min), max)
}

export const buildWorkAreaRect = (
  position: { x: number; y: number },
  size: { width: number; height: number }
): WorkAreaRect => ({
  left: position.x,
  top: position.y,
  right: position.x + size.width,
  bottom: position.y + size.height,
  width: size.width,
  height: size.height
})

export const clampWindowPositionToWorkArea = (
  frame: { left: number; top: number; width: number; height: number },
  workArea: WorkAreaRect
) => {
  const maxLeft = workArea.right - frame.width
  const maxTop = workArea.bottom - frame.height

  return {
    left: clamp(frame.left, workArea.left, maxLeft),
    top: clamp(frame.top, workArea.top, maxTop)
  }
}

export const chooseBubbleCandidate = (
  state: BubbleWindowState,
  workArea: WorkAreaRect
): BubbleCandidate => {
  const aboveSpace = state.anchorY - workArea.top - SCREEN_MARGIN - ABOVE_GAP
  const leftSpace = state.faceLeft - workArea.left - SCREEN_MARGIN - SIDE_GAP
  const rightSpace = workArea.right - state.faceRight - SCREEN_MARGIN - SIDE_GAP
  const globalWidth = Math.max(
    MIN_BUBBLE_WIDTH,
    Math.min(MAX_BUBBLE_WIDTH, workArea.width - SCREEN_MARGIN * 2)
  )
  const globalHeight = Math.max(
    MIN_ABOVE_HEIGHT,
    Math.min(MAX_BUBBLE_HEIGHT, workArea.height - SCREEN_MARGIN * 2)
  )

  const aboveCandidate: BubbleCandidate = {
    placement: 'above',
    maxWidth: globalWidth,
    maxHeight: Math.max(MIN_ABOVE_HEIGHT, Math.min(globalHeight, aboveSpace))
  }

  const rightCandidate: BubbleCandidate = {
    placement: 'upper-right',
    maxWidth: Math.max(120, Math.min(globalWidth, rightSpace)),
    maxHeight: globalHeight
  }

  const leftCandidate: BubbleCandidate = {
    placement: 'upper-left',
    maxWidth: Math.max(120, Math.min(globalWidth, leftSpace)),
    maxHeight: globalHeight
  }

  if (aboveSpace >= MIN_ABOVE_HEIGHT && aboveCandidate.maxWidth >= MIN_BUBBLE_WIDTH) {
    return aboveCandidate
  }

  if (rightCandidate.maxWidth >= leftCandidate.maxWidth && rightCandidate.maxWidth >= 140) {
    return rightCandidate
  }

  if (leftCandidate.maxWidth >= 140) {
    return leftCandidate
  }

  return aboveCandidate
}

export const finalizeBubbleLayout = (
  state: BubbleWindowState,
  workArea: WorkAreaRect,
  candidate: BubbleCandidate,
  bubbleSize: { width: number; height: number }
): BubbleLayoutResult => {
  const maxLeft = workArea.right - SCREEN_MARGIN - bubbleSize.width
  const maxTop = workArea.bottom - SCREEN_MARGIN - bubbleSize.height

  if (candidate.placement === 'above') {
    const left = clamp(
      Math.round(state.anchorX - bubbleSize.width / 2),
      workArea.left + SCREEN_MARGIN,
      maxLeft
    )
    const top = clamp(
      Math.round(state.anchorY - bubbleSize.height - ABOVE_GAP),
      workArea.top + SCREEN_MARGIN,
      maxTop
    )

    return {
      ...candidate,
      left,
      top,
      tailSide: 'bottom',
      tailOffsetX: clamp(state.anchorX - left, TAIL_PADDING, bubbleSize.width - TAIL_PADDING),
      tailOffsetY: 0
    }
  }

  if (candidate.placement === 'upper-right') {
    const left = clamp(
      Math.round(state.faceRight + SIDE_GAP),
      workArea.left + SCREEN_MARGIN,
      maxLeft
    )
    const top = clamp(
      Math.round(state.anchorY - bubbleSize.height + 28),
      workArea.top + SCREEN_MARGIN,
      maxTop
    )

    return {
      ...candidate,
      left,
      top,
      tailSide: 'left',
      tailOffsetX: 0,
      tailOffsetY: clamp(state.anchorY - top, TAIL_PADDING, bubbleSize.height - TAIL_PADDING)
    }
  }

  const left = clamp(
    Math.round(state.faceLeft - SIDE_GAP - bubbleSize.width),
    workArea.left + SCREEN_MARGIN,
    maxLeft
  )
  const top = clamp(
    Math.round(state.anchorY - bubbleSize.height + 28),
    workArea.top + SCREEN_MARGIN,
    maxTop
  )

  return {
    ...candidate,
    left,
    top,
    tailSide: 'right',
    tailOffsetX: 0,
    tailOffsetY: clamp(state.anchorY - top, TAIL_PADDING, bubbleSize.height - TAIL_PADDING)
  }
}
