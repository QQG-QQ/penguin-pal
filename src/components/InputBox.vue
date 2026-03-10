<script setup lang="ts">
defineProps<{
  modelValue: string
  busy: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  send: []
  focus: []
  blur: []
}>()

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault()
    emit('send')
  }
}
</script>

<template>
  <section class="input-shell">
    <div class="composer">
      <textarea
        :value="modelValue"
        rows="2"
        placeholder="直接聊天，回车发送。输入“打开设置”或“隐藏到托盘”。"
        :disabled="busy"
        @focus="emit('focus')"
        @blur="emit('blur')"
        @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
        @keydown="handleKeydown"
      />
    </div>
  </section>
</template>

<style scoped>
.input-shell {
  width: min(100%, 280px);
}

.composer {
  padding: 11px 12px;
  border-radius: 24px;
  background: rgba(255, 255, 255, 0.95);
  box-shadow:
    0 16px 30px rgba(6, 18, 32, 0.12),
    inset 0 1px 0 rgba(255, 255, 255, 0.76);
}

textarea {
  width: 100%;
  min-height: 54px;
  border: none;
  resize: none;
  outline: none;
  background: transparent;
  color: #183949;
  font-size: 13px;
  line-height: 1.5;
}
</style>
