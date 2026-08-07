<template>
  <div class="rule-editor">
    <div class="rule-group-tabs">
      <button
        v-for="g in groups"
        :key="g.key"
        class="rule-tab"
        :class="{ active: activeGroup === g.key }"
        type="button"
        @click="activeGroup = g.key"
      >
        {{ g.label }}
      </button>
    </div>

    <div class="rule-fields">
      <template v-for="field in activeFields" :key="field.id">
        <label class="field">
          <span class="field-label">
            {{ field.title }}
            <small v-if="field.hint" class="field-hint">{{ field.hint }}</small>
          </span>

          <select
            v-if="field.type === 'select'"
            class="field-input"
            :value="fieldValue(field)"
            @change="setField(field, ($event.target as HTMLSelectElement).value)"
          >
            <option v-for="opt in field.options" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>

          <input
            v-else-if="field.type === 'number'"
            class="field-input"
            type="number"
            :value="fieldValue(field)"
            @input="setField(field, ($event.target as HTMLInputElement).value)"
          />

          <div v-else-if="field.type === 'boolean'" class="field-toggle">
            <button
              class="toggle-btn"
              :class="{ on: fieldValue(field) === 'true' }"
              type="button"
              @click="setField(field, fieldValue(field) === 'true' ? 'false' : 'true')"
            >
              {{ fieldValue(field) === 'true' ? '开启' : '关闭' }}
            </button>
          </div>

          <textarea
            v-else
            class="field-input field-textarea"
            :value="fieldValue(field)"
            :placeholder="field.placeholder"
            rows="field.rows || 2"
            spellcheck="false"
            @input="setField(field, ($event.target as HTMLTextAreaElement).value)"
          ></textarea>
        </label>
      </template>

      <p v-if="!activeFields.length" class="rule-empty">
        当前书源未配置该组规则，可直接在下方的 JSON 中填写。
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import type { BookSource } from '../../types'

const props = defineProps<{
  source: BookSource | null
  editorText: string
}>()

const emit = defineEmits<{
  'update:editorText': [value: string]
}>()

type GroupKey = 'base' | 'search' | 'find' | 'detail' | 'directory' | 'content' | 'other'

interface FieldDef {
  id: string
  title: string
  /** 点分路径，如 `ruleSearch.name`；顶层字段就是字段名。 */
  path: string
  type?: 'string' | 'number' | 'boolean' | 'select'
  hint?: string
  placeholder?: string
  rows?: number
  options?: { value: string; label: string }[]
}

interface GroupDef {
  key: GroupKey
  label: string
  fields: FieldDef[]
}

