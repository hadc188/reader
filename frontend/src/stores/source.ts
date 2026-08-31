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
