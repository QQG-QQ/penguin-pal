<script setup lang="ts">
defineProps<{
  modelValue: string
  busy: boolean
  listening: boolean
  voiceSupported: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  send: []
  voiceStart: []
  voiceStop: []
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
    <button
      class="voice-button"
      type="button"
      :disabled="!voiceSupported || busy"
      :class="{ active: listening }"
      @pointerdown.prevent="emit('voiceStart')"
      @pointerup.prevent="emit('voiceStop')"
      @pointerleave="emit('voiceStop')"
      @pointercancel="emit('voiceStop')"
    >
      <span class="voice-label">{{ listening ? '松开' : '按住' }}</span>
      <span class="voice-copy">
        {{ voiceSupported ? (listening ? '结束并发送' : '直接说话') : '未检测到麦克风' }}
      </span>
    </button>

    <div class="composer">
      <textarea
        :value="modelValue"
        rows="2"
        placeholder="直接聊天，或输入“打开设置”“隐藏到托盘”。"
        :disabled="busy"
        @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
        @keydown="handleKeydown"
      />

      <div class="composer-actions">
        <span class="composer-hint">
          {{
            voiceSupported
              ? '检测到麦克风后默认启用语音输入，松开发送。'
              : '当前没有可用麦克风或语音识别环境，只能使用文字输入。'
          }}
        </span>
        <button class="send-button" type="button" :disabled="busy" @click="emit('send')">
          {{ busy ? '处理中' : '发送' }}
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.input-shell {
  display: grid;
  grid-template-columns: 70px 1fr;
  gap: 8px;
  width: min(100%, 280px);
}

.voice-button,
.send-button {
  border: none;
  cursor: pointer;
}

.voice-button {
  min-height: 82px;
  padding: 12px 9px;
  border-radius: 24px;
  background: linear-gradient(180deg, rgba(17, 48, 62, 0.96), rgba(8, 23, 33, 0.98));
  color: #effbff;
  box-shadow:
    0 16px 30px rgba(4, 17, 29, 0.16),
    inset 0 1px 0 rgba(255, 255, 255, 0.16);
}

.voice-button.active {
  background: linear-gradient(180deg, #0d796b, #0f5363);
}

.voice-button:disabled,
.send-button:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.voice-label,
.voice-copy {
  display: block;
}

.voice-label {
  font-size: 14px;
  font-weight: 600;
}

.voice-copy {
  margin-top: 6px;
  font-size: 11px;
  line-height: 1.45;
  color: rgba(232, 248, 252, 0.78);
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

.composer-actions {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: center;
  margin-top: 8px;
}

.composer-hint {
  color: #436171;
  font-size: 11px;
  line-height: 1.45;
}

.send-button {
  min-width: 72px;
  min-height: 34px;
  padding: 0 14px;
  border-radius: 999px;
  background: linear-gradient(135deg, #0c6e93, #17a58b);
  color: #f4fbff;
  font-size: 13px;
}
</style>
