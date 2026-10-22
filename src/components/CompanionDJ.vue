<script setup lang="ts">
import { computed } from 'vue'
import CompanionSprite from './CompanionSprite.vue'
import type { MusicClock } from '../lib/companionMusic'
import metadata from '../../penguin/music-dj/media.json'
const props=defineProps<{clock:MusicClock;paused:boolean;videoEnabled:boolean;probe?:boolean}>()
const emit=defineEmits<{failed:[];verified:[{width:number;height:number;corner:number;center:number;playing:boolean}]}>()
const assets=import.meta.glob(['../../penguin/music-dj/*.png','../../penguin/music-dj/*.webm'],{eager:true,query:'?url',import:'default'}) as Record<string,string>
const asset=(file:string)=>assets[`../../penguin/music-dj/${file}`]
const poster=computed(()=>asset(metadata.frames[Math.floor(props.clock.elapsed/(1000/24))%metadata.frameCount]))
const video=asset('dj.webm')
const bars=computed(()=>[props.clock.bass,props.clock.treble].map((energy,side)=>{
  const height=Math.max(.08,Math.min(1,energy*.65+props.clock.level*.2+props.clock.pulse*.25))
  return Array.from({length:15},(_,i)=>({
    opacity:i/15<height?.4+.6*props.clock.pulse:.10,
    background:`hsl(${(props.clock.elapsed*.007+i*18+side*40)%360} 88% 64%)`,
    color:`hsl(${(props.clock.elapsed*.007+i*18+side*40)%360} 88% 64%)`,
  }))
}))
</script>
<template>
  <div class="dj-scene" aria-label="企鹅随音乐打碟">
    <div v-for="(bar,index) in bars" :key="index" class="light-bar" :class="index?'right':'left'" aria-hidden="true"><i v-for="(style,i) in bar" :key="i" :style="style"/></div>
    <CompanionSprite :src="poster" alt="内置企鹅打碟" :video="videoEnabled?video:undefined" :paused="paused" :loop="true" :rate="clock.rate" :probe="probe" @verified="p=>emit('verified',p)" @failed="emit('failed')"/>
  </div>
</template>
<style scoped>
.dj-scene{position:relative;width:250px;height:250px;pointer-events:none}
.light-bar{position:absolute;bottom:16px;width:12px;height:174px;display:flex;flex-direction:column-reverse;justify-content:space-between;padding:3px;background:#101522c0;border-radius:4px;pointer-events:none}
.light-bar.left{left:12px}.light-bar.right{right:12px}
.light-bar i{display:block;width:6px;height:8px;border-radius:2px;transition:opacity 60ms linear;box-shadow:0 0 5px currentColor}
.dj-scene :deep(.sprite-media){position:relative;z-index:1}
</style>
