// 检测并安装 LLVM 依赖
import { existsSync } from 'fs'
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')
const llvmBin = join(projectRoot, 'src-tauri', '.llvm', 'bin')
const libclangDll = join(llvmBin, 'libclang.dll')

if (existsSync(libclangDll)) {
  console.log('[OK] LLVM 已安装')
  process.exit(0)
}

console.log('[INFO] LLVM 未安装，正在安装...')
console.log('[INFO] 这可能需要几分钟，请耐心等待...')

const setupScript = join(projectRoot, 'src-tauri', 'setup-llvm.ps1')

const ps = spawn('powershell', [
  '-ExecutionPolicy', 'Bypass',
  '-File', setupScript
], {
  stdio: 'inherit',
  cwd: join(projectRoot, 'src-tauri')
})

ps.on('close', (code) => {
  if (code !== 0) {
    console.error('[ERROR] LLVM 安装失败')
    process.exit(1)
  }
  console.log('[OK] LLVM 安装完成')
})
