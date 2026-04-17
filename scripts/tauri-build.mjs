// Tauri 构建脚本
// 1. 在 Windows 上确保 LLVM/CMake/Ninja 已就绪
// 2. 清理 whisper-rs-sys 旧缓存，确保新的 CMake 配置生效
// 3. 预编译触发 whisper-rs-sys 生成原生库
// 4. 修复 Ninja 下的 whisper/ggml 产物路径
// 5. 执行 tauri dev/build
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { existsSync, readdirSync } from 'fs'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')
const whisperBuildPrefix = 'whisper-rs-sys-'

function hasWhisperBuildOutput(profile) {
  const buildRoot = join(projectRoot, 'src-tauri', 'target', profile, 'build')
  if (!existsSync(buildRoot)) {
    return false
  }

  return readdirSync(buildRoot).some((entry) => entry.startsWith(whisperBuildPrefix))
}

function run(cmd, args, cwd = projectRoot, extraEnv = {}) {
  return new Promise((resolve) => {
    console.log(`[build] Running: ${cmd} ${args.join(' ')}`)
    const proc = spawn(cmd, args, {
      stdio: 'inherit',
      cwd,
      shell: true,
      env: {
        ...process.env,
        ...extraEnv
      }
    })
    proc.on('close', (code) => resolve(code))
    proc.on('error', () => resolve(1))
  })
}

async function main() {
  const args = process.argv.slice(2)
  const tauriArgs = args.length > 0 ? args : ['build']
  const isReleaseBuild = tauriArgs[0] === 'build'
  const forceWhisperRebuild = process.env.PENGUIN_FORCE_WHISPER_REBUILD === '1'

  if (process.platform === 'win32') {
    const cargoEnv = isReleaseBuild
      ? {
          CARGO_BUILD_JOBS: '1',
          CARGO_INCREMENTAL: '0'
        }
      : {}

    console.log('[build] Step 1: Checking local LLVM/CMake/Ninja...')
    const ensureCode = await run('node', ['./scripts/ensure-llvm.mjs'])
    if (ensureCode !== 0) {
      process.exit(1)
    }

    if (isReleaseBuild) {
      console.log('[build] Step 2: Cleaning stale release artifacts...')
      await run('cargo', ['clean', '--release'], join(projectRoot, 'src-tauri'), cargoEnv)
    }

    const profile = isReleaseBuild ? 'release' : 'debug'
    const hasCachedWhisperBuild = hasWhisperBuildOutput(profile)
    const shouldRebuildWhisper = isReleaseBuild || forceWhisperRebuild || !hasCachedWhisperBuild

    if (shouldRebuildWhisper) {
      console.log('[build] Step 3: Cleaning stale whisper-rs-sys artifacts...')
      await run('cargo', ['clean', '-p', 'whisper-rs-sys'], join(projectRoot, 'src-tauri'), cargoEnv)
    } else {
      console.log('[build] Step 3: Reusing cached whisper-rs-sys dev artifacts')
    }

    const prebuildArgs = isReleaseBuild
      ? ['build', '--release']
      : ['build']

    if (isReleaseBuild) {
      console.log('[build] Windows release packaging uses single-job Cargo to reduce rmeta/pagefile failures.')
    }

    if (shouldRebuildWhisper) {
      console.log('[build] Step 4: Pre-compiling whisper-rs-sys artifacts...')
      await run('cargo', prebuildArgs, join(projectRoot, 'src-tauri'), cargoEnv)
    } else {
      console.log('[build] Step 4: Skipping whisper-rs-sys prebuild for dev reuse')
    }

    console.log('[build] Step 5: Fixing whisper-rs-sys output paths...')
    const fixCode = await run('node', ['./scripts/fix-whisper-path.mjs'])
    if (fixCode !== 0) {
      process.exit(1)
    }
  }

  const tauriEnv = process.platform === 'win32' && isReleaseBuild
    ? {
        CARGO_BUILD_JOBS: '1',
        CARGO_INCREMENTAL: '0'
      }
    : {}

  console.log('[build] Step 6: Running tauri', tauriArgs.join(' '), '...')
  const tauriCode = await run('npx', ['tauri', ...tauriArgs], projectRoot, tauriEnv)
  process.exit(tauriCode)
}

main()
