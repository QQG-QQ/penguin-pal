<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
const props=defineProps<{src:string;alt:string;video?:string;paused:boolean;loop:boolean;probe?:boolean;request?:number}>()
const emit=defineEmits<{failed:[string,string];playing:[string,number];ended:[number];imageFailed:[];verified:[{width:number;height:number;corner:number;center:number;playing:boolean}]}>()
const active=ref(-1),loaded=ref(false)
const videos:HTMLVideoElement[]=[],sources:string[]=[],attempts=[0,0],retries:(number|undefined)[]=[],frameCallbacks:(number|undefined)[]=[]
let generation=0,pending=-1,timer:number|undefined,disposed=false
function clear(index:number){
  attempts[index]++;window.clearTimeout(retries[index]);sources[index]=''
  const v=videos[index]
  if(v){if(frameCallbacks[index]!==undefined)v.cancelVideoFrameCallback(frameCallbacks[index]!);frameCallbacks[index]=undefined;v.onloadeddata=null;v.onloadedmetadata=null;v.onerror=null;v.onended=null;v.pause();v.preload='none';v.removeAttribute('src');v.load()}
}
function reset(){generation++;window.clearTimeout(timer);pending=-1;active.value=-1;loaded.value=false;videos.forEach((_,i)=>clear(i))}
function current(index:number,token:number){return !disposed&&token===generation&&sources[index]===props.video}
function fail(index:number,token:number,reason:string){if(!current(index,token))return;const url=sources[index];reset();emit('failed',url,reason)}
function verify(index:number,token:number){
  if(!props.probe)return
  const v=videos[index]
  if(frameCallbacks[index]!==undefined)v.cancelVideoFrameCallback(frameCallbacks[index]!)
  frameCallbacks[index]=v.requestVideoFrameCallback(()=>{
    frameCallbacks[index]=undefined
    if(!current(index,token)||props.paused||active.value!==index)return
    const canvas=document.createElement('canvas');canvas.width=256;canvas.height=256
    const ctx=canvas.getContext('2d')
    if(ctx){ctx.drawImage(v,0,0,256,256);emit('verified',{width:v.videoWidth,height:v.videoHeight,corner:ctx.getImageData(0,0,1,1).data[3],center:ctx.getImageData(128,128,1,1).data[3],playing:!v.paused})}
  })
}
async function play(index:number,token:number,retry=0){
  if(!current(index,token)||props.paused||active.value!==index)return
  const v=videos[index],attempt=++attempts[index]
  try{
    await v.play()
    if(current(index,token)&&attempt===attempts[index]&&!props.paused){emit('playing',sources[index],props.request??0);verify(index,token)}
  }catch(error){
    // pause(), source replacement and teardown legitimately abort an outstanding play().
    if(!current(index,token)||attempt!==attempts[index]||props.paused||active.value!==index)return
    if(error instanceof DOMException&&error.name==='AbortError'&&retry<1){
      retries[index]=window.setTimeout(()=>{void play(index,token,retry+1)},50);return
    }
    fail(index,token,error instanceof DOMException&&error.name==='NotAllowedError'?'blocked':'play')
  }
}
function armTimeout(index:number,token:number){
  window.clearTimeout(timer)
  if(!props.paused)timer=window.setTimeout(()=>fail(index,token,'timeout'),8000)
}
async function load(url:string|undefined){
  if(!url){reset();return}
  await nextTick()
  if(disposed||url!==props.video)return
  const token=++generation,index=active.value===0?1:0,v=videos[index]
  const request=props.request??0
  if(!v){emit('failed',url,'load');return}
  window.clearTimeout(timer);clear(index);pending=index;sources[index]=url
  v.muted=true;v.defaultMuted=true;v.playsInline=true;v.loop=props.loop
  v.onloadedmetadata=()=>{if(v.videoWidth!==256||v.videoHeight!==256||!Number.isFinite(v.duration)||v.duration<=0||v.duration>60)fail(index,token,'metadata')}
  v.onloadeddata=()=>{
    if(!current(index,token))return
    window.clearTimeout(timer);pending=-1
    // Publish the ready buffer before play(), so a concurrent pause sees its ownership.
    const previous=active.value;active.value=index;loaded.value=true
    if(previous>=0&&previous!==index)clear(previous)
    void play(index,token)
  }
  v.onerror=()=>fail(index,token,({1:'aborted',2:'network',3:'decode',4:'format'} as Record<number,string>)[v.error?.code??0]??'load')
  v.onended=()=>{if(current(index,token)&&active.value===index&&!props.paused)emit('ended',request)}
  v.preload='auto';v.src=url;v.load();armTimeout(index,token)
}
watch(()=>[props.video,props.request],()=>{void load(props.video)},{flush:'post'})
onMounted(()=>{void load(props.video)})
watch(()=>[props.paused,props.loop],()=>{
  videos.forEach((v,i)=>{v.loop=props.loop;if(props.paused||i!==active.value){attempts[i]++;window.clearTimeout(retries[i]);v.pause()}})
  if(props.paused)window.clearTimeout(timer)
  else if(pending>=0)armTimeout(pending,generation)
  else if(active.value>=0)void play(active.value,generation)
})
onBeforeUnmount(()=>{disposed=true;reset()})
</script>
<template>
  <span class="sprite-media">
    <img :src="src" :alt="alt" @error="emit('imageFailed')" draggable="false" :style="{visibility:loaded?'hidden':'visible'}"/>
    <video v-for="index in [0,1]" :key="index" :ref="el=>{if(el)videos[index]=el as HTMLVideoElement}" muted playsinline preload="none" disablepictureinpicture aria-hidden="true" :style="{visibility:active===index?'visible':'hidden'}"/>
  </span>
</template>
<style scoped>
.sprite-media{display:block;position:relative;width:250px;height:250px;pointer-events:none}.sprite-media img,.sprite-media video{position:absolute;inset:0;width:100%;height:100%;object-fit:contain}
</style>
