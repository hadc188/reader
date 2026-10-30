<template>
  <Transition name="scale">
    <div v-if="modelValue && source" class="source-login-overlay" @click.self="close" @keydown.esc.stop="close">
      <section class="source-login-dialog" :class="{ 'is-web': !!webLoginUrl }" role="dialog" aria-modal="true" :aria-labelledby="`${formId}-title`" :inert="!!browser">
        <header class="login-header">
          <div class="login-heading">
            <h3 :id="`${formId}-title`">{{ source.bookSourceName }} · 登录</h3>
            <p :title="previewUrl || source.bookSourceUrl">{{ previewUrl || source.bookSourceUrl }}</p>
          </div>
          <button class="close-button" type="button" aria-label="关闭登录窗口" @click="close">×</button>
        </header>
        <div class="login-body">
          <p v-if="loading" class="login-status" role="status">正在读取登录配置…</p>
          <p v-if="error" class="login-error" role="alert">{{ error }}</p>
          <div v-if="view?.mode === 'form'" class="login-form" :aria-busy="busy">
            <template v-for="(field, index) in view.fields" :key="`${field.name}-${index}`">
            <div v-if="field.style?.layout_wrapBefore" class="login-wrap" aria-hidden="true"></div>
            <div class="login-field" :style="fieldStyle(field)">
              <p v-if="field.type === 'label'" class="login-note login-label">{{ field.viewName || field.name }}</p>
              <p v-else-if="field.type === 'button' && !field.action?.trim()" class="login-note">{{ field.viewName || field.name }}</p>
              <button v-else-if="field.type === 'button'" type="button" class="login-button" :disabled="busy || savingCookie" @click="runAction(`button:${index}`)">
                {{ field.viewName || field.name }}
              </button>
              <template v-else>
                <label :for="`${formId}-${index}`">{{ field.viewName || field.name }}</label>
                <select v-if="field.type === 'select'" :id="`${formId}-${index}`" v-model="values[field.name]" :disabled="busy" @change="field.action && runAction(`button:${index}`)">
                  <option v-for="option in field.chars?.filter((option) => option != null) || []" :key="String(option)" :value="String(option ?? '')">{{ option }}</option>
                </select>
                <input v-else-if="field.type === 'toggle'" :id="`${formId}-${index}`" type="checkbox" :checked="values[field.name] === String(field.chars?.[0] ?? 'true')" :disabled="busy" @change="toggle(field, index, $event)" />
                <input v-else :id="`${formId}-${index}`" v-model="values[field.name]" :type="field.type" :placeholder="field.placeholder || ''" :disabled="busy" :autocomplete="field.type === 'password' ? 'current-password' : 'off'" />
              </template>
            </div>
            </template>
          </div>
          <div v-if="messages.length" class="login-messages" role="status">
            <p v-for="(message, index) in messages" :key="index">{{ message }}</p>
          </div>
          <!-- 网页登录时把 Cookie 区收成一行折叠条: 登录页本身要占满剩余高度,
               而这块 static 内容会把它挤成一条窄缝(见 .is-web 的说明)。 -->
          <details v-if="webLoginUrl" class="cookie-section cookie-fold">
            <summary>手动导入 Cookie</summary>
            <p>粘贴在网站登录后获取的 Cookie，保存后此书源会使用新的登录状态。</p>
            <textarea :id="`${formId}-cookie`" v-model="cookieText" rows="3" spellcheck="false" autocomplete="off" placeholder="例如：session=…; user=…" :disabled="savingCookie || busy"></textarea>
            <div class="cookie-actions">
              <button class="login-button primary" type="button" :disabled="!cookieText.trim() || savingCookie || busy" @click="saveCookie(false)">{{ savingCookie ? '保存中…' : '导入 Cookie' }}</button>
              <button class="login-button" type="button" :disabled="savingCookie || busy" @click="saveCookie(true)">清除 Cookie</button>
            </div>
          </details>
          <section v-else class="cookie-section" :aria-labelledby="`${formId}-cookie-label`">
            <label :id="`${formId}-cookie-label`" :for="`${formId}-cookie`">手动导入 Cookie</label>
            <p>粘贴在网站登录后获取的 Cookie，保存后此书源会使用新的登录状态。</p>
            <textarea :id="`${formId}-cookie`" v-model="cookieText" rows="3" spellcheck="false" autocomplete="off" placeholder="例如：session=…; user=…" :disabled="savingCookie || busy"></textarea>
            <div class="cookie-actions">
              <button class="login-button primary" type="button" :disabled="!cookieText.trim() || savingCookie || busy" @click="saveCookie(false)">{{ savingCookie ? '保存中…' : '导入 Cookie' }}</button>
              <button class="login-button" type="button" :disabled="savingCookie || busy" @click="saveCookie(true)">清除 Cookie</button>
            </div>
          </section>
          <!-- 网页登录页改用**独立窗口**打开（见 applyView 的说明）：上游常用
               X-Frame-Options / CSP frame-ancestors 或前端 SDK 检测 iframe 嵌入，
               嵌在这里会整页替换成「渲染安全策略拦截」。这里只显示入口状态。 -->
          <p v-if="webLoginUrl" class="login-web-hint">
            登录页已在独立窗口打开。完成登录后关闭该窗口，Cookie 会自动写回此书源。
            <button class="login-button" type="button" :disabled="busy" @click="reopenWebLogin">重新打开</button>
          </p>
        </div>
        <footer class="login-footer">
          <span v-if="busy" role="status">正在执行书源操作…</span>
          <button class="login-button" type="button" @click="close">关闭</button>
          <button v-if="view?.mode === 'form'" class="login-button primary" type="button" :disabled="busy || savingCookie || loading" @click="runAction('login')">确认登录</button>
        </footer>
      </section>
    </div>
  </Transition>
  <SourceBrowserPanel :request="browser" @close="browser = null" @login-data="updateLoginData" @refresh-login="refreshLogin" @toast="notify" />
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, useId, watch } from 'vue'
import { loginBookSource, setBookSourceCookie, type SourceLoginField, type SourceLoginView } from '../../api/source'
import type { BookSource } from '../../types'
import SourceBrowserPanel from '../SourceBrowserPanel.vue'
import { closeSourcePanel, openExternalWindow, type ShowBrowserRequest } from '../../api/sourceClick'
import type { CSSProperties } from 'vue'
import { useAppStore } from '../../stores/app'

