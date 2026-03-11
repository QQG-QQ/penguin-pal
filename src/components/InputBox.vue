<script setup lang="ts">
import { ref } from 'vue'

defineProps<{
  modelValue: string
  busy: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  send: []
  focus: []
  blur: []
  historyUp: []
  historyDown: []
}>()

const handleKeydown = (event: KeyboardEvent) => {
  const target = event.target as HTMLTextAreaElement | null
  const selectionStart = target?.selectionStart ?? 0
  const selectionEnd = target?.selectionEnd ?? 0
  const valueLength = target?.value.length ?? 0

  if (event.key === 'ArrowUp' && !event.shiftKey && selectionStart === selectionEnd && selectionStart === 0) {
    event.preventDefault()
    emit('historyUp')
    return
  }

  if (
    event.key === 'ArrowDown' &&
    !event.shiftKey &&
    selectionStart === selectionEnd &&
    selectionEnd === valueLength
  ) {
    event.preventDefault()
    emit('historyDown')
    return
  }

  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault()
    emit('send')
  }
}

const textareaRef = ref<HTMLTextAreaElement | null>(null)

const focusComposer = () => {
  textareaRef.value?.focus()
}

defineExpose({
  focusComposer
})
</script>

<template>
  <section class="input-shell">
    <div class="composer">
      <textarea
        ref="textareaRef"
        :value="modelValue"
        rows="2"
        placeholder="直接聊天，回车发送。输入 /help、打开设置 或 隐藏到托盘。"
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
