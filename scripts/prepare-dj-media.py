"""Offline DJ2 authoring: original pixels, fixed registration, RIFE loop bridge.

Usage: python prepare-dj-media.py source.mp4 ffmpeg.exe rife-directory output-directory
Requires Pillow and numpy only for authoring. No models ship with the application.
"""
from pathlib import Path
import hashlib, json, subprocess, sys, tempfile
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

source, ffmpeg, rife, output = map(Path, sys.argv[1:5])
output.mkdir(parents=True, exist_ok=True)

def run(args, data=None):
    p = subprocess.run([str(a) for a in args], input=data, stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, timeout=240)
    if p.returncode:
        raise RuntimeError(p.stderr.decode('utf-8', errors='replace')[-2400:])
    return p.stdout

raw = run([ffmpeg, '-hide_banner', '-loglevel', 'error', '-nostdin', '-i', source,
           '-an', '-f', 'rawvideo', '-pix_fmt', 'rgb24', 'pipe:1'])
frames = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 720, 720, 3)

def cut(rgb):
    v = rgb.astype(np.int16)
    key = (v[:,:,0]-v[:,:,1] > 40) & (v[:,:,2]-v[:,:,1] > 30) & (v[:,:,0] > 85)
    mask = Image.fromarray((~key).astype('uint8')*255).copy()
    ImageDraw.floodfill(mask, (360,340), 128, thresh=0)
    mask = Image.fromarray((np.asarray(mask)==128).astype('uint8')*255)
    # Small erosion removes contaminated edge pixels; retain the original interior.
    mask = mask.filter(ImageFilter.MinFilter(3)).filter(ImageFilter.GaussianBlur(.5))
    image = Image.fromarray(rgb).convert('RGBA'); image.putalpha(mask)
    return image

images = [cut(f) for f in frames]
boxes = [im.getbbox() for im in images]
# Use one transform throughout; never normalize each frame (would cause breathing size).
top = min(b[1] for b in boxes); bottom = max(b[3] for b in boxes)
ratio = 216/(bottom-top)
center = (min(b[0] for b in boxes)+max(b[2] for b in boxes))/2
registered=[]
for im in images:
    resized=im.resize((round(720*ratio),round(720*ratio)), Image.Resampling.LANCZOS)
    canvas=Image.new('RGBA',(256,256))
    canvas.alpha_composite(resized,(round(128-center*ratio),round(240-bottom*ratio)))
    registered.append(canvas)

# Source candidates were compared in pose and velocity. An explicit short bridge
# closes the remaining difference instead of crossfading two visible penguins.
start,end,bridge_frames=51,219,8
body=registered[start:end]
with tempfile.TemporaryDirectory(prefix='penguin-dj-') as temp:
    temp=Path(temp); inp=temp/'in'; out=temp/'out'; inp.mkdir(); out.mkdir()
    for n,im in enumerate([registered[end],registered[start]]):
        a=np.asarray(im).astype(np.float32); alpha=a[:,:,3:4]/255
        black=a[:,:,:3]*alpha; white=black+255*(1-alpha)
        packed=np.concatenate([black,white],axis=0).round().astype('uint8')
        Image.fromarray(packed).save(inp/f'{n:04d}.png')
    run([rife/'rife-ncnn-vulkan.exe','-i',inp,'-o',out,'-m',rife/'rife-v4.6',
         '-n',str(bridge_frames+2),'-g','0','-j','1:1:1','-f','%04d.png'])
    interpolated=sorted(out.glob('*.png'))
    if len(interpolated)!=bridge_frames+2: raise RuntimeError('Incomplete loop bridge')
    for file in interpolated[1:-1]:
        packed=np.asarray(Image.open(file).convert('RGB')).astype(np.float32)
        black,white=packed[:256],packed[256:]
        alpha=np.clip(255-(white-black).mean(axis=2),0,255)
        rgba=np.zeros((256,256,4),dtype='uint8')
        rgba[:,:,3]=np.where(alpha<4,0,alpha).round().astype('uint8')
        rgba[:,:,:3]=np.clip(black*255/np.maximum(alpha[:,:,None],1),0,255).round().astype('uint8')
        rgba[:8,:,3]=0;rgba[-8:,:,3]=0;rgba[:,:8,3]=0;rgba[:,-8:,3]=0
        body.append(Image.fromarray(rgba))

for n,im in enumerate(body): im.save(output/f'dj-{n:03d}.png')
body[0].save(output/'dj.png')
run([ffmpeg,'-hide_banner','-loglevel','error','-nostdin','-y','-f','rawvideo',
     '-pix_fmt','rgba','-s','256x256','-framerate','24','-i','pipe:0','-an',
     '-c:v','libvpx-vp9','-pix_fmt','yuva420p','-auto-alt-ref','0',
     '-crf','22','-b:v','0',output/'dj.webm'],b''.join(im.tobytes() for im in body))
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
metadata={'sourceSha256':sha(source),'sourceFps':24,'sourceFrameRange':[start,end],
          'bridgeFrames':bridge_frames,'method':'original Doubao frames; RIFE 4.6 dual matte loop closure',
          'fps':24,'frameCount':len(body),'durationMs':len(body)/24*1000,
          'poster':'dj.png','video':'dj.webm','frames':[f'dj-{n:03d}.png' for n in range(len(body))],
          'alphaBBox':list(body[0].getbbox()),
          'hashes':{p.name:sha(p) for p in output.iterdir() if p.suffix in ['.png','.webm']}}
(output/'media.json').write_text(json.dumps(metadata,indent=2),encoding='utf-8')
print(json.dumps({k:v for k,v in metadata.items() if k not in ['frames','hashes']}),flush=True)
