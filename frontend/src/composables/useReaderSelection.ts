import { ref } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import type { useReaderStore } from '../stores/reader'
import type { useAppStore } from '../stores/app'
import { saveReplaceRule } from '../api/replaceRule'

type ReaderStore = ReturnType<typeof useReaderStore>
type AppStore = ReturnType<typeof useAppStore>

export function useReaderSelection(
  store: ReaderStore,
  appStore: AppStore,
  config: ComputedRef<{ selectAction: 'popup' | 'ignore' }>,
  scrollContainerRef: Ref<HTMLElement | undefined>,
) {
  const isTouchDevice = typeof window !== 'undefined'
    && ('ontouchstart' in window || navigator.maxTouchPoints > 0)
  const isAndroid = typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent)
  const selectionMenu = ref({
    visible: false,
    text: '',
    top: 0,
    left: 0,
  })
  const activeSelectionText = ref('')
  const suppressSelectionCloseUntil = ref(0)
  let selectionMenuUpdateTimer: number | null = null

  function hideSelectionMenu() {
    selectionMenu.value.visible = false
  }

  function scheduleSelectionMenuUpdate(delay = 220) {
    if (selectionMenuUpdateTimer) {
      clearTimeout(selectionMenuUpdateTimer)
    }
    selectionMenuUpdateTimer = window.setTimeout(() => {
      updateSelectionMenu()
    }, delay)
  }

  function handleMouseUpSelection() {
    scheduleSelectionMenuUpdate(120)
  }

  function handleTouchEndSelection() {
    scheduleSelectionMenuUpdate(260)
  }

  function handleSelectionChange() {
    scheduleSelectionMenuUpdate(isAndroid ? 320 : 220)
  }

  function updateSelectionMenu() {
    if (config.value.selectAction !== 'popup') {
      hideSelectionMenu()
      return
    }

    const selection = window.getSelection?.()
    const text = selection?.toString().trim() || ''
    if (!selection || selection.rangeCount === 0 || !text || selection.isCollapsed) {
      hideSelectionMenu()
      return
    }
    if (isTouchDevice && text.length < (isAndroid ? 2 : 4)) {
      hideSelectionMenu()
      return
    }

    const container = scrollContainerRef.value
    const range = selection.getRangeAt(0)
    const commonAncestor = range.commonAncestorContainer
    const targetNode = commonAncestor.nodeType === Node.TEXT_NODE ? commonAncestor.parentElement : commonAncestor as HTMLElement | null
    if (!container || !targetNode || !container.contains(targetNode)) {
      hideSelectionMenu()
      return
    }

    const rect = range.getBoundingClientRect()
    if (!rect.width && !rect.height) {
      hideSelectionMenu()
      return
    }
    suppressSelectionCloseUntil.value = Date.now() + 250
    activeSelectionText.value = text
    selectionMenu.value = {
      visible: true,
      text: text.length > 48 ? `${text.slice(0, 48)}...` : text,
      top: isTouchDevice
        ? Math.min(window.innerHeight - 76, Math.max(16 + Math.max(0, parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--safe-area-top')) || 0), rect.bottom + 12))
        : Math.max(16 + Math.max(0, parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--safe-area-top')) || 0), rect.top - 56),
      left: Math.min(window.innerWidth - 240, Math.max(16, rect.left + rect.width / 2 - 110)),
    }
  }

  async function addSelectionBookmark() {
    const selection = window.getSelection?.()
    const text = activeSelectionText.value || selection?.toString().trim() || ''
    if (!text) return
    try {
      const pos = scrollContainerRef.value?.scrollTop || 0
      await store.addBookmark(pos, text)
      selection?.removeAllRanges()
      activeSelectionText.value = ''
      hideSelectionMenu()
      appStore.showToast('已加入书签', 'success')
    } catch {
      appStore.showToast('加入书签失败', 'error')
    }
  }

  async function addSelectionReplaceRule(mode: 'book' | 'source') {
    const selection = window.getSelection?.()
    const text = activeSelectionText.value || selection?.toString().trim() || ''
    if (!text || !store.book) return

    try {
      const scope = mode === 'source'
        ? `source:${store.book.origin}`
        : `book:${store.book.bookUrl}`
      await saveReplaceRule({
        id: 0,
        name: `${mode === 'source' ? '书源替换' : '本书替换'} ${store.replaceRules.length + 1}`,
        pattern: text,
        replacement: '',
        scope,
        isEnabled: true,
        isRegex: false,
        order: store.replaceRules.length + 1,
      })
      await store.fetchReplaceRules()
      selection?.removeAllRanges()
      activeSelectionText.value = ''
      hideSelectionMenu()
      appStore.showToast(mode === 'source' ? '已加入书源替换规则' : '已加入本书替换规则', 'success')
    } catch {
      appStore.showToast('加入替换规则失败', 'error')
    }
  }

  function clearSelectionState() {
    activeSelectionText.value = ''
    hideSelectionMenu()
    window.getSelection?.()?.removeAllRanges()
  }

  function disposeSelection() {
    if (selectionMenuUpdateTimer) {
      clearTimeout(selectionMenuUpdateTimer)
      selectionMenuUpdateTimer = null
    }
  }

  return {
    selectionMenu,
    suppressSelectionCloseUntil,
    hideSelectionMenu,
    scheduleSelectionMenuUpdate,
    handleMouseUpSelection,
    handleTouchEndSelection,
    handleSelectionChange,
    updateSelectionMenu,
    addSelectionBookmark,
    addSelectionReplaceRule,
    clearSelectionState,
    disposeSelection,
  }
}
