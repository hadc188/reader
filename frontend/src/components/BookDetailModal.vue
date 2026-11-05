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
            <p
              :class="{ collapsed: !introExpanded }"
              @click="introExpanded = !introExpanded"
            >{{ displayBook.intro }}</p>
            <button
              class="intro-toggle"
              @click="introExpanded = !introExpanded"
            >{{ introExpanded ? '收起' : '展开全部' }}</button>
          </div>

          <!-- Available Sources -->
          <!--
            Web 端不显示: 这个列表靠 `getAvailableBookSourceSSE` 拉, 而它是
            `tauri::ipc::Channel` 流(sse.ts 的 openSse 直接调 invoke), 浏览器里
            连不上; 就算列表出来了, 换源要写 `setBookSource`, 也没有 HTTP 路由。
            两头都缺, 整块藏掉 —— 与顶栏多源搜索的处理一致。
          -->
          <div v-if="!isLocal && !appStore.isWebMode" class="source-section">
            <div class="section-heading">
              <h3>{{ singleSourceOnly ? '书源' : '可读书源' }}</h3>
              <!-- 有缓存时给一枚小角标, 说明列表不是刚搜的; 没缓存就什么都不显示,
                   避免「未搜索」这类字样喧宾夺主。 -->
              <span v-if="sourcesHaveCache && !sourcesLoading" class="section-hint" :title="sourcesCacheHint">
                缓存结果
              </span>
              <!-- 与手机端详情页的 menu_refresh 对齐：随时重搜可用书源。
                   **现在这是搜源的唯一入口** —— 打开弹窗不再自动并发搜索
                   （书源清单变动除外，那种情况会自动重搜一次）。
                   搜索中则变成「停止」—— 与换源面板和手机端一致，
                   某个源卡住时不必等所有源都超时。 -->
              <button
                v-if="!singleSourceOnly"
                type="button"
                class="section-refresh"
                :title="sourcesLoading ? '停止搜索其他书源' : '搜索其他书源，获取最新章节'"
                :aria-label="sourcesLoading ? '停止搜索其他书源' : '搜索其他书源，获取最新章节'"
                @click="sourcesLoading ? stopSourceCandidates() : refreshSourceCandidates()"
              >
                <svg v-if="sourcesLoading" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <rect x="7" y="7" width="10" height="10" rx="1.5" />
                </svg>
                <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6" />
                </svg>
              </button>
            </div>
            <div v-if="sourcesLoading && !sourceCandidates.length" class="source-loading">
              <div class="loading-spinner"></div>
              正在查找其他书源...
            </div>
            <!-- 停止后一条都没搜到：不能说"未找到其他书源" —— 那是搜索跑完的
                 结论，而这里是被用户主动打断的。 -->
            <div v-else-if="!sourceCandidates.length && sourcesStopped" class="source-empty">
              已停止搜索
            </div>
            <!-- 什么都没搜过 = 打开时没缓存。别写「未找到其他书源」，那是搜完才
                 能下的结论，会让人以为这本书真的没有别的源。 -->
            <div v-else-if="!sourceCandidates.length && !sourcesIncomplete" class="source-empty">
              尚未搜索其他书源，点右侧按钮开始
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
                <span class="source-name">
                  {{ sourceNameByOrigin(cand.origin) }}
                  <small v-if="cand.author && normalizeSourceSwitchAuthor(cand.author) !== normalizeSourceSwitchAuthor(book?.author)" class="source-author">{{ cand.author }}</small>
                </span>
                <span v-if="cand.lastChapter" class="source-latest">{{ cand.lastChapter }}</span>
              </div>
            </div>
            <!-- 追加中 / 部分源失败的状态条放在列表**下方**，不占列表行位。
                 之前塞在列表内部（sticky 页脚），等于从 5 行里再吃掉一行，
                 用户看到的就只有 4 个书源了。 -->
            <div v-if="sourcesLoading && sourceCandidates.length" class="source-foot">
              <div class="loading-spinner"></div>
              继续查找其他书源...
            </div>
            <!-- 单源模式下不提供「重试」：它的语义是"再并搜一遍其它源"，
                 而这时我们刻意不搜。 -->
            <div
              v-else-if="!singleSourceOnly && (sourcesIncomplete || sourcesStopped) && sourceCandidates.length"
              class="source-foot warn"
              role="status"
            >
              <svg class="incomplete-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
                <circle cx="12" cy="12" r="9" />
                <path d="M12 7.5v5.5M12 16.5h.01" />
              </svg>
              <span class="incomplete-text">{{ sourcesStopped ? '已停止搜索' : '部分书源访问失败' }}</span>
              <button type="button" class="incomplete-retry" @click="loadSourceCandidates(detailLoadId, detailKey, true)">
                {{ sourcesStopped ? '继续搜索' : '重试' }}
              </button>
            </div>
          </div>

          <!-- Chapters -->
          <div class="chapter-section" v-if="chapters.length > 0">
            <div class="chapter-heading">
              <h3>目录 ({{ chapters.length }})</h3>
              <span
                v-if="shownCatalogSourceName || chapterSourceName"
                class="chapter-source-status"
                role="status"
                :title="shownCatalogSourceName || chapterSourceName"
              >
                {{ shownCatalogSourceName || chapterSourceName }}
              </span>
            </div>
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
          <div v-else-if="chaptersLoading" class="chapter-loading-state" role="status">
            <div class="chapter-heading">
              <h3>正在加载目录</h3>
              <span v-if="chapterSourceName" class="chapter-source-status" :title="chapterSourceName">
                {{ chapterSourceName }}
              </span>
            </div>
            <div class="chapter-loading">
              <div class="loading-spinner"></div>
              正在获取章节列表…
            </div>
          </div>
          <div v-else-if="chapterError" class="chapter-loading-state" role="alert">
            <div class="chapter-heading">
              <h3>目录加载失败</h3>
              <span v-if="chapterSourceName" class="chapter-source-status error" :title="chapterSourceName">
                {{ chapterSourceName }}
              </span>
            </div>
            <div class="chapter-error-message">
              <span>暂时无法获取该源目录，请重试或切换书源。</span>
              <button type="button" class="chapter-retry-btn" @click="retryChapters">重试</button>
            </div>
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
                <!--
                  编辑信息 / 取消加入都是写操作, Web 端没有对应路由
                  (`deleteBook`、`uploadBookCover` 等只存在于 Tauri)。
                  按钮留着只会让人点出一个 404, 所以按桌面专属处理。
                -->
                <button v-if="!appStore.isWebMode" class="action-btn" @click="startEdit">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
                    <path d="M12 20h9" />
                    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
                  </svg>
                  编辑信息
                </button>
                <button
                  v-if="!appStore.isWebMode"
                  class="action-btn"
                  :disabled="removingFromShelf"
                  @click="removeFromShelf"
                >
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
import { getCoverUrl, getChapterList, saveBook, setBookSource, getBookInfo } from '../api/bookshelf'
import { getAvailableBookSourceSSE } from '../api/search'
import type { SseLike } from '../api/sse'
import { useSourceStore } from '../stores/source'
import { useBookshelfStore } from '../stores/bookshelf'
import { useReaderStore } from '../stores/reader'
import { useAppStore } from '../stores/app'
import type { Book, SearchBook, BookChapter } from '../types'
import { isLocalBook } from '../utils/localBook'
import {
  getSourceCandidateCache,
  mergeForCommit,
  sourceCandidateBookKey,
  sourceCatalogSignatureOf,
} from '../utils/sourceCandidateCache'
import { matchesSourceSwitchTitle, mergeSourceCandidates, normalizeSourceSwitchAuthor, shelfMatchKey, sourceCandidateKey } from '../utils/searchRank'
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

