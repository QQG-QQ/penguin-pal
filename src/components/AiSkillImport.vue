<script setup lang="ts">
import { computed, ref } from 'vue'

interface Skill { id: string; name: string; source: string }
interface App { id: string; label: string }
const props = defineProps<{ skills: Skill[]; apps: App[]; client: string; busy: boolean }>()
const emit = defineEmits<{ preview: [ids: string[], targets: string[]]; cancel: [] }>()
const query = ref('')
const ids = ref<string[]>([])
const targets = ref([props.client])
const sourceName = (source: string) => props.apps.find(app => app.id === source)?.label
  ?? ({ agents: '共享 Skills', penguinpal: '桌宠技能库' }[source] || source)
const matches = computed(() => {
  const filter = query.value.trim().toLowerCase()
  return props.skills.filter(skill => (skill.name + ' ' + sourceName(skill.source)).toLowerCase().includes(filter))
})
function selectMatches() {
  const chosen = new Set(props.skills.filter(skill => ids.value.includes(skill.id)).map(skill => skill.name.toLowerCase()))
  for (const skill of matches.value) {
    const name = skill.name.toLowerCase()
    if (ids.value.length === 50) break
    if (!chosen.has(name)) {
      chosen.add(name)
      ids.value.push(skill.id)
    }
  }
}
function choose(skill: Skill, event: Event) {
  ids.value = ids.value.filter(id => id !== skill.id && !props.skills.some(item => item.id === id && item.name.toLowerCase() === skill.name.toLowerCase()))
  if ((event.target as HTMLInputElement).checked) ids.value.push(skill.id)
}
</script>

<template>
  <form class="skill-import" @submit.prevent="emit('preview', [...ids], [...targets])">
    <p>复制到桌宠技能库，并在所选客户端启用。</p>
    <div class="toolbar">
      <input v-model="query" type="search" aria-label="搜索本机 Skill" placeholder="筛选名称或来源" />
      <button type="button" :disabled="!matches.length || ids.length >= 50" @click="selectMatches">全选当前结果</button>
      <button type="button" :disabled="!ids.length" @click="ids = []">清空</button>
    </div>
    <div class="skills">
      <p v-if="!matches.length">没有找到匹配的本机 Skill。</p>
      <label v-for="skill in matches" :key="skill.id">
        <input type="checkbox" :checked="ids.includes(skill.id)" :disabled="!ids.includes(skill.id) && ids.length >= 50" @change="choose(skill, $event)" />
        <span><strong>{{ skill.name }}</strong><small>{{ sourceName(skill.source) }}</small></span>
      </label>
    </div>
    <fieldset>
      <legend>导入后启用</legend>
      <label v-for="app in apps" :key="app.id"><input v-model="targets" type="checkbox" :value="app.id" />{{ app.label }}</label>
    </fieldset>
    <div class="toolbar">
      <button type="button" @click="emit('cancel')">取消</button>
      <button class="primary" :disabled="!ids.length || busy">预览导入{{ ids.length ? ' ' + ids.length + ' 个' : '' }}</button>
    </div>
  </form>
</template>

<style scoped>
.skill-import{display:grid;gap:10px;margin-top:14px;padding-top:14px;border-top:1px solid #dbe7ee}
p{margin:0}
.toolbar{display:flex;gap:8px;flex-wrap:wrap}
.toolbar input{flex:1;min-width:130px}
.skills{max-height:230px;overflow:auto;border:1px solid #dbe7ee;border-radius:8px;padding:8px}
.skills label,fieldset label{display:flex;align-items:center;gap:8px}
.skills label{padding:7px 4px;border-bottom:1px solid #edf2f5}
.skills label span{min-width:0;overflow-wrap:anywhere}
small{display:block;color:#526578}
fieldset{border:1px solid #dbe7ee;border-radius:8px;min-width:0;display:grid;grid-template-columns:repeat(auto-fit,minmax(145px,1fr));gap:8px 14px;padding:10px 12px}
fieldset label{min-height:28px}
button,input[type=search]{font:inherit;color:inherit;background:#fff;border:1px solid #cbdbe7;border-radius:7px;padding:8px 10px}
button{cursor:pointer}
button:disabled{cursor:default;opacity:.5}
.primary{background:#24576d;color:#fff;border-color:#24576d}
</style>
