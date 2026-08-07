# Source Manager Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the Vue 3 source manager into a clearer workbench layout while preserving all existing source management behavior.

**Architecture:** Keep `SourceManager.vue` as the orchestration component for API calls, local state, confirmations, import/export, and toast feedback. Extract focused presentational/workspace components under `frontend/src/components/source-manager/`, and move pure grouping/summary logic into tested utilities.

**Tech Stack:** Vue 3 Composition API, TypeScript, Pinia toast store, existing `frontend/src/api/source.ts`, Vitest, `vue-tsc`, Vite.

---

## File Structure

- Modify: `frontend/src/utils/sourceSelection.ts`
  - Add pure helpers for source group extraction, filtering, summary metrics, and source display metadata.
- Modify: `frontend/src/utils/sourceSelection.test.ts`
  - Add failing tests for the new pure helpers before implementation.
- Create: `frontend/src/components/source-manager/SourceManagerHeader.vue`
  - Header metrics and global action buttons.
- Create: `frontend/src/components/source-manager/SourceFilterBar.vue`
  - Search, group select, current-result selection controls.
- Create: `frontend/src/components/source-manager/SourceList.vue`
  - Source rows, selection, enable toggle, edit/delete row events, loading/empty states.
- Create: `frontend/src/components/source-manager/SourceEditorPanel.vue`
  - Overview / JSON / Login Debug tabs and editor actions.
- Create: `frontend/src/components/source-manager/SourceSubscriptionPanel.vue`
  - Remote URL input and saved subscription list.
- Modify: `frontend/src/components/SourceManager.vue`
  - Use extracted components, workbench layout, and existing API orchestration.

## Task 1: Pure Source Utilities

**Files:**
- Modify: `frontend/src/utils/sourceSelection.test.ts`
- Modify: `frontend/src/utils/sourceSelection.ts`

- [ ] **Step 1: Write failing utility tests**

Add these tests to `frontend/src/utils/sourceSelection.test.ts`:

```ts
it('extracts sorted groups from comma-like source group separators', () => {
  const grouped: BookSource[] = [
    { bookSourceName: 'A', bookSourceUrl: 'a', bookSourceGroup: '小说, API' },
    { bookSourceName: 'B', bookSourceUrl: 'b', bookSourceGroup: 'API；精选、小说' },
    { bookSourceName: 'C', bookSourceUrl: 'c' },
  ]

  expect(getBookSourceGroups(grouped)).toEqual(['API', '小说', '精选'])
})

it('filters book sources by text and group', () => {
  const grouped: BookSource[] = [
    { bookSourceName: '猫眼看书', bookSourceUrl: 'https://maoyan.example', bookSourceGroup: 'API' },
    { bookSourceName: '笔趣阁', bookSourceUrl: 'https://biqu.example', bookSourceGroup: '网页' },
  ]

  expect(filterBookSources(grouped, 'mao', '')).toEqual([grouped[0]])
  expect(filterBookSources(grouped, '', '网页')).toEqual([grouped[1]])
})

it('summarizes source counts and selected source metadata', () => {
  const list: BookSource[] = [
    { bookSourceName: 'Enabled', bookSourceUrl: 'enabled', enabled: true, ruleSearch: {}, ruleToc: {} },
    { bookSourceName: 'Disabled', bookSourceUrl: 'disabled', enabled: false },
  ]

  expect(getBookSourceStats(list, [list[0]])).toEqual({
    total: 2,
    enabled: 1,
    filtered: 1,
  })
  expect(getBookSourceOverview(list[0])).toMatchObject({
    group: '未分组',
    statusText: '启用',
    hasSearch: true,
    hasToc: true,
  })
})
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cd frontend && npm test -- sourceSelection
```

Expected: FAIL because `getBookSourceGroups`, `filterBookSources`, `getBookSourceStats`, and `getBookSourceOverview` are not exported yet.

- [ ] **Step 3: Implement utility helpers**

Add exports to `frontend/src/utils/sourceSelection.ts`:

```ts
export function getBookSourceGroups(sources: Pick<BookSource, 'bookSourceGroup'>[]) {
  const groups = new Set<string>()
  sources.forEach((source) => {
    source.bookSourceGroup?.split(/[,;；、]/).forEach((group) => {
      const trimmed = group.trim()
      if (trimmed) groups.add(trimmed)
    })
  })
  return Array.from(groups).sort()
}

export function filterBookSources(
  sources: BookSource[],
  filterText: string,
  filterGroup: string,
) {
  const keyword = filterText.trim().toLowerCase()
  return sources.filter((source) => {
    const matchesText = !keyword
      || source.bookSourceName.toLowerCase().includes(keyword)
      || source.bookSourceUrl.toLowerCase().includes(keyword)
    const matchesGroup = !filterGroup || source.bookSourceGroup?.includes(filterGroup)
    return matchesText && matchesGroup
  })
}

export function getBookSourceStats(allSources: BookSource[], filteredSources: BookSource[]) {
  return {
    total: allSources.length,
    enabled: allSources.filter((source) => source.enabled !== false).length,
    filtered: filteredSources.length,
  }
}

export function getBookSourceOverview(source: BookSource | null) {
  if (!source) {
    return null
  }
  return {
    group: source.bookSourceGroup?.trim() || '未分组',
    statusText: source.enabled === false ? '停用' : '启用',
    exploreText: source.enabledExplore === false ? '发现停用' : '发现可用',
    cookieText: source.enabledCookieJar ? 'Cookie 独立' : '默认 Cookie',
    hasSearch: Boolean(source.searchUrl || source.ruleSearch),
    hasExplore: Boolean(source.exploreUrl || source.ruleExplore),
    hasBookInfo: Boolean(source.ruleBookInfo),
    hasToc: Boolean(source.ruleToc),
    hasContent: Boolean(source.ruleContent),
    hasLogin: Boolean(source.loginUrl?.trim()),
  }
}
```

