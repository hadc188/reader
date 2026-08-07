import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getBookSources } from '../api/source'
import type { BookSource } from '../types'

export const useSourceStore = defineStore('source', () => {
  const sources = ref<BookSource[]>([])
  const loading = ref(false)
  let loadingTask: Promise<void> | null = null

  async function fetchSources() {
    if (loadingTask) return loadingTask
    loading.value = true
    loadingTask = getBookSources()
      .then((list) => {
        sources.value = list
      })
      .finally(() => {
        loading.value = false
        loadingTask = null
      })
    return loadingTask
  }

  return {
    sources,
    loading,
    fetchSources
  }
})
