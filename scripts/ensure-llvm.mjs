// 检测并安装构建依赖 (LLVM + CMake + Ninja)
import { existsSync } from 'fs'
import { spawn } from 'child_process'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '..')
const srcTauri = join(projectRoot, 'src-tauri')

if (process.platform !== 'win32') {
  console.log('[skip] local LLVM/CMake/Ninja bootstrap only runs on Windows')
  process.exit(0)
}

// 依赖检测配置
const deps = [
  {
    name: 'LLVM',
    checkFile: join(srcTauri, '.llvm', 'bin', 'libclang.dll'),
    setupScript: join(srcTauri, 'setup-llvm.ps1'),
  },
  {
    name: 'CMake',
    checkFile: join(srcTauri, '.cmake', 'bin', 'cmake.exe'),
    setupScript: join(srcTauri, 'setup-cmake.ps1'),
  },
  {
    name: 'Ninja',
    checkFile: join(srcTauri, '.ninja', 'ninja.exe'),
    setupScript: join(srcTauri, 'setup-ninja.ps1'),
  },
]

async function installDep(dep) {
  return new Promise((resolve, reject) => {
    console.log(`[INFO] ${dep.name} not installed, installing...`)
    console.log(`[INFO] This may take a few minutes, please wait...`)

    const ps = spawn('powershell', [
      '-ExecutionPolicy', 'Bypass',
      '-File', dep.setupScript
    ], {
      stdio: 'inherit',
      cwd: srcTauri
    })

    ps.on('close', (code) => {
      if (code !== 0) {
        reject(new Error(`${dep.name} installation failed`))
      } else {
        console.log(`[OK] ${dep.name} installed`)
        resolve()
      }
    })

    ps.on('error', (err) => {
      reject(new Error(`Failed to run ${dep.name} installer: ${err.message}`))
    })
  })
}

async function main() {
  const missing = deps.filter(dep => !existsSync(dep.checkFile))

  if (missing.length === 0) {
    console.log('[OK] All local build dependencies installed (LLVM, CMake, Ninja)')
    console.log('[INFO] Whisper still requires Visual Studio Build Tools (Desktop C++) and Windows SDK')
    process.exit(0)
  }

  console.log(`[INFO] Missing dependencies: ${missing.map(d => d.name).join(', ')}`)

  for (const dep of missing) {
    try {
      await installDep(dep)
    } catch (err) {
      console.error(`[ERROR] ${err.message}`)
      process.exit(1)
    }
  }

  console.log('[OK] All local dependencies installed')
  console.log('[INFO] Whisper still requires Visual Studio Build Tools (Desktop C++) and Windows SDK')
}

main()