const props = defineProps<{ modelValue: boolean; source: BookSource | null }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
const appStore = useAppStore()
const formId = useId()
const view = ref<SourceLoginView | null>(null)
const values = ref<Record<string, string>>({})
const loading = ref(false)
const busy = ref(false)
const savingCookie = ref(false)
const error = ref('')
const messages = ref<string[]>([])
const cookieText = ref('')
const browser = ref<ShowBrowserRequest | null>(null)
let generation = 0
onBeforeUnmount(() => { ++generation })

const previewUrl = computed(() => view.value?.openUrl || view.value?.url || '')
/** 书源脚本的 toast / 操作结果统一走全局弹窗：登录对话框下半部分常被
 *  预览 iframe 占满，写在对话框里的消息用户根本看不到。 */
function notify(message: string, type: 'success' | 'error' | 'warning' = 'success') {
  const text = String(message || '').trim()
  if (!text) return
  messages.value = [text]
  appStore.showToast(text, type)
}

/**
 * `mode: "web"` 的登录页地址。这种页面**不再用 iframe 内嵌**，而是交给独立
 * 窗口打开（`open_external_window`）—— 上游常带 `X-Frame-Options: SAMEORIGIN`
 * 或 CSP `frame-ancestors`，或者用前端安全 SDK 检测「自己是否被 iframe 嵌入」，
 * 命中就把整页替换成「页面渲染安全策略拦截」。独立窗口里 `window.top === window`，
 * 这些检测都不触发，与手机端 legado 直接 `loadUrl` 的行为一致。
 *
 * 这里保留地址只用于「重新打开」按钮与状态提示。
 */
const webLoginUrl = computed(() => {
  if (!view.value?.externalWindow) return ''
  const target = previewUrl.value
  return /^https?:\/\//i.test(target) ? target : ''
})

