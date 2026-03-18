// Tauri 构建脚本
// 1. 在 Windows 上确保 LLVM/CMake 已就绪
// 2. 交给上游 whisper-rs-sys + CMake 正常构建
// 3. 执行 tauri dev/build
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')

function run(cmd, args, cwd = projectRoot) {
  return new Promise((resolve) => {
    console.log(`[build] Running: ${cmd} ${args.join(' ')}`)
    const proc = spawn(cmd, args, {
      stdio: 'inherit',
      cwd,
      shell: true
    })
    proc.on('close', (code) => resolve(code))
    proc.on('error', () => resolve(1))
  })
}

async function main() {
  const args = process.argv.slice(2)
  const tauriArgs = args.length > 0 ? args : ['build']

  if (process.platform === 'win32') {
    console.log('[build] Step 1: Checking local LLVM/CMake...')
    const ensureCode = await run('node', ['./scripts/ensure-llvm.mjs'])
    if (ensureCode !== 0) {
      process.exit(1)
    }
  }

  console.log('[build] Step 2: Running tauri', tauriArgs.join(' '), '...')
  const tauriCode = await run('npx', ['tauri', ...tauriArgs])
  process.exit(tauriCode)
}

main()
