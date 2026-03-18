// 修复 whisper-rs-sys 构建路径问题
// Ninja 生成器输出在 build/，但 whisper-rs-sys 期望 build/Release/
import { existsSync, mkdirSync, copyFileSync, readdirSync, statSync } from 'fs'
import { join } from 'path'
import { fileURLToPath } from 'url'
import { dirname } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const targetDir = join(__dirname, '..', 'src-tauri', 'target', 'release', 'build')

function findWhisperOutDir() {
  if (!existsSync(targetDir)) return null

  const dirs = readdirSync(targetDir)
  for (const dir of dirs) {
    if (dir.startsWith('whisper-rs-sys-')) {
      const outDir = join(targetDir, dir, 'out', 'build')
      if (existsSync(outDir)) {
        return outDir
      }
    }
  }
  return null
}

function fixWhisperPath() {
  const outDir = findWhisperOutDir()
  if (!outDir) {
    console.log('[whisper-fix] No whisper-rs-sys build dir found, skipping')
    return
  }

  const whisperLib = join(outDir, 'whisper.lib')
  const releaseDir = join(outDir, 'Release')
  const releaseLib = join(releaseDir, 'whisper.lib')

  // 检查是否需要修复
  if (!existsSync(whisperLib)) {
    console.log('[whisper-fix] whisper.lib not found in build dir, skipping')
    return
  }

  if (existsSync(releaseLib)) {
    console.log('[whisper-fix] Release/whisper.lib already exists, skipping')
    return
  }

  // 创建 Release 目录并复制文件
  console.log('[whisper-fix] Fixing whisper-rs-sys output path...')
  mkdirSync(releaseDir, { recursive: true })
  copyFileSync(whisperLib, releaseLib)
  console.log('[whisper-fix] Created Release/whisper.lib')
}

fixWhisperPath()
