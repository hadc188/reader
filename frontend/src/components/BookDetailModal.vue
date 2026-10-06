<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="modelValue" class="modal-overlay" @click="close"></div>
    </Transition>
    <Transition name="scale">
      <div v-if="modelValue && book" :key="detailKey" class="modal-container" @click.self="close">
        <div class="detail-modal">
          <button class="modal-close" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>

          <!-- ── 编辑态: 只保留可编辑的四项, 不显示书源/目录等只读信息 ── -->
          <div v-if="isEditing" class="edit-form">
            <h2 class="edit-form-title">编辑书籍信息</h2>

            <div class="edit-cover-block">
              <div class="edit-cover" :class="{ empty: !coverSrc }">
                <img v-if="coverSrc" :src="coverSrc" :alt="draft.name" @error="coverFailed = true">
                <span v-else class="edit-cover-empty">无封面</span>
              </div>
              <div class="edit-cover-side">
                <div class="edit-cover-buttons">
                  <button
                    class="cover-edit-btn"
                    type="button"
                    :disabled="uploadingCover"
                    @click="coverInputRef?.click()"
                  >{{ uploadingCover ? '上传中…' : (draft.customCoverUrl ? '更换图片' : '选择图片') }}</button>
                  <button
                    v-if="draft.customCoverUrl"
                    class="cover-edit-btn subtle"
                    type="button"
                    @click="resetCover"
                  >恢复原始封面</button>
                </div>
                <p class="edit-cover-note">支持 PNG / JPG / GIF / WebP / AVIF，不超过 8 MB</p>
              </div>
              <input
                ref="coverInputRef"
                class="hidden-file-input"
                type="file"
                accept="image/png,image/jpeg,image/gif,image/webp,image/avif"
                @change="handleCoverFileChange"
              >
            </div>

            <label class="edit-field">
              <span class="edit-field-label">书名</span>
              <input
                v-model="draft.name"
                class="edit-field-input"
                type="text"
                maxlength="120"
                placeholder="书名"
              >
            </label>

            <label class="edit-field">
              <span class="edit-field-label">作者</span>
              <input
                v-model="draft.author"
                class="edit-field-input"
                type="text"
                maxlength="120"
                placeholder="可留空"
              >
            </label>

            <label class="edit-field">
              <span class="edit-field-label">简介</span>
              <textarea
                v-model="draft.intro"
                class="edit-field-textarea"
                rows="5"
                maxlength="5000"
                placeholder="可留空"
              ></textarea>
            </label>
          </div>

          <!-- ── 详情态 ── -->
          <template v-else>
          <!-- Book Header -->
          <div class="book-header">
            <div class="book-cover-lg">
              <img
                v-if="coverSrc"
                :src="coverSrc"
                :alt="displayBook.name"
                @error="coverFailed = true"
              />
              <div v-else class="cover-placeholder-lg">
                <span>{{ displayBook.name }}</span>
              </div>
            </div>
            <div class="book-header-info">
              <h2>{{ displayBook.name }}</h2>
              <p class="author">{{ displayBook.author || '未知作者' }}</p>
              <div class="book-tags">
                <span v-if="book.kind" class="tag">{{ book.kind }}</span>
                <span v-if="(book as Book).totalChapterNum" class="tag">共{{ (book as Book).totalChapterNum }}章</span>
                <span v-if="displayOriginName" class="tag origin">{{ displayOriginName }}</span>
              </div>
              <p v-if="(book as Book).durChapterTitle" class="progress">
                已读至：{{ (book as Book).durChapterTitle }}
              </p>
            </div>
          </div>

          <!-- Intro -->
          <div v-if="displayBook.intro" class="book-intro">
            <h3>简介</h3>
            <p>{{ displayBook.intro }}</p>
          </div>

          <!-- Available Sources -->
          <div v-if="!isLocal" class="source-section">
            <h3>可读书源</h3>
            <div v-if="sourcesLoading && !sourceCandidates.length" class="source-loading">
              <div class="loading-spinner"></div>
              正在查找其他书源...
            </div>
            <div v-else-if="!sourceCandidates.length" class="source-empty">
              未找到其他书源
            </div>
            <div v-else class="source-list">
              <div
                v-for="cand in sourceCandidates"
                :key="`${cand.origin}::${cand.bookUrl}`"
                class="source-item"
                :class="{ selected: selectedSource?.origin === cand.origin && selectedSource?.bookUrl === cand.bookUrl }"
                @click="selectSource(cand)"
              >
                <span class="source-radio" :class="{ checked: selectedSource?.origin === cand.origin && selectedSource?.bookUrl === cand.bookUrl }" />
                <span class="source-name">{{ sourceNameByOrigin(cand.origin) }}</span>
                <span v-if="cand.lastChapter" class="source-latest">{{ cand.lastChapter }}</span>
              </div>
            </div>
          </div>

          <!-- Chapters -->
          <div class="chapter-section" v-if="chapters.length > 0">
            <h3>目录 ({{ chapters.length }})</h3>
            <div class="chapter-list">
              <div
                v-for="(chapter, i) in displayChapters"
                :key="chapter.url"
                class="chapter-item"
                :class="{ current: i === (book as Book).durChapterIndex }"
                @click="readChapter(i)"
              >
                <span class="chapter-index">{{ i + 1 }}</span>
                <span class="chapter-title">{{ chapter.title }}</span>
              </div>
            </div>
            <button
              v-if="chapters.length > 50 && !showAllChapters"
              class="show-more-btn"
              @click="showAllChapters = true"
            >
              显示全部 {{ chapters.length }} 章
            </button>
          </div>
          <div v-else-if="chaptersLoading" class="chapter-loading">
            <div class="loading-spinner"></div>
            加载目录中...
          </div>
          </template>

          <!-- Actions -->
          <div class="modal-actions">
            <!-- 编辑态: 只留还原/保存/取消, 避免误触阅读或退架 -->
            <template v-if="isEditing">
              <button
                class="action-btn"
                type="button"
                :disabled="!canRevert || savingEdit"
                :title="canRevert
                  ? '放弃已保存的自定义内容，恢复为最初的书籍信息（点保存后生效）'
                  : '这本书还没有自定义过，没有可还原的内容'"
                @click="revertEdit"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                  <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
                  <path d="M3 3v5h5" />
                </svg>
                还原
              </button>
              <button class="action-btn primary" :disabled="savingEdit || uploadingCover" @click="saveEdit">
                {{ uploadingCover ? '封面上传中…' : (savingEdit ? '正在保存…' : '保存') }}
              </button>
              <button class="action-btn" :disabled="savingEdit" @click="cancelEdit">取消</button>
            </template>
            <template v-else>
              <button class="action-btn primary" @click="startReading">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                  <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
                  <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
                </svg>
                {{ (book as Book).durChapterIndex ? '继续阅读' : '开始阅读' }}
              </button>
              <button v-if="!isShelfBook()" class="action-btn" @click="addToShelf">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                  <path d="M12 5v14M5 12h14" />
                </svg>
                加入书架
              </button>
              <template v-else>
                <!-- 只有书架里的书才谈得上编辑信息 -->
                <button class="action-btn" @click="startEdit">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                    <path d="M12 20h9" />
                    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
                  </svg>
                  编辑信息
                </button>
                <button class="action-btn" :disabled="removingFromShelf" @click="removeFromShelf">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                    <path d="M5 12h14" />
                  </svg>
                  {{ removingFromShelf ? '正在取消...' : '取消加入' }}
                </button>
              </template>
              <button class="action-btn" @click="close">关闭</button>
            </template>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, computed, nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { getCoverUrl, getChapterList, saveBook, setBookSource } from '../api/bookshelf'
