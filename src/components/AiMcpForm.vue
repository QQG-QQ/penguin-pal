<script setup lang="ts">
import { computed, reactive } from 'vue'

type Transport = 'stdio' | 'http' | 'sse'
interface App { id: string; label: string; mcpSse?: boolean }
interface Definition { type: Transport; command?: string; url?: string; args?: string[]; cwd?: string }
const props = defineProps<{
  client: string; apps: App[]; initialName: string; initial?: Definition; keepsPrivate?: boolean; busy: boolean
}>()
const emit = defineEmits<{ preview: [name: string, definition: Record<string, unknown>, targets: string[]]; cancel: [] }>()
const draft = reactive({
  name: props.initialName, type: props.initial?.type ?? 'stdio', command: props.initial?.command ?? '',
  cwd: props.initial?.cwd ?? '', url: props.initial?.url ?? '', args: [...(props.initial?.args ?? [])],
  fields: [] as { name: string; value: string }[], targets: [props.client],
})
const blocked = computed(() => draft.type === 'sse' && props.apps.some(app => draft.targets.includes(app.id) && !app.mcpSse))
const privateLabel = computed(() => draft.type === 'stdio' ? '环境变量' : '请求头')
function submit() {
  const pairs = draft.fields.filter(row => row.name.trim())
  const privateFields = pairs.length ? Object.fromEntries(pairs.map(row => [row.name.trim(), row.value])) : undefined
  const fields = draft.type === 'stdio'
    ? { command: draft.command.trim(), args: draft.args, ...(draft.cwd.trim() ? { cwd: draft.cwd.trim() } : {}), env: privateFields }
    : { url: draft.url.trim(), headers: privateFields }
  emit('preview', draft.name.trim(), { type: draft.type, ...fields }, [...draft.targets])
}
</script>

<template>
  <form class="mcp-form" @submit.prevent="submit">
    <div class="form-row">
      <label>配置名称<input v-model="draft.name" required maxlength="64" pattern="[A-Za-z0-9_\-]+" placeholder="company-docs" :readonly="!!initialName" /></label>
      <label>连接方式<select v-model="draft.type" aria-label="连接方式">
        <option value="stdio">本地命令 (stdio)</option><option value="http">HTTP</option><option value="sse">SSE</option>
      </select></label>
    </div>
    <template v-if="draft.type === 'stdio'">
      <label>命令<input v-model="draft.command" required placeholder="例如 npx 或 uvx" /></label>
      <div class="repeated">
        <label v-for="(_, index) in draft.args" :key="index" class="entry">
          <input v-model="draft.args[index]" :aria-label="'参数 ' + (index + 1)" />
          <button type="button" :aria-label="'移除参数 ' + (index + 1)" @click="draft.args.splice(index, 1)">移除</button>
        </label>
        <button type="button" @click="draft.args.push('')">添加参数</button>
      </div>
      <label>工作目录（可选）<input v-model="draft.cwd" /></label>
    </template>
    <label v-else>服务地址<input v-model="draft.url" type="url" required placeholder="https://example.com/mcp" /></label>
    <div class="repeated">
      <div v-for="(field, index) in draft.fields" :key="index" class="entry">
        <input v-model="field.name" :aria-label="privateLabel + '名称'" placeholder="名称" required />
        <input v-model="field.value" :aria-label="privateLabel + '值'" placeholder="值" type="password" autocomplete="off" />
        <button type="button" @click="draft.fields.splice(index, 1)">移除</button>
      </div>
      <button type="button" @click="draft.fields.push({ name: '', value: '' })">添加{{ privateLabel }}</button>
      <small v-if="keepsPrivate">命令或服务地址变更后，需重新填写该连接的凭据。</small>
    </div>
    <fieldset v-if="!initialName">
      <legend>写入到</legend>
      <label v-for="app in apps" :key="app.id" class="target">
        <input v-model="draft.targets" type="checkbox" :value="app.id" />{{ app.label }}
      </label>
    </fieldset>
    <p v-if="blocked" class="warning" role="status">所选客户端有的只支持 HTTP 或 stdio，请调整连接方式或目标。</p>
    <div class="buttons">
      <button type="button" @click="emit('cancel')">取消</button>
      <button class="primary" :disabled="busy || !draft.targets.length || blocked">预览写入</button>
    </div>
  </form>
</template>

<style scoped>
.mcp-form{display:grid;gap:12px;margin-top:14px;padding-top:14px;border-top:1px solid #dbe7ee}
.mcp-form label,.repeated{display:grid;gap:5px;min-width:0}
.form-row,.entry,.buttons{display:flex;gap:8px;align-items:end}
.form-row label,.entry input{flex:1;min-width:0}
.entry button{flex-shrink:0}
.repeated>button,.buttons{justify-self:start}
input:not([type=checkbox]),select,button{box-sizing:border-box;font:inherit;color:inherit;border:1px solid #cbdbe7;border-radius:7px;padding:8px 10px;background:#fff}
button{cursor:pointer}
button:disabled{cursor:default;opacity:.5}
.primary{background:#24576d;color:#fff;border-color:#24576d}
fieldset{border:1px solid #dbe7ee;border-radius:8px;min-width:0;display:grid;grid-template-columns:repeat(auto-fit,minmax(145px,1fr));gap:8px 14px;padding:10px 12px}
.mcp-form label.target{display:flex;align-items:center;gap:7px;min-height:28px}
.warning{color:#8f2f23}
</style>
