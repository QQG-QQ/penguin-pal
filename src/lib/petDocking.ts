import type { PetDockState } from '../types/assistant'
import type { WorkAreaRect } from './petLayout'

export interface PetWindowFrame {
  left: number
  top: number
  width: number
  height: number
}

export interface PetWindowSize {
  width: number
  height: number
}

export const PET_DOCK_IDLE_DELAY_MS = 1500
export const PET_DOCK_EDGE_THRESHOLD_PX = 72

const SCREEN_MARGIN = 12

const dockedFrameByState: Record<Exclude<PetDockState, 'normal'>, PetWindowSize & { revealPx: number }> = {
  dockedLeft: { width: 248, height: 332, revealPx: 56 },
  dockedRight: { width: 248, height: 332, revealPx: 56 },
  dockedTop: { width: 248, height: 320, revealPx: 64 }
}

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max)

export const choosePetDockState = (
  frame: PetWindowFrame,
  workArea: WorkAreaRect
): Exclude<PetDockState, 'normal'> => {
  const distances = [
    { dockState: 'dockedLeft' as const, distance: Math.abs(frame.left - workArea.left) },
    { dockState: 'dockedRight' as const, distance: Math.abs(workArea.right - (frame.left + frame.width)) },
    { dockState: 'dockedTop' as const, distance: Math.abs(frame.top - workArea.top) }
  ]

  distances.sort((left, right) => left.distance - right.distance)
  return distances[0]?.dockState ?? 'dockedRight'
}

export const isPetNearDockEdge = (
  frame: PetWindowFrame,
  workArea: WorkAreaRect,
  threshold = PET_DOCK_EDGE_THRESHOLD_PX
) => {
  const distances = [
    Math.abs(frame.left - workArea.left),
    Math.abs(workArea.right - (frame.left + frame.width)),
    Math.abs(frame.top - workArea.top)
  ]

  return distances.some((distance) => distance <= threshold)
}

export const getDockedWindowSize = (
  dockState: Exclude<PetDockState, 'normal'>
): PetWindowSize => {
  const frame = dockedFrameByState[dockState]
  return { width: frame.width, height: frame.height }
}

export const planDockedWindowFrame = (
  dockState: Exclude<PetDockState, 'normal'>,
  workArea: WorkAreaRect,
  previousVisibleFrame: PetWindowFrame
): PetWindowFrame => {
  const preset = dockedFrameByState[dockState]
  const width = preset.width
  const height = preset.height
  const previousBottom = previousVisibleFrame.top + previousVisibleFrame.height
  const previousCenterX = previousVisibleFrame.left + previousVisibleFrame.width / 2

  if (dockState === 'dockedLeft') {
    return {
      left: Math.round(workArea.left - width + preset.revealPx),
      top: Math.round(
        clamp(previousBottom - height, workArea.top + SCREEN_MARGIN, workArea.bottom - height - SCREEN_MARGIN)
      ),
      width,
      height
    }
  }

  if (dockState === 'dockedRight') {
    return {
      left: Math.round(workArea.right - preset.revealPx),
      top: Math.round(
        clamp(previousBottom - height, workArea.top + SCREEN_MARGIN, workArea.bottom - height - SCREEN_MARGIN)
      ),
      width,
      height
    }
  }

  return {
    left: Math.round(
      clamp(previousCenterX - width / 2, workArea.left + SCREEN_MARGIN, workArea.right - width - SCREEN_MARGIN)
    ),
    top: Math.round(workArea.top - height + preset.revealPx),
    width,
    height
  }
}
