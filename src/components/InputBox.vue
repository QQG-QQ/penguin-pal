<script setup lang="ts">
defineProps<{
  modelValue: string
  busy: boolean
  listening: boolean
  voiceSupported: boolean
  voiceReplyEnabled: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  send: []
  voiceStart: []
  voiceStop: []
  toggleVoiceReply: [value: boolean]
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
      {{ listening ? '松开发送' : '按住说话' }}
    </button>

    <div class="composer">
      <textarea
        :value="modelValue"
        rows="2"
        placeholder="和企鹅助手说点什么，或者描述想让她代办的白名单动作。"
        :disabled="busy"
        @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
        @keydown="handleKeydown"
      />

      <div class="composer-actions">
        <label class="voice-toggle">
          <input
            type="checkbox"
            :checked="voiceReplyEnabled"
            @change="emit('toggleVoiceReply', ($event.target as HTMLInputElement).checked)"
          />
          语音回复
        </label>
        <button class="send-button" type="button" :disabled="busy" @click="emit('send')">
          {{ busy ? '处理中...' : '发送' }}
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.input-shell {
  display: grid;
  grid-template-columns: 114px 1fr;
  gap: 12px;
  width: min(100%, 360px);
}

.voice-button,
.send-button {
  border: none;
  cursor: pointer;
}

.voice-button {
  min-height: 132px;
  padding: 14px 16px;
  border-radius: 26px;
  background:
    linear-gradient(180deg, rgba(17, 48, 62, 0.98), rgba(8, 23, 33, 0.98));
  color: #effbff;
  font-size: 14px;
  line-height: 1.6;
  box-shadow:
    0 18px 36px rgba(3, 15, 28, 0.26),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
}

.voice-button.active {
  background: linear-gradient(180deg, #0f7366, #0f4c5f);
}

.voice-button:disabled,
.send-button:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.composer {
  padding: 12px;
  border-radius: 28px;
  background: rgba(255, 255, 255, 0.94);
  box-shadow:
    0 18px 36px rgba(6, 18, 32, 0.16),
    inset 0 1px 0 rgba(255, 255, 255, 0.72);
}

textarea {
  width: 100%;
  min-height: 76px;
  border: none;
  resize: none;
  outline: none;
  background: transparent;
  color: #183949;
  font-size: 14px;
  line-height: 1.6;
}

.composer-actions {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
  margin-top: 10px;
}

.voice-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: #385364;
  font-size: 13px;
}

.send-button {
  min-width: 94px;
  min-height: 42px;
  padding: 0 16px;
  border-radius: 999px;
  background: linear-gradient(135deg, #0c6e93, #17a58b);
  color: #f4fbff;
  font-size: 14px;
}
</style>