import { getAvailableBookSourceSSE } from '../api/search'
import type { SseLike } from '../api/sse'
import { useSourceStore } from '../stores/source'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import type { Book, SearchBook, BookChapter } from '../types'
import { isLocalBook } from '../utils/localBook'
import { matchesSourceSwitchAuthor, searchMergeKey } from '../utils/searchRank'
import { deleteBookCover, uploadBookCover } from '../api/bookCover'
import {
  canRevertToOrigin,
  createBookEditDraft,
  editableFieldsForSave,
  normalizeBookEditDraft,
  obsoleteCoverUrl,
  originPatchForSave,
  resolveRevertDraft,
  validateBookEditDraft,
  validateCoverFile,
  type BookEditDraft,
} from '../utils/bookEdit'

const SOURCE_CANDIDATES_CACHE_LIMIT = 20
const sourceCandidatesCache = new Map<string, SearchBook[]>()

const props = defineProps<{
  modelValue: boolean
  book: Book | SearchBook | null
  /** 打开弹窗后直接进入编辑态(书架右键「编辑信息」用)。 */
  startInEdit?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const router = useRouter()
const readerStore = useReaderStore()
const shelfStore = useBookshelfStore()
const sourceStore = useSourceStore()
const appStore = useAppStore()

const coverFailed = ref(false)
const chapters = ref<BookChapter[]>([])
const chaptersLoading = ref(false)
const showAllChapters = ref(false)
const sourceCandidates = ref<SearchBook[]>([])
const sourcesLoading = ref(false)
const selectedSource = ref<SearchBook | null>(null)
const sourceCatalog = ref(new Map<string, string>())
const removingFromShelf = ref(false)
let detailLoadId = 0
let chapterLoadId = 0
let sourceSSE: SseLike | null = null

const isLocal = computed(() => isLocalBook(props.book))

const detailKey = computed(() => {
  const book = props.book
  if (!book) return ''
  return `${book.name}::${book.author}::${book.bookUrl}::${book.origin}`
})

/** 保存成功后覆盖在本地的展示值。
 *
 *  props.book 是父组件在打开弹窗时传入的对象引用, store 重新拉取书架不会改到它,
 *  所以保存后必须自己记住新值, 否则弹窗仍显示旧书名/旧封面, 看起来像没保存。 */
const savedOverride = ref<BookEditDraft | null>(null)

const coverSrc = computed(() => {
  if (coverFailed.value || !props.book) return ''
  // 编辑态读草稿(选完图立刻能看到); 非编辑态优先用刚保存的结果。
  //
  // 注意 `savedOverride` 的存在性要单独判, 不能把它的 `customCoverUrl` 混进 OR 链:
  // 用户点「恢复原始封面」时保存的是**空串**(表示回落到书源封面), 空串是 falsy,
  // 会被 `||` 跳过而回落到 `props.book.customCoverUrl` —— 那正是刚被删除的旧封面
  // 文件, 图片 404 → 显示「无封面」, 用户刚点完还原却看不到书源封面。
  const url = isEditing.value
    ? (draft.value.customCoverUrl || props.book.coverUrl)
    : savedOverride.value
    ? (savedOverride.value.customCoverUrl || props.book.coverUrl)
    : ((props.book as Book).customCoverUrl || props.book.coverUrl)
  return url ? getCoverUrl(url) : ''
})

const displayChapters = computed(() => {
  if (showAllChapters.value) return chapters.value
  return chapters.value.slice(0, 50)
})

/** 非编辑态展示用的书籍信息: 优先用刚保存的结果, 其次才是 props。
 *  props.book 是打开弹窗时的对象引用, 保存后不会自动更新。 */
const displayBook = computed(() => {
  const base = props.book
  if (!base) return { name: '', author: '', intro: '' }
  if (savedOverride.value) {
    return {
      name: savedOverride.value.name,
      author: savedOverride.value.author,
      intro: savedOverride.value.intro,
    }
  }
  return {
    name: base.name,
    author: base.author || '',
    intro: base.intro || '',
  }
})

const displayOriginName = computed(() => {
  if (!props.book) return ''
  return sourceCatalog.value.get(sourceKey(props.book.origin))
    || (props.book as Book).originName
    || props.book.origin
})

function sourceNameByOrigin(origin: string): string {
  return sourceCatalog.value.get(sourceKey(origin))
    || sourceStore.sources.find((source) => sourceKey(source.bookSourceUrl) === sourceKey(origin))?.bookSourceName
    || origin
}

function isCurrentDetail(loadId: number, bookKey: string): boolean {
  return loadId === detailLoadId
    && props.modelValue
    && detailKey.value === bookKey
}

function sourceKey(origin: string): string {
  const normalized = origin.trim()
  if (!normalized) return ''
  return normalized.replace(/\/+$/, '')
}

function candidateKey(candidate: SearchBook): string {
  return `${sourceKey(candidate.origin)}::${candidate.bookUrl}`
}

function sourceCatalogSignature(): string {
  return sourceStore.sources
    .filter((source) => source.enabled !== false)
    .map((source) => sourceKey(source.bookSourceUrl))
    .sort()
    .join('\n')
}

function sourceCandidatesCacheKey(bookKey: string): string {
  return `${bookKey}::${sourceCatalogSignature()}`
}

function storeSourceCandidatesCache(key: string, candidates: SearchBook[]) {
  sourceCandidatesCache.delete(key)
  sourceCandidatesCache.set(key, candidates)
  while (sourceCandidatesCache.size > SOURCE_CANDIDATES_CACHE_LIMIT) {
    const oldestKey = sourceCandidatesCache.keys().next().value
    if (oldestKey === undefined) break
    sourceCandidatesCache.delete(oldestKey)
  }
}

function canonicalSourceOrigin(origin: string): string {
  const found = sourceStore.sources.find((source) => (
    sourceKey(source.bookSourceUrl) === sourceKey(origin)
  ))
  return found?.bookSourceUrl || origin
}

function resetDetailState() {
  closeSourceSSE()
  coverFailed.value = false
  showAllChapters.value = false
  chapters.value = []
  sourceCandidates.value = []
  selectedSource.value = null
  sourceCatalog.value = new Map()
  sourcesLoading.value = false
}

function closeSourceSSE() {
  sourceSSE?.close()
  sourceSSE = null
}

function loadSourceCandidates(loadId: number, bookKey: string) {
  const b = props.book
  if (!b || !b.name) {
    return
  }
  closeSourceSSE()
  sourcesLoading.value = true
  const current = toSearchBook(b as Book)
  const storedCandidates = (b as Book).sourceCandidates || []
  const initialCandidates = [current, ...storedCandidates]
  const cachedCandidates = sourceCandidatesCache.get(sourceCandidatesCacheKey(bookKey))
  if (cachedCandidates) {
    if (!isCurrentDetail(loadId, bookKey)) return
    setSourceCandidates([...initialCandidates, ...cachedCandidates])
    sourcesLoading.value = false
    return
  }
  // 先显示当前书源和已经保存的候选，其他书源搜索完成后逐个追加。
  setSourceCandidates(initialCandidates)

  const stream = getAvailableBookSourceSSE({
    url: b.bookUrl,
    name: b.name,
    author: b.author,
    origin: b.origin,
    lastIndex: -1,
    resultLimit: 100,
    concurrentCount: 12,
  })
  sourceSSE = stream
  let completed = false

  const finish = (cacheResult: boolean) => {
    if (completed) return
    completed = true
    if (sourceSSE === stream) sourceSSE = null
    if (!isCurrentDetail(loadId, bookKey)) return
    sourcesLoading.value = false
    if (cacheResult) {
      storeSourceCandidatesCache(
        sourceCandidatesCacheKey(bookKey),
        sourceCandidates.value.slice(),
      )
    }
  }

  stream.onmessage = (event) => {
    if (sourceSSE !== stream || !isCurrentDetail(loadId, bookKey)) {
      stream.close()
      return
    }
    const data = event.data as { data?: SearchBook[] }
    if (Array.isArray(data.data) && data.data.length > 0) {
      setSourceCandidates([...sourceCandidates.value, ...data.data])
    }
  }

  stream.addEventListener('end', (event) => {
    const payload = event.data as { hasMore?: boolean }
    finish(payload.hasMore !== true)
  })
  stream.onerror = () => finish(false)
}

function toSearchBook(book: Book): SearchBook {
  return {
    name: book.name,
    author: book.author,
    bookUrl: book.bookUrl,
    origin: book.origin,
    coverUrl: book.coverUrl,
    intro: book.intro,
    kind: book.kind,
    lastChapter: book.latestChapterTitle,
  }
}

function setSourceCandidates(candidates: SearchBook[]) {
  const previousSelection = selectedSource.value ? candidateKey(selectedSource.value) : ''
  const currentOrigin = sourceKey(props.book?.origin || '')
  const currentAuthor = props.book?.author
  const seenSources = new Set<string>()
  sourceCandidates.value = candidates.map((candidate) => ({
    ...candidate,
    origin: canonicalSourceOrigin(candidate.origin),
  })).filter((candidate) => {
    if (!candidate.origin || !candidate.bookUrl) return false
    if (!matchesSourceSwitchAuthor(currentAuthor, candidate.author)) return false
    const originKey = sourceKey(candidate.origin)
    if (!sourceCatalog.value.has(originKey) || seenSources.has(originKey)) {
      return false
    }
    seenSources.add(originKey)
    return true
  })
  selectedSource.value = sourceCandidates.value.find((candidate) => (
    candidateKey(candidate) === previousSelection
  )) || sourceCandidates.value.find((candidate) => (
    sourceKey(candidate.origin) === currentOrigin
  )) || sourceCandidates.value[0] || null
}

function selectSource(candidate: SearchBook) {
  if (!props.modelValue || !props.book) return
  if (
    sourceKey(selectedSource.value?.origin || '') === sourceKey(candidate.origin)
    && selectedSource.value?.bookUrl === candidate.bookUrl
  ) return
  selectedSource.value = candidate
  void loadChaptersFor(candidate.bookUrl, candidate.origin, detailLoadId, detailKey.value)
}

async function loadChaptersFor(
  bookUrl: string,
  origin: string,
  loadId = detailLoadId,
  bookKey = detailKey.value,
) {
  const requestId = ++chapterLoadId
  chaptersLoading.value = true
  try {
    const nextChapters = await getChapterList({ bookUrl, bookSourceUrl: origin })
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = nextChapters
  } catch {
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = []
  } finally {
    if (requestId === chapterLoadId) chaptersLoading.value = false
  }
}

watch([() => props.modelValue, detailKey], async ([visible, bookKey]) => {
  const loadId = ++detailLoadId
  chapterLoadId += 1

  if (!visible || !props.book) {
    resetDetailState()
    return
  }

  resetDetailState()
  const b = props.book as Book
  if (isLocal.value) {
    await loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey)
    return
  }

  sourcesLoading.value = true
  // The source manager force-refreshes this shared store after source changes.
  // A detail view only needs to initialize it when no source list exists yet.
  if (sourceStore.sources.length === 0) {
    await sourceStore.fetchSources().catch(() => undefined)
  }
  if (!isCurrentDetail(loadId, bookKey)) return

  sourceCatalog.value = new Map(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => [sourceKey(source.bookSourceUrl), source.bookSourceName]),
  )
  if (!sourceCatalog.value.has(sourceKey(b.origin))) {
    // 保留当前书籍自身的源，避免书源列表加载失败时详情页完全没有可选项。
    sourceCatalog.value.set(sourceKey(b.origin), (b as Book).originName || b.origin)
  }
  const sourceTask = loadSourceCandidates(loadId, bookKey)
  await Promise.all([
    sourceTask,
    loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey),
  ])
}, { immediate: true })

