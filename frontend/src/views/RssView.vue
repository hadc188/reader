<template>
  <div class="rss-view">
    <header class="rss-toolbar">
      <div class="toolbar-top">
        <div class="toolbar-top-left">
          <span v-if="store.activeSource?.sourceGroup" class="meta-chip">{{ store.activeSource.sourceGroup }}</span>
          <label class="source-select">
            <span>当前源</span>
            <select v-model="store.activeSourceUrl" @change="handleReaderSourceChange" v-if="store.enabledSources.length">
              <option v-for="source in store.enabledSources" :key="source.sourceUrl" :value="source.sourceUrl">
                {{ source.sourceName }}
              </option>
            </select>
          </label>
        </div>
        <div class="rss-hero-actions">
          <button class="ghost-btn icon-btn" @click="goManage" aria-label="管理订阅源" title="管理订阅源">
            <span class="btn-icon">⚙</span>
            <span class="btn-text">管理订阅源</span>
          </button>
          <button
            class="ghost-btn icon-btn"
            :disabled="!store.enabledSources.length"
            @click="store.fetchArticles(true)"
            aria-label="刷新文章"
            title="刷新文章"
          >
            <span class="btn-icon">↻</span>
            <span class="btn-text">刷新文章</span>
          </button>
        </div>
      </div>

      <div class="toolbar-middle" v-if="store.enabledSources.length">
        <section class="rss-scope-bar" v-if="store.enabledSources.length">
          <button
            class="scope-chip"
            :class="{ active: store.articleScope === 'source' }"
            @click="store.setSource(store.activeSourceUrl)"
          >
            当前源
          </button>
          <button
            class="scope-chip"
            :class="{ active: store.articleScope === 'all' }"
            @click="store.setAllSources()"
          >
            全部文章
          </button>
          <button
            v-for="group in store.groupNames"
            :key="group"
            class="scope-chip"
            :class="{ active: store.articleScope === 'group' && store.activeGroupName === group }"
            @click="store.setGroup(group)"
          >
            {{ group }}
          </button>
        </section>
      </div>
    </header>

    <section
      class="rss-main"
      :class="{
        'has-active-article': !!store.activeArticle,
      }"
    >
      <aside class="article-list-panel">
        <div class="panel-head">
          <h2>文章列表</h2>
          <span>{{ store.articles.length }}</span>
        </div>
        <div class="panel-scroll article-list-scroll">
          <div v-if="!store.sources.length" class="empty-box">还没有 RSS 源，先去管理页添加。</div>
          <div v-else-if="!store.activeSourceUrl" class="empty-box">请选择一个 RSS 源。</div>
          <template v-else>
            <div class="article-list-stack">
            <button
              v-for="article in store.articles"
              :key="`${article.variable || 'rss'}-${article.link}`"
              class="article-item"
              :class="{ active: store.activeArticle?.link === article.link && store.activeArticle?.variable === article.variable }"
              @click="handleOpenArticle(article)"
            >
              <div class="article-title">{{ article.title || '无标题' }}</div>
              <div class="article-meta-line">
                <span>{{ formatRelativeTime(article.pubDate) || '暂无时间' }}</span>
                <span v-if="article.origin" class="meta-sep">·</span>
                <span v-if="article.origin">{{ article.origin }}</span>
              </div>
              <div v-if="article.description" class="article-desc">{{ toPlainPreview(article.description) }}</div>
            </button>
            </div>
            <button v-if="store.hasMore && !store.loading" class="load-more-btn" @click="store.fetchArticles()">加载更多</button>
            <div v-if="store.loading" class="empty-box loading-box">文章加载中...</div>
          </template>
        </div>
      </aside>

      <article v-if="!isMobileLayout" class="article-content-panel" :class="{ collapsed: !store.activeArticle }">
        <template v-if="store.activeArticle">
          <div class="panel-head">
            <div class="content-head-main">
              <h2>{{ store.activeArticle.title || '正文' }}</h2>
              <div class="content-head-meta">
                <div v-if="store.activeArticle.pubDate" class="content-head-time">
                  {{ formatRelativeTime(store.activeArticle.pubDate) }}
                </div>
                <div v-if="store.activeArticle.origin" class="content-head-origin">{{ store.activeArticle.origin }}</div>
              </div>
            </div>
          </div>
          <div class="panel-scroll content-scroll">
            <div v-if="store.contentLoading" class="empty-box">正文加载中...</div>
            <div v-else-if="store.activeContent" class="content-html" v-html="store.activeContent"></div>
            <div v-else class="empty-box">这篇文章暂时没有可显示的正文。</div>
          </div>
        </template>
        <div v-else class="content-placeholder">
          <div class="content-placeholder-title">选择一篇文章开始阅读</div>
          <div class="content-placeholder-text">正文区会在你点开文章后展开显示。</div>
        </div>
      </article>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useRssStore } from '../stores/rss'
