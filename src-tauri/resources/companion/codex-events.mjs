// Only allowlisted lifecycle metadata leaves stdin. Never log or store raw payloads.
import { createHash, randomUUID } from 'node:crypto'
import { mkdir, readFile, readdir, unlink, writeFile, rename } from 'node:fs/promises'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

export const events = new Set(['UserPromptSubmit', 'PermissionRequest', 'PostToolUse', 'Stop', 'Interrupt', 'SessionEnd'])
export function sanitize(input, at = Date.now()) {
  if (!input || !events.has(input.hook_event_name) || typeof input.session_id !== 'string'
    || !input.session_id || input.session_id.length > 256 || input.agent_id || input.subagent_id) return null
  const hash = value => createHash('sha256').update(value).digest('hex').slice(0, 32)
  const response = input.tool_response
  // Explicit structured tool error flags only; never inspect text or infer turn success.
  const issue = input.hook_event_name === 'PostToolUse' && response && typeof response === 'object'
    && (response.isError === true || response.is_error === true || response.success === false
      || (Number.isInteger(response.exit_code) && response.exit_code !== 0))
  const result = { event: input.hook_event_name, session: hash(input.session_id),
    turn: hash(typeof input.turn_id === 'string' ? input.turn_id : 'session'), at }
  if (issue) result.issue = true
  return result
}
export async function emit(root, input) {
  if (!root || (await readFile(join(root, 'bridge-enabled.json'), 'utf8')).trim() !== 'true') return
  const event = sanitize(input)
  if (!event) return
  const dir = join(root, 'events')
  await mkdir(dir, { recursive: true })
  const files = (await readdir(dir)).filter(name => /^[0-9]+-[a-f0-9-]+\.json$/.test(name)).sort()
  // Bounded queue when the pet is closed. Only delete this bridge's own filenames.
  for (const name of files.slice(0, Math.max(0, files.length - 127))) await unlink(join(dir, name)).catch(() => {})
  const name = `${event.at}-${randomUUID()}`
  await writeFile(join(dir, `${name}.tmp`), JSON.stringify(event), { flag: 'wx' })
  await rename(join(dir, `${name}.tmp`), join(dir, `${name}.json`))
  // Concurrent hooks can all see the same pre-write count. Prune after publishing too.
  const published = (await readdir(dir)).filter(name => /^[0-9]+-[a-f0-9-]+\.json$/.test(name)).sort()
  for (const file of published.slice(0, Math.max(0, published.length - 128))) await unlink(join(dir, file)).catch(() => {})
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  let bytes = 0, chunks = []
  try {
    for await (const chunk of process.stdin) {
      bytes += chunk.length
      if (bytes > 1048576) { chunks = []; break }
      chunks.push(chunk)
    }
    if (bytes <= 1048576) await emit(process.argv[2], JSON.parse(Buffer.concat(chunks).toString('utf8')))
  } catch { /* Pet failures never block Codex and never echo private hook payloads. */ }
}
