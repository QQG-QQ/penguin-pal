import dockedLeftArt from '../../penguin/penguin-docked-left-cutout.png'
import dockedRightArt from '../../penguin/penguin-docked-right-cutout.png'
import dockedTopArt from '../../penguin/penguin-docked-top-cutout.png'
import guardedArt from '../../penguin/penguin-guarded-cutout.png'
import idleArt from '../../penguin/penguin-idle-cutout.png'
import listeningArt from '../../penguin/penguin-listening-cutout.png'
import speakingArt from '../../penguin/penguin-speaking-cutout.png'
import thinkingArt from '../../penguin/penguin-thinking-cutout.png'
import type { PetDockState, PetLayoutMetrics, PetMode } from '../types/assistant'

export interface PetAlphaBBox {
  left: number
  top: number
  right: number
  bottom: number
}

export interface PetArtworkDescriptor {
  src: string
  alt: string
  naturalWidth: number
  naturalHeight: number
  alphaBBox: PetAlphaBBox
}

export interface PetAlphaMask {
  width: number
  height: number
  alpha: Uint8ClampedArray
}

export interface RectLike {
  left: number
  top: number
  width: number
  height: number
}

const ALPHA_HIT_THRESHOLD = 8

const normalArtwork: Record<PetMode, PetArtworkDescriptor> = {
  idle: {
    src: idleArt,
    alt: '管理员企鹅待机立绘',
    naturalWidth: 1024,
    naturalHeight: 1024,
    alphaBBox: { left: 249, top: 108, right: 795, bottom: 891 }
  },
  listening: {
    src: listeningArt,
    alt: '管理员企鹅聆听立绘',
    naturalWidth: 1024,
    naturalHeight: 1024,
    alphaBBox: { left: 223, top: 84, right: 815, bottom: 918 }
  },
  thinking: {
    src: thinkingArt,
    alt: '管理员企鹅思考立绘',
    naturalWidth: 1024,
    naturalHeight: 1024,
    alphaBBox: { left: 255, top: 123, right: 854, bottom: 891 }
  },
  speaking: {
    src: speakingArt,
    alt: '管理员企鹅回复立绘',
    naturalWidth: 1024,
    naturalHeight: 1024,
    alphaBBox: { left: 257, top: 104, right: 804, bottom: 891 }
  },
  guarded: {
    src: guardedArt,
    alt: '管理员企鹅警戒立绘',
    naturalWidth: 1024,
    naturalHeight: 1024,
    alphaBBox: { left: 252, top: 114, right: 795, bottom: 892 }
  }
}

const dockedArtwork: Record<Exclude<PetDockState, 'normal'>, PetArtworkDescriptor> = {
  dockedLeft: {
    src: dockedLeftArt,
    alt: '管理员企鹅左侧半隐藏待机立绘',
    naturalWidth: 1365,
    naturalHeight: 2048,
    alphaBBox: { left: 666, top: 735, right: 986, bottom: 1442 }
  },
  dockedRight: {
    src: dockedRightArt,
    alt: '管理员企鹅右侧半隐藏待机立绘',
    naturalWidth: 1365,
    naturalHeight: 2048,
    alphaBBox: { left: 677, top: 734, right: 987, bottom: 1445 }
  },
  dockedTop: {
    src: dockedTopArt,
    alt: '管理员企鹅顶部半隐藏待机立绘',
    naturalWidth: 1024,
    naturalHeight: 1536,
    alphaBBox: { left: 207, top: 867, right: 729, bottom: 1126 }
  }
}

const alphaMaskCache = new Map<string, Promise<PetAlphaMask>>()

export const getPetArtwork = (mode: PetMode, dockState: PetDockState): PetArtworkDescriptor => {
  if (dockState !== 'normal') {
    return dockedArtwork[dockState]
  }

  return normalArtwork[mode]
}

export const getDockedArtwork = (
  dockState: Exclude<PetDockState, 'normal'>
): PetArtworkDescriptor => dockedArtwork[dockState]

export const normalizeAlphaBBox = (descriptor: PetArtworkDescriptor) => ({
  left: descriptor.alphaBBox.left / descriptor.naturalWidth,
  top: descriptor.alphaBBox.top / descriptor.naturalHeight,
  right: descriptor.alphaBBox.right / descriptor.naturalWidth,
  bottom: descriptor.alphaBBox.bottom / descriptor.naturalHeight
})

