# Source Manager Refactor Design

## Context

The current Vue 3 source manager is implemented mostly in `frontend/src/components/SourceManager.vue`. It already supports local import, remote subscription sync, export, add/edit/delete, batch delete, enable toggles, JSON editing, and source login preview. The page has grown dense: global actions, remote subscription controls, filtering, list selection, JSON editing, and login debug all compete for the same vertical space.

The refactor should improve operation efficiency, editing clarity, visual hierarchy, and mobile layout while preserving the existing workflows and backend APIs.

## Goals

- Keep source management as one modal/workbench instead of adding a new route.
- Preserve existing functions: refresh, local import, remote sync/subscriptions, export, add, edit, save, delete, batch delete, enable/disable, JSON format, source login preview.
- Reorganize the UI into a stable workbench: global actions at the top, source list on the left, selected source workspace on the right.
- Reduce `SourceManager.vue` complexity by extracting focused UI units and keeping API orchestration in the parent.
- Improve mobile behavior by stacking list and editor instead of forcing a cramped two-column layout.

## Non-Goals

- No backend API changes.
- No changes to book source JSON format.
- No new global state system for source editing.
- No full visual redesign of the whole application.
- No replacement of JSON editing with a complete form editor in this pass.

## Proposed UI

Use the approved "workbench two-column" direction.

The header shows the page title and compact status metrics: total sources, enabled sources, filtered count, and selected count. Primary global actions live beside it: refresh, local import, remote sync, export, and add source.

Below the header, a compact toolbar handles search, group filtering, and current-result selection. The remote subscription UI moves out of the always-visible large panel and into a sync drawer or lightweight panel opened from the global action area. This keeps subscription management available without permanently consuming list height.

The main content uses two panes on desktop:

- Left pane: source list with checkbox selection, enabled status, source name, URL, group, and row actions.
- Right pane: selected source workspace with tabs for Overview, JSON, and Login Debug.

If no source is selected, the right pane shows a useful empty state with actions to add or import sources.

## Components

Keep `SourceManager.vue` as the modal coordinator. It owns data loading, API calls, toast feedback, import/export handlers, delete confirmation, and login preview state.

Extract focused components under `frontend/src/components/source-manager/`:

- `SourceManagerHeader.vue`: title, metrics, refresh/import/sync/export/add actions.
- `SourceFilterBar.vue`: search, group filter, select current results, clear selection.
- `SourceList.vue`: list rendering, row selection, enable toggle, edit/delete row actions, empty/loading states.
- `SourceEditorPanel.vue`: selected source workspace, tabs, overview summary, JSON editor, format/save actions, login debug action.
- `SourceSubscriptionPanel.vue`: remote URL input, saved subscription list, sync/delete subscription actions.

Pure selection helpers stay in `frontend/src/utils/sourceSelection.ts`. Any new pure helpers for grouping or source summaries should live in a small utility file and have tests.

## Data Flow

`SourceManager.vue` keeps `sources` as the single source of truth. Derived state remains computed: `filteredSources`, `enabledCount`, `groupList`, `selectedFilteredSources`, and editor metadata.

Editing flow:

1. User selects a source from `SourceList`.
2. Parent stores `editingSource` and JSON text.
3. `SourceEditorPanel` edits local JSON text and emits format/save/login events.
4. Parent validates JSON, calls `saveBookSource`, reloads sources, and reselects the saved source.

Import/sync flow:

1. Local import reads file through existing `readSourceFile`.
2. Remote sync reads through existing `readRemoteSourceFile`.
3. Parsed sources are saved through existing `saveBookSources`.
4. Parent reloads sources and prunes selected URLs.

Delete flow:

1. Single and batch delete keep existing confirmations.
2. Batch delete uses the visible selected sources and `deleteBookSources`.
3. Parent removes deleted items from local state, clears selection, and resets editor if needed.

## Error Handling

Use existing toast behavior through `appStore.showToast`.

- JSON parse/validation failures show field-specific messages: missing name, missing URL, invalid JSON.
- Remote sync errors show the thrown message or "远程同步失败".
- Empty import/sync results show a clear warning that no sources were found.
- Delete operations keep confirmation dialogs and report failure through toast.
- Login debug is only enabled when the current JSON has `bookSourceUrl` and `loginUrl`.

## Responsive Behavior

Desktop keeps the two-pane layout.

At tablet/mobile widths, the workbench stacks vertically:

- Header actions wrap into two rows if needed.
- Filters remain above the list.
- List appears first.
- Editor panel appears below the list for the selected source.
- Subscription panel opens as a full-width block or modal-like panel.

Controls should keep stable heights and avoid text overflow. Buttons should use concise labels already present in the app.

## Testing

Add or preserve focused tests for pure logic:

- visible selection from filtered sources,
- batch delete payload creation,
- group extraction/filtering if moved to a helper,
- source summary metadata if introduced.

Run existing frontend checks after implementation:

- `cd frontend && npm run build`
- targeted Vitest tests if relevant, such as `cd frontend && npm test -- sourceSelection`

Use the browser to inspect the modal at desktop and mobile-sized viewports after implementation.

## Implementation Boundaries

The refactor should be staged to reduce risk:

1. Extract utility/component boundaries without changing behavior.
2. Introduce the workbench layout and subscription panel placement.
3. Add editor tabs and overview panel while preserving JSON as the authoritative edit mode.
4. Polish responsive layout and empty/loading states.
5. Run build/tests and manually verify the modal.

Do not rewrite API modules or backend handlers during this refactor.