watch(() => sourceStore.availabilityVersion, async () => {
  if (!props.modelValue || !props.book || isLocal.value) return
  sourceCandidatesCache.clear()
  const loadId = ++detailLoadId
  chapterLoadId += 1
  const bookKey = detailKey.value
  resetDetailState()
  sourcesLoading.value = true
  if (!isCurrentDetail(loadId, bookKey)) return
  sourceCatalog.value = new Map(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => [sourceKey(source.bookSourceUrl), source.bookSourceName]),
  )
  const b = props.book as Book
  if (!sourceCatalog.value.has(sourceKey(b.origin))) {
    sourceCatalog.value.set(sourceKey(b.origin), b.originName || b.origin)
  }
  await Promise.all([
    loadSourceCandidates(loadId, bookKey),
    loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey),
  ])
}, { flush: 'post' })

function close() {
  emit('update:modelValue', false)
}

/* ─── 编辑书籍信息 ───
 *  只改书架里的记录(书名/作者/封面/简介), 不碰磁盘上的原始文件 ——
 *  本地导入的书籍改的也只是显示名。 */

const isEditing = ref(false)
const savingEdit = ref(false)
const uploadingCover = ref(false)
const coverInputRef = ref<HTMLInputElement | null>(null)
const draft = ref<BookEditDraft>(createBookEditDraft({ name: '' }))

