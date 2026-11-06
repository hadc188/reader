<template>
  <!-- 与 HttpTtsManager 同理：从「阅读设置」抽屉里打开，必须 Teleport 到 body，
       否则 position:fixed 会被抽屉的 overflow 裁成一条。 -->
  <Teleport to="body">
    <!-- 手机端就是这个形态：半透明黑遮罩直接盖在正文上，九格占满整屏，点哪格改哪格
         （见 legado 的 ClickActionConfigDialog）。这里不做小弹窗，避免"设置里看小图、
         回正文靠想象"的割裂感。 -->
    <div class="click-area-screen">
      <header class="click-area-bar">
        <button
          class="click-area-close"
          type="button"
          aria-label="关闭"
          @click="close"
        >×</button>
        <h3 class="click-area-title">点击区域设置</h3>
        <div class="click-area-modes" role="group" aria-label="点击区域模式">
          <button
            type="button"
            :class="{ on: currentMode === 'auto' }"
            title="沿用原来的三分区：两端翻页、中间呼出菜单"
            @click="currentMode = 'auto'"
          >默认分区</button>
          <button
            type="button"
            :class="{ on: currentMode === 'custom' }"
            title="每格动作自己定"
            @click="enableCustom"
          >九宫格</button>
        </div>
      </header>

      <div class="click-area-grid" :class="{ readonly: currentMode !== 'custom' }">
        <button
          v-for="zone in CLICK_AREA_ZONES"
          :key="zone"
          type="button"
          class="click-area-cell"
          :class="{
            editing: editorZone === zone,
            idle: shownAreas[zone] === 'none',
          }"
          :disabled="currentMode !== 'custom'"
          @click="editorZone = zone"
        >
          <span class="click-area-cell-action">{{ clickAreaActionLabel(shownAreas[zone]) }}</span>
          <span class="click-area-cell-zone">{{ ZONE_LABELS[zone] }}</span>
        </button>
      </div>

      <footer class="click-area-bar click-area-bar--footer">
        <div class="click-area-profiles" role="group" aria-label="切换到另一种阅读模式的九宫格">
          <span class="click-area-profiles-label">正在设置</span>
          <button
            v-for="profile in CLICK_AREA_PROFILES"
            :key="profile"
            type="button"
            class="click-area-chip"
            :class="{ on: draftProfile === profile }"
            :title="profileHint(profile)"
            @click="switchProfile(profile)"
          >
            {{ CLICK_AREA_PROFILE_LABELS[profile] }}
            <span v-if="draftModes[profile] === 'custom'" class="click-area-chip-dot" aria-hidden="true"></span>
          </button>
        </div>
        <div class="click-area-confirm">
          <button type="button" class="click-area-btn" @click="resetToDefault">恢复本套默认</button>
          <button type="button" class="click-area-btn" @click="close">取消</button>
          <button type="button" class="click-area-btn primary" @click="save">保存</button>
        </div>
      </footer>

      <!-- 选动作：与手机端的 ActionPickerOverlay 一致 —— 点格子弹一层列表，
           选中即写入草稿并关掉列表，不用再点"确定"。 -->
      <div v-if="editorZone" class="click-area-picker" @click.self="editorZone = null">
        <div class="click-area-picker-panel" role="dialog" :aria-label="`${ZONE_LABELS[editorZone]}的动作`">
          <div class="click-area-picker-head">
            <span>{{ ZONE_LABELS[editorZone] }}</span>
            <button type="button" class="click-area-close small" aria-label="关闭" @click="editorZone = null">×</button>
          </div>
          <div class="click-area-picker-list">
            <button
              v-for="action in CLICK_AREA_ACTIONS"
              :key="action.id"
              type="button"
              class="click-area-option"
              :class="{ on: editingAction === action.id }"
              @click="applyAction(action.id)"
            >
              <span>{{ action.label }}</span>
              <svg v-if="editingAction === action.id" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6 9 17l-5-5" /></svg>
            </button>
          </div>
        </div>
      </div>

      <p v-if="notice" class="click-area-notice">{{ notice }}</p>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useReaderStore } from '../../stores/reader'
import {
  CLICK_AREA_ACTIONS,
  CLICK_AREA_PROFILES,
  CLICK_AREA_PROFILE_LABELS,
  CLICK_AREA_ZONES,
  clickAreaActionLabel,
  presetClickAreas,
  setClickAreaAction,
  type ClickAreaAction,
  type ClickAreaMap,
  type ClickAreaModes,
  type ClickAreaProfile,
  type ClickAreaProfiles,
  type ClickAreaZone,
} from '../../utils/readerClickAreas'

const emit = defineEmits<{ close: [] }>()

const store = useReaderStore()

/**
 * 本地草稿：两套九宫格 + 两套开关各存一份，点「取消」整体丢弃。
 *
 * 两套同时留在内存里，是为了让底部的「左右翻页 / 上下滚动」能来回切着配 ——
 * 切过去看到的是那一套已经存下的内容，而不是被另一套覆盖过的结果。
 */