/**
 * 「可读书源」结果的会话内缓存（issue #28 第 2 点）。
 *
 * 之前每次打开详情弹窗都会自动并发搜 12 路书源 —— 用户只是想翻翻目录，也要
 * 陪等一次全量搜索，还会反复打到各源站。现在打开弹窗**默认不搜**，只读这里
 * 的缓存；要最新的自己点标题右侧的刷新按钮。
 *
 * 两条失效规则：
 * - **书源清单变了**（增删/停启用）→ 缓存里的候选可能指向已被删掉的源，
 *   打开详情时自动重搜一次。判断依据是缓存记录里的 `catalogSignature`。
 * - 用户点刷新 → 强制重搜（不清旧缓存，见下）。
 *
 * 中断/失败的结果也要存：用户刷出 8 个源、中途点了停止，下次打开还得是这 8 个，
 * 不能退回刷新前的 4 个。所以落库时用的是「旧缓存 ∪ 当前列表」的并集。
 */
const sourceCandidateCache = getSourceCandidateCache()
/** 当前列表是否有缓存垫底（决定标题旁边显示「缓存结果」还是「未搜索」）。 */
const sourcesCachedAt = ref(0)
/** 这份缓存的年龄是否已超过保质期（仅用于提示，不阻止显示）。 */
const sourcesCacheExpired = ref(false)
const sourcesHaveCache = computed(() => sourcesCachedAt.value > 0)

