import { createApp } from 'vue'
import { createPinia } from 'pinia'
import router from './router'
import App from './App.vue'
import './styles/global.css'
import { useAppStore } from './stores/app'
import { registerViewportSync } from './utils/viewport'

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(router)
registerViewportSync()
const appStore = useAppStore(pinia)
appStore.setOnlineStatus(navigator.onLine)
window.addEventListener('online', () => {
  appStore.setOnlineStatus(true)
  appStore.showToast('网络已恢复', 'success')
})
window.addEventListener('offline', () => {
  appStore.setOnlineStatus(false)
  appStore.showToast('已进入离线模式', 'warning')
})

app.mount('#app')
