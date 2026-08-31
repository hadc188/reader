import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useSourceStore } from './source'
import { getBookSources } from '../api/source'
import type { BookSource } from '../types'

vi.mock('../api/source', () => ({
  getBookSources: vi.fn(),
}))

const getBookSourcesMock = vi.mocked(getBookSources)

describe('source store availability version', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getBookSourcesMock.mockReset()
  })

  it('changes when a source is disabled or enabled', async () => {
    const initialSource = source('https://example.test')
    getBookSourcesMock.mockResolvedValueOnce([initialSource])
    const store = useSourceStore()
    await store.fetchSources(true)
    expect(store.availabilityVersion).toBe(0)

    getBookSourcesMock.mockResolvedValueOnce([{ ...initialSource, enabled: false }])
    await store.fetchSources(true)
    expect(store.availabilityVersion).toBe(1)

    getBookSourcesMock.mockResolvedValueOnce([{ ...initialSource, enabled: false }])
    await store.fetchSources(true)
    expect(store.availabilityVersion).toBe(1)
  })

  it('changes when a source is added or removed', async () => {
    getBookSourcesMock.mockResolvedValueOnce([source('https://a.example')])
    const store = useSourceStore()
    await store.fetchSources(true)

    getBookSourcesMock.mockResolvedValueOnce([
      source('https://a.example'),
      source('https://b.example'),
    ])
    await store.fetchSources(true)
    expect(store.availabilityVersion).toBe(1)

    getBookSourcesMock.mockResolvedValueOnce([])
    await store.fetchSources(true)
    expect(store.availabilityVersion).toBe(2)
  })
})

function source(url: string): BookSource {
  return {
    bookSourceName: url,
    bookSourceUrl: url,
  }
}
