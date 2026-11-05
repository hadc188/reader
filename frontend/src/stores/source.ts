import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getBookSources } from '../api/source'
import type { BookSource } from '../types'

export const useSourceStore = defineStore('source', () => {
  const sources = ref<BookSource[]>([])
  const loading = ref(false)
  const availabilityVersion = ref(0)
  let loadingTask: Promise<void> | null = null
  let requestId = 0
  let sourceStateSignature = ''

  /** 上次见到的书源清单签名（跨会话持久化）。
   *
   *  `availabilityVersion` 的口径是「这次 fetch 相比上次**本进程**看到的清单变了
   *  没有」，所以 `sourceStateSignature` 一直是内存变量。但候选缓存
   *  （`sourceCandidateCache`）是写进 localStorage 的，**跨会话存活** ——
   *  于是应用重启后基线为空，首次 fetch 只记录基线不判定变化，用户在上次会话
   *  「启用了书源但没能成功刷新详情页」这件事就永远补不上。
   *
   *  这里把基线也存一份到 localStorage：重启后第一次 fetch 就能发现真实变化，
   *  详情页的 availabilityVersion 监听与签名比对都能正常工作。
   *  用独立键，避免和候选缓存的 schema 混在一起。 */
  const SOURCE_STATE_SIGNATURE_KEY = 'source-state-signature'

  function loadPersistedSignature(): string {
    try {
      return localStorage.getItem(SOURCE_STATE_SIGNATURE_KEY) || ''
    } catch {
      return ''
    }
  }

  function persistSignature(signature: string) {
    try {
      localStorage.setItem(SOURCE_STATE_SIGNATURE_KEY, signature)
    } catch {
      // 存储不可用（隐私模式/配额满）时退化成纯内存基线，功能不受影响。
    }
  }

  sourceStateSignature = loadPersistedSignature()

  function sourceIdentity(url: string) {
    const normalized = url.trim()
    if (normalized.startsWith('http://') || normalized.startsWith('https://')) {
      return normalized.replace(/\/+$/, '')
    }
    return normalized
  }

  function getSourceStateSignature(list: BookSource[]) {
    return list
      .map((source) => `${sourceIdentity(source.bookSourceUrl)}:${source.enabled === false ? '0' : '1'}`)
      .sort()
      .join('\n')
  }

  async function fetchSources(force = false) {
    if (loadingTask && !force) return loadingTask
    loading.value = true
    const currentRequestId = ++requestId
    const task = getBookSources()
      .then((list) => {
        if (currentRequestId !== requestId) return
        const nextSignature = getSourceStateSignature(list)
        if (sourceStateSignature && nextSignature !== sourceStateSignature) {
          availabilityVersion.value += 1
          // **不要在这里 clear() 候选缓存**。
          //
          // 详情页与换源面板判断「打开时要不要自动重搜」的方式是：
          //   cached = cache.get(bookKey)
          //   catalogChanged = !!cached && !cache.matches(cached, 当前签名)
          // 条目一旦被整份清掉，`cached` 变成 undefined，`catalogChanged` 恒为
          // false —— 于是「启动/停止书源后再打开详情页」看到的仍是打开时那份
          // 旧结果，而且不会自动刷新（这个 bug 就是上一轮加清理时这样引进来的）。
          //
          // 容量问题不靠清空解决：每条记录的 `catalogSignature` 会在下次读到时
          // 自然对不上，详情页据此重搜并覆盖；LRU 上限（20 条）负责淘汰。
        }
        sourceStateSignature = nextSignature
        persistSignature(nextSignature)
        sources.value = list
      })
      .finally(() => {
        if (currentRequestId !== requestId) return
        loading.value = false
        loadingTask = null
      })
    loadingTask = task
    return task
  }

  return {
    sources,
    loading,
    availabilityVersion,
    fetchSources
  }
})
