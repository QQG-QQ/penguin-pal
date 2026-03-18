// 智能 Tauri 构建脚本
// 1. 确保依赖安装
// 2. 预编译触发 whisper-rs-sys 构建
// 3. 修复路径问题
// 4. 完整构建
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')
const srcTauri = join(projectRoot, 'src-tauri')

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

  // 1. 确保依赖
  console.log('[build] Step 1: Checking dependencies...')
  const ensureCode = await run('node', ['./scripts/ensure-llvm.mjs'])
  if (ensureCode !== 0) {
    process.exit(1)
  }

  // 2. 预编译（可能失败，没关系）
  console.log('[build] Step 2: Pre-compiling to generate whisper-rs-sys...')
  await run('cargo', ['build', '--release'], srcTauri)

  // 3. 修复路径
  console.log('[build] Step 3: Fixing whisper-rs-sys path...')
  await run('node', ['./scripts/fix-whisper-path.mjs'])

  // 4. 完整构建
  console.log('[build] Step 4: Running tauri', tauriArgs.join(' '), '...')
  const tauriCode = await run('npx', ['tauri', ...tauriArgs])
  process.exit(tauriCode)
}

main()
