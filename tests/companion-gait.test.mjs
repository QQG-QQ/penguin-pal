import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {inflateSync} from 'node:zlib'
import {pathToFileURL} from 'node:url'

// Decode the actual checked-in PNGs with Node built-ins, so this regression
// checks foot geometry instead of accepting different file names or hashes.
function rgba(file){
 const png=readFileSync(file),data=[];let width,height,type,depth,interlace
 assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a')
 for(let p=8;p<png.length;){const size=png.readUInt32BE(p),tag=png.toString('ascii',p+4,p+8),chunk=png.subarray(p+8,p+8+size)
  if(tag==='IHDR'){width=chunk.readUInt32BE(0);height=chunk.readUInt32BE(4);depth=chunk[8];type=chunk[9];interlace=chunk[12]}
  if(tag==='IDAT')data.push(chunk);p+=size+12
 }
 assert.equal(width,256);assert.equal(height,256);assert.equal(type,6);assert.equal(depth,8);assert.equal(interlace,0)
 const raw=inflateSync(Buffer.concat(data)),pixels=Buffer.alloc(width*height*4),stride=width*4
 const paeth=(a,b,c)=>{const p=a+b-c,aa=Math.abs(p-a),bb=Math.abs(p-b),cc=Math.abs(p-c);return aa<=bb&&aa<=cc?a:bb<=cc?b:c}
 for(let y=0;y<height;y++){
  const mode=raw[y*(stride+1)];assert.ok(mode<=4)
  for(let x=0;x<stride;x++){
   const k=y*stride+x,a=x>=4?pixels[k-4]:0,b=y?pixels[k-stride]:0,c=y&&x>=4?pixels[k-stride-4]:0
   pixels[k]=(raw[y*(stride+1)+1+x]+[0,a,b,Math.floor((a+b)/2),paeth(a,b,c)][mode])&255
  }
 }
 return pixels
}
function feet(pixels){
 const mask=new Uint8Array(65536),groups=[]
 for(let y=185;y<256;y++)for(let x=0;x<256;x++){const i=(y*256+x)*4;mask[y*256+x]=pixels[i+3]>128&&pixels[i]>150&&pixels[i+1]>85&&pixels[i+2]<140&&pixels[i]>pixels[i+1]*1.04&&pixels[i+1]>pixels[i+2]*1.2?1:0}
 for(let p=185*256;p<65536;p++)if(mask[p]){
  const q=[p];mask[p]=0;let area=0,sx=0,sy=0,bottom=0
  for(let j=0;j<q.length;j++){const k=q[j],x=k%256,y=Math.floor(k/256);area++;sx+=x;sy+=y;bottom=Math.max(bottom,y)
   for(const n of [x>0?k-1:-1,x<255?k+1:-1,k>=256?k-256:-1,k<65280?k+256:-1])if(n>=0&&mask[n]){mask[n]=0;q.push(n)}
  }
  if(area>30)groups.push({area,x:sx/area,y:sy/area,bottom})
 }
 return groups.sort((a,b)=>b.area-a.area)
}
const root=process.env.PENGUIN_GAIT_FIXTURE?pathToFileURL(process.env.PENGUIN_GAIT_FIXTURE.replace(/[\\/]?$/,'/')):new URL('../penguin/companion-v3/',import.meta.url)

test('builtin idle PNG loop closes within an ordinary frame step without a pose flash',()=>{
 const dir=new URL('../penguin/companion-v4/',import.meta.url)
 const clip=JSON.parse(readFileSync(new URL('motions.json',dir),'utf8')).clips.find(c=>c.slot==='idle')
 const frames=clip.frames.map(file=>rgba(new URL(file,dir)))
 const delta=(a,b)=>{
  let total=0
  for(let i=0;i<a.length;i+=4)for(let c=0;c<3;c++){
   const aa=a[i+3]/255,ba=b[i+3]/255
   total+=Math.abs(a[i+c]*aa+190*(1-aa)-b[i+c]*ba-190*(1-ba))
  }
  return total/(256*256*3)
 }
 const steps=frames.slice(1).map((frame,i)=>delta(frames[i],frame)).sort((a,b)=>a-b)
 const seam=delta(frames.at(-1),frames[0]),p95=steps[Math.floor((steps.length-1)*.95)]
 assert.ok(seam<=p95,`loop seam ${seam.toFixed(3)} exceeds normal frame delta ${p95.toFixed(3)}`)
 assert.ok(Math.abs(clip.durationMs-clip.frames.length*clip.frameMs)<.001)
 for(const frame of frames)for(const i of [0,255,255*256,65535])assert.equal(frame[i*4+3],0,'transparent corners')
})
for(const [direction,sign] of [['right',1],['left',-1]])test(`${direction} gait exchanges support feet and lifts each foot in turn`,()=>{
 const rows=JSON.parse(readFileSync(new URL('motions.json',root),'utf8')),clip=rows.find(c=>c.slot===`walk-${direction}`)
 assert.equal(clip.loop,true);assert.equal(clip.frameMs,200);assert.equal(clip.frames.length,4)
 const poses=clip.frames.map(file=>feet(rgba(new URL(file,root))))
 for(const pose of poses)assert.equal(pose.length,2,'Two distinct yellow boots must remain visible')
 // Perspective keeps the near boot larger than the far one. The two contact
 // poses must exchange their horizontal order, while both stay grounded.
 assert.ok((poses[0][0].x-poses[0][1].x)*sign>20,'near foot must lead contact A')
 assert.ok((poses[2][0].x-poses[2][1].x)*sign< -20,'far foot must lead contact B')
 for(const n of [0,2])assert.ok(Math.abs(poses[n][0].bottom-poses[n][1].bottom)<=8,'contact feet share floor')
 assert.ok(poses[1][0].y-poses[1][1].y>12,'far boot lifts while near boot supports')
 assert.ok(poses[3][1].y-poses[3][0].y>12,'near boot lifts while far boot supports')
 assert.ok(Math.abs(poses[1][0].bottom-poses[3][1].bottom)<=4,'support floor remains stable')
})
