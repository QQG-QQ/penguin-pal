import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(__dirname, '..')
const runtimeRoot = join(repoRoot, 'src-tauri', '.codex-runtime', 'windows-x64')
const codexCmd = join(runtimeRoot, 'node_modules', '.bin', 'codex.cmd')
const packageJsonPath = join(runtimeRoot, 'node_modules', '@openai', 'codex', 'package.json')
const requestedVersion = process.env.PENGUINPAL_CODEX_VERSION?.trim()
let packageSpec = requestedVersion ? `@openai/codex@${requestedVersion}` : '@openai/codex@latest'

if (process.platform !== 'win32') {
  console.log('[skip] embedded dev Codex bootstrap only runs on Windows')
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

const npmExecPath = process.env.npm_execpath
if (!npmExecPath) {
  console.error('[error] npm_execpath is missing. Please run this via npm/npx on Windows.')
  process.exit(1)
}

const readInstalledVersion = () => {
  if (!existsSync(packageJsonPath)) {
    return null
  }

  try {
    const parsed = JSON.parse(readFileSync(packageJsonPath, 'utf8'))
    return typeof parsed.version === 'string' && parsed.version.trim() ? parsed.version.trim() : null
  } catch {
    return null
  }
}

const runNpm = (args) =>
  spawnSync(process.execPath, [npmExecPath, ...args], {
    cwd: runtimeRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    shell: false
  })

const resolveTargetVersion = () => {
  if (requestedVersion) {
    return requestedVersion
  }

  const view = runNpm(['view', '@openai/codex', 'version', '--json'])
  if (view.error || view.status !== 0) {
    return null
  }

  const raw = `${view.stdout ?? ''}`.trim()
  if (!raw) {
    return null
  }

  try {
    const parsed = JSON.parse(raw)
    return typeof parsed === 'string' && parsed.trim() ? parsed.trim() : null
  } catch {
    const trimmed = raw.replace(/^"+|"+$/g, '').trim()
    return trimmed || null
  }
}

const installedVersion = readInstalledVersion()
const targetVersion = resolveTargetVersion()

if (targetVersion) {
  packageSpec = `@openai/codex@${targetVersion}`
}

if (existsSync(codexCmd) && installedVersion && targetVersion && installedVersion === targetVersion) {
  console.log(`[ok] Codex runtime already up to date: ${installedVersion}`)
  process.exit(0)
}

if (existsSync(codexCmd) && installedVersion && !targetVersion) {
  console.log(
    `[warn] Unable to resolve latest Codex version, keeping current runtime: ${installedVersion}`
  )
  process.exit(0)
}

console.log(`[info] Installing private Codex runtime into src-tauri/.codex-runtime/windows-x64 (${packageSpec})`)

const install = spawnSync(
  process.execPath,
  [npmExecPath, 'install', '--no-fund', '--no-audit', '--save-exact', packageSpec],
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

const finalVersion = readInstalledVersion()
console.log(
  `[done] Private Codex runtime ready: ${codexCmd}${finalVersion ? ` (version ${finalVersion})` : ''}`
)
