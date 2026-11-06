<template>
  <div class="recent-view">
    <div class="recent-content">
      <div class="recent-header">
        <h1 class="recent-title">
          最近阅读
          <span class="recent-count">({{ filteredRecentBooks.length }})</span>
        </h1>
        <div class="recent-actions">
          <div class="recent-filters">
            <button class="filter-chip" :class="{ active: activeFilter === 'all' }" @click="activeFilter = 'all'">全部</button>
            <button class="filter-chip" :class="{ active: activeFilter === 'book' }" @click="activeFilter = 'book'">书籍</button>
            <button class="filter-chip" :class="{ active: activeFilter === 'rss' }" @click="activeFilter = 'rss'">RSS</button>
          </div>
          <button
            class="recent-clear-btn"
            :disabled="!shelfStore.recentBooks.length"
            @click="handleClearRecent"
          >
            一键清空
          </button>
        </div>
      </div>

      <div class="recent-search-row">
        <input
          v-model.trim="searchText"
          class="recent-search-input"
          placeholder="搜索最近阅读"
        />
      </div>

      <BookGrid
        :books="filteredRecentBooks"
        :loading="shelfStore.loading"
        empty-text="暂无最近阅读"
        :show-delete-action="true"
        @click="handleBookClick"
        @info="handleBookInfo"
        @delete="handleRecentDelete"
        @contextmenu="handleBookContextMenu"
      />
    </div>

    <BookDetailModal
      v-model="showDetail"
      :book="selectedBook"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import BookDetailModal from '../components/BookDetailModal.vue'
import BookGrid from '../components/BookGrid.vue'
import { showContextMenu } from '../composables/useContextMenu'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { filterShelfBooks } from '../utils/bookshelfFilter'
import type { Book, SearchBook } from '../types'

const router = useRouter()
const shelfStore = useBookshelfStore()
const readerStore = useReaderStore()

const showDetail = ref(false)
const selectedBook = ref<Book | SearchBook | null>(null)
const openingBookUrl = ref('')
const activeFilter = ref<'all' | 'book' | 'rss'>('all')
const searchText = ref('')

/** 关键词过滤复用书架那一套(`filterShelfBooks`): 两处搜索同一批书, 匹配字段
 *  与大小写规则必须一致, 否则同一关键词在书架能搜到、在最近阅读却搜不到。 */
const filteredRecentBooks = computed(() => {
  let list = shelfStore.recentBooks
  if (activeFilter.value === 'book') {
    list = list.filter((book) => book.recentKind !== 'rss')
  } else if (activeFilter.value === 'rss') {
    list = list.filter((book) => book.recentKind === 'rss')
  }

  return filterShelfBooks(list, searchText.value)
})

onMounted(async () => {
  await shelfStore.fetchBooks().catch(() => undefined)
  await shelfStore.refreshRecentBooks().catch(() => undefined)
})

async function handleBookClick(book: Book | SearchBook) {
  const currentBook = book as Book
  if (currentBook.recentKind === 'rss' && currentBook.rssSourceUrl && (currentBook.rssLink || currentBook.bookUrl)) {
    await router.push({
      name: 'rss-article',
      query: {
        source: currentBook.rssSourceUrl,
        link: currentBook.rssLink || currentBook.bookUrl,
        title: currentBook.name || '',
        pubDate: currentBook.rssPubDate || '',
        origin: currentBook.author || '',
      },
    })
    return
  }
  if (!currentBook.origin || !currentBook.bookUrl) return
  if (openingBookUrl.value === currentBook.bookUrl) return

  openingBookUrl.value = currentBook.bookUrl
  try {
    // 置顶只是书架排序，与"打开这本书"无关；await 它等于把一次书架整表写盘
    // 插在打开链路前面 —— 手机端同样是先开书、排序在后台做。这里改成并行发起，
    // 失败也不影响阅读（原来是 catch 掉，行为不变）。
    void shelfStore.moveBookToFront(currentBook.bookUrl).catch(() => undefined)
    const loadBookTask = readerStore.loadBook(currentBook)
    await router.push('/reader')
    await loadBookTask
  } finally {
    openingBookUrl.value = ''
  }
}

function handleBookInfo(book: Book | SearchBook) {
  const currentBook = book as Book
  if (currentBook.recentKind === 'rss') {
    handleBookClick(book)
    return
  }
  selectedBook.value = book
  showDetail.value = true
}

async function handleRecentDelete(book: Book | SearchBook) {
  await shelfStore.removeRecentBook(book as Book).catch(() => undefined)
}

function handleBookContextMenu({ book, event }: { book: Book | SearchBook; event: MouseEvent }) {
  const currentBook = book as Book
  const isRss = currentBook.recentKind === 'rss'
  const menuItems = [
    { label: isRss ? '打开文章' : '开始阅读', action: () => handleBookClick(book) },
    { label: '查看详情', action: () => handleBookInfo(book) },
    { divider: true },
    { label: '从最近移除', danger: true, action: () => handleRecentDelete(book) },
  ]
  showContextMenu(event, menuItems, book)
}

async function handleClearRecent() {
  await shelfStore.clearAllRecentBooks().catch(() => undefined)
}
</script>

<style scoped>
.recent-view {
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.recent-content {
  height: 100%;
  max-width: var(--content-max-width);
  margin: 0 auto;
  padding: 0 var(--space-6);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.recent-header {
  padding: var(--space-6) 0 var(--space-3);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  flex-shrink: 0;
}

.recent-content :deep(.book-grid) {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.recent-search-row {
  padding-bottom: var(--space-3);
  flex-shrink: 0;
}

.recent-search-input {
  width: 100%;
  border: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
  border-radius: var(--radius-lg);
  padding: 12px 14px;
  font-size: var(--text-sm);
  color: var(--color-text);
}

.recent-search-input::placeholder {
  color: var(--color-text-tertiary);
}

.recent-title {
  font-size: var(--text-2xl);
  font-weight: 700;
  letter-spacing: 0;
}

.recent-count {
  font-size: var(--text-base);
  font-weight: 400;
  color: var(--color-text-tertiary);
}

.recent-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.recent-filters {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.filter-chip {
  padding: 8px 14px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  transition: all var(--duration-fast) var(--ease-out);
}

.filter-chip.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.1);
  color: var(--color-primary);
}

.recent-clear-btn {
  padding: 10px 16px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-light);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  transition: all var(--duration-fast) var(--ease-out);
}

.recent-clear-btn:hover:not(:disabled) {
  border-color: rgba(225, 76, 76, 0.22);
  color: var(--color-danger);
}

.recent-clear-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

@media (max-width: 640px) {
  .recent-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .recent-actions {
    width: 100%;
    justify-content: space-between;
  }
}
</style>