const groups: GroupDef[] = [
  {
    key: 'base',
    label: '基础',
    fields: [
      { id: 'bookSourceName', title: '书源名称', path: 'bookSourceName', type: 'string', hint: '必填', placeholder: '书源名称' },
      { id: 'bookSourceUrl', title: '书源 URL', path: 'bookSourceUrl', type: 'string', hint: '必填，http/https', placeholder: 'https://example.com' },
      { id: 'bookSourceGroup', title: '分组', path: 'bookSourceGroup', type: 'string', hint: '逗号分隔', placeholder: '玄幻,都市' },
      { id: 'bookSourceType', title: '书源类型', path: 'bookSourceType', type: 'select', options: [
        { value: '0', label: '文本' }, { value: '1', label: '音频' }, { value: '2', label: '图片' },
        { value: '3', label: '文件' }, { value: '4', label: '视频' },
      ] },
      { id: 'bookUrlPattern', title: '书籍链接匹配', path: 'bookUrlPattern', type: 'string', hint: '详情页 URL 正则', placeholder: 'https://example.com/book/\\d+' },
      { id: 'header', title: '请求头', path: 'header', type: 'string', hint: 'JSON 对象', placeholder: '{"User-Agent":"..."}', rows: 3 },
      { id: 'concurrentRate', title: '并发/限速', path: 'concurrentRate', type: 'string', hint: '如 1 或 3/1000', placeholder: '1' },
      { id: 'jsLib', title: 'JS 库', path: 'jsLib', type: 'string', hint: '全局 JS，规则前执行', placeholder: 'function md5(s){...}', rows: 3 },
      { id: 'loginUrl', title: '登录 URL', path: 'loginUrl', type: 'string', placeholder: 'https://example.com/login' },
      { id: 'loginUi', title: '登录 UI', path: 'loginUi', type: 'string', rows: 2 },
      { id: 'loginCheckJs', title: '登录校验 JS', path: 'loginCheckJs', type: 'string', rows: 2 },
      { id: 'coverDecodeJs', title: '封面解码 JS', path: 'coverDecodeJs', type: 'string', rows: 2 },
      { id: 'bookSourceComment', title: '书源备注', path: 'bookSourceComment', type: 'string', rows: 2 },
      { id: 'variableComment', title: '变量说明', path: 'variableComment', type: 'string', rows: 2 },
    ],
  },
  {
    key: 'search',
    label: '搜索',
    fields: [
      { id: 'searchUrl', title: '搜索 URL', path: 'searchUrl', type: 'string', hint: '{{key}} 为关键词，{{page}} 为页码', placeholder: 'https://example.com/search?q={{key}}' },
      { id: 'checkKeyWord', title: '搜索关键词', path: 'ruleSearch.checkKeyWord', type: 'string', hint: '测试书源时用的默认关键词', placeholder: '斗破苍穹' },
      { id: 'bookList', title: '书籍列表', path: 'ruleSearch.bookList', type: 'string', placeholder: '.book-list li' },
      { id: 'name', title: '书名', path: 'ruleSearch.name', type: 'string', placeholder: 'div.bookname@text' },
      { id: 'author', title: '作者', path: 'ruleSearch.author', type: 'string', placeholder: 'div.author@text' },
      { id: 'bookUrl', title: '书籍链接', path: 'ruleSearch.bookUrl', type: 'string', placeholder: 'a@href' },
      { id: 'coverUrl', title: '封面', path: 'ruleSearch.coverUrl', type: 'string', placeholder: 'img@src' },
      { id: 'intro', title: '简介', path: 'ruleSearch.intro', type: 'string', rows: 2 },
      { id: 'kind', title: '类型', path: 'ruleSearch.kind', type: 'string' },
      { id: 'lastChapter', title: '最新章节', path: 'ruleSearch.lastChapter', type: 'string' },
      { id: 'updateTime', title: '更新时间', path: 'ruleSearch.updateTime', type: 'string' },
      { id: 'wordCount', title: '字数', path: 'ruleSearch.wordCount', type: 'string' },
    ],
  },
  {
    key: 'find',
    label: '发现',
    fields: [
      { id: 'exploreUrl', title: '发现 URL', path: 'exploreUrl', type: 'string', hint: '分类列表：名称::url，用 && 连接', placeholder: '玄幻::https://example.com/fenlei/1&&都市::https://example.com/fenlei/2', rows: 3 },
      { id: 'exploreScreen', title: '发现筛选', path: 'exploreScreen', type: 'string' },
      { id: 'bookList', title: '书籍列表', path: 'ruleExplore.bookList', type: 'string', placeholder: '.book-list li' },
      { id: 'name', title: '书名', path: 'ruleExplore.name', type: 'string', placeholder: 'div.bookname@text' },
      { id: 'author', title: '作者', path: 'ruleExplore.author', type: 'string' },
      { id: 'bookUrl', title: '书籍链接', path: 'ruleExplore.bookUrl', type: 'string', placeholder: 'a@href' },
      { id: 'coverUrl', title: '封面', path: 'ruleExplore.coverUrl', type: 'string', placeholder: 'img@src' },
      { id: 'intro', title: '简介', path: 'ruleExplore.intro', type: 'string', rows: 2 },
      { id: 'kind', title: '类型', path: 'ruleExplore.kind', type: 'string' },
      { id: 'lastChapter', title: '最新章节', path: 'ruleExplore.lastChapter', type: 'string' },
      { id: 'updateTime', title: '更新时间', path: 'ruleExplore.updateTime', type: 'string' },
      { id: 'wordCount', title: '字数', path: 'ruleExplore.wordCount', type: 'string' },
    ],
  },
  {
    key: 'detail',
    label: '详情',
    fields: [
      { id: 'init', title: '初始化 JS', path: 'ruleBookInfo.init', type: 'string', rows: 2 },
      { id: 'name', title: '书名', path: 'ruleBookInfo.name', type: 'string', placeholder: 'h1@text' },
      { id: 'author', title: '作者', path: 'ruleBookInfo.author', type: 'string' },
      { id: 'intro', title: '简介', path: 'ruleBookInfo.intro', type: 'string', rows: 3 },
      { id: 'coverUrl', title: '封面', path: 'ruleBookInfo.coverUrl', type: 'string', placeholder: 'img.cover@src' },
      { id: 'kind', title: '类型', path: 'ruleBookInfo.kind', type: 'string' },
      { id: 'lastChapter', title: '最新章节', path: 'ruleBookInfo.lastChapter', type: 'string' },
      { id: 'updateTime', title: '更新时间', path: 'ruleBookInfo.updateTime', type: 'string' },
      { id: 'wordCount', title: '字数', path: 'ruleBookInfo.wordCount', type: 'string' },
      { id: 'tocUrl', title: '目录链接', path: 'ruleBookInfo.tocUrl', type: 'string', placeholder: 'a.catalog@href' },
      { id: 'canReName', title: '可重命名', path: 'ruleBookInfo.canReName', type: 'string' },
      { id: 'downloadUrls', title: '下载链接', path: 'ruleBookInfo.downloadUrls', type: 'string', rows: 2 },
    ],
  },
  {
    key: 'directory',
    label: '目录',
    fields: [
      { id: 'preUpdateJs', title: '预处理 JS', path: 'ruleToc.preUpdateJs', type: 'string', rows: 2 },
      { id: 'chapterList', title: '章节列表', path: 'ruleToc.chapterList', type: 'string', placeholder: '.chapter-list li' },
      { id: 'chapterName', title: '章节名', path: 'ruleToc.chapterName', type: 'string', placeholder: 'a@text' },
      { id: 'chapterUrl', title: '章节链接', path: 'ruleToc.chapterUrl', type: 'string', placeholder: 'a@href' },
      { id: 'nextTocUrl', title: '下一页目录', path: 'ruleToc.nextTocUrl', type: 'string', placeholder: 'a.next@href' },
      { id: 'formatJs', title: '格式化 JS', path: 'ruleToc.formatJs', type: 'string', rows: 2 },
      { id: 'isVolume', title: '卷标记', path: 'ruleToc.isVolume', type: 'string' },
      { id: 'isVip', title: 'VIP 标记', path: 'ruleToc.isVip', type: 'string' },
      { id: 'isPay', title: '付费标记', path: 'ruleToc.isPay', type: 'string' },
      { id: 'updateTime', title: '更新时间', path: 'ruleToc.updateTime', type: 'string' },
    ],
  },
  {
    key: 'content',
    label: '正文',
    fields: [
      { id: 'content', title: '正文', path: 'ruleContent.content', type: 'string', placeholder: 'div.content@html', rows: 3 },
      { id: 'subContent', title: '子正文', path: 'ruleContent.subContent', type: 'string', rows: 2 },
      { id: 'title', title: '标题', path: 'ruleContent.title', type: 'string' },
      { id: 'nextContentUrl', title: '下一页正文', path: 'ruleContent.nextContentUrl', type: 'string', placeholder: 'a.next@href' },
      { id: 'webJs', title: '网页 JS', path: 'ruleContent.webJs', type: 'string', rows: 2 },
      { id: 'sourceRegex', title: '源正则', path: 'ruleContent.sourceRegex', type: 'string', hint: '## 分隔的替换规则', rows: 2 },
      { id: 'replaceRegex', title: '替换正则', path: 'ruleContent.replaceRegex', type: 'string', hint: '## 分隔的替换规则', rows: 2 },
      { id: 'imageStyle', title: '图片样式', path: 'ruleContent.imageStyle', type: 'string' },
      { id: 'imageDecode', title: '图片解码', path: 'ruleContent.imageDecode', type: 'string' },
      { id: 'payAction', title: '付费动作', path: 'ruleContent.payAction', type: 'string' },
      { id: 'callBackJs', title: '回调 JS', path: 'ruleContent.callBackJs', type: 'string', rows: 2 },
    ],
  },
  {
    key: 'other',
    label: '其他',
    fields: [
      { id: 'enabled', title: '启用', path: 'enabled', type: 'boolean' },
      { id: 'enabledExplore', title: '启用发现', path: 'enabledExplore', type: 'boolean' },
      { id: 'enabledCookieJar', title: '启用 Cookie', path: 'enabledCookieJar', type: 'boolean' },
      { id: 'loadWithBaseUrl', title: '基于书源 URL 加载', path: 'loadWithBaseUrl', type: 'boolean' },
      { id: 'singleUrl', title: '单 URL', path: 'singleUrl', type: 'boolean' },
      { id: 'weight', title: '权重', path: 'weight', type: 'number' },
      { id: 'customOrder', title: '自定义排序', path: 'customOrder', type: 'number' },
    ],
  },
]