/** 进入编辑态那一刻的快照, 供「还原」把草稿恢复回来。 */
const editBaseline = ref<BookEditDraft>(createBookEditDraft({ name: '' }))

/** 本次编辑会话中上传过、但尚未写进书架记录的封面 URL。
 *
 *  用户可能传了图又点「恢复原始封面」(或直接取消) —— 那些文件已成孤儿。
 *  取消时的残留是可接受的, 但保存时能顺手清掉就该清掉。 */
const pendingCoverUploads = ref<string[]>([])

/** 是否有「最初的书籍信息」可还原。
 *
 *  曾自定义过(后端记下了 original*)或当前有自定义封面时为真。保存之后它依然为真
 *  —— 这正是用户要的: 保存过也能一键回到原样。 */
const canRevert = computed(() => {
  const shelfBook = findShelfBook()
  return shelfBook ? canRevertToOrigin(shelfBook) : false
})

function startEdit() {
  // 只允许编辑书架里的书: 搜索结果形态缺 toc_url, 走 save_book 会触发书源网络
  // 回填并按「当前启用书源」过滤 source_candidates, 可能永久剔除已禁用源的候选。
  const shelfBook = findShelfBook()
  if (!shelfBook) {
    // 右键菜单会立刻调这个函数, 但书架数据是异步拉的 —— 还没到位时静默 return
    // 会让用户以为「编辑信息」这个菜单项坏了。给一句明确反馈。
    appStore.showToast('书架信息还没加载完，请稍后再试', 'warning')
    return
  }
  const baseline = createBookEditDraft(shelfBook)
  editBaseline.value = baseline
  draft.value = { ...baseline }
  pendingCoverUploads.value = []
  coverFailed.value = false
  isEditing.value = true
}