const props = defineProps<{
  modelValue: boolean
  book: Book | SearchBook | null
  /** 打开弹窗后直接进入编辑态(书架右键「编辑信息」用)。 */
  startInEdit?: boolean
  /**
   * 只显示本书自身的书源，不自动并发搜索其它源。
   *
   * 发现页的书来自聚合源的特定榜单（如聚合源的「排行榜」），书名/作者往往只有
   * 聚合源认得；默认行为会打开弹窗就并发搜 12 路书源，既慢又几乎搜不到东西，
   * 用户看到的「可读书源」反而只有当前这一条。传 true 时直接跳过搜索。
   */
  singleSourceOnly?: boolean
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
const sourcesIncomplete = ref(false)
/** 用户主动中止过本次书源搜索。用来区分"搜完了没有结果"与"被我停掉了" ——
 *  只有后者才该显示「已停止搜索」。 */
const sourcesStopped = ref(false)
const selectedSource = ref<SearchBook | null>(null)
const sourceCatalog = ref(new Map<string, string>())
const removingFromShelf = ref(false)
const catalogTarget = ref('')
const chapterError = ref('')
const loadedCatalogIdentity = ref('')
let detailLoadId = 0
let chapterLoadId = 0
let sourceSSE: SseLike | null = null

const isLocal = computed(() => isLocalBook(props.book))

const detailKey = computed(() => {
  const book = props.book
  if (!book) return ''
  // 与阅读页换源面板共用同一个键生成函数: 两边各写一份格式，迟早会漂移成
  // 互相看不见的两份缓存。
  return sourceCandidateBookKey({
    name: book.name,
    author: book.author,
    bookUrl: book.bookUrl,
    origin: book.origin,
  })
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
    ? (draft.value.customCoverUrl || props.book.coverUrl || detailCover.value)
    : savedOverride.value
    ? (savedOverride.value.customCoverUrl || props.book.coverUrl || detailCover.value)
    : ((props.book as Book).customCoverUrl || props.book.coverUrl || detailCover.value)
  // 带上书源地址: 加密封面要靠它反查 `coverDecodeJs`（issue #23）。
  return url ? getCoverUrl(url, props.book.origin) : ''
})

const displayChapters = computed(() => {
  if (showAllChapters.value) return chapters.value
  return chapters.value.slice(0, 50)
})

/** 简介是否展开。书源的 `intro` 往往是一整块信息(最新/点击/评分/简介),
 *  收起时给足行数, 需要时再展开看全文。 */
const introExpanded = ref(false)

/** 从书源详情页取回的完整简介。
 *
 *  搜索/发现页的 `ruleSearch.intro` 常常是站点摘要（末尾带 `..`），而完整的
 *  `ruleBookInfo.intro` 只有访问详情页才拿得到。弹窗打开时补拉一次，避免
 *  「手机端简介很长、桌面端只有一行」这种差异。 */
const detailIntro = ref('')
/** 详情页补拉回来的封面地址。
 *
 *  很多书源的 `ruleSearch.coverUrl` 是空的（搜索列表页压根没有封面图），
 *  封面只写在详情页规则里。早先这里只补简介、不补封面，于是「搜索结果有、
 *  点进详情弹窗却是空封面」—— 后台 `/cover` 其实能拿到图，只是前端从没
 *  请求过它。 */
const detailCover = ref('')

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
    // 详情页拿到的简介比搜索结果里的摘要完整，优先用它。
    intro: detailIntro.value || base.intro || '',
  }
})

const displayOriginName = computed(() => {
  const source = selectedSource.value
  if (source) return sourceNameByOrigin(source.origin)
  if (!props.book) return ''
  return sourceCatalog.value.get(sourceKey(props.book.origin))
    || (props.book as Book).originName
    || props.book.origin
})

const chapterSourceName = computed(() => {
  const source = selectedSource.value
  return source ? sourceNameByOrigin(source.origin) : ''
})

/** 「缓存结果」角标的悬停说明：告诉用户这份结果是什么时候的、怎么更新。 */
const sourcesCacheHint = computed(() => {
  if (!sourcesHaveCache.value) return '还没有搜索过其他书源，点右侧按钮开始搜索'
  const minutes = Math.max(0, Math.floor((Date.now() - sourcesCachedAt.value) / 60000))
  const when = minutes < 1 ? '刚刚' : `${minutes} 分钟前`
  // 过期只提示、不自动重搜：issue #28 第 2 点明确要求打开详情别自动刷新。
  if (sourcesCacheExpired.value) {
    return `结果记于 ${when}，可能已过时，点右侧按钮获取最新`
  }
  return `结果记于 ${when}，点右侧按钮获取最新`
})