const activeGroup = ref<GroupKey>('base')

const activeFields = computed(() => {
  const group = groups.find((g) => g.key === activeGroup.value)
  return group ? group.fields : []
})

function currentObject(): Record<string, unknown> {
  try {
    const parsed = JSON.parse(props.editorText)
    return typeof parsed === 'object' && parsed ? parsed : {}
  } catch {
    return {}
  }
}

function getByPath(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((acc, key) => {
    if (acc && typeof acc === 'object') return (acc as Record<string, unknown>)[key]
    return undefined
  }, obj)
}

function setByPath(obj: Record<string, unknown>, path: string, value: unknown): void {
  const parts = path.split('.')
  let target = obj
  for (let i = 0; i < parts.length - 1; i++) {
    const key = parts[i]
    const next = target[key]
    if (!next || typeof next !== 'object') {
      target[key] = {}
    }
    target = target[key] as Record<string, unknown>
  }
  target[parts[parts.length - 1]] = value
}

function fieldValue(field: FieldDef): string {
  const value = getByPath(currentObject(), field.path)
  if (value === undefined || value === null) return ''
  if (field.type === 'boolean') return value === true ? 'true' : 'false'
  return String(value)
}

function setField(field: FieldDef, raw: string) {
  const obj = currentObject()
  let value: unknown = raw
  if (field.type === 'number') {
    value = raw === '' ? undefined : Number(raw)
  } else if (field.type === 'boolean') {
    value = raw === 'true'
  } else if (raw === '') {
    value = undefined
  }
  setByPath(obj, field.path, value)
  emit('update:editorText', JSON.stringify(obj, null, 2))
}
</script>

<style scoped>
.rule-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  flex: 1;
}

.rule-group-tabs {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.rule-tab {
  min-height: 30px;
  padding: 0 12px;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 13px;
  border: 1px solid var(--color-border);
}

.rule-tab.active {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.rule-fields {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: auto;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.field-label {
  font-size: 12px;
  color: var(--color-text-secondary);
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.field-hint {
  color: var(--color-text-tertiary);
  font-size: 11px;
}

.field-input {
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg);
  color: var(--color-text);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  outline: none;
}

.field-textarea {
  padding: 8px 12px;
  resize: vertical;
  line-height: 1.5;
}

.field-toggle {
  display: flex;
}

.toggle-btn {
  min-height: 30px;
  padding: 0 14px;
  border-radius: var(--radius-full);
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 12px;
}

.toggle-btn.on {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.rule-empty {
  color: var(--color-text-tertiary);
  font-size: 13px;
}
</style>