/** 还原: 回到「最初的书籍信息」(首次自定义之前的书名/作者/简介, 以及书源封面)。
 *
 *  只改表单, 仍需点「保存」才写入 —— 与编辑页其它改动保持一致, 也让用户能先看清
 *  还原后的样子再决定。 */
function revertEdit() {
  const shelfBook = findShelfBook()
  if (!shelfBook) return
  draft.value = resolveRevertDraft(shelfBook, editBaseline.value)
  coverFailed.value = false
}

function cancelEdit() {
  isEditing.value = false
  savingEdit.value = false
  // 取消时已上传的文件留在磁盘上, 但 UI 不该再记得它们。
  pendingCoverUploads.value = []
}

/** 恢复书源自带的封面: 清空自定义封面即可回落到 coverUrl。 */
function resetCover() {
  draft.value.customCoverUrl = ''
  coverFailed.value = false
}

async function handleCoverFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  // 无论成败都要清空, 否则同一张图再选一次不会触发 change。
  input.value = ''
  if (!file || uploadingCover.value) return

  const invalid = validateCoverFile(file)
  if (invalid) {
    appStore.showToast(invalid, 'error')
    return
  }

  uploadingCover.value = true
  try {
    const url = await uploadBookCover(file)
    draft.value.customCoverUrl = url
    pendingCoverUploads.value = [...pendingCoverUploads.value, url]
    coverFailed.value = false
  } catch (error) {
    appStore.showToast((error as Error).message || '封面上传失败', 'error')
  } finally {
    uploadingCover.value = false
  }
}

