import manifest from '../../penguin/companion-v4/artwork.json'
import rows from '../../penguin/companion-v4/motions.json'
import type { PetArtworkDescriptor } from './petArtwork'
import type { MediaPack, MediaClip, MediaSegment, MotionSlot } from './companionMedia'
const assets=import.meta.glob(['../../penguin/companion-v4/*.png','../../penguin/companion-v4/*.webm','../../penguin/companion-v4/*.webp'],{eager:true,query:'?url',import:'default'}) as Record<string,string>
const url=(file:string)=>assets[`../../penguin/companion-v4/${file}`]
export function companionArtwork(key:string):PetArtworkDescriptor {
  const frame=manifest[key in manifest?key as keyof typeof manifest:'idle']
  return {src:url(frame.file),alt:`企鹅 ${key}`,naturalWidth:frame.width,naturalHeight:frame.height,alphaBBox:frame.alphaBBox}
}
const segment=(s:MediaSegment|undefined)=>s?{...s,poster:url(s.poster),frames:s.frames.map(url),video:s.video?url(s.video):undefined}:undefined
export const builtinMotionPack:MediaPack={schema:1,id:'builtin',name:'内置企鹅',clips:rows.clips.map(row=>{const c=row as MediaClip;return{...c,slot:c.slot as MotionSlot,...segment(c),enter:segment(c.enter),exit:segment(c.exit)}})}