/** 重新打开独立登录窗口（用户第一次关早了、或想换账号）。 */
function reopenWebLogin() {
  const target = webLoginUrl.value
  if (!target) return
  void openExternalWindow(target, `登录 ${props.source?.bookSourceName || ''}`, props.source?.bookSourceUrl)
    .catch((err) => notify(err instanceof Error ? err.message : '打开登录窗口失败', 'error'))
}

function fieldStyle(field: SourceLoginField): CSSProperties {
  const style = field.style || {}
  const numeric = (key: string, fallback: number) => typeof style[key] === 'number' && Number.isFinite(style[key]) ? Math.max(0, Number(style[key])) : fallback
  const requestedBasis = Number(style.layout_flexBasisPercent)
  const basis = Number.isFinite(requestedBasis) && requestedBasis > 0 ? Math.min(1, requestedBasis) : field.type === 'button' ? .5 : 1
  const alignment: Record<string, CSSProperties['alignSelf']> = { flex_start: 'flex-start', flex_end: 'flex-end', center: 'center', baseline: 'baseline', stretch: 'stretch' }
  return {
    flexBasis: `calc(${basis * 100}% - ${12 * (1 - basis)}px)`,
    flexGrow: numeric('layout_flexGrow', 0), flexShrink: numeric('layout_flexShrink', 1),
    alignSelf: alignment[String(style.layout_alignSelf)] || 'auto',
    '--field-align': style.layout_justifySelf === 'flex_start' ? 'left' : style.layout_justifySelf === 'flex_end' || style.layout_justifySelf === 'right' ? 'right' : 'center',
  }
}

function showBrowser(request: ShowBrowserRequest, current: number) {
  // 「看一眼就走」的链接（注册页 / 用户后台 / 网页版阅读）走**独立窗口**：
  // 这些页面常带 X-Frame-Options / CSP frame-ancestors，或用前端安全 SDK
  // 检测「是否被 iframe 嵌入」并整页替换成拦截提示。独立窗口不触发。
  if (request.externalWindow) {
    if (current !== generation || !props.modelValue) return
    void openExternalWindow(request.url, request.title || '书源网页', props.source?.bookSourceUrl)
      .catch((err) => notify(err instanceof Error ? err.message : '打开窗口失败', 'error'))
    return
  }
  if (current === generation && props.modelValue) browser.value = request
  else if (request.panelId) void closeSourcePanel(request.panelId).catch(() => {})
}

function updateLoginData(data: Record<string, unknown> | null) {
  // `label` 是展示控件，不进 `values`（后端也不会为它建取值键）。
  const editable = (view.value?.fields || []).filter((field) => field.type !== 'button' && field.type !== 'label')
  const updates = data || Object.fromEntries(editable.map((field) => [field.name, field.default ?? field.chars?.[0] ?? '']))
  if (data === null) values.value = {}
  for (const [key, value] of Object.entries(updates)) {
    const field = view.value?.fields.find((field) => field.name === key)
    if (field?.type === 'button') field.viewName = String(value ?? field.name)
    else values.value[key] = String(value ?? field?.default ?? '')
  }
}

async function refreshLogin() {
  if (!props.source || busy.value) return
  const current = generation
  try {
    const result = await loginBookSource(props.source.bookSourceUrl, undefined, { ...values.value }, (request) => showBrowser(request, current))
    if (current === generation) applyView(result)
  } catch (err) {
    if (current === generation) {
      const message = err instanceof Error ? err.message : '刷新登录表单失败'
      error.value = message
      appStore.showToast(message, 'error')
    }
  }
}

function applyView(result: SourceLoginView) {
  view.value = result
  values.value = { ...result.values }
  for (const message of result.messages || []) notify(message)
  if (result.showBrowser) browser.value = result.showBrowser
  // `mode: "web"` 的登录页必须用**独立窗口**打开：上游常带 X-Frame-Options /
  // CSP frame-ancestors，或用前端安全 SDK 检测 iframe 嵌入并整页替换成拦截
  // 提示。窗口关闭时后端会把 Cookie 收回书源 jar。
  if (result.externalWindow && result.url) {
    void openExternalWindow(result.url, `登录 ${props.source?.bookSourceName || ''}`, props.source?.bookSourceUrl)
      .catch((err) => {
        const message = err instanceof Error ? err.message : '打开登录窗口失败'
        error.value = message
        appStore.showToast(message, 'error')
      })
  }
}