const draftProfiles = ref<ClickAreaProfiles>({
  paged: { ...store.config.clickAreaProfiles.paged },
  scroll: { ...store.config.clickAreaProfiles.scroll },
})
const draftModes = ref<ClickAreaModes>({ ...store.config.clickAreaModes })

/** 当前正在编辑哪一套。默认跟着当前阅读方式走，省得用户先切一下。 */
const draftProfile = ref<ClickAreaProfile>(
  store.config.readMethod === '左右翻页' ? 'paged' : 'scroll',
)
const editorZone = ref<ClickAreaZone | null>(null)
const notice = ref('')

const ZONE_LABELS: Record<ClickAreaZone, string> = {
  tl: '左上', tc: '中上', tr: '右上',
  ml: '左中', mc: '正中', mr: '右中',
  bl: '左下', bc: '中下', br: '右下',
}

/** 当前这一套的草稿网格。 */
const draft = computed(() => draftProfiles.value[draftProfile.value])
/** 当前这一套的开关。 */
const currentMode = computed({
  get: () => draftModes.value[draftProfile.value],
  set: (value: 'auto' | 'custom') => { draftModes.value[draftProfile.value] = value },
})

/**
 * 「默认分区」模式下网格显示的是**这一套对应的三分区**，而不是空白或上一次的
 * 草稿 —— 用户切过去能直接看出"这一套现在点两端到底是翻页还是呼菜单"。
 */
const shownAreas = computed<ClickAreaMap>(() => (
  currentMode.value === 'custom'
    ? draft.value
    : presetClickAreas(draftProfile.value)
))

/** 浮层里打勾的那一项。单独算成 computed，模板里就不用写 `draft[editorZone!]`。 */
const editingAction = computed<ClickAreaAction | undefined>(() => (
  editorZone.value ? draft.value[editorZone.value] : undefined
))

function profileHint(profile: ClickAreaProfile) {
  return draftModes.value[profile] === 'custom'
    ? `${CLICK_AREA_PROFILE_LABELS[profile]}：已启用九宫格`
    : `${CLICK_AREA_PROFILE_LABELS[profile]}：使用默认三分区`
}

/** 切到另一套。两套草稿互不干扰，切回来还是刚才编辑的样子。 */
function switchProfile(profile: ClickAreaProfile) {
  if (draftProfile.value === profile) return
  draftProfile.value = profile
  editorZone.value = null
  notice.value = ''
}

/** 首次切到九宫格：如果还是「全菜单」这种没配过的空壳，顺手填这一套的预设。 */
function enableCustom() {
  currentMode.value = 'custom'
  if (!CLICK_AREA_ZONES.some((zone) => draft.value[zone] !== 'menu')) {
    draftProfiles.value[draftProfile.value] = presetClickAreas(draftProfile.value)
    notice.value = ''
  }
}

function applyAction(action: ClickAreaAction) {
  if (!editorZone.value) return
  const result = setClickAreaAction(draft.value, editorZone.value, action)
  draftProfiles.value[draftProfile.value] = result.areas
  editorZone.value = null
  // 手机端的 detectClickArea: 九格都没有菜单时把正中抢回来，否则再也呼不出工具栏。
  notice.value = result.forcedCenterMenu
    ? '九格都没有「呼出菜单」，已把正中一格设回菜单。'
    : ''
}

/** 只把**当前这一套**恢复成该模式的默认三分区，另一套不动。 */
function resetToDefault() {
  draftProfiles.value[draftProfile.value] = presetClickAreas(draftProfile.value)
  draftModes.value[draftProfile.value] = 'auto'
  editorZone.value = null
  notice.value = ''
}

function save() {
  // 两套一起落盘：用户可能只改了一套，另一套保持原样。
  store.updateConfig('clickAreaProfiles', {
    paged: { ...draftProfiles.value.paged },
    scroll: { ...draftProfiles.value.scroll },
  })
  store.updateConfig('clickAreaModes', { ...draftModes.value })
  emit('close')
}

function close() {
  emit('close')
}
</script>

<style scoped>
.click-area-screen {
  position: fixed;
  inset: 0;
  /* 与 HttpTtsManager 同一层：压过阅读抽屉(40–90)与主设置面板(600)，又在 toast(700) 之下。 */
  z-index: var(--z-nested-overlay);
  display: flex;
  flex-direction: column;
  gap: 6px;
  /* 手机端是 6dp 边距 + 黑 54% 遮罩：九格几乎贴边，能真实反映点哪儿算哪格。 */
  padding: 6px;
  background: rgba(0, 0, 0, 0.54);
  color: #fff;
  font-size: 13px;
}

.click-area-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
  min-height: 44px;
  padding: 5px 10px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.16);
}

.click-area-bar--footer {
  justify-content: space-between;
  gap: 8px;
}

.click-area-title {
  flex: 1;
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.click-area-close {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font-size: 20px;
  line-height: 1;
  opacity: 0.7;
  cursor: pointer;
}

.click-area-close:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.14);
}

