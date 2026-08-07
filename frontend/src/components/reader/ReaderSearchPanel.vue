<template>
  <Transition name="fade">
    <div v-if="show" class="reader-search-panel" :style="{ background: theme.popup, color: theme.fontColor }">
      <div class="reader-search-header">
        <div class="reader-search-title">章节搜索</div>
        <button @click="$emit('close')" class="close-search">×</button>
      </div>
      <div class="reader-search-form">
        <input
          :value="query"
          placeholder="搜索当前章节内容..."
          ref="inputRef"
          @input="$emit('update:query', ($event.target as HTMLInputElement).value)"
          @keyup.enter="$emit('search')"
        />
        <button class="search-submit" @click="$emit('search')" :disabled="!query">
          搜索
        </button>
      </div>
      <div class="search-status-row">
        <span v-if="count">{{ activeIndex + 1 }} / {{ count }}</span>
        <span v-else>{{ status }}</span>
        <div class="search-actions">
          <button @click="$emit('prev')" :disabled="!count">↑</button>
          <button @click="$emit('next')" :disabled="!count">↓</button>
        </div>
      </div>
      <div class="reader-search-results">
        <div v-if="!query" class="search-empty">输入关键词开始搜索</div>
        <div v-else-if="!results.length" class="search-empty">当前章节没有匹配内容</div>
        <button
          v-for="(result, idx) in results"
          :key="`${result.chapterIndex}-${idx}-${result.snippet}`"
          class="search-result-item"
          :class="{ active: idx === activeIndex }"
          @click="$emit('jump', result, idx)"
        >
          <div class="search-result-title">{{ result.chapterTitle }}</div>
          <div class="search-result-snippet">{{ result.snippet }}</div>
        </button>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import type { SearchResultItem } from '../../composables/useReaderSearch'
import type { ThemePreset } from '../../stores/reader'

const props = defineProps<{
  show: boolean
  theme: ThemePreset | { popup: string; fontColor: string }
  query: string
  results: SearchResultItem[]
  activeIndex: number
  count: number
  status: string
}>()

const emit = defineEmits<{
  close: []
  search: []
  next: []
  prev: []
  'update:query': [value: string]
  jump: [result: SearchResultItem, idx: number]
}>()

const inputRef = ref<HTMLInputElement>()

watch(() => props.show, (visible) => {
  if (visible) {
    setTimeout(() => inputRef.value?.focus(), 0)
  }
})
</script>

<style scoped>
.reader-search-panel {
  position: fixed;
  top: calc(var(--titlebar-height, 32px) + 24px + var(--safe-area-top));
  right: 80px;
  width: min(420px, calc(100vw - 120px));
  padding: 14px 16px;
  border-radius: 16px;
  box-shadow: 0 4px 20px rgba(0,0,0,0.15);
  display: flex;
  flex-direction: column;
  gap: 12px;
  z-index: 30;
  border: 1px solid rgba(0,0,0,0.05);
}

.reader-search-header,
.search-status-row,
.reader-search-form {
  display: flex;
  align-items: center;
}

.reader-search-header,
.search-status-row {
  justify-content: space-between;
  gap: 12px;
}

.reader-search-title {
  font-size: 15px;
  font-weight: 700;
}

.reader-search-form {
  gap: 10px;
}

.reader-search-form input {
  flex: 1;
  min-width: 0;
  background: rgba(0, 0, 0, 0.04);
  border: 1px solid rgba(0, 0, 0, 0.06);
  outline: none;
  color: inherit;
  font-size: 14px;
  padding: 11px 12px;
  border-radius: 12px;
}

.search-submit {
  border: none;
  border-radius: 12px;
  background: var(--color-primary);
  color: #fff;
  font-size: 13px;
  padding: 11px 14px;
  cursor: pointer;
}

.search-submit:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.reader-search-results {
  max-height: min(48vh, 420px);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.search-result-item {
  border: 1px solid rgba(0,0,0,0.06);
  background: rgba(0,0,0,0.02);
  color: inherit;
  border-radius: 12px;
  padding: 10px 12px;
  text-align: left;
  cursor: pointer;
}

.search-result-item.active {
  border-color: var(--color-primary);
  background: rgba(201, 127, 58, 0.08);
}

.search-result-title {
  font-size: 13px;
  font-weight: 700;
  margin-bottom: 4px;
}

.search-result-snippet {
  font-size: 12px;
  line-height: 1.55;
  opacity: 0.78;
  word-break: break-all;
}

.search-empty {
  text-align: center;
  opacity: 0.55;
  padding: 28px 12px;
  font-size: 13px;
}

.search-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  opacity: 0.8;
}

.search-actions button {
  background: rgba(0,0,0,0.05);
  border: none;
  width: 24px;
  height: 24px;
  border-radius: 4px;
  cursor: pointer;
  color: inherit;
}

.close-search {
  border: none;
  background: transparent;
  color: inherit;
  font-size: 20px;
  line-height: 1;
  opacity: 0.65;
  cursor: pointer;
}

@media (max-width: 768px) {
  .reader-search-panel {
    top: auto;
    bottom: calc(80px + var(--safe-area-bottom));
    right: 16px;
    left: 16px;
    width: auto;
  }
}
</style>