import type { RssArticle } from '../types'

const router = useRouter()
const store = useRssStore()
const isMobileLayout = ref(false)

onMounted(async () => {
  syncViewportMode()
  window.addEventListener('resize', syncViewportMode)
  await store.fetchSources()
  if (!store.activeSourceUrl && store.enabledSources.length) {
    await store.setSource(store.enabledSources[0]!.sourceUrl)
    return
  }
  if (store.activeSourceUrl && store.articles.length === 0) {
    await store.fetchArticles(true)
  }
  if (isMobileLayout.value) {
    store.activeArticle = null
    store.activeContent = ''
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', syncViewportMode)
})

function goManage() {
  router.push('/rss/manage')
}

function syncViewportMode() {
  isMobileLayout.value = window.innerWidth <= 960
}

function toPlainPreview(html: string) {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/\s+/g, ' ')
    .trim()
}

function formatRelativeTime(value?: string) {
  if (!value) return ''
  const timestamp = Date.parse(value)
  if (Number.isNaN(timestamp)) return value

  const diff = Date.now() - timestamp
  if (diff < 0) return '刚刚'

  const minute = 60 * 1000
  const hour = 60 * minute
  const day = 24 * hour
  const month = 30 * day
  const year = 365 * day

  if (diff < minute) return '刚刚'
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`
  if (diff < month) return `${Math.floor(diff / day)} 天前`
  if (diff < year) return `${Math.floor(diff / month)} 个月前`
  return `${Math.floor(diff / year)} 年前`
}

async function handleReaderSourceChange() {
  if (store.activeSourceUrl) {
    await store.setSource(store.activeSourceUrl)
  }
}

async function handleOpenArticle(article: RssArticle & { variable?: string }) {
  if (isMobileLayout.value) {
    await router.push({
      name: 'rss-article',
      query: {
        source: article.variable || store.activeSourceUrl,
        link: article.link,
        title: article.title || '',
        pubDate: article.pubDate || '',
        origin: article.origin || '',
      },
    })
    return
  }
  await store.openArticle(article)
}
</script>

<style scoped>
.rss-view {
  height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
  min-height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  overflow: hidden;
}

.rss-toolbar,
.article-list-panel,
.article-content-panel {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-light);
  border-radius: 24px;
}

.rss-toolbar {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 6px 10px;
}

.toolbar-top,
.toolbar-middle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-wrap: wrap;
}

.toolbar-top-left {
  display: flex;
  align-items: center;
  min-height: 24px;
  gap: 6px;
  flex: 1;
  min-width: 0;
}

.rss-hero-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.btn-icon {
  font-size: 14px;
  line-height: 1;
}

.btn-text {
  line-height: 1;
}

.source-select {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
  flex: 0 0 auto;
  min-width: 0;
  font-size: 13px;
}

.source-select select {
  width: 18ch;
  min-width: 10ch;
  max-width: 18ch;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  border-radius: 12px;
  padding: 4px 8px;
}

.meta-chip {
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(201, 127, 58, 0.12);
  color: var(--color-primary);
  font-size: 10px;
}

.meta-text {
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.rss-scope-bar {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-top: 0;
  width: 100%;
  justify-content: flex-start;
}

.scope-chip,
.load-more-btn,
.ghost-btn {
  padding: 4px 9px;
  border-radius: 999px;
  border: 1px solid var(--color-border-light);
  background: var(--color-bg);
  transition: all var(--duration-fast) var(--ease-out);
  white-space: nowrap;
  font-size: 12px;
}

.scope-chip.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.1);
  color: var(--color-primary);
}

.load-more-btn:hover,
.ghost-btn:hover:not(:disabled),
.scope-chip:hover {
  border-color: var(--color-primary-border);
  color: var(--color-primary);
}

.rss-main {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 360px minmax(0, 1fr);
  gap: 8px;
  overflow: hidden;
}

.article-list-panel,
.article-content-panel {
  min-height: 0;
  overflow: hidden;
  padding: 12px;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  height: 100%;
  box-sizing: border-box;
}

.article-content-panel.collapsed {
  display: flex;
  align-items: center;
  justify-content: center;
}

.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.panel-head h2 {
  margin: 0;
  font-size: 16px;
}

.panel-head span {
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.panel-scroll {
  min-height: 0;
  height: 100%;
  overflow: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
}

.article-list-scroll {
  padding-right: 4px;
}

.article-list-stack {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.content-scroll {
  padding-right: 6px;
}

.content-head-main {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.content-head-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.content-head-time,
.content-head-origin {
  font-size: 12px;
  color: var(--color-text-tertiary);
}

.content-placeholder {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
  color: var(--color-text-tertiary);
}

.content-placeholder-title {
  font-size: 20px;
  font-weight: 700;
  color: var(--color-text-primary);
}

.content-placeholder-text {
  font-size: 14px;
}

.article-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 14px;
  border-radius: 18px;
  border: 1px solid transparent;
  background: var(--color-bg);
  transition: all var(--duration-fast) var(--ease-out);
  flex: 0 0 auto;
}

.article-item:hover {
  border-color: var(--color-primary-border);
  transform: translateY(-1px);
}

.article-item.active {
  border-color: rgba(201, 127, 58, 0.26);
  background: rgba(201, 127, 58, 0.08);
}

.article-title {
  font-weight: 600;
  line-height: 1.45;
}

.article-meta-line {
  margin-top: 6px;
  font-size: 12px;
  color: var(--color-text-tertiary);
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.meta-sep {
  opacity: 0.5;
}

.article-desc {
  margin-top: 8px;
  font-size: 13px;
  color: var(--color-text-secondary);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.content-html {
  line-height: 1.85;
  color: var(--color-text);
  max-width: 100%;
  overflow-wrap: anywhere;
  width: min(760px, 100%);
  margin: 0 auto;
}

.content-html :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 14px;
}

.content-html :deep(video),
.content-html :deep(iframe),
.content-html :deep(table),
.content-html :deep(pre),
.content-html :deep(code) {
  max-width: 100%;
}

.content-html :deep(pre) {
  overflow: auto;
}

.empty-box {
  min-height: 140px;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  color: var(--color-text-tertiary);
}

.loading-box {
  min-height: 72px;
}

@media (max-width: 960px) {
  .rss-view {
    height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
    min-height: calc(var(--app-height, 100dvh) - var(--header-height) - 104px - var(--safe-area-top) - var(--safe-area-bottom));
    padding: 6px;
    gap: 4px;
    overflow: hidden;
  }

  .rss-main {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .toolbar-top,
  .toolbar-middle {
    align-items: flex-start;
  }

  .article-list-panel {
    flex: 1;
    min-height: 0;
    max-height: none;
  }

  .article-list-panel,
  .article-content-panel {
    padding: 8px;
  }

  .content-html {
    width: 100%;
  }
}

@media (max-width: 640px) {
  .rss-view {
    padding: 4px;
    gap: 3px;
  }

  .rss-toolbar {
    padding: 5px 7px;
    gap: 4px;
  }

  .rss-hero-actions {
    margin-left: auto;
  }

  .toolbar-top {
    align-items: center;
  }

  .toolbar-top-left {
    flex: 1;
    min-width: 0;
  }

  .source-select {
    align-items: center;
    flex-direction: row;
    gap: 6px;
  }

  .source-select select {
    width: 18ch;
    min-width: 8ch;
    max-width: 18ch;
  }

  .rss-main {
    gap: 8px;
    flex: 1;
    min-height: 0;
  }

  .article-list-panel,
  .article-content-panel {
    padding: 8px;
    border-radius: 20px;
  }

  .panel-head {
    margin-bottom: 8px;
  }

  .article-item {
    padding: 12px;
    border-radius: 16px;
  }

  .article-list-stack {
    gap: 8px;
  }

  .article-title {
    font-size: 15px;
  }

  .article-desc {
    -webkit-line-clamp: 1;
  }

  .article-list-panel {
    flex: 1;
    min-height: 0;
    max-height: none;
  }

  .btn-text {
    display: none;
  }

  .icon-btn {
    padding-left: 8px;
    padding-right: 8px;
  }
}
</style>
