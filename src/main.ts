import { createApp } from 'vue'
import CompanionApp from './CompanionApp.vue'

if (import.meta.env.VITE_PENGUIN_LEGACY === '1') {
  void import('./App.vue').then(({ default: App }) => createApp(App).mount('#app'))
} else {
  createApp(CompanionApp).mount('#app')
}
