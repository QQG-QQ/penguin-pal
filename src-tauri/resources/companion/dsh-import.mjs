// 把 DSH 会话日志导入桌宠的历史库，供现有记忆学习流程分批整理。
// 只导入 user/message 的文本内容；跳过子代理会话与工具输出。
import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync, writeFileSync, existsSync } from 'node:fs'
import { DatabaseSync } from 'node:sqlite'
import { join } from 'node:path'
import { zstdDecompressSync } from 'node:zlib'

const MAGIC = Buffer.from([0x28, 0xb5, 0x2f, 0xfd])
const HOME = process.env.USERPROFILE
const ROOT = join(process.env.LOCALAPPDATA, 'com.penguinpal.app', 'companion')
const SESSIONS = join(HOME, '.dsh', 'sessions')
const STATE = join(ROOT, 'dsh-import.json')

function decompress(buffer) {
  const offsets = []
  let index = 0
  while ((index = buffer.indexOf(MAGIC, index)) !== -1) {
    offsets.push(index)
    index += 4
  }
  if (!offsets.length) return buffer
  const parts = []
  for (const offset of offsets) {
    try { parts.push(zstdDecompressSync(buffer.subarray(offset))) } catch {}
  }
  return Buffer.concat(parts)
}

function walk(dir, out) {
  let entries = []
  try { entries = readdirSync(dir, { withFileTypes: true }) } catch { return out }
  for (const entry of entries) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) walk(full, out)
    else if (entry.name.endsWith('.jsonl.zstd')) out.push(full)
  }
  return out
}

function hash(value) {
  return createHash('sha256').update(String(value)).digest('hex').slice(0, 32)
}

function textOf(content) {
  if (!Array.isArray(content)) return ''
  return content.filter((part) => part && part.type === 'text' && typeof part.text === 'string')
    .map((part) => part.text)
    .join('\n')
    .trim()
}

const state = existsSync(STATE) ? JSON.parse(readFileSync(STATE, 'utf8')) : { files: {} }
state.files ??= {}

const db = new DatabaseSync(join(ROOT, 'sync', 'history.sqlite3'))
db.exec('PRAGMA busy_timeout = 5000')
const insert = db.prepare('INSERT OR IGNORE INTO messages(id, session, role, text, at, done) VALUES (?, ?, ?, ?, ?, 0)')

let imported = 0
let scanned = 0
const cutoff = Date.now() - 60 * 24 * 3600 * 1000
for (const file of walk(SESSIONS, [])) {
  let stat
  try { stat = statSync(file) } catch { continue }
  if (stat.mtimeMs < cutoff) continue
  const previous = state.files[file] ?? {}
  if (previous.size === stat.size && previous.mtime === stat.mtimeMs) continue
  let lines
  try { lines = decompress(readFileSync(file)).toString('utf8').split('\n').filter(Boolean) } catch { continue }
  const header = (() => { try { return JSON.parse(lines[0] ?? '{}') } catch { return {} } })()
  if (header.origin === 'subagent' || header.parentSession) {
    state.files[file] = { size: stat.size, mtime: stat.mtimeMs }
    continue
  }
  const session = hash(header.id ?? file)
  let lastSeq = previous.seq ?? 0
  for (const line of lines) {
    let event
    try { event = JSON.parse(line) } catch { continue }
    if (event.type !== 'user/message') continue
    const seq = Number(event.seq ?? 0)
    if (seq <= lastSeq) continue
    const text = textOf(event.data?.content)
    if (text.length < 2 || text.length > 6000) { lastSeq = seq; continue }
    const at = Number(event.time ?? 0) || Date.now()
    insert.run(hash(`${header.id}:${seq}`), session, 'user', text, at)
    imported += 1
    lastSeq = seq
  }
  scanned += 1
  state.files[file] = { size: stat.size, mtime: stat.mtimeMs, seq: lastSeq }
}

db.close()
writeFileSync(STATE, JSON.stringify(state))
console.log(JSON.stringify({ scanned, imported }))