async function saveEdit() {
  const shelfBook = findShelfBook()
  if (!shelfBook || savingEdit.value) return
  // 上传还没结束就保存的话, 提交的仍是旧封面, 而用户以为保存成功了。
  if (uploadingCover.value) {
    appStore.showToast('封面还在上传，请稍候再保存', 'error')
    return
  }

  const invalid = validateBookEditDraft(draft.value)
  if (invalid) {
    appStore.showToast(invalid, 'error')
    return
  }
  const next = normalizeBookEditDraft(draft.value)

  savingEdit.value = true
  const bookKeyAtSave = detailKey.value
  try {
    // 换封面时旧文件会变成孤儿, 保存后用它的 URL 清理(远程封面会被后端忽略)。
    const obsolete = obsoleteCoverUrl(shelfBook.customCoverUrl, next.customCoverUrl)
    // 首次自定义时把「编辑前」的值记成原始值, 之后不再变动 —— 这是「还原」能
    // 回到最初样子的依据。已经记过的书这里返回空对象, 不会覆盖。
    const originPatch = originPatchForSave(shelfBook, editBaseline.value)
    // 原本没有值的字段若照 '' 回传, 会把后端的 None 写成 Some(""), 使
    // merge_book 的「书源有简介就回填」判据失效 —— 用这个函数省略掉这类键。
    const edited = editableFieldsForSave(shelfBook, next)
    await saveBook({ ...shelfBook, ...originPatch, ...edited })
    // 本次会话里传过但最终没被采用的图(换了两次 / 又点回原始封面)也要清掉。
    const unused = pendingCoverUploads.value.filter((url) => url !== next.customCoverUrl)
    if (bookKeyAtSave !== detailKey.value) return
    await shelfStore.fetchBooks()
    savedOverride.value = next
    for (const url of [obsolete, ...unused]) {
      if (!url) continue
      // 清理失败不该让用户以为保存失败 —— 记录保存在前, 这里只是收尾。
      await deleteBookCover(url).catch(() => undefined)
    }
    pendingCoverUploads.value = []
    appStore.showToast('已保存书籍信息', 'success')
    isEditing.value = false
  } catch (error) {
    if (bookKeyAtSave === detailKey.value) {
      appStore.showToast((error as Error).message || '保存失败', 'error')
    }
  } finally {
    savingEdit.value = false
  }
}

/** 关窗/切书时退出编辑态, 避免下次打开还停在编辑中。
 *  未保存的改动直接丢弃 —— 换封面上传的文件会留在磁盘上, 但那是几十 KB 的
 *  无害孤儿, 不值得为它做一套回收机制。 */
watch([() => props.modelValue, detailKey], ([visible]) => {
  isEditing.value = false
  savingEdit.value = false
  pendingCoverUploads.value = []
  // 换了另一本书就丢掉旧的覆盖值, 否则会显示成上一本改后的名字/封面。
  savedOverride.value = null
  // 书架右键「编辑信息」进来时直接落到编辑态。
  // 用 nextTick 让书架数据/详情先就位, findShelfBook 才找得到这本书。
  if (visible && props.startInEdit) {
    void nextTick(() => startEdit())
  }
}, { immediate: true })

/** The book to open for reading, preferring the user-selected source. */
function activeBook(): Book {
  const base = props.book as Book
  const sel = selectedSource.value
  if (sel) {
    return {
      ...base,
      bookUrl: sel.bookUrl,
      origin: sel.origin,
      originName: sourceNameByOrigin(sel.origin),
      coverUrl: sel.coverUrl || base.coverUrl,
      intro: sel.intro || base.intro,
      kind: sel.kind || base.kind,
      latestChapterTitle: sel.lastChapter || base.latestChapterTitle,
      durChapterIndex: 0,
      durChapterTitle: undefined,
      // A different source has its own toc URL; clear the old one so the reader
      // fetches the new source's chapter list instead of failing on a stale one.
      tocUrl: undefined,
      sourceCandidates: sourceCandidates.value.slice(),
    }
  }
  return base
}

/** True when the shown book already lives on the shelf (so a source switch should persist). */
function isShelfBook(): boolean {
  return Boolean(findShelfBook())
}

function findShelfBook(): Book | undefined {
  const b = props.book
  if (!b) return undefined
  const identity = searchMergeKey(b)
  return shelfStore.books.find((item) => (
    item.bookUrl === b.bookUrl || searchMergeKey(item) === identity
  ))
}