const shownCatalogSourceName = computed(() => {
  const identity = loadedCatalogIdentity.value
  if (!identity) return ''
  return sourceNameByOrigin(catalogIdentityOrigin(identity))
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
  return sourceCandidateKey(candidate)
}

/**
 * 目录身份的键必须与 `candidateKey()` **同源**。
 *
 * 早先这里用 `${sourceKey(origin)}::${bookUrl}` 自造格式, 而比较时用的是
 * `candidateKey()` 的 JSON 格式, 两者永不相等 —— `setSourceCandidates` 里
 * "保住已加载目录的那个源"的分支恒为 false, 后续 SSE 批次(每批都会重新调用)
 * 就把用户手选的源重置回本书原源。结果是目录来自 B 源、选中项与 `activeBook()`
 * 都是 A 源, 读正文时拿 B 的章节号去 A 源取, 正文错位。
 */
function catalogIdentityKey(origin: string, bookUrl: string): string {
  return sourceCandidateKey({ origin, bookUrl })
}

/** 从目录身份键里取回书源 origin; 键格式统一由 `catalogIdentityKey` 产生。 */
function catalogIdentityOrigin(identity: string): string {
  if (!identity) return ''
  try {
    const parsed = JSON.parse(identity) as unknown
    return Array.isArray(parsed) && typeof parsed[0] === 'string' ? parsed[0] : ''
  } catch {
    return ''
  }
}

/** 当前启用书源的签名。算法在 utils 里与阅读页共用，保证两处判断一致。 */
function sourceCatalogSignature(): string {
  return sourceCatalogSignatureOf(sourceStore.sources)
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
  sourcesStopped.value = false
  sourcesIncomplete.value = false
  sourcesCachedAt.value = 0
  sourcesCacheExpired.value = false
  catalogTarget.value = ''
  chapterError.value = ''
  loadedCatalogIdentity.value = ''
  detailIntro.value = ''
  detailCover.value = ''
}

/** 补拉书源详情页的完整简介与封面（搜索结果的 intro 通常只是站点摘要，
 *  coverUrl 更常常是空的）。失败时保持原有信息，不影响其它展示。 */
async function loadDetailIntro(loadId: number, bookKey: string) {
  const b = props.book as Book | null
  if (!b || !b.bookUrl || isLocal.value) return
  try {
    const info = await getBookInfo(b.bookUrl, b.origin)
    if (!isCurrentDetail(loadId, bookKey)) return
    if (info?.coverUrl && !b.coverUrl) {
      detailCover.value = info.coverUrl
      coverFailed.value = false
    }
    if (info?.intro) detailIntro.value = info.intro
  } catch {
    // 详情页拉取失败就退回搜索结果里的简介，不打扰用户。
  }
}

function closeSourceSSE() {
  sourceSSE?.close()
  sourceSSE = null
}

/** 把本轮结果写回缓存。
 *
 *  - 完整搜完 → 直接用这一轮的列表（它已经是权威结果）。
 *  - 被打断/部分失败 → **与旧缓存合并**：用户刷新只是想补全，不是想丢掉
 *    已经看到的那些源。之前这里直接覆盖，于是「刷新显示 8 个、中途停止、
 *    再打开变回 4 个」。
 * 合并后标记 `complete = false`，下次打开能如实提示「部分书源访问失败」。
 */
function commitSourceCandidates(
  bookKey: string,
  books: SearchBook[],
  complete: boolean,
  previous: SearchBook[] = [],
) {
  const merged = mergeForCommit(previous, books, complete, mergeSourceCandidates)
  if (merged.length <= 1) return
  sourceCandidateCache.set(bookKey, sourceCatalogSignature(), merged, complete)
}

/** 填充「可读书源」列表。
 *
 *  `refresh = false`（打开弹窗）：读缓存，不发请求；若书源清单变过则自动重搜。
 *  `refresh = true`（用户点刷新 / 部分源失败后重试）：强制并发搜索。
 */
function loadSourceCandidates(loadId: number, bookKey: string, refresh = false) {
  const b = props.book
  if (!b || !b.name) {
    return
  }
  // Web 端整块书源列表都不渲染(见模板里的 `.source-section`), 但 watch 仍然
  // 会走到这里。不能只靠"看不见"—— `getAvailableBookSourceSSE` 底层是
  // `tauri::ipc::Channel`, 浏览器里根本没有, 发出去只会抛错或挂住。
  if (appStore.isWebMode) {
    sourcesLoading.value = false
    return
  }
  closeSourceSSE()
  sourcesIncomplete.value = false
  sourcesStopped.value = false

  const current = toSearchBook(b)
  const storedCandidates = b.sourceCandidates || []
  const initialCandidates = [current, ...storedCandidates]

  // 缓存按「书」存一条，签名只是记录当时用的是哪份书源清单。
  const cached = sourceCandidateCache.get(bookKey)
  const signature = sourceCatalogSignature()
  // 书源增删/停启用后，旧结果里可能有已删除源的条目，不可信 → 自动重搜。
  const catalogChanged = !!cached && !sourceCandidateCache.matches(cached, signature)
  // 过期但**仍然可用**：按设计「超时后仍可读，由调用方决定怎么提示」。
  // 直接丢掉会让用户以为之前搜到的源没了，所以照样摆出来，只是角标按过期时间
  // 显示（`sourcesCacheHint`），由用户自己决定要不要点刷新。
  const cacheExpired = !!cached && sourceCandidateCache.isStale(cached)
  // 上次**没搜完**（中途停止 / 部分源失败）→ 照样自动重搜一次。
  //
  // 这条很关键：`commitSourceCandidates` 无论完整与否都会写入**当前**签名，
  // 所以光比签名看不出「这份缓存其实只搜了一半」。用户刷新到一半退出，缓存里
  // 就是半份结果 + 与清单相符的签名 → 下次打开判成「没变」，永远不再补搜，
  // 表现正是「刷新中途退出后再打开加载的是旧缓存」。
  // 重搜时旧结果会作为 `previousBooks` 垫底，已看到的源不会丢。
  const cacheIncomplete = !!cached && !cached.complete

  if (!refresh && !catalogChanged && !cacheIncomplete) {
    if (cached) {
      if (!isCurrentDetail(loadId, bookKey)) return
      sourcesCachedAt.value = cached.savedAt
      sourcesCacheExpired.value = cacheExpired
      // 旧缓存里的候选可能包含已禁用/已删除的源；`setSourceCandidates` 会按
      // 当前 `sourceCatalog` 过滤，所以这里直接并进去即可。
      setSourceCandidates([...initialCandidates, ...cached.books])
      sourcesIncomplete.value = !cached.complete
      sourcesLoading.value = false
      return
    }
    // 没有缓存: 只摆出本书自己的源, 不发请求 —— 这正是 issue #28 第 2 点要的
    // 「点进详情别自动刷新」。用户要别的源就点标题右边的刷新按钮。
    sourcesCachedAt.value = 0
    sourcesCacheExpired.value = false
    setSourceCandidates(initialCandidates)
    sourcesLoading.value = false
    return
  }

  // 走到这里说明要真正发请求：用户点了刷新，或书源清单变了。
  sourcesLoading.value = true
  // 刷新时保留旧结果在界面上（下面 finish 时还会并进缓存），只是重新搜一遍；
  // 清空列表会让用户以为「之前搜到的源丢了」。
  const previousBooks = (cached?.books || []).filter((candidate) => (
    sourceCatalog.value.has(sourceKey(candidate.origin))
  ))
  setSourceCandidates([...initialCandidates, ...previousBooks])

  const requestPage = (lastIndex: number) => {
    const stream = getAvailableBookSourceSSE({
      url: b.bookUrl,
      name: b.name,
      author: b.author,
      origin: b.origin,
      lastIndex,
      concurrentCount: 12,
      // 缓存命中（书源清单变了）时不必带 refresh=1：后端的源列表缓存仍然有效，
      // 只是本地这份候选过期了，重跑一遍即可。
      refresh: refresh ? 1 : undefined,
    })
    sourceSSE = stream

    const isActive = () => sourceSSE === stream && isCurrentDetail(loadId, bookKey)
    const finish = (cacheResult: boolean) => {
      stream.close()
      if (!isActive()) return
      sourceSSE = null
      sourcesLoading.value = false
      commitSourceCandidates(
        bookKey,
        sourceCandidates.value.slice(),
        cacheResult,
        previousBooks,
      )
    }

    stream.onmessage = (event) => {
      if (!isActive()) {
        stream.close()
        return
      }
      const data = event.data as { data?: SearchBook[] }
      if (Array.isArray(data.data) && data.data.length > 0) {
        setSourceCandidates([...sourceCandidates.value, ...data.data])
      }
    }

    stream.addEventListener('end', (event) => {
      if (!isActive()) {
        stream.close()
        return
      }
      const payload = event.data as { hasMore?: boolean; lastIndex?: number; hasErrors?: boolean }
      sourcesIncomplete.value ||= payload.hasErrors === true
      if (
        payload.hasMore === true
        && typeof payload.lastIndex === 'number'
        && Number.isInteger(payload.lastIndex)
        && payload.lastIndex > lastIndex
      ) {
        stream.close()
        requestPage(payload.lastIndex)
        return
      }
      finish(payload.hasMore !== true && !sourcesIncomplete.value)
    })
    stream.onerror = () => {
      if (!isActive()) return
      sourcesIncomplete.value = true
      finish(false)
    }
  }

  requestPage(-1)
}

function toSearchBook(book: Book | SearchBook): SearchBook {
  return {
    name: book.name,
    author: book.author,
    bookUrl: book.bookUrl,
    origin: book.origin,
    coverUrl: book.coverUrl,
    intro: book.intro,
    kind: book.kind,
    lastChapter: (book as SearchBook).lastChapter || (book as Book).latestChapterTitle,
  }
}

function setSourceCandidates(candidates: SearchBook[]) {
  const previousSelection = selectedSource.value ? candidateKey(selectedSource.value) : ''
  const currentKey = props.book ? sourceCandidateKey(props.book) : ''
  const currentTitle = props.book?.name || ''
  sourceCandidates.value = mergeSourceCandidates(candidates.map((candidate) => ({
    ...candidate,
    origin: canonicalSourceOrigin(candidate.origin),
  })).filter((candidate) => (
    matchesSourceSwitchTitle(currentTitle, candidate.name)
    && sourceCatalog.value.has(sourceKey(candidate.origin))
  )))
  const selectedCandidate = selectedSource.value
  // 目录身份键与 `candidateKey()` 同源, 所以这里能真正判定"当前选中的源就是
  // 已经加载出目录的那个源"。判定成立时必须保留用户的选择, 不能被下面的
  // 回退分支(优先命中本书原源)覆盖 —— 否则 SSE 每追加一批候选就会把选中项
  // 拨回原源, 目录却仍是手选源的, 二者错位。
  selectedSource.value = (
    selectedCandidate
    && catalogTarget.value === candidateKey(selectedCandidate)
    && sourceCandidates.value.some((candidate) => (
      candidateKey(candidate) === catalogTarget.value
    ))
  )
    ? selectedCandidate
    : sourceCandidates.value.find((candidate) => (
    candidateKey(candidate) === currentKey
  )) || sourceCandidates.value.find((candidate) => (
    candidateKey(candidate) === previousSelection
  )) || sourceCandidates.value[0] || null
}

function selectSource(candidate: SearchBook) {
  if (!props.modelValue || !props.book) return
  if (
    sourceKey(selectedSource.value?.origin || '') === sourceKey(candidate.origin)
    && selectedSource.value?.bookUrl === candidate.bookUrl
  ) return
  selectedSource.value = candidate
  chapters.value = []
  showAllChapters.value = false
  chapterError.value = ''
  loadedCatalogIdentity.value = ''
  void loadChaptersFor(candidate.bookUrl, candidate.origin, detailLoadId, detailKey.value)
}

async function loadChaptersFor(
  bookUrl: string,
  origin: string,
  loadId = detailLoadId,
  bookKey = detailKey.value,
) {
  const requestId = ++chapterLoadId
  const sourceIdentity = catalogIdentityKey(origin, bookUrl)
  chaptersLoading.value = true
  catalogTarget.value = sourceIdentity
  chapterError.value = ''
  try {
    const nextChapters = await getChapterList({
      bookUrl,
      bookSourceUrl: origin,
      // 空串目录地址不能传：后端会当成"没有"并回落到 bookUrl 去取详情。
      // （从手机端导入的书 tocUrl 常常是空串。）
      tocUrl: (origin === sourceKey(props.book?.origin || '')
        ? (props.book as Book | null)?.tocUrl?.trim()
        : undefined) || undefined,
    })
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = nextChapters
    loadedCatalogIdentity.value = sourceIdentity
  } catch {
    if (!isCurrentDetail(loadId, bookKey) || requestId !== chapterLoadId) return
    chapters.value = []
    loadedCatalogIdentity.value = ''
    chapterError.value = '目录加载失败，请重试或切换书源'
  } finally {
    if (requestId === chapterLoadId) chaptersLoading.value = false
  }
}

function retryChapters() {
  const source = selectedSource.value
  if (!source || !props.book) return
  void loadChaptersFor(source.bookUrl, source.origin, detailLoadId, detailKey.value)
}

/** 重搜「可读书源」列表（对齐手机端详情页的刷新）。
 *
 *  现在打开详情页**不再自动搜源**（issue #28 第 2 点），这里是用户主动要最新
 *  结果的唯一入口，也是「部分书源访问失败」时的重试入口。
 *
 *  只重跑书源搜索，不动书籍详情与目录：
 *  - `refresh = true` 会丢掉这本书记住的候选并带 `refresh: 1` 请求后端，
 *    否则后端直接把上次的结果原样吐回来，用户点了"刷新"看不出任何变化；
 *  - 列表会先回落到「当前源 + 书架已保存的候选」，再由新结果逐个追加 ——
 *    刷新本来就该丢掉上次那批可能已失效的源，留着它们等于没刷；
 *  - 单源模式（发现页的书）本来就不搜其它源，按钮不渲染 —— 与"重试"的取舍一致。 */
function refreshSourceCandidates() {
  if (sourcesLoading.value || props.singleSourceOnly) return
  loadSourceCandidates(detailLoadId, detailKey.value, true)
}

/** 中止「可读书源」搜索（对齐手机端换源弹窗的停止、以及手机端详情页的取消）。
 *
 *  关键是 `closeSourceSSE()`：`openSse().close()` 在未收到 end/error 时会向
 *  后端发 `cancel_book_search`，取消标志被置位后服务端会 abort 在途请求，
 *  而不是让剩下几十个源继续跑完（那些请求会占着书源限速槽位，拖慢随后的
 *  换源和阅读）。
 *
 *  状态复位必须自己做：`sourcesLoading` 只在 SSE 的 end/error 回调里清，
 *  流被本地关掉后不会再触发，留着它按钮会永久停在"停止"态。
 *  已拿到的候选保留 —— 用户中途停止想要的正是「就这些，别再等了」；
 *  `hasMoreSources` 没有独立变量，分页链靠 `sourceSSE` 是否仍是当前流来续，
 *  关流即断，不会再自动请求下一页。 */
function stopSourceCandidates() {
  closeSourceSSE()
  sourcesLoading.value = false
  // 主动停止不等于"部分源访问失败"：别让底部弹出「重试」提示。
  sourcesIncomplete.value = false
  sourcesStopped.value = true
  // **中断也要落缓存**：用户刷新到 8 个源、中途点停止，下次打开必须还是这 8 个。
  // 之前这里什么都不写，缓存停留在刷新前的 4 个，于是「再打开变回 4 个」。
  // `complete = false` 让下次打开仍能看到「部分书源访问失败 → 重试」。
  const bookKey = detailKey.value
  if (bookKey) {
    commitSourceCandidates(
      bookKey,
      sourceCandidates.value.slice(),
      false,
      sourceCandidateCache.get(bookKey)?.books || [],
    )
    sourcesCachedAt.value = sourceCandidateCache.get(bookKey)?.savedAt || Date.now()
    // 刚落库的结果一定是最新的，把可能残留的过期标记清掉。
    sourcesCacheExpired.value = false
  }
  // 搜索可能一个结果都还没回来，界面上原本只有一行转圈；给一个明确回执，
  // 否则用户分不清是"停了"还是"卡住没动"。
  const found = Math.max(0, sourceCandidates.value.length - 1)
  appStore.showToast(found > 0 ? `已停止搜索，保留 ${found} 个其他书源` : '已停止搜索书源', 'success')
}

/** 已经由「打开弹窗」这条路径处理过的书源版本号。
 *
 *  打开弹窗时会主动 `fetchSources()` 刷新清单，如果这期间书源状态确实变了，
 *  store 会把 `availabilityVersion` 加一，从而又触发下面的 availabilityVersion
 *  watch —— 两次都搜一遍就重复了。这里记下「我刚处理到的版本」，让那个 watch
 *  跳过同一次变化。 */
let handledAvailabilityVersion = -1

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

  // 详情页的简介比搜索结果的摘要完整，先补拉（不阻塞目录加载）。
  void loadDetailIntro(loadId, bookKey)

  // **每次打开都重新读一遍书源清单**，不能只在为空时读。
  // 缓存是否可信是靠「书源清单签名」判断的，如果沿用一份过期的 store，
  // 用户刚启用的新书源不会出现在签名里 → 被判成「书源没变」→ 不自动刷新。
  // 这正是「启动新书源后打开详情页也不刷新」的原因。读的是本地库，开销可忽略。
  await sourceStore.fetchSources().catch(() => undefined)
  // 这次读清单引起的版本变化由本 watch 自己处理（下面 loadSourceCandidates
  // 会按新签名判断要不要搜），别再让 availabilityVersion 的 watch 搜第二遍。
  handledAvailabilityVersion = sourceStore.availabilityVersion
  if (!isCurrentDetail(loadId, bookKey)) return

  // 只认本书自身的书源：发现页的书只有聚合源认得，并发搜 12 路既慢又搜不到，
  // 结果就是用户看到的「只有一个源」。这里直接跳过搜索，仍要加载目录。
  if (props.singleSourceOnly) {
    const name = (b as Book).originName
      || sourceStore.sources.find((s) => sourceKey(s.bookSourceUrl) === sourceKey(b.origin))?.bookSourceName
      || sourceStore.sources.find((s) => s.bookSourceName === b.origin)?.bookSourceName
      || b.origin
    sourceCatalog.value = new Map([[sourceKey(b.origin), name]])
    setSourceCandidates([toSearchBook(b)])
    sourcesLoading.value = false
    await loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey)
    return
  }

  sourceCatalog.value = new Map(
    sourceStore.sources
      .filter((source) => source.enabled !== false)
      .map((source) => [sourceKey(source.bookSourceUrl), source.bookSourceName]),
  )
  if (!sourceCatalog.value.has(sourceKey(b.origin))) {
    // 保留当前书籍自身的源，避免书源列表加载失败时详情页完全没有可选项。
    sourceCatalog.value.set(sourceKey(b.origin), (b as Book).originName || b.origin)
  }
  void loadSourceCandidates(loadId, bookKey)
  await loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey)
}, { immediate: true })