- [ ] **Step 4: Run utility tests and verify they pass**

Run:

```bash
cd frontend && npm test -- sourceSelection
```

Expected: PASS.

## Task 2: Extract Workbench Components

**Files:**
- Create: `frontend/src/components/source-manager/SourceManagerHeader.vue`
- Create: `frontend/src/components/source-manager/SourceFilterBar.vue`
- Create: `frontend/src/components/source-manager/SourceList.vue`
- Create: `frontend/src/components/source-manager/SourceSubscriptionPanel.vue`

- [ ] **Step 1: Create `SourceManagerHeader.vue`**

Implement props for counts and loading, and emit `refresh`, `import-local`, `open-subscriptions`, `export`, `create`, and `close`.

- [ ] **Step 2: Create `SourceFilterBar.vue`**

Implement `filterText` / `filterGroup` with `v-model` props, group options, current selection state, and emit `toggle-current-selection` / `clear-selection`.

- [ ] **Step 3: Create `SourceList.vue`**

Render loading, empty state, rows, checkboxes, enable toggles, edit and delete buttons. Emit `edit`, `toggle-enabled`, `toggle-selection`, and `delete`.

- [ ] **Step 4: Create `SourceSubscriptionPanel.vue`**

Render a modal-like panel with remote URL input, sync/save actions, saved subscriptions, last sync time, and remove/sync row actions. Emit `update:remote-url`, `sync`, `save`, `sync-subscription`, `remove-subscription`, and `close`.

- [ ] **Step 5: Run TypeScript check**

Run:

```bash
cd frontend && npm run build
```

Expected: If `SourceManager.vue` does not import the new components yet, the build may still pass because the components are standalone. Any type errors in props/emits must be fixed before Task 3.

## Task 3: Extract Editor Panel

**Files:**
- Create: `frontend/src/components/source-manager/SourceEditorPanel.vue`
- Modify: `frontend/src/utils/sourceSelection.ts` if overview metadata needs small additions.

- [ ] **Step 1: Create editor panel component**

Implement props for `source`, `editorText`, `canLogin`, and `loginLoading`. Keep JSON text as a `v-model` via `update:editorText`. Add three tabs: `overview`, `json`, and `login`.

- [ ] **Step 2: Implement overview tab**

Use `getBookSourceOverview(source)` to show status, group, rule coverage, login availability, and source URL. Empty state shows actions for creating or importing sources.

- [ ] **Step 3: Implement JSON tab**

Render the textarea, Format button, Save button, and Login button when available. Emit `format`, `save`, `login`, `create`, and `import-local`.

- [ ] **Step 4: Implement login tab**

Show login availability and trigger `login` when `canLogin` is true. Do not move iframe preview into this component; keep the existing top-level login preview modal in `SourceManager.vue`.

- [ ] **Step 5: Run TypeScript check**

Run:

```bash
cd frontend && npm run build
```

Expected: PASS or component-local type errors to fix before Task 4.

## Task 4: Rework `SourceManager.vue`

**Files:**
- Modify: `frontend/src/components/SourceManager.vue`

- [ ] **Step 1: Replace inline header/filter/list/editor/subscription template with extracted components**

Wire existing parent methods to component events. Keep existing methods for `loadSources`, `toggleSource`, `removeSource`, `removeSelectedSources`, `createSource`, `editSource`, `formatEditor`, `saveEditor`, `handleSourceLogin`, import/export, and subscription persistence.

- [ ] **Step 2: Replace inline computed group/filter logic**

Use:

```ts
const groupList = computed(() => getBookSourceGroups(sources.value))
const filteredSources = computed(() => filterBookSources(sources.value, filterText.value, filterGroup.value))
const sourceStats = computed(() => getBookSourceStats(sources.value, filteredSources.value))
```

- [ ] **Step 3: Add subscription panel visibility state**

Replace the always-visible remote panel with:

```ts
const subscriptionPanelVisible = ref(false)
```

Open it from `SourceManagerHeader`, close it from `SourceSubscriptionPanel`, and preserve `remoteUrl` / `subscriptions` behavior.

- [ ] **Step 4: Keep login preview modal unchanged**

Retain the existing `loginPreviewVisible`, `loginPreviewUrl`, `loginPreviewFrameUrl`, and iframe modal markup at the top level.

- [ ] **Step 5: Update scoped styles**

Keep modal, overlay, content grid, shared buttons, login preview modal, and responsive behavior in `SourceManager.vue`. Move row/editor/subscription-specific styles into child components where practical.

- [ ] **Step 6: Run focused frontend tests**

Run:

```bash
cd frontend && npm test -- sourceSelection
```

Expected: PASS.

## Task 5: Visual and Build Verification

**Files:**
- Modify only if verification finds concrete layout or type issues.

- [ ] **Step 1: Run production build**

Run:

```bash
cd frontend && npm run build
```

Expected: PASS.

- [ ] **Step 2: Start the frontend dev server**

Run:

```bash
cd frontend && npm run dev -- --host 127.0.0.1
```

Expected: Vite prints a local URL.

- [ ] **Step 3: Inspect in browser**

Open the local URL, open the source manager modal, and inspect desktop and mobile-sized layouts. Confirm:

- Header metrics and actions are visible.
- Search, group filter, and batch selection work visually.
- Source list and editor use two panes on desktop.
- Editor tabs switch without shifting the modal.
- Subscription panel opens and closes.
- Mobile layout stacks list and editor.

- [ ] **Step 4: Stop dev server**

Stop the Vite session after inspection.