watch(() => [props.modelValue, props.source?.bookSourceUrl] as const, async ([visible]) => {
  const current = ++generation
  view.value = null
  values.value = {}
  error.value = ''
  messages.value = []
  cookieText.value = ''
  browser.value = null
  busy.value = false
  savingCookie.value = false
  loading.value = false
  if (!visible || !props.source) return
  loading.value = true
  try {
    const result = await loginBookSource(props.source.bookSourceUrl, undefined, undefined, (request) => showBrowser(request, current))
    if (current === generation) applyView(result)
  } catch (err) {
    if (current === generation) error.value = err instanceof Error ? err.message : '读取登录配置失败'
  } finally {
    if (current === generation) loading.value = false
  }
}, { immediate: true })

async function runAction(action: string) {
  if (!props.source || busy.value || savingCookie.value) return
  const current = generation
  busy.value = true
  error.value = ''
  messages.value = []
  try {
    const result = await loginBookSource(props.source.bookSourceUrl, action, { ...values.value }, (request) => showBrowser(request, current))
    if (current !== generation) return
    applyView(result)
    if (!messages.value.length && !result.showBrowser && !result.cancelled) notify(action === 'login' ? '登录操作已完成' : '操作已完成')
  } catch (err) {
    if (current === generation) {
      const message = err instanceof Error ? err.message : '登录操作失败'
      error.value = message
      appStore.showToast(message, 'error')
    }
  } finally {
    if (current === generation) busy.value = false
  }
}

function toggle(field: SourceLoginField, index: number, event: Event) {
  values.value[field.name] = String((event.target as HTMLInputElement).checked ? field.chars?.[0] ?? 'true' : field.chars?.[1] ?? 'false')
  if (field.action) void runAction(`button:${index}`)
}

async function saveCookie(clear: boolean) {
  if (!props.source || savingCookie.value || busy.value) return
  const current = generation
  savingCookie.value = true
  error.value = ''
  try {
    const result = await setBookSourceCookie(props.source.bookSourceUrl, clear ? '' : cookieText.value.trim())
    if (current !== generation) return
    if (!result.saved) throw new Error('保存 Cookie 失败')
    cookieText.value = ''
    notify(clear ? 'Cookie 已清除' : 'Cookie 已保存')
  } catch (err) {
    if (current === generation) {
      const message = err instanceof Error ? err.message : '保存 Cookie 失败'
      error.value = message
      appStore.showToast(message, 'error')
    }
  } finally {
    if (current === generation) savingCookie.value = false
  }
}

function close() { ++generation; browser.value = null; emit('update:modelValue', false) }
</script>

<style scoped>
.source-login-overlay { position: fixed; inset: 0; z-index: calc(var(--z-modal, 1000) + 2); display: flex; align-items: center; justify-content: center; padding: 20px; background: rgba(0, 0, 0, .35); }
.source-login-dialog { width: min(880px, 100%); max-height: calc(100dvh - 40px); height: min(86dvh, 880px); display: flex; flex-direction: column; overflow: hidden; background: var(--color-bg-elevated); color: var(--color-text); border-radius: var(--radius-xl, 16px); box-shadow: var(--shadow-xl); }
.login-header { flex: none; display: flex; align-items: center; gap: 16px; padding: 18px 22px; border-bottom: 1px solid var(--color-border-light); }
.login-heading { min-width: 0; flex: 1; }
.login-heading h3 { margin: 0; font-size: 17px; }
.login-heading p { margin: 6px 0 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--color-text-tertiary); }
.close-button { flex: none; border: 0; background: transparent; color: inherit; width: 32px; height: 32px; font-size: 25px; cursor: pointer; }
.login-body { flex: 1; min-height: 0; overflow-y: auto; padding: 20px 22px; display: flex; flex-direction: column; gap: 18px; }
.login-form { display: flex; flex-wrap: wrap; gap: 12px; flex: none; }
.login-wrap { flex-basis: 100%; height: 0; }
.login-field { min-width: 0; display: flex; flex-direction: column; gap: 7px; }
.login-field > .login-button { height: 100%; text-align: var(--field-align, center); overflow-wrap: anywhere; }
.login-note { margin: 4px 0; font-size: 13px; text-align: var(--field-align, center); color: var(--color-text-secondary); }
/* `label` 控件是整行说明文字（书源用它写站点限频/使用提示），
   不占输入框的宽度节奏，也不被按钮的 50% flex-basis 挤成半行。 */
