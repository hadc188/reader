const DB_NAME = 'reader-browser-cache'
const DB_VERSION = 1
const STORE_NAME = 'chapters'
let dbPromise: Promise<IDBDatabase> | null = null

export interface BrowserChapterCacheRecord {
  key: string
  bookUrl: string
  chapterUrl: string
  chapterTitle: string
  content: string
  size: number
  updatedAt: number
}

export interface BrowserBookCacheSummary {
  bookUrl: string
  cachedChapterCount: number
  bytes: number
  updatedAt: number
}

function cacheKey(bookUrl: string, chapterUrl: string) {
  return `${bookUrl}::${chapterUrl}`
}

function openDb(): Promise<IDBDatabase> {
  if (!dbPromise) {
    dbPromise = new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION)

      request.onerror = () => reject(request.error)
      request.onsuccess = () => {
        const db = request.result
        db.onversionchange = () => {
          db.close()
          dbPromise = null
        }
        resolve(db)
      }
      request.onupgradeneeded = () => {
        const db = request.result
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          const store = db.createObjectStore(STORE_NAME, { keyPath: 'key' })
          store.createIndex('bookUrl', 'bookUrl', { unique: false })
          store.createIndex('updatedAt', 'updatedAt', { unique: false })
        }
      }
    }).catch((error: unknown) => {
      dbPromise = null
      throw error
    })
  }
  return dbPromise!
}

async function withStore<T>(mode: IDBTransactionMode, handler: (store: IDBObjectStore) => Promise<T>): Promise<T> {
  const db = await openDb()
  return new Promise<T>((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, mode)
    const store = tx.objectStore(STORE_NAME)

    handler(store)
      .then((result) => {
        tx.oncomplete = () => {
          resolve(result)
        }
        tx.onerror = () => {
          reject(tx.error)
        }
      })
      .catch((error) => {
        reject(error)
      })
  })
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

export async function getBrowserCachedChapter(bookUrl: string, chapterUrl: string) {
  return withStore('readonly', async (store) => {
    const result = await requestToPromise(store.get(cacheKey(bookUrl, chapterUrl)))
    const record = result as BrowserChapterCacheRecord | undefined
    return record?.content || null
  })
}

export async function setBrowserCachedChapter(params: {
  bookUrl: string
  chapterUrl: string
  chapterTitle?: string
  content: string
}) {
  return withStore('readwrite', async (store) => {
    const record: BrowserChapterCacheRecord = {
      key: cacheKey(params.bookUrl, params.chapterUrl),
      bookUrl: params.bookUrl,
      chapterUrl: params.chapterUrl,
      chapterTitle: params.chapterTitle || '',
      content: params.content,
      size: new Blob([params.content]).size,
      updatedAt: Date.now(),
    }
    await requestToPromise(store.put(record))
  })
}

export async function deleteBrowserBookCache(bookUrl: string) {
  return withStore('readwrite', async (store) => {
    const index = store.index('bookUrl')
    const records = await requestToPromise(index.getAll(IDBKeyRange.only(bookUrl)))
    await Promise.all((records as BrowserChapterCacheRecord[]).map((record) => requestToPromise(store.delete(record.key))))
  })
}

export async function countBrowserBookCache(bookUrl: string) {
  const summaries = await listBrowserCacheSummary()
  return summaries.find((item) => item.bookUrl === bookUrl)?.cachedChapterCount || 0
}

export async function listBrowserCachedChapterUrls(bookUrl: string) {
  return withStore('readonly', async (store) => {
    const index = store.index('bookUrl')
    const records = await requestToPromise(index.getAll(IDBKeyRange.only(bookUrl)))
    return new Set((records as BrowserChapterCacheRecord[]).map((record) => record.chapterUrl).filter(Boolean))
  })
}

export async function listBrowserCacheSummary(): Promise<BrowserBookCacheSummary[]> {
  return withStore('readonly', async (store) => {
    const records = await requestToPromise(store.getAll())
    const summaryMap = new Map<string, BrowserBookCacheSummary>()

    ;(records as BrowserChapterCacheRecord[]).forEach((record) => {
      const current = summaryMap.get(record.bookUrl) || {
        bookUrl: record.bookUrl,
        cachedChapterCount: 0,
        bytes: 0,
        updatedAt: 0,
      }
      current.cachedChapterCount += 1
      current.bytes += record.size || 0
      current.updatedAt = Math.max(current.updatedAt, record.updatedAt || 0)
      summaryMap.set(record.bookUrl, current)
    })

    return Array.from(summaryMap.values()).sort((a, b) => b.updatedAt - a.updatedAt)
  })
}

export async function clearAllBrowserCache() {
  return withStore('readwrite', async (store) => {
    await requestToPromise(store.clear())
  })
}
