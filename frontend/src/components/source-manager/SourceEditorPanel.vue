<template>
  <section class="editor-panel">
    <header class="editor-head">
      <div class="editor-title">
        <h3>{{ source ? source.bookSourceName : '书源工作区' }}</h3>
        <p>{{ source ? source.bookSourceUrl : '选择一个书源查看详情，或新建书源' }}</p>
      </div>
      <div class="editor-actions">
        <button class="mini-btn" type="button" @click="$emit('format')">格式化</button>
        <button class="mini-btn primary" type="button" @click="$emit('save')">保存</button>
      </div>
    </header>

    <nav class="editor-tabs" aria-label="书源编辑标签">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="tab-btn"
        :class="{ active: activeTab === tab.key }"
        type="button"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
      </button>
    </nav>

    <div v-if="activeTab === 'overview'" class="overview-pane">
      <template v-if="overview && source">
        <div class="overview-grid">
          <div class="overview-card">
            <span>状态</span>
            <strong>{{ overview.statusText }}</strong>
          </div>
          <div class="overview-card">
            <span>分组</span>
            <strong>{{ overview.group }}</strong>
          </div>
          <div class="overview-card">
            <span>发现</span>
            <strong>{{ overview.exploreText }}</strong>
          </div>
          <div class="overview-card">
            <span>Cookie</span>
            <strong>{{ overview.cookieText }}</strong>
          </div>
        </div>

        <div class="rule-section">
          <h4>规则覆盖</h4>
          <div class="rule-chips">
            <span :class="{ active: overview.hasSearch }">搜索</span>
            <span :class="{ active: overview.hasExplore }">发现</span>
            <span :class="{ active: overview.hasBookInfo }">详情</span>
            <span :class="{ active: overview.hasToc }">目录</span>
            <span :class="{ active: overview.hasContent }">正文</span>
            <span :class="{ active: overview.hasLogin }">登录</span>
          </div>
        </div>

        <div class="source-url-block">
          <span>书源 URL</span>
          <p>{{ source.bookSourceUrl }}</p>
        </div>
      </template>
      <div v-else class="editor-empty">
        <strong>还没有选中书源</strong>
        <span>可以从左侧列表选择，也可以新建或导入书源。</span>
        <div class="empty-actions">
          <button class="mini-btn primary" type="button" @click="$emit('create')">新增书源</button>
          <button class="mini-btn" type="button" @click="$emit('import-local')">本地导入</button>
        </div>
      </div>
    </div>

    <div v-else-if="activeTab === 'json'" class="json-pane">
      <textarea
        :value="editorText"
        class="editor-textarea"
        spellcheck="false"
        @input="$emit('update:editorText', ($event.target as HTMLTextAreaElement).value)"
      ></textarea>
    </div>

    <div v-else-if="activeTab === 'visual'" class="visual-pane">
      <SourceRuleEditor
        :source="source"
        :editor-text="editorText"
        @update:editor-text="$emit('update:editorText', $event)"
      />
    </div>

    <div v-else-if="activeTab === 'login'" class="login-pane">
      <div class="login-card">
        <h4>书源登录调试</h4>
        <p v-if="canLogin">当前 JSON 包含 `bookSourceUrl` 和 `loginUrl`，可以打开代理登录预览。</p>
        <p v-else>当前书源未配置 `loginUrl`，或 JSON 暂时无法解析。</p>
        <button class="mini-btn primary" type="button" :disabled="!canLogin || loginLoading" @click="$emit('login')">
          {{ loginLoading ? '登录中...' : '打开登录页' }}
        </button>
      </div>
    </div>

    <div v-else class="debug-pane">
      <SourceDebugger :source="source" :editor-text="editorText" />
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { BookSource } from '../../types'
import { getBookSourceOverview } from '../../utils/sourceSelection'
import SourceDebugger from './SourceDebugger.vue'
import SourceRuleEditor from './SourceRuleEditor.vue'

type TabKey = 'overview' | 'visual' | 'json' | 'login' | 'debug'