/** If the user picked a different source for a shelved book, persist the switch. */
async function persistSourceSwitchIfNeeded() {
  const sel = selectedSource.value
  if (!sel || !isShelfBook()) return
  const base = props.book as Book
  if (sel.origin === base.origin && sel.bookUrl === base.bookUrl) return
  try {
    const updated = await setBookSource({
      bookUrl: base.bookUrl,
      newUrl: sel.bookUrl,
      bookSourceUrl: sel.origin,
      name: base.name,
      author: base.author,
      coverUrl: sel.coverUrl || base.coverUrl,
      intro: sel.intro || base.intro,
      kind: sel.kind || base.kind,
      latestChapterTitle: sel.lastChapter || base.latestChapterTitle,
      durChapterIndex: base.durChapterIndex,
      durChapterTitle: base.durChapterTitle,
      durChapterPos: base.durChapterPos,
      durChapterTime: base.durChapterTime,
    })
    if (updated) {
      await shelfStore.fetchBooks().catch(() => undefined)
    }
  } catch (e: unknown) {
    console.warn('切换书源失败', e)
  }
}

async function startReading() {
  await persistSourceSwitchIfNeeded()
  const b = activeBook()
  // 置顶只是书架排序, 不该挡在跳转前面 —— 失败也无所谓, 不打断阅读。
  void shelfStore.moveBookToFront(b.bookUrl).catch(() => undefined)
  close()
  // 先跳转、后加载: loadBook 自己会取目录并加载 durChapterIndex 那一章,
  // 阅读页挂载时用 waitForChapterListReady 承接等待。
  void readerStore.loadBook(b).catch((error: unknown) => {
    appStore.showToast((error as Error).message || '加载书籍失败', 'error')
  })
  router.push('/reader')
}

/** 打开指定章节。
 *
 *  先把目标章号写进传入的书(阅读页靠它决定初始章), 再立刻跳转 —— 目录抓取与正文
 *  下载都在阅读页里进行, 用户点章节后马上就进入阅读页, 不再对着详情页干等。
 *  注意绝不能 await loadBook/loadChapter: 那正是本次要消除的「加载完才跳转」。 */
async function readChapter(index: number) {
  await persistSourceSwitchIfNeeded()
  // activeBook() 在未换源时返回 props.book 本身, 直接改会污染调用方的对象。
  const b = { ...activeBook(), durChapterIndex: index, durChapterPos: 0 }
  void shelfStore.moveBookToFront(b.bookUrl).catch(() => undefined)
  close()
  void readerStore.loadBook(b).catch((error: unknown) => {
    appStore.showToast((error as Error).message || '加载章节失败', 'error')
  })
  router.push('/reader')
}

async function addToShelf() {
  const b = activeBook()
  try {
    await saveBook(b)
    await shelfStore.fetchBooks()
    appStore.showToast('成功加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '加入书架失败', 'error')
  }
}

async function removeFromShelf() {
  const shelfBook = findShelfBook()
  if (!shelfBook || removingFromShelf.value) return
  removingFromShelf.value = true
  try {
    await shelfStore.removeBook(shelfBook)
    appStore.showToast('已取消加入书架', 'success')
  } catch (e: unknown) {
    appStore.showToast((e as Error).message || '取消加入失败', 'error')
  } finally {
    removingFromShelf.value = false
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: var(--z-overlay);
  backdrop-filter: blur(4px);
}

.modal-container {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding:
    calc(var(--space-6) + var(--safe-area-top))
    calc(var(--space-6) + var(--safe-area-right))
    calc(var(--space-6) + var(--safe-area-bottom))
    calc(var(--space-6) + var(--safe-area-left));
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
}

.detail-modal {
  width: 100%;
  max-width: 600px;
  max-height: min(85vh, calc(var(--app-height, 100dvh) - var(--safe-area-top) - var(--safe-area-bottom) - 32px));
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  background: var(--color-bg-elevated);
  border-radius: var(--radius-xl);
  padding: var(--space-8);
  position: relative;
  box-shadow: var(--shadow-xl);
}

.modal-close {
  position: absolute;
  top: max(var(--space-4), calc(var(--safe-area-top) * 0.35));
  right: var(--space-4);
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  color: var(--color-text-tertiary);
  transition: all var(--duration-fast);
  z-index: 1;
}

.modal-close:hover {
  background: var(--color-bg-hover);
  color: var(--color-text);
}

.modal-close svg {
  width: 18px;
  height: 18px;
}

.book-header {
  display: flex;
  gap: var(--space-5);
  margin-bottom: var(--space-6);
}

.book-cover-lg {
  width: 120px;
  height: 160px;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--color-bg-sunken);
  box-shadow: var(--shadow-md);
}

.book-cover-lg img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder-lg {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, var(--color-primary-bg), var(--color-bg-sunken));
  padding: var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-primary);
}

.book-header-info {
  flex: 1;
  min-width: 0;
}

.book-header-info h2 {
  font-size: var(--text-xl);
  font-weight: 700;
  margin-bottom: var(--space-2);
  line-height: var(--leading-tight);
}

.author {
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
}