.click-area-close.small {
  width: 26px;
  height: 26px;
  font-size: 17px;
}

.click-area-modes {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  flex-shrink: 0;
  border-radius: 9px;
  background: rgba(0, 0, 0, 0.22);
}

.click-area-modes button {
  min-height: 26px;
  padding: 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: inherit;
  font-size: 12.5px;
  font-weight: 500;
  white-space: nowrap;
  opacity: 0.72;
  cursor: pointer;
}

.click-area-modes button:hover {
  opacity: 1;
}

.click-area-modes button.on {
  background: rgba(255, 255, 255, 0.9);
  color: #1f2430;
  opacity: 1;
}

.click-area-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  grid-template-rows: repeat(3, minmax(0, 1fr));
  gap: 6px;
}

.click-area-grid.readonly .click-area-cell {
  cursor: default;
  /* 只读预览也要看得清：压暗但不至于像禁用控件那样发灰到读不出字。 */
  opacity: 0.86;
}

.click-area-cell {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-height: 0;
  padding: 8px;
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.1);
  color: inherit;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.click-area-cell:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.2);
  border-color: rgba(255, 255, 255, 0.4);
}

.click-area-cell:disabled {
  cursor: default;
}

.click-area-cell.editing {
  border-color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.26);
}

.click-area-cell.idle .click-area-cell-action {
  opacity: 0.45;
}

.click-area-cell-action {
  font-size: clamp(13px, 1.6vw, 18px);
  font-weight: 600;
  line-height: 1.25;
  text-align: center;
  word-break: break-all;
}

.click-area-cell-zone {
  font-size: 11px;
  opacity: 0.45;
}

.click-area-profiles {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.click-area-profiles-label {
  margin-right: 2px;
  font-size: 11.5px;
  opacity: 0.5;
}

.click-area-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
  border: 1px solid rgba(255, 255, 255, 0.22);
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.click-area-chip:hover {
  background: rgba(255, 255, 255, 0.14);
}

/* 选中的那套：实心亮底，一眼看出「现在编辑的是这一套」。 */
.click-area-chip.on {
  border-color: rgba(255, 255, 255, 0.75);
  background: rgba(255, 255, 255, 0.9);
  color: #1f2430;
  font-weight: 600;
}

/* 该套已启用九宫格的小圆点：切走之后也能看出另一套是不是自定义。 */
.click-area-chip-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.55;
}

.click-area-confirm {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.click-area-btn {
  padding: 6px 16px;
  border: 1px solid rgba(255, 255, 255, 0.28);
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}

.click-area-btn:hover {
  background: rgba(255, 255, 255, 0.14);
}

.click-area-btn.primary {
  border-color: transparent;
  background: rgba(255, 255, 255, 0.92);
  color: #1f2430;
  font-weight: 600;
}

.click-area-btn.primary:hover {
  background: #fff;
}

/* 选动作浮层：手机端是「黑 26% 再压一层 + 居中列表，上限 70% 高」。 */
.click-area-picker {
  position: absolute;
  inset: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: rgba(0, 0, 0, 0.26);
}

.click-area-picker-panel {
  width: min(320px, 100%);
  max-height: min(70%, 460px);
  display: flex;
  flex-direction: column;
  border-radius: 14px;
  background: #23262e;
  color: #fff;
  box-shadow: 0 22px 56px rgba(0, 0, 0, 0.45);
  overflow: hidden;
}

.click-area-picker-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 8px 10px 16px;
  /* 与手机端一致：标题写的是格位名，用户一眼知道自己在改哪格。 */
  font-size: 13.5px;
  font-weight: 600;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.click-area-picker-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px;
  overflow-y: auto;
}

.click-area-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-height: 40px;
  padding: 0 12px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.click-area-option:hover {
  background: rgba(255, 255, 255, 0.1);
}

.click-area-option.on {
  background: rgba(255, 255, 255, 0.16);
  font-weight: 600;
}

.click-area-option svg {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

.click-area-notice {
  position: absolute;
  left: 50%;
  bottom: 74px;
  transform: translateX(-50%);
  max-width: calc(100% - 32px);
  margin: 0;
  padding: 8px 14px;
  border-radius: 9px;
  background: rgba(200, 129, 28, 0.94);
  color: #fff;
  font-size: 12.5px;
  line-height: 1.5;
  text-align: center;
}

@media (max-width: 560px) {
  .click-area-bar {
    flex-wrap: wrap;
  }

  .click-area-title {
    flex-basis: auto;
  }

  .click-area-modes {
    order: 3;
    width: 100%;
    justify-content: stretch;
  }

  .click-area-modes button {
    flex: 1;
  }

  .click-area-cell-action {
    font-size: 14px;
  }

  .click-area-bar--footer {
    flex-direction: column;
    align-items: stretch;
  }

  .click-area-profiles {
    justify-content: center;
  }

  .click-area-confirm {
    justify-content: flex-end;
  }
}
</style>
