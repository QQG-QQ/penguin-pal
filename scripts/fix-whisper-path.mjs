// 修复 whisper-rs-sys 构建路径问题
// 在 Windows + Ninja 下，whisper/ggml 静态库有时会落在 out/lib 或 build 子目录里，
// 但上游 build.rs 实际只搜 OUT_DIR 根目录和 OUT_DIR/build/**。
import { existsSync, mkdirSync, copyFileSync, readdirSync } from 'fs'
import { basename, join } from 'path'
import { fileURLToPath } from 'url'
import { dirname } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const srcTauri = join(__dirname, '..', 'src-tauri')
const buildRoots = [
  join(srcTauri, 'target', 'debug', 'build'),
  join(srcTauri, 'target', 'release', 'build'),
]
const libraryNames = new Set([
  'whisper.lib',
  'ggml.lib',
  'ggml-base.lib',
  'ggml-cpu.lib',
  'libwhisper.a',
  'libggml.a',
  'libggml-base.a',
  'libggml-cpu.a',
])
const buildSubdirs = ['.', 'Release', 'RelWithDebInfo', 'Debug']

function walkFiles(dir, results = []) {
  if (!existsSync(dir)) return results

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const fullPath = join(dir, entry.name)
    if (entry.isDirectory()) {
      walkFiles(fullPath, results)
    } else {
      results.push(fullPath)
    }
  }

  return results
}

function findWhisperOutEntries() {
  const outEntries = []

  for (const buildRoot of buildRoots) {
    if (!existsSync(buildRoot)) {
      continue
    }

    const dirs = readdirSync(buildRoot)
    for (const dir of dirs) {
      if (!dir.startsWith('whisper-rs-sys-')) {
        continue
      }

      const outRoot = join(buildRoot, dir, 'out')
      const buildDir = join(outRoot, 'build')
      if (!existsSync(outRoot)) {
        continue
      }

      outEntries.push({ outRoot, buildDir })
    }
  }

  return outEntries
}

function discoverLibraries(rootDir) {
  const libraries = new Map()
  for (const file of walkFiles(rootDir)) {
    const name = basename(file)
    if (!libraryNames.has(name)) {
      continue
    }

    if (!libraries.has(name)) {
      libraries.set(name, file)
    }
  }

  return libraries
}

function buildTargets(outRoot, buildDir) {
  const targets = [outRoot]

  if (existsSync(buildDir)) {
    targets.push(buildDir)
    for (const subdir of buildSubdirs) {
      if (subdir === '.') {
        continue
      }
      targets.push(join(buildDir, subdir))
    }
  }

  return targets
}

function mirrorLibraries(targetDirs, libraries) {
  let copied = 0

  for (const [name, source] of libraries.entries()) {
    for (const targetDir of targetDirs) {
      const targetFile = join(targetDir, name)

      if (targetFile === source || existsSync(targetFile)) {
        continue
      }

      mkdirSync(targetDir, { recursive: true })
      copyFileSync(source, targetFile)
      copied += 1
      console.log(`[whisper-fix] Copied ${name} -> ${targetFile}`)
    }
  }

  return copied
}

function fixWhisperPath() {
  const outEntries = findWhisperOutEntries()
  if (outEntries.length === 0) {
    console.log('[whisper-fix] No whisper-rs-sys build dir found, skipping')
    return
  }

  let totalCopied = 0

  for (const { outRoot, buildDir } of outEntries) {
    const libraries = discoverLibraries(outRoot)
    if (libraries.size === 0) {
      console.log(`[whisper-fix] No whisper/ggml static libs found under ${outRoot}, skipping`)
      continue
    }

    totalCopied += mirrorLibraries(buildTargets(outRoot, buildDir), libraries)
  }

  if (totalCopied === 0) {
    console.log('[whisper-fix] No path fix needed')
  } else {
    console.log(`[whisper-fix] Fixed ${totalCopied} library path entries`)
  }
}

fixWhisperPath()