export const projectAlphaBBox = (rect: RectLike, descriptor: PetArtworkDescriptor) => {
  const normalized = normalizeAlphaBBox(descriptor)

  return {
    left: rect.left + rect.width * normalized.left,
    top: rect.top + rect.height * normalized.top,
    right: rect.left + rect.width * normalized.right,
    bottom: rect.top + rect.height * normalized.bottom
  }
}

export const computePetLayoutMetrics = (
  rect: RectLike,
  descriptor: PetArtworkDescriptor
): PetLayoutMetrics => {
  const projected = projectAlphaBBox(rect, descriptor)
  const visibleWidth = projected.right - projected.left
  const visibleHeight = projected.bottom - projected.top
  const anchorX = projected.left + visibleWidth / 2
  const anchorY = projected.top + visibleHeight * 0.18
  const faceHalfWidth = visibleWidth * 0.2
  const faceTop = projected.top + visibleHeight * 0.12
  const faceBottom = projected.top + visibleHeight * 0.46

  return {
    anchorX,
    anchorY,
    petLeft: projected.left,
    petTop: projected.top,
    petRight: projected.right,
    petBottom: projected.bottom,
    faceLeft: anchorX - faceHalfWidth,
    faceTop,
    faceRight: anchorX + faceHalfWidth,
    faceBottom
  }
}

export const loadPetAlphaMask = (descriptor: PetArtworkDescriptor): Promise<PetAlphaMask> => {
  const cached = alphaMaskCache.get(descriptor.src)
  if (cached) {
    return cached
  }

  const pending = new Promise<PetAlphaMask>((resolve, reject) => {
    const image = new Image()
    image.decoding = 'async'
    image.onload = () => {
      const canvas = document.createElement('canvas')
      canvas.width = image.naturalWidth
      canvas.height = image.naturalHeight
      const context = canvas.getContext('2d')
      if (!context) {
        reject(new Error('无法创建桌宠命中画布。'))
        return
      }

      context.drawImage(image, 0, 0)
      const { data, width, height } = context.getImageData(0, 0, canvas.width, canvas.height)
      const alpha = new Uint8ClampedArray(width * height)
      for (let index = 0; index < width * height; index += 1) {
        alpha[index] = data[index * 4 + 3]
      }

      resolve({ width, height, alpha })
    }
    image.onerror = () => reject(new Error(`桌宠素材加载失败：${descriptor.src}`))
    image.src = descriptor.src
  })

  alphaMaskCache.set(descriptor.src, pending)
  return pending
}

export const samplePetAlphaAtPoint = (
  descriptor: PetArtworkDescriptor,
  rect: RectLike,
  mask: PetAlphaMask | null,
  clientX: number,
  clientY: number
) => {
  if (
    clientX < rect.left ||
    clientX > rect.left + rect.width ||
    clientY < rect.top ||
    clientY > rect.top + rect.height
  ) {
    return 0
  }

  if (!mask) {
    const bbox = projectAlphaBBox(rect, descriptor)
    return clientX >= bbox.left &&
      clientX <= bbox.right &&
      clientY >= bbox.top &&
      clientY <= bbox.bottom
      ? 255
      : 0
  }

  const scaleX = mask.width / rect.width
  const scaleY = mask.height / rect.height
  const sampleX = Math.max(0, Math.min(mask.width - 1, Math.floor((clientX - rect.left) * scaleX)))
  const sampleY = Math.max(0, Math.min(mask.height - 1, Math.floor((clientY - rect.top) * scaleY)))

  return mask.alpha[sampleY * mask.width + sampleX] ?? 0
}

export const isPetOpaqueHit = (
  descriptor: PetArtworkDescriptor,
  rect: RectLike,
  mask: PetAlphaMask | null,
  clientX: number,
  clientY: number
) => samplePetAlphaAtPoint(descriptor, rect, mask, clientX, clientY) >= ALPHA_HIT_THRESHOLD

export const petAlphaHitThreshold = ALPHA_HIT_THRESHOLD