.book-tags {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.tag {
  padding: 2px var(--space-2);
  background: var(--color-bg-sunken);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
}

.tag.origin {
  background: var(--color-primary-bg);
  color: var(--color-primary);
}

.progress {
  font-size: var(--text-sm);
  color: var(--color-primary);
}

.book-intro {
  margin-bottom: var(--space-6);
}

.book-intro h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-2);
}

.book-intro p {
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  line-height: var(--leading-relaxed);
  white-space: pre-wrap;
}

.source-section {
  margin-bottom: var(--space-6);
}

.source-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-3);
}

.source-loading,
.source-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-4);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.source-list {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.source-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  transition: background var(--duration-fast);
  font-size: var(--text-sm);
  border-bottom: 1px solid var(--color-divider);
}

.source-item:last-child {
  border-bottom: none;
}

.source-item:hover {
  background: var(--color-bg-hover);
}

.source-item.selected {
  background: var(--color-primary-bg);
}

.source-radio {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 2px solid var(--color-border);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: all var(--duration-fast);
}

.source-radio.checked {
  border-color: var(--color-primary);
}

.source-radio.checked::after {
  content: '';
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--color-primary);
}

.source-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}

.source-item.selected .source-name {
  color: var(--color-primary);
}

.source-latest {
  flex-shrink: 0;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
}

.chapter-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: var(--space-3);
}

.chapter-list {
  max-height: 300px;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
}

@media (max-width: 768px) {
  .detail-modal {
    padding: var(--space-6);
    border-radius: 20px;
  }
}

.chapter-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  transition: background var(--duration-fast);
  font-size: var(--text-sm);
  border-bottom: 1px solid var(--color-divider);
}

.chapter-item:last-child {
  border-bottom: none;
}

.chapter-item:hover {
  background: var(--color-bg-hover);
}

.chapter-item.current {
  color: var(--color-primary);
  background: var(--color-primary-bg);
}

.chapter-index {
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  min-width: 28px;
}

.chapter-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.show-more-btn {
  width: 100%;
  padding: var(--space-3);
  text-align: center;
  color: var(--color-primary);
  font-size: var(--text-sm);
  font-weight: 500;
  margin-top: var(--space-2);
  border-radius: var(--radius-md);
  transition: background var(--duration-fast);
}

.show-more-btn:hover {
  background: var(--color-primary-bg);
}

.chapter-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-6);
  color: var(--color-text-tertiary);
  font-size: var(--text-sm);
}

.loading-spinner {
  width: 18px;
  height: 18px;
  border: 2px solid var(--color-border);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.modal-actions {
  display: flex;
  gap: var(--space-3);
  margin-top: var(--space-6);
  padding-top: var(--space-5);
  border-top: 1px solid var(--color-divider);
}

.action-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 600;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  transition: all var(--duration-fast);
}

.action-btn:hover {
  background: var(--color-bg-hover);
}

.action-btn.primary {
  background: var(--color-primary);
  color: white;
  border-color: var(--color-primary);
}

.action-btn.primary:hover {
  background: var(--color-primary-dark);
}

/* ─── 编辑书籍信息 ───
   编辑态是一张独立的表单, 只放可编辑的四项; 书源/目录等只读信息不在此显示。 */
.edit-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.edit-form-title {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 700;
}

.edit-cover-block {
  display: flex;
  align-items: flex-start;
  gap: var(--space-4);
}

.edit-cover {
  position: relative;
  flex-shrink: 0;
  width: 96px;
  height: 128px;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--color-bg-sunken);
  border: 1px solid var(--color-border);
  display: flex;
  align-items: center;
  justify-content: center;
}

.edit-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.edit-cover.empty {
  border-style: dashed;
}

.edit-cover-empty {
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
}

.edit-cover-side {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  min-width: 0;
}

.edit-cover-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.edit-cover-note {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--color-text-tertiary);
  line-height: 1.5;
}

.edit-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.edit-field-label {
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
}

.edit-field-input,
.edit-field-textarea {
  width: 100%;
  box-sizing: border-box;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text);
  font-family: inherit;
  font-size: var(--text-sm);
  transition: border-color var(--duration-fast), box-shadow var(--duration-fast);
}

.edit-field-input:focus,
.edit-field-textarea:focus {
  outline: none;
  border-color: var(--color-primary);
  box-shadow: 0 0 0 3px var(--color-primary-bg);
}

.edit-field-textarea {
  resize: vertical;
  line-height: 1.7;
  min-height: 96px;
}

.cover-edit-btn {
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: var(--text-sm);
  font-weight: 600;
  cursor: pointer;
  transition: all var(--duration-fast);
}

.cover-edit-btn:hover:not(:disabled) {
  background: var(--color-bg-hover);
  border-color: var(--color-primary-border);
}

.cover-edit-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.cover-edit-btn.subtle {
  background: transparent;
  color: var(--color-text-secondary);
}

.hidden-file-input {
  display: none;
}

.action-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
</style>
