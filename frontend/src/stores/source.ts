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
