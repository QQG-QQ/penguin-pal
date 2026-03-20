// Tauri 构建脚本
// 1. 在 Windows 上确保 LLVM/CMake/Ninja 已就绪
// 2. 清理 whisper-rs-sys 旧缓存，确保新的 CMake 配置生效
// 3. 预编译触发 whisper-rs-sys 生成原生库
// 4. 修复 Ninja 下的 whisper/ggml 产物路径
// 5. 执行 tauri dev/build
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')

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

  if (process.platform === 'win32') {
    const cargoEnv = tauriArgs[0] === 'build'
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

    if (tauriArgs[0] === 'build') {
      console.log('[build] Step 2: Cleaning stale release artifacts...')
      await run('cargo', ['clean', '--release'], join(projectRoot, 'src-tauri'), cargoEnv)
    }

    console.log('[build] Step 3: Cleaning stale whisper-rs-sys artifacts...')
    await run('cargo', ['clean', '-p', 'whisper-rs-sys'], join(projectRoot, 'src-tauri'), cargoEnv)

    const prebuildArgs = tauriArgs[0] === 'build'
      ? ['build', '--release']
      : ['build']

    if (tauriArgs[0] === 'build') {
      console.log('[build] Windows release packaging uses single-job Cargo to reduce rmeta/pagefile failures.')
    }

    console.log('[build] Step 4: Pre-compiling whisper-rs-sys artifacts...')
    await run('cargo', prebuildArgs, join(projectRoot, 'src-tauri'), cargoEnv)

    console.log('[build] Step 5: Fixing whisper-rs-sys output paths...')
    const fixCode = await run('node', ['./scripts/fix-whisper-path.mjs'])
    if (fixCode !== 0) {
      process.exit(1)
    }
  }

  const tauriEnv = process.platform === 'win32' && tauriArgs[0] === 'build'
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
