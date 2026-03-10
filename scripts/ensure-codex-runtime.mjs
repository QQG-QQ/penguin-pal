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
const install = spawnSync(
  'npm.cmd',
  ['install', '--no-fund', '--no-audit', '@openai/codex@latest'],
  {
    cwd: runtimeRoot,
    stdio: 'inherit',
    shell: false
  }
)

if (install.status !== 0) {
  process.exit(install.status ?? 1)
}

if (!existsSync(codexCmd)) {
  console.error('[error] Codex runtime install finished but codex.cmd was not found')
  process.exit(1)
}

console.log(`[done] Private Codex runtime ready: ${codexCmd}`)
