// Offline authoring tool. No interpolation model or executable ships with the pet.
import {copyFile,mkdtemp,readFile,rename,rm,writeFile,mkdir} from 'node:fs/promises'
import {dirname,resolve,join,relative} from 'node:path'
import {fileURLToPath} from 'node:url'
import {spawnSync} from 'node:child_process'
import {createHash} from 'node:crypto'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const [ffmpegArg,rifeArg]=process.argv.slice(2)
if(!ffmpegArg||!rifeArg)throw Error('Usage: node scripts/prepare-companion-videos.mjs <ffmpeg-executable> <rife-20221029-directory> [--only-walk]')
if(process.argv[4]&&process.argv[4]!=='--only-walk')throw Error('Unknown authoring option')
const ffmpeg=resolve(ffmpegArg),rife=resolve(rifeArg),dir=join(root,'penguin/companion-v3')
const rows=JSON.parse(await readFile(join(dir,'motions.json'),'utf8'))
const clips=[...rows.map(c=>({...c,fps:c.slot.startsWith('walk-')?1000/c.frameMs:6,source:dir})),{id:'greet',video:'greeting.webm',loop:false,fps:7,source:join(root,'penguin/companion-v2'),frames:Array.from({length:8},(_,i)=>`wave-${i}.png`)}]
function run(exe,args,input){
 const r=spawnSync(exe,args,{input,maxBuffer:32*1024*1024,timeout:180000,windowsHide:true})
 if(r.status!==0)throw Error(`${exe}: ${String(r.stderr)}`)
 return r.stdout
}
const decode=(file,format)=>run(ffmpeg,['-hide_banner','-loglevel','error','-threads','1','-i',file,'-frames:v','1','-f','rawvideo','-pix_fmt',format,'-'])
const ffArgs=['-hide_banner','-loglevel','error','-y','-threads','2']
const work=await mkdtemp(join(dir,'.video-build-'))
try{
 for(const clip of process.argv[4]==='--only-walk'?clips.filter(c=>['walk-left','walk-right'].includes(c.id)):clips){
  if(!/^[a-z-]+\.webm$/.test(clip.video)||clip.frames.some(f=>!/^[a-z-]+-\d\.png$/.test(f)))throw Error('Invalid internal clip filename')
  const count=clip.frames.length,frameCount=Math.round(count/clip.fps*30)
  const walking=['walk-left','walk-right'].includes(clip.id)
  const input=join(work,clip.id+'-in'),output=join(work,clip.id+'-out')
  await mkdir(input);await mkdir(output)
  // RIFE accepts RGB, not RGBA. Stack black/white composites and recover alpha
  // from their difference after interpolation. Never feed four-channel PNGs.
  for(let n=0;n<count;n++){
   const rgba=decode(join(clip.source,clip.frames[n]),'rgba')
   if(rgba.length!==256*256*4)throw Error('Source must be 256x256: '+clip.id)
   if(walking)for(let i=0;i<256*256;i++){
    const x=i%256,y=Math.floor(i/256)
    if((x<8||x>=248||y<8||y>=248)&&rgba[i*4+3]>4)throw Error('Walking source enters its transparent guard: '+clip.id)
   }
   const packed=Buffer.alloc(256*512*3),half=256*256*3
   for(let i=0;i<256*256;i++)for(let c=0;c<3;c++){
    const alpha=rgba[i*4+3]/255,value=rgba[i*4+c]*alpha
    packed[i*3+c]=Math.round(value);packed[half+i*3+c]=Math.round(value+255*(1-alpha))
   }
   run(ffmpeg,[...ffArgs,'-f','rawvideo','-pix_fmt','rgb24','-s','256x512','-i','pipe:0','-frames:v','1','-pix_fmt','rgb24',join(input,String(n).padStart(4,'0')+'.png')],packed)
  }
  // Append the next loop pose or hold the final one-shot pose for its last span.
  await copyFile(join(input,String(clip.loop?0:count-1).padStart(4,'0')+'.png'),join(input,String(count).padStart(4,'0')+'.png'))
  run(join(rife,'rife-ncnn-vulkan.exe'),['-i',input,'-o',output,'-m',join(rife,'rife-v4.6'),'-n',String(Math.round((count+1)/clip.fps*30)),'-g','0','-j','1:1:1','-f','%04d.png'])
  const decoded=[]
  for(let n=1;n<=frameCount;n++){
   const raw=decode(join(output,String(n).padStart(4,'0')+'.png'),'rgb24')
   if(raw.length!==256*512*3)throw Error('Invalid RGB interpolation output')
   const rgba=Buffer.alloc(256*256*4),half=256*256*3
   for(let i=0;i<256*256;i++){
    const offset=i*3,alpha=Math.max(0,Math.min(255,255-((raw[half+offset]-raw[offset])+(raw[half+offset+1]-raw[offset+1])+(raw[half+offset+2]-raw[offset+2]))/3))
    // Dual-matte inference/quantization can leave alpha 1-3 in empty background.
    // Remove only this numerical floor; keep meaningful antialiased boundaries.
    const x=i%256,y=Math.floor(i/256)
    // Every walking source was checked to be empty in this guard band. RIFE
    // may invent low-alpha background residue there; do not crop real artwork.
    rgba[i*4+3]=walking&&(x<8||x>=248||y<8||y>=248)?0:alpha<4?0:Math.round(alpha)
    for(let c=0;c<3;c++)rgba[i*4+c]=alpha>1?Math.min(255,Math.round(raw[offset+c]*255/alpha)):0
   }
   decoded.push(rgba)
  }
  const encoded=join(work,clip.id+'.webm')
  run(ffmpeg,[...ffArgs,'-f','rawvideo','-pix_fmt','rgba','-s','256x256','-framerate','30','-i','pipe:0','-frames:v',String(frameCount),'-c:v','libvpx-vp9','-pix_fmt','yuva420p','-auto-alt-ref','0','-b:v','0','-crf','24','-an',encoded],Buffer.concat(decoded))
  await rename(encoded,join(dir,clip.video))
  console.log(`${clip.id}: ${frameCount} frames at 30 fps`)
 }
 const sha=async path=>createHash('sha256').update(await readFile(path)).digest('hex')
 await writeFile(join(dir,'video-build.json'),JSON.stringify({fps:30,method:'RIFE 4.6 dual black/white matte, VP9 alpha',runtimeInterpolation:false,tool:{release:'20221029',url:'https://github.com/nihui/rife-ncnn-vulkan/releases/tag/20221029',exeSha256:await sha(join(rife,'rife-ncnn-vulkan.exe')),modelSha256:await sha(join(rife,'rife-v4.6/flownet.bin'))},clips:clips.map(c=>({id:c.id,video:c.video,sourceFps:c.fps,sourceFrames:c.frames.length,outputFrames:Math.round(c.frames.length/c.fps*30),loop:c.loop}))},null,2)+'\n')
}finally{
 const name=relative(dir,work)
 if(!name.startsWith('.video-build-')||name.includes('/')||name.includes('\\'))throw Error('Unsafe cleanup path')
 await rm(work,{recursive:true,force:true})
}
