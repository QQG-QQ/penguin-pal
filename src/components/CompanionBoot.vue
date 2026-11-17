<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
// One random boot animation per launch. The assets are animated WebP (256x256,
// alpha, 2.4 s) authored to end exactly on the idle pose, so handing over to the
// normal sprite is seamless. They are decorative: no input, no layout shift.
const BOOT_MS = 2400
const emit = defineEmits<{ done: [] }>()
const files = import.meta.glob('../../penguin/companion-v4/boot-*.webp', { eager: true, query: '?url', import: 'default' }) as Record<string, string>
const pool = Object.keys(files).sort().map(key => files[key])
const src = pool.length ? pool[Math.floor(Math.random() * pool.length)] : ''
const failed = ref(false)
let timer: number | undefined
let finished = false
function finish() { if (finished) return; finished = true; window.clearTimeout(timer); emit('done') }
function loaded() { window.clearTimeout(timer); timer = window.setTimeout(finish, BOOT_MS) }
function failedToLoad() { failed.value = true; finish() }
onMounted(() => { if (!src) { finish(); return } timer = window.setTimeout(finish, BOOT_MS + 2500) })
onBeforeUnmount(() => window.clearTimeout(timer))
</script>

<template>
  <img v-if="src && !failed" class="boot-media" :src="src" alt="" draggable="false" @load="loaded" @error="failedToLoad" />
</template>

<style scoped>
.boot-media{position:absolute;inset:0;width:100%;height:100%;object-fit:contain;pointer-events:none}
</style>