.login-label { flex: none; width: 100%; margin: 2px 0; line-height: 1.7; text-align: left; color: var(--color-text-tertiary); font-size: 12.5px; }
.login-field label, .cookie-section > label { font-size: 13px; font-weight: 600; }
.login-field input:not([type=checkbox]), .login-field select, .cookie-section textarea { box-sizing: border-box; width: 100%; border: 1px solid var(--color-border); border-radius: var(--radius-md, 8px); background: var(--color-bg-field, var(--color-bg)); color: var(--color-text); padding: 10px 12px; font: inherit; font-size: 13px; }
.login-field input[type=checkbox] { align-self: flex-start; }
.login-button { min-height: 36px; padding: 8px 14px; border: 1px solid var(--color-border); border-radius: var(--radius-md, 8px); color: var(--color-text); background: var(--color-bg); cursor: pointer; font-size: 13px; }
.login-button:hover:not(:disabled) { background: var(--color-bg-hover); }
.login-button.primary { background: var(--color-primary-soft-bg); color: var(--color-primary-soft-text); border-color: var(--color-primary-soft-border); }
.login-button:disabled { opacity: .5; cursor: default; }
.cookie-section { flex: none; display: flex; flex-direction: column; gap: 9px; padding-top: 16px; border-top: 1px solid var(--color-border-light); }
.cookie-section p, .login-status { margin: 0; font-size: 12px; color: var(--color-text-secondary); line-height: 1.6; }
.cookie-section textarea { resize: vertical; min-height: 78px; max-height: 220px; }
.cookie-actions, .login-footer { display: flex; align-items: center; gap: 10px; }
.login-footer { flex: none; justify-content: flex-end; padding: 14px 22px; border-top: 1px solid var(--color-border-light); }
.login-footer span { margin-right: auto; font-size: 12px; color: var(--color-text-secondary); }
.login-error { color: var(--color-danger, #b23b38); margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; flex: none; }
.login-messages { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; flex: none; }
.login-messages p { margin: 0 0 6px; }
/* 网页登录态: 登录页在**独立窗口**里跑, 弹窗这边只留一条状态提示, 所以不需要
   放大到满窗 —— 保持普通尺寸反而更像「一个设置项」。 */
.login-web-hint {
  margin: 12px 22px 0;
  padding: 10px 12px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--color-text-secondary);
  background: var(--color-bg-secondary, rgba(127, 127, 127, .08));
  border-radius: 8px;
}
.login-web-hint .login-button { margin-left: 8px; }
/* Cookie 区在网页登录时折叠成一行, 展开后才占高度。 */
.cookie-fold { margin: 12px 22px 0; padding-top: 12px; }
.cookie-fold > summary { cursor: pointer; font-size: 13px; color: var(--color-text-secondary); list-style: none; }
.cookie-fold > summary::-webkit-details-marker { display: none; }
.cookie-fold > summary::before { content: '▸ '; }
.cookie-fold[open] > summary::before { content: '▾ '; }
.cookie-fold > summary:hover { color: var(--color-text); }
.cookie-fold[open] { flex: none; padding-bottom: 12px; }
.cookie-fold:not([open]) { flex: none; }

@media (max-width: 560px) { .source-login-overlay { padding: 8px; } .source-login-dialog { height: calc(100dvh - 16px); max-height: calc(100dvh - 16px); } .login-header, .login-body, .login-footer { padding-left: 14px; padding-right: 14px; } .cookie-fold { margin-left: 14px; margin-right: 14px; } }
</style>
