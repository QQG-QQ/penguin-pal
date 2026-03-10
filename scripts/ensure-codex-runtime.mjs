import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(__dirname, '..')
const runtimeRoot = join(repoRoot, 'src-tauri', '.codex-runtime', 'windows-x64')
const codexCmd = join(runtimeRoot, 'node_modules', '.bin', 'codex.cmd')

if (process.platform !== 'win32') {
  console.log('[skip] embedded dev Codex bootstrap only runs on Windows')
  process.exit(0)
}

if (existsSync(codexCmd)) {
  console.log(`[ok] Codex runtime already present: ${codexCmd}`)
  process.exit(0)
}

mkdirSync(runtimeRoot, { recursive: true })

const runtimePkg = join(runtimeRoot, 'package.json')
if (!existsSync(runtimePkg)) {
  writeFileSync(
    runtimePkg,
    JSON.stringify(
      {
        name: 'penguin-pal-codex-runtime',
        private: true,
        version: '0.0.0'
      },
      null,
      2
    )
  )
}

console.log('[info] Installing private Codex runtime into src-tauri/.codex-runtime/windows-x64')
const npmExecPath = process.env.npm_execpath
if (!npmExecPath) {
  console.error('[error] npm_execpath is missing. Please run this via npm/npx on Windows.')
  process.exit(1)
}

const install = spawnSync(
  process.execPath,
  [npmExecPath, 'install', '--no-fund', '--no-audit', '@openai/codex@latest'],
  {
    cwd: runtimeRoot,
    stdio: 'inherit',
    shell: false
  }
)

if (install.error) {
  console.error(`[error] Failed to spawn npm installer: ${install.error.message}`)
  process.exit(1)
}

if (install.status !== 0) {
  console.error(`[error] Private Codex runtime install failed with exit code ${install.status ?? 'unknown'}`)
  process.exit(install.status ?? 1)
}

if (!existsSync(codexCmd)) {
  console.error('[error] Codex runtime install finished but codex.cmd was not found')
  process.exit(1)
}

console.log(`[done] Private Codex runtime ready: ${codexCmd}`)