watch(() => sourceStore.availabilityVersion, async () => {
  if (!props.modelValue || !props.book || isLocal.value) return
  // 打开弹窗时自己读清单引起的那次版本变化，上面那个 watch 已经处理过了。
  if (sourceStore.availabilityVersion === handledAvailabilityVersion) return
  // 书源增删/停启用后：缓存里的候选可能属于已被删除（或禁用）的源，必须重搜。
  // 注意**不要**在这里整份清空缓存 —— 清空等于把「用户刷出来但还没搜完」的
  // 结果也一起丢掉。让签名对不上、走 `catalogChanged` 分支即可：那里会保留
  // 旧结果（按当前清单过滤）并自动发一次请求。
  const loadId = ++detailLoadId
  chapterLoadId += 1
  const bookKey = detailKey.value
  resetDetailState()
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
  // 这里传 `false`（不是 true）：让 `loadSourceCandidates` 自己根据签名判断
  // 该不该搜、要不要带 `refresh: 1`。书源清单确实变了 → catalogChanged 为真
  // → 自动重搜；否则就是一次普通打开，不该发请求。
  void loadSourceCandidates(loadId, bookKey)
  await loadChaptersFor(b.bookUrl, b.origin, loadId, bookKey)
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
  // 简介的展开状态同理: 换书后回到收起态。
  introExpanded.value = false
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
  // shelfMatchKey 会区分本地书 / 网络书：本地导入的《遮天》不能让详情页里的
  // 网络《遮天》被当成「已在书架」，否则会走换源持久化那条分支改错记录。
  const identity = shelfMatchKey(b)
  return shelfStore.books.find((item) => (
    item.bookUrl === b.bookUrl || shelfMatchKey(item) === identity
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
  cursor: pointer;
}

/* 书源的 `intro` 通常是一整块信息(最新更新/点击/评分/简介), 收起态只留 5 行
   —— 详情页里简介是"扫一眼判断这本书是什么"的配角, 给太多行会把目录和
   书源列表推到很下面。要看全文点「展开全部」。 */
.book-intro p.collapsed {
  display: -webkit-box;
  -webkit-line-clamp: 5;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.book-intro .intro-toggle {
  margin-top: var(--space-1, 4px);
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--color-primary, #c97f3a);
  font-size: var(--text-xs, 11px);
  cursor: pointer;
  opacity: 0.8;
}

.book-intro .intro-toggle:hover {
  opacity: 1;
}

.source-section {
  margin-bottom: var(--space-6);
}

.source-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
  margin: 0;
}

.section-heading {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.section-heading h3 {
  flex: 0 0 auto;
}

/* 「缓存结果 / 未搜索」角标：解释列表里为什么只有一条。 */
.section-hint {
  flex: 0 0 auto;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.6;
}

/* 「可读书源」标题右侧的刷新/停止按钮：与手机端详情页工具栏图标同一语义。
   空闲时是刷新图标；搜索中换成停止图标，点了能立刻中止，不必等所有源超时。 */
.section-refresh {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-width: 24px;
  height: 24px;
  /* 现在打开详情不再自动搜源, 这个按钮是唯一入口, 所以靠右对齐、常驻可见。 */
  margin-left: auto;
  padding: 0;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  opacity: 0.65;
  transition: opacity 0.2s ease, background 0.2s ease;
}

.section-refresh svg {
  width: 15px;
  height: 15px;
}

.section-refresh:hover:not(:disabled) {
  opacity: 1;
  background: var(--color-bg-secondary, rgba(0, 0, 0, 0.05));
}

.section-refresh:disabled {
  cursor: default;
  opacity: 0.45;
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

/* 列表下方的状态条（追加中 / 部分源失败 / 已停止）。
   放在列表外面而不是里面: 塞进列表里会从「5 行」里再吃掉一行, 用户实际只
   看到 4 个书源。放在外面时它只把下面的目录往下推 30px 左右, 列表本身仍
   稳定显示 5 条。 */
.source-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: var(--space-2);
  padding: 5px 6px 5px 10px;
  border-radius: var(--radius-sm);
  background: var(--color-bg-sunken);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.4;
}

.source-foot.warn {
  color: var(--color-text-secondary);
}

.source-foot .incomplete-icon {
  flex: 0 0 auto;
  width: 14px;
  height: 14px;
  color: var(--color-primary);
  opacity: 0.85;
}

.source-foot .incomplete-text {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.source-foot .incomplete-retry {
  flex: 0 0 auto;
  padding: 2px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-primary-border);
  background: var(--color-primary-bg);
  color: var(--color-text);
  font-size: var(--text-xs);
  font-weight: 500;
  cursor: pointer;
  transition: background 0.16s ease, border-color 0.16s ease;
}

.source-foot .incomplete-retry:hover {
  background: var(--color-selected-bg-hover);
  border-color: var(--color-primary);
}

.source-foot .loading-spinner {
  width: 12px;
  height: 12px;
  border-width: 1.5px;
}

/* 「可读书源」列表: 高度固定成正好 5 条, 源再多也在框内滚动（issue #28 第 3 点）。
   之前列表随搜索逐条变高, 每多一个源就把下面的目录往下推一截, 想看目录得滚很久。
   5 条是取舍: 再少就得多滚列表, 再多会把目录挤出可视区。

   行高必须是确定的 —— 所以行用 `height` 而不是 padding 撑开, 并且作者名(小字)
   从「第二行」改成跟在书名后面同一行, 否则一条书源会占两行高, 「5 条」就不再
   是 5 条。`max-height` 而不是 `height`: 只有两三个源时不该留一块空白。 */
.source-list {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-md);
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  /* +2px 是列表自身的上下边框 */
  max-height: calc(var(--source-row-height, 38px) * 5 + 2px);
}

.source-item {
  box-sizing: border-box;
  height: var(--source-row-height, 38px);
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 0 var(--space-3);
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

.source-author {
  margin-left: 4px;
  font-size: var(--text-xs);
  font-weight: 400;
  color: var(--color-text-tertiary);
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

.chapter-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-bottom: var(--space-3);
}

.chapter-heading h3 {
  min-width: 0;
  margin: 0;
}

.chapter-source-status {
  min-width: 0;
  max-width: 55%;
  padding: 2px var(--space-2);
  overflow: hidden;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  font-size: var(--text-xs);
  font-weight: 500;
  line-height: 1.5;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chapter-source-status.error {
  border-color: color-mix(in srgb, var(--color-danger) 35%, var(--color-border));
  color: var(--color-danger);
}

.chapter-error-message {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-danger) 7%, transparent);
  color: var(--color-text-secondary);
  font-size: var(--text-sm);
}

.chapter-retry-btn {
  padding: 2px var(--space-2);
  border: 1px solid currentColor;
  border-radius: var(--radius-sm);
  color: var(--color-danger);
  background: transparent;
  font-size: var(--text-xs);
}

.chapter-section h3 {
  font-size: var(--text-base);
  font-weight: 600;
}

.chapter-list {
  max-height: 240px;
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

.chapter-loading-state {
  margin-top: var(--space-4);
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
  background: var(--color-primary-soft-bg);
  color: var(--color-primary-soft-text);
  border-color: var(--color-primary-soft-border);
}

.action-btn.primary:hover {
  background: var(--color-primary-soft-bg-hover);
  border-color: var(--color-primary);
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