const props = defineProps<{
  source: BookSource | null
  editorText: string
  canLogin: boolean
  loginLoading: boolean
  /** 每次 +1 触发面板切到可视化编辑 tab（用于新建书源）。 */
  resetSignal?: number
}>()

defineEmits<{
  'update:editorText': [value: string]
  format: []
  save: []
  login: []
  create: []
  'import-local': []
}>()

const activeTab = ref<TabKey>('overview')
const tabs: { key: TabKey; label: string }[] = [
  { key: 'overview', label: '概览' },
  { key: 'visual', label: '可视化编辑' },
  { key: 'json', label: 'JSON' },
  { key: 'debug', label: '调试' },
  { key: 'login', label: '登录调试' },
]

const overview = computed(() => getBookSourceOverview(props.source))

// 新建书源（source 为空）时切到可视化编辑 tab，避免停在空态概览让人以为没反应。
watch(
  () => props.resetSignal,
  () => {
    if (!props.source && props.resetSignal) {
      activeTab.value = 'visual'
    }
  },
)
</script>

<style scoped>
.editor-panel {
  min-height: 0;
  border: 1px solid var(--color-border-light);
  border-radius: 14px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
}

.editor-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border-bottom: 1px solid var(--color-border-light);
}

.editor-title {
  min-width: 0;
}

.editor-title h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.editor-title p {
  margin-top: 4px;
  color: var(--color-text-tertiary);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.editor-actions,
.empty-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.editor-tabs {
  display: flex;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--color-border-light);
  background: var(--color-bg);
}

.tab-btn {
  min-height: 30px;
  padding: 0 12px;
  border-radius: var(--radius-full);
  color: var(--color-text-secondary);
  font-size: 13px;
}

.tab-btn.active {
  background: var(--color-primary);
  color: #fff;
}

.overview-pane,
.json-pane,
.login-pane,
.debug-pane,
.visual-pane {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.debug-pane,
.visual-pane {
  padding: 16px;
}

.overview-pane,
.login-pane {
  padding: 16px;
}

.overview-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.overview-card,
.login-card,
.source-url-block {
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-lg);
  background: var(--color-bg);
}

.overview-card {
  padding: 12px;
}

.overview-card span,
.source-url-block span {
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.overview-card strong {
  display: block;
  margin-top: 6px;
  font-size: 15px;
}

.rule-section {
  margin-top: 16px;
}

.rule-section h4,
.login-card h4 {
  margin: 0 0 10px;
  font-size: 14px;
}

.rule-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.rule-chips span {
  padding: 5px 9px;
  border-radius: var(--radius-full);
  background: var(--color-bg-hover);
  color: var(--color-text-tertiary);
  font-size: 12px;
}

.rule-chips span.active {
  background: var(--color-primary-bg);
  color: var(--color-primary-dark);
}

.source-url-block {
  margin-top: 16px;
  padding: 12px;
}

.source-url-block p {
  margin-top: 6px;
  color: var(--color-text-secondary);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.editor-textarea {
  width: 100%;
  min-height: 100%;
  resize: none;
  border: none;
  outline: none;
  padding: 16px;
  background: #181614;
  color: #f4ede4;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.55;
}

.editor-empty {
  min-height: 260px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
  color: var(--color-text-tertiary);
}

.editor-empty strong {
  color: var(--color-text-secondary);
}

.login-card {
  padding: 16px;
}

.login-card p {
  margin-bottom: 14px;
  color: var(--color-text-secondary);
  font-size: 13px;
}

.mini-btn {
  min-height: 32px;
  padding: 0 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  font-size: 12px;
  white-space: nowrap;
  transition: all var(--duration-fast);
}

.mini-btn.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.mini-btn:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.mini-btn:hover:not(:disabled) {
  background: var(--color-bg-hover);
}

.mini-btn.primary:hover:not(:disabled) {
  background: var(--color-primary-dark);
}

@media (max-width: 560px) {
  .editor-head {
    flex-direction: column;
  }

  .editor-actions {
    width: 100%;
  }

  .overview-grid {
    grid-template-columns: 1fr;
  }
}
</style>
