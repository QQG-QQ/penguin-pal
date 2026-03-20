import type { PetDockState } from '../types/assistant'
import { getDockedArtwork, normalizeAlphaBBox } from './petArtwork'
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
  dockedLeft: { width: 288, height: 432, revealPx: 34 },
  dockedRight: { width: 288, height: 432, revealPx: 34 },
  dockedTop: { width: 240, height: 360, revealPx: 28 }
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
  const artwork = getDockedArtwork(dockState)
  const normalized = normalizeAlphaBBox(artwork)
  const preset = dockedFrameByState[dockState]
  const width = preset.width
  const height = preset.height
  const bboxLeft = width * normalized.left
  const bboxTop = height * normalized.top
  const bboxRight = width * normalized.right
  const bboxBottom = height * normalized.bottom
  const bboxWidth = bboxRight - bboxLeft
  const previousBottom = previousVisibleFrame.top + previousVisibleFrame.height
  const previousCenterX = previousVisibleFrame.left + previousVisibleFrame.width / 2

  if (dockState === 'dockedLeft') {
    return {
      left: Math.round(workArea.left - bboxRight + preset.revealPx),
      top: Math.round(
        clamp(
          previousBottom - bboxBottom,
          workArea.top + SCREEN_MARGIN - bboxTop,
          workArea.bottom - SCREEN_MARGIN - bboxBottom
        )
      ),
      width,
      height
    }
  }

  if (dockState === 'dockedRight') {
    return {
      left: Math.round(workArea.right - preset.revealPx - bboxLeft),
      top: Math.round(
        clamp(
          previousBottom - bboxBottom,
          workArea.top + SCREEN_MARGIN - bboxTop,
          workArea.bottom - SCREEN_MARGIN - bboxBottom
        )
      ),
      width,
      height
    }
  }

  return {
    left: Math.round(
      clamp(
        previousCenterX - (bboxLeft + bboxWidth / 2),
        workArea.left + SCREEN_MARGIN - bboxLeft,
        workArea.right - SCREEN_MARGIN - bboxRight
      )
    ),
    top: Math.round(workArea.top - bboxBottom + preset.revealPx),
    width,
    height
  }
}
