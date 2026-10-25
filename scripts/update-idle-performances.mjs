// Add the two generated idle performances to the builtin motion manifest.
// Frame files are produced by scripts/build-idle-performances.py.
import { readFileSync, writeFileSync } from 'node:fs'

const file = new URL('../penguin/companion-v4/motions.json', import.meta.url)
const manifest = JSON.parse(readFileSync(file, 'utf8'))
const generated = JSON.parse(readFileSync(new URL('../penguin/companion-v4/perform-report.json', import.meta.url), 'utf8'))

const clips = [
  { prefix: 'lion', id: 'idle-lion-dance' },
  { prefix: 'holo', id: 'idle-holo-disc' },
]

manifest.clips = manifest.clips.filter((clip) => !clips.some(({ id }) => id === clip.id))
for (const { prefix, id } of clips) {
  const report = generated[prefix]
  if (!report) throw new Error(`missing generation report for ${prefix}`)
  manifest.clips.push({
    id,
    slot: 'perform',
    weight: 1,
    frameMs: report.meta.frame_ms,
    loop: false,
    poster: `${prefix}-poster.webp`,
    frames: report.names,
    durationMs: report.meta.duration_ms,
  })
}
writeFileSync(file, JSON.stringify(manifest, null, 2) + '\n')
console.log(`clips: ${manifest.clips.length}`)
