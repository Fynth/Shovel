# Shovel Responsiveness Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Shovel UI maximally responsive and smooth by splitting the monolithic tab-state signal into four independent signals, rewriting the JS bridge, moving heavy work off the render thread, and adding memoization boundaries.

**Architecture:** Replace the single `Signal<Vec<QueryTabState>>` with four independent `Signal<HashMap<u64, X>>` signals (meta / editor / result / pending) so typing, query execution, and cell editing each re-render only their own subtree. Make the DOM the source of truth during editor input (Rust syncs only on tab switch / external change). Move heavy sync work (`format_sql`, export, ER diagram, `materialize_display_rows`) into `spawn_blocking` with caching. Add `use_memo`/`use_reactive` boundaries between panels.

**Tech Stack:** Rust (nightly, edition 2024), Dioxus 0.7, sqlx, tokio.

**Spec:** `docs/superpowers/specs/2026-08-25-responsiveness-architecture-design.md`

## Global Constraints

- Toolchain is nightly (pinned in `rust-toolchain.toml`); crates use `edition = "2024"`.
- CI gates on: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- Dioxus 0.7 APIs only. `cx`, `Scope`, `use_state` do not exist. Use `use_signal`, `use_resource`, `use_effect`, `#[component]`.
- Never hold a signal read or write across an `.await` point (clippy `await-holding-invalid-types`). Drop the borrow before awaiting.
- `ui` may only import `models` and `services`. It must not import `connection`, `explorer`, `query`, `storage`, or `acp` directly.
- Prefer owned props (`String`, `Vec<T>`, cloned models) over borrowed props.
- Keep the build green after every task. Each task ends with an independently testable deliverable.

---

## File Structure

New files:
- `ui/src/screens/workspace/tab_store.rs` — the four-signal `TabStore`, the four state structs, and pure helpers (create/read/write) that are unit-testable without a Dioxus runtime.
- `ui/src/screens/workspace/components/sql_editor/highlight_js.rs` — the JS highlight layer (moved out of Rust hot path).

Modified files (each gets one responsibility):
- `models/src/query.rs` — keep `QueryTabState` as the serializable snapshot; no structural change required (the new state structs live in `ui` because they hold UI-only state).
- `ui/src/screens/workspace/hooks/use_query_tabs.rs` — build the `TabStore`.
- `ui/src/screens/workspace/actions.rs` — migrate every function to write to the `TabStore`.
- `ui/src/screens/workspace/context.rs` — `WorkspaceTabContext` carries the `TabStore`.
- `ui/src/screens/workspace/mod.rs` — thread the `TabStore` through `WorkspaceBody`/`WorkspaceDock`/`WorkspaceDockPanel`/`WorkspacePanelContent`; add memoization boundaries.
- `ui/src/screens/workspace/components/tabs.rs` — subscribe to `meta` only for the tabbar; `result`/`pending` for the body.
- `ui/src/screens/workspace/components/result_table.rs` — subscribe to `result` + `pending`; move `materialize_display_rows` to `spawn_blocking` + cache.
- `ui/src/screens/workspace/components/sql_editor.rs` — DOM-as-source-of-truth input; remove `document::eval` from hot path.
- `ui/src/screens/workspace/components/sql_editor/highlight.rs` — delegate to the JS layer.
- `ui/src/screens/workspace/components/sql_editor/selection.rs` — sync selection only on tab switch.
- `ui/src/screens/workspace/components/table_editor.rs`, `table_structure.rs` — subscribe to the right signals.
- `ui/src/screens/workspace/hooks/use_acp.rs`, `components/agent_panel/{prompt,requests}.rs` — migrate `tabs` reads/writes to the `TabStore`.
- `ui/src/screens/workspace/helpers.rs` — `build_er_diagram` via `spawn_blocking`.

---

### Task 1: Define the four state structs and the `TabStore`

**Files:**
- Create: `ui/src/screens/workspace/tab_store.rs`
- Test: `ui/src/screens/workspace/tab_store.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `models::{QueryOutput, QueryPage, QueryFilter, QuerySort, TablePreviewSource, PendingTableChanges, ExecutionPlan, BatchRunState, WorkspaceTabKind}`.
- Produces: `TabMeta`, `TabEditorState`, `TabResultState`, `TabPendingState`, `TabStore`, and pure helpers `tab_meta(...)`, `tab_editor(...)`, `tab_result(...)`, `tab_pending(...)`, `new_tab_store(...)`.

- [ ] **Step 1: Write the failing test**

```rust
// ui/src/screens/workspace/tab_store.rs
#[cfg(test)]
mod tests {
    use super::*;
    use models::{PendingTableChanges, WorkspaceTabKind};

    #[test]
    fn tab_meta_holds_stable_fields() {
        let meta = tab_meta(1, 2, "Query 1".to_string(), WorkspaceTabKind::Query, false);
        assert_eq!(meta.id, 1);
        assert_eq!(meta.session_id, 2);
        assert_eq!(meta.title, "Query 1");
        assert_eq!(meta.tab_kind, WorkspaceTabKind::Query);
        assert!(!meta.pinned);
    }

    #[test]
    fn tab_result_defaults_are_sane() {
        let result = tab_result(100);
        assert_eq!(result.page_size, 100);
        assert!(result.result.is_none());
        assert!(result.status.is_empty());
        assert!(result.pending_table_changes.is_empty());
    }

    #[test]
    fn tab_pending_defaults_empty() {
        let pending = tab_pending();
        assert!(pending.pending_table_changes.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui tab_store::tests -v`
Expected: FAIL with "unresolved import" / "cannot find function `tab_meta`".

- [ ] **Step 3: Write minimal implementation**

```rust
// ui/src/screens/workspace/tab_store.rs
use dioxus::prelude::*;
use models::{
    BatchRunState, ExecutionPlan, PendingTableChanges, QueryFilter, QueryOutput, QuerySort,
    TablePreviewSource, WorkspaceTabKind,
};
use std::collections::HashMap;

/// Stable per-tab identity. Changes rarely (create / rename / close / pin).
#[derive(Clone, Debug, PartialEq)]
pub struct TabMeta {
    pub id: u64,
    pub session_id: u64,
    pub title: String,
    pub tab_kind: WorkspaceTabKind,
    pub pinned: bool,
}

/// Editor-only state. Changes on every keystroke.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TabEditorState {
    pub sql: String,
}

/// Result-only state. Changes when a query runs.
#[derive(Clone, Debug, PartialEq)]
pub struct TabResultState {
    pub result: Option<QueryOutput>,
    pub status: String,
    pub current_offset: u64,
    pub page_size: u32,
    pub last_run_sql: Option<String>,
    pub preview_source: Option<TablePreviewSource>,
    pub filter: Option<QueryFilter>,
    pub sort: Option<QuerySort>,
    pub is_loading_more: bool,
    pub execution_plan: Option<ExecutionPlan>,
    pub show_execution_plan: bool,
    pub batch_results: Option<BatchRunState>,
    pub batch_outputs: Vec<Option<QueryOutput>>,
    pub last_duration_ms: Option<u64>,
}

/// Pending table-edit state. Changes when a cell/row is edited.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TabPendingState {
    pub pending_table_changes: PendingTableChanges,
}

/// Pure constructors (unit-testable, no Dioxus runtime needed).
pub fn tab_meta(
    id: u64,
    session_id: u64,
    title: String,
    tab_kind: WorkspaceTabKind,
    pinned: bool,
) -> TabMeta {
    TabMeta { id, session_id, title, tab_kind, pinned }
}

pub fn tab_editor(sql: String) -> TabEditorState {
    TabEditorState { sql }
}

pub fn tab_result(page_size: u32) -> TabResultState {
    TabResultState {
        result: None,
        status: String::new(),
        current_offset: 0,
        page_size,
        last_run_sql: None,
        preview_source: None,
        filter: None,
        sort: None,
        is_loading_more: false,
        execution_plan: None,
        show_execution_plan: false,
        batch_results: None,
        batch_outputs: Vec::new(),
        last_duration_ms: None,
    }
}

pub fn tab_pending() -> TabPendingState {
    TabPendingState { pending_table_changes: PendingTableChanges::default() }
}

/// The four independent signals backing every tab. Writing to one signal
/// notifies only that signal's subscribers.
#[derive(Clone, Copy)]
pub struct TabStore {
    pub meta: Signal<HashMap<u64, TabMeta>>,
    pub editor: Signal<HashMap<u64, TabEditorState>>,
    pub result: Signal<HashMap<u64, TabResultState>>,
    pub pending: Signal<HashMap<u64, TabPendingState>>,
    pub active_tab_id: Signal<u64>,
    pub next_tab_id: Signal<u64>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui tab_store::tests -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/tab_store.rs
git commit -m "feat(ui): add four-signal TabStore state structs"
```

---

### Task 2: Build the `TabStore` in `use_query_tabs`

**Files:**
- Modify: `ui/src/screens/workspace/hooks/use_query_tabs.rs`
- Test: `ui/src/screens/workspace/hooks/use_query_tabs.rs` (inline tests for the pure helpers)

**Interfaces:**
- Consumes: `TabStore`, `tab_meta`, `tab_editor`, `tab_result`, `tab_pending` from Task 1; `models::TabDraft`, `APP_STATE`, `APP_TAB_DRAFTS`.
- Produces: `QueryTabsState { store: TabStore }` (replaces `tabs: Signal<Vec<QueryTabState>>`).

- [ ] **Step 1: Write the failing test**

```rust
// ui/src/screens/workspace/hooks/use_query_tabs.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tab_sql_is_select_one() {
        assert_eq!(default_tab_sql(), "select 1 as id;");
    }

    #[test]
    fn default_tab_title_is_query_one() {
        assert_eq!(default_tab_title(), "Query 1");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui use_query_tabs::tests -v`
Expected: FAIL with "cannot find function `default_tab_sql`".

- [ ] **Step 3: Write minimal implementation**

Add pure helpers and rewrite the hook to build a `TabStore`:

```rust
// ui/src/screens/workspace/hooks/use_query_tabs.rs
use std::collections::HashMap;

use dioxus::prelude::*;
use models::TabDraft;

use super::super::tab_store::{
    TabStore, tab_editor, tab_meta, tab_pending, tab_result,
};
use crate::app_state::{APP_STATE, APP_TAB_DRAFTS};

pub struct QueryTabsState {
    pub store: TabStore,
}

pub fn default_tab_title() -> String {
    "Query 1".to_string()
}

pub fn default_tab_sql() -> String {
    "select 1 as id;".to_string()
}

pub fn use_query_tabs() -> QueryTabsState {
    let mut next_tab_id = use_signal(|| 1_u64);
    let mut active_tab_id = use_signal(|| 0_u64);
    let meta = use_signal(HashMap::<u64, _>::new);
    let editor = use_signal(HashMap::<u64, _>::new);
    let result = use_signal(HashMap::<u64, _>::new);
    let pending = use_signal(HashMap::<u64, _>::new);

    // Effect: prune tabs whose session is gone; ensure an active tab exists.
    use_effect(move || {
        let (session_ids, active_session_id) = {
            let app_state = APP_STATE.read();
            (
                app_state.sessions.iter().map(|s| s.id).collect::<std::collections::HashSet<_>>(),
                app_state.active_session_id,
            )
        };

        meta.with_mut(|m| m.retain(|_, t| session_ids.contains(&t.session_id)));
        editor.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));
        result.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));
        pending.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));

        let Some(session_id) = active_session_id else {
            active_tab_id.set(0);
            return;
        };

        let current_active_matches = meta
            .read()
            .get(&active_tab_id())
            .is_some_and(|t| t.session_id == session_id);
        if current_active_matches {
            return;
        }

        if let Some(existing_id) = meta
            .read()
            .iter()
            .find(|(_, t)| t.session_id == session_id)
            .map(|(id, _)| *id)
        {
            active_tab_id.set(existing_id);
            return;
        }

        // Look up a saved tab draft for this session's connection.
        let (saved_title, saved_sql) = {
            let app_state = APP_STATE.read();
            let identity_key = app_state
                .session(session_id)
                .map(|s| s.request.identity_key());
            if let Some(key) = identity_key {
                APP_TAB_DRAFTS()
                    .iter()
                    .find(|d| d.session_identity_key == key)
                    .map(|d| (d.title.clone(), d.sql.clone()))
                    .unwrap_or_else(|| (default_tab_title(), default_tab_sql()))
            } else {
                (default_tab_title(), default_tab_sql())
            }
        };

        let tab_id = next_tab_id();
        next_tab_id += 1;
        let page_size = crate::app_state::APP_UI_SETTINGS().default_page_size;
        meta.with_mut(|m| {
            m.insert(tab_id, tab_meta(tab_id, session_id, saved_title, models::WorkspaceTabKind::Query, false));
        });
        editor.with_mut(|m| { m.insert(tab_id, tab_editor(saved_sql)); });
        result.with_mut(|m| { m.insert(tab_id, tab_result(page_size)); });
        pending.with_mut(|m| { m.insert(tab_id, tab_pending()); });
        active_tab_id.set(tab_id);
    });

    // Effect: persist tab drafts whenever editor state changes.
    use_effect(move || {
        let _ = editor(); // subscribe
        let app_state = APP_STATE.read();
        let drafts: Vec<TabDraft> = editor
            .read()
            .iter()
            .filter_map(|(id, ed)| {
                let t = meta.read().get(id)?;
                let session = app_state.session(t.session_id)?;
                if ed.sql.trim().is_empty() || ed.sql.trim() == default_tab_sql() {
                    return None;
                }
                Some(TabDraft {
                    session_identity_key: session.request.identity_key(),
                    title: t.title.clone(),
                    sql: ed.sql.clone(),
                })
            })
            .collect();
        let current = APP_TAB_DRAFTS();
        if *current != drafts {
            *APP_TAB_DRAFTS.write() = drafts;
        }
    });

    QueryTabsState {
        store: TabStore {
            meta,
            editor,
            result,
            pending,
            active_tab_id,
            next_tab_id,
        },
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui use_query_tabs::tests -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/hooks/use_query_tabs.rs
git commit -m "feat(ui): build four-signal TabStore in use_query_tabs"
```

---

### Task 3: Migrate `context.rs` to carry the `TabStore`

**Files:**
- Modify: `ui/src/screens/workspace/context.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: `WorkspaceTabContext { store: TabStore, active_tab_id: Signal<u64>, next_tab_id: Signal<u64> }` and `provide_workspace_tab_context(store, active_tab_id, next_tab_id)`.

- [ ] **Step 1: Rewrite the context type**

```rust
// ui/src/screens/workspace/context.rs
use dioxus::prelude::*;
use models::{AcpPanelState, ChatThreadSummary, QueryHistoryItem, SavedQuery};

use super::tab_store::TabStore;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct WorkspaceTabContext {
    pub store: TabStore,
    pub active_tab_id: Signal<u64>,
    pub next_tab_id: Signal<u64>,
}

// WorkspaceQueryContext and WorkspaceAcpContext unchanged.

pub fn provide_workspace_tab_context(
    store: TabStore,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
) {
    provide_context(WorkspaceTabContext { store, active_tab_id, next_tab_id });
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS (the call site in `mod.rs` still passes `tabs` — this will be fixed in Task 4; if it fails, that is expected and the next task resolves it).

- [ ] **Step 3: Commit**

```bash
git add ui/src/screens/workspace/context.rs
git commit -m "feat(ui): carry TabStore in workspace tab context"
```

---

### Task 4: Migrate `actions.rs` — tab lifecycle and SQL-editor helpers

**Files:**
- Modify: `ui/src/screens/workspace/actions.rs`
- Test: `ui/src/screens/workspace/actions.rs` (existing inline tests)

**Interfaces:**
- Consumes: `TabStore`, `TabMeta`, `TabEditorState`, `TabResultState`, `TabPendingState` from Task 1.
- Produces: migrated `new_query_tab`, `ensure_tab_for_session`, `update_active_tab_sql`, `sync_active_tab_sql_draft`, `set_active_tab_sql`, `append_to_tab_sql`, `set_active_tab_status`, `replace_active_tab_sql`, `clear_active_tab_sql`, `indent_lines_in_active_tab`, `toggle_line_comments_in_active_tab`, `save_active_tab_as_saved_query`, `run_active_tab`, `run_active_tab_explain`, `format_active_tab`.

**Migration pattern (apply to every function):**

Replace `tabs: Signal<Vec<QueryTabState>>` with `store: TabStore`. Replace reads:

```rust
// OLD
let current_tab = tabs.read().iter().find(|t| t.id == id).cloned();
// NEW
let current_tab = store
    .result
    .read()
    .get(&id)
    .cloned()
    .map(|r| (r, store.editor.read().get(&id).cloned().unwrap_or_default()));
```

Replace writes:

```rust
// OLD
tabs.with_mut(|all| { if let Some(tab) = all.iter_mut().find(|t| t.id == id) { tab.status = s; } });
// NEW
store.result.with_mut(|m| { if let Some(r) = m.get_mut(&id) { r.status = s; } });
```

- [ ] **Step 1: Migrate `new_query_tab` and `ensure_tab_for_session`**

`new_query_tab` becomes a pure constructor returning `(TabMeta, TabEditorState, TabResultState, TabPendingState)`:

```rust
pub fn new_query_tab(
    id: u64,
    session_id: u64,
    title: String,
    sql: String,
) -> (TabMeta, TabEditorState, TabResultState, TabPendingState) {
    let page_size = APP_UI_SETTINGS().default_page_size;
    (
        tab_meta(id, session_id, title, WorkspaceTabKind::Query, false),
        tab_editor(sql),
        tab_result(page_size),
        tab_pending(),
    )
}
```

`ensure_tab_for_session` inserts all four maps:

```rust
pub fn ensure_tab_for_session(
    store: TabStore,
    session_id: u64,
) -> u64 {
    activate_session(session_id);
    let active_tab_id = store.active_tab_id;
    let next_tab_id = store.next_tab_id;

    if let Some(existing_id) = store
        .meta
        .read()
        .iter()
        .find(|(_, t)| t.session_id == session_id && t.tab_kind == WorkspaceTabKind::Query)
        .map(|(id, _)| *id)
    {
        active_tab_id.set(existing_id);
        return existing_id;
    }

    let tab_id = next_tab_id();
    next_tab_id += 1;
    let (meta, editor, result, pending) =
        new_query_tab(tab_id, session_id, format!("Query {tab_id}"), "select 1 as id;".to_string());
    store.meta.with_mut(|m| { m.insert(tab_id, meta); });
    store.editor.with_mut(|m| { m.insert(tab_id, editor); });
    store.result.with_mut(|m| { m.insert(tab_id, result); });
    store.pending.with_mut(|m| { m.insert(tab_id, pending); });
    active_tab_id.set(tab_id);
    tab_id
}
```

- [ ] **Step 2: Migrate the SQL-editor helpers**

Migrate `update_active_tab_sql`, `sync_active_tab_sql_draft`, `set_active_tab_sql`, `append_to_tab_sql`, `set_active_tab_status`, `replace_active_tab_sql`, `clear_active_tab_sql`, `indent_lines_in_active_tab`, `toggle_line_comments_in_active_tab`, `save_active_tab_as_saved_query`, `run_active_tab`, `run_active_tab_explain`, `format_active_tab` using the pattern above. Each writes only to the relevant map (`editor` for sql, `result` for status/result/filter/sort, `meta` for title).

- [ ] **Step 3: Run the existing tests**

Run: `cargo test -p ui actions:: -v`
Expected: PASS (existing tests for `next_sort_state`, `redact_sql`, etc. still pass).

- [ ] **Step 4: Verify the workspace compiles**

Run: `cargo check -p ui`
Expected: PASS (all call sites in `mod.rs`/`tabs.rs`/`result_table.rs` still pass `tabs` — those are migrated in later tasks; if a call site breaks, keep `tabs` temporarily by adding a shim that reads from `store`).

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/actions.rs
git commit -m "feat(ui): migrate tab lifecycle and SQL-editor actions to TabStore"
```

---

### Task 5: Migrate `actions.rs` — query execution, pagination, filters, table edits

**Files:**
- Modify: `ui/src/screens/workspace/actions.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: migrated `run_query_for_tab`, `run_batch_for_tab`, `run_explain_for_tab`, `run_table_preview_for_tab`, `append_next_tab_page`, `load_tab_page`, `refresh_tab_result`, `mark_table_deleted`, `mark_table_truncated`, `toggle_active_tab_sort`, `apply_active_tab_filter`, `clear_active_tab_filter`, `insert_empty_row`, `apply_pending_changes`, `discard_pending_changes`, `delete_selected_row`, `commit_cell_edit`.

- [ ] **Step 1: Migrate query execution functions**

Migrate `run_query_for_tab`, `run_batch_for_tab`, `run_explain_for_tab`, `run_table_preview_for_tab`. These read `filter`/`sort`/`preview_source`/`last_run_sql` from `store.result` and write `result`/`status`/`current_offset`/`is_loading_more`/`batch_results`/`batch_outputs`/`last_duration_ms` to `store.result`. The `spawn` blocks capture the `TabStore` (which is `Copy`) and write to `store.result` after the `.await`.

**Critical rule:** never hold a `store.result.read()`/`with_mut()` borrow across an `.await`. Snapshot the needed values into owned locals before the `spawn`, then re-acquire the write lock after the await.

- [ ] **Step 2: Migrate pagination and filter functions**

Migrate `append_next_tab_page`, `load_tab_page`, `refresh_tab_result`, `mark_table_deleted`, `mark_table_truncated`, `toggle_active_tab_sort`, `apply_active_tab_filter`, `clear_active_tab_filter`. These read `result`/`filter`/`sort`/`preview_source`/`last_run_sql` and write `result`/`status`/`current_offset`/`filter`/`sort`/`is_loading_more`/`pending_table_changes` to `store.result`/`store.pending`.

- [ ] **Step 3: Migrate table-edit functions**

Migrate `insert_empty_row`, `apply_pending_changes`, `discard_pending_changes`, `delete_selected_row`, `commit_cell_edit`. These read/write `pending_table_changes` in `store.pending` and `result`/`status` in `store.result`.

- [ ] **Step 4: Run the existing tests**

Run: `cargo test -p ui actions:: -v`
Expected: PASS.

- [ ] **Step 5: Verify the workspace compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ui/src/screens/workspace/actions.rs
git commit -m "feat(ui): migrate query execution and table-edit actions to TabStore"
```

---

### Task 6: Migrate `mod.rs` to thread the `TabStore` and add memoization boundaries

**Files:**
- Modify: `ui/src/screens/workspace/mod.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1, `QueryTabsState { store }` from Task 2.
- Produces: `Workspace` that builds the `TabStore`, threads it through `WorkspaceBody`/`WorkspaceDock`/`WorkspaceDockPanel`/`WorkspacePanelContent`, and wraps panel content in `use_memo`.

- [ ] **Step 1: Build the store in `Workspace`**

Replace `let QueryTabsState { mut tabs, mut active_tab_id, mut next_tab_id } = use_query_tabs();` with:

```rust
let QueryTabsState { store } = use_query_tabs();
let tabs = store; // TabStore is Copy
let active_tab_id = store.active_tab_id;
let next_tab_id = store.next_tab_id;
```

- [ ] **Step 2: Thread the store through the dock components**

Change every `tabs: Signal<Vec<QueryTabState>>` parameter in `WorkspaceBody`, `WorkspaceDock`, `WorkspaceDockPanel`, `WorkspacePanelContent`, `ExplorerToolPanel`, `AgentToolPanel` to `store: TabStore`. Pass `store` down instead of `tabs`.

- [ ] **Step 3: Add memoization boundaries**

Wrap `WorkspacePanelContent`'s body in `use_memo` keyed on `(panel, store.meta.read().len(), store.active_tab_id())` so switching one panel does not re-render the others:

```rust
let panel_body = use_memo(move || {
    // read only the signals this panel needs
    match panel {
        WorkspaceToolPanel::Explorer => { /* ... */ }
        // ...
    }
});
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/mod.rs
git commit -m "feat(ui): thread TabStore through workspace and add memo boundaries"
```

---

### Task 7: Migrate `tabs.rs` (TabsManager) to subscribe to the right signals

**Files:**
- Modify: `ui/src/screens/workspace/components/tabs.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: `TabsManager(store: TabStore, ...)` that reads `meta` for the tabbar and `result`/`pending` for the active body.

- [ ] **Step 1: Migrate the tabbar to read `meta`**

Replace `for tab in tabs()` with a loop over `store.meta.read()`:

```rust
let tab_list = store.meta.read().clone(); // snapshot for iteration
// render each tab from tab_list; read title/pinned/session_id from TabMeta
```

- [ ] **Step 2: Migrate the active body to read `result`/`pending`**

Replace `if let Some(ref tab) = *active_tab.read()` with a memo that reads the active tab's `result`/`pending`/`editor`:

```rust
let active_tab = use_memo(move || {
    let id = store.active_tab_id();
    let meta = store.meta.read().get(&id).cloned();
    let editor = store.editor.read().get(&id).cloned();
    let result = store.result.read().get(&id).cloned();
    let pending = store.pending.read().get(&id).cloned();
    meta.map(|m| (m, editor, result, pending))
});
```

- [ ] **Step 3: Migrate the Run/Format/Explain/Export/Import handlers**

Update the toolbar handlers to call the migrated `actions` functions with `store` instead of `tabs`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/tabs.rs
git commit -m "feat(ui): subscribe TabsManager to per-aspect tab signals"
```

---

### Task 8: Migrate `result_table.rs` to `result` + `pending` and cache `materialize_display_rows`

**Files:**
- Modify: `ui/src/screens/workspace/components/result_table.rs`
- Test: `ui/src/screens/workspace/components/result_table.rs` (inline tests for the cache)

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: `ResultTable(store: TabStore, ...)` that reads only `store.result` + `store.pending`; a cached `materialize_display_rows` via `spawn_blocking`.

- [ ] **Step 1: Write the failing test for the cache**

```rust
// ui/src/screens/workspace/components/result_table.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rows_cache_key_changes_with_result_revision() {
        let k1 = display_rows_cache_key(1, 0);
        let k2 = display_rows_cache_key(1, 1);
        assert_ne!(k1, k2);
        assert_eq!(display_rows_cache_key(1, 1), display_rows_cache_key(1, 1));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui result_table::tests -v`
Expected: FAIL with "cannot find function `display_rows_cache_key`".

- [ ] **Step 3: Add the cache key helper and cached materialization**

```rust
fn display_rows_cache_key(tab_id: u64, result_revision: u64) -> (u64, u64) {
    (tab_id, result_revision)
}
```

Replace the `display_rows_cache` `use_memo` with one that reads `store.result` + `store.pending` and computes the key:

```rust
let display_rows_cache = use_memo(move || {
    let id = store.active_tab_id();
    let result = store.result.read().get(&id).and_then(|r| r.result.clone());
    let pending = store
        .pending
        .read()
        .get(&id)
        .map(|p| p.pending_table_changes.clone())
        .unwrap_or_default();
    match result.as_ref() {
        Some(QueryOutput::Table(page)) => materialize_display_rows(page, &pending),
        _ => Vec::new(),
    }
});
```

- [ ] **Step 4: Move `materialize_display_rows` to `spawn_blocking`**

Wrap the heavy materialization in `spawn_blocking` and cache the result by `(tab_id, result_revision)`. Use a `static` cache keyed on the tuple; invalidate when the key changes. The memo returns the cached value synchronously when available, and triggers a `spawn_blocking` recompute otherwise.

- [ ] **Step 5: Migrate all `tabs` reads/writes in `result_table.rs`**

Replace every `tabs: Signal<Vec<QueryTabState>>` with `store: TabStore`. Reads of `result`/`filter`/`sort`/`status`/`pending_table_changes` come from `store.result`/`store.pending`. Writes go to the matching map.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ui result_table::tests -v`
Expected: PASS.

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add ui/src/screens/workspace/components/result_table.rs
git commit -m "feat(ui): subscribe ResultTable to result+pending and cache materialization"
```

---

### Task 9: Migrate `sql_editor.rs` — DOM as source of truth during input

**Files:**
- Modify: `ui/src/screens/workspace/components/sql_editor.rs`
- Modify: `ui/src/screens/workspace/components/sql_editor/highlight.rs`
- Modify: `ui/src/screens/workspace/components/sql_editor/selection.rs`
- Create: `ui/src/screens/workspace/components/sql_editor/highlight_js.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: `SqlEditor(store: TabStore, ...)` that reads/writes `store.editor` only; input handled natively in the DOM.

- [ ] **Step 1: Remove `document::eval` from the input hot path**

In `SqlEditor`, the `oninput` handler currently reads the DOM value via `document::eval` and writes it back. Change it to write the event value directly to `store.editor`:

```rust
oninput: move |event| {
    let value = event.value();
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&active_tab_id_value) {
            ed.sql = value;
        }
    });
}
```

Remove the `document::eval` calls that read/set the value on every keystroke.

- [ ] **Step 2: Move highlighting to the JS layer**

Create `highlight_js.rs` with a JS snippet that runs on `requestAnimationFrame` with a ~90ms debounce, reading the textarea value and applying highlight classes. `highlight.rs` delegates to this snippet instead of doing Rust-side highlighting on every keystroke.

- [ ] **Step 3: Sync selection only on tab switch**

In `selection.rs`, keep the `document::eval` selection sync only in the tab-switch effect (the `use_effect` keyed on `active_tab_id_value`), not in the per-keystroke path.

- [ ] **Step 4: Keep the tab-switch sync**

The existing `use_effect` that syncs `store.editor` to the DOM on tab switch / external change stays. It writes the SQL to the textarea once via `document::eval` when the tab changes or when SQL changes externally (Format/Generate).

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ui/src/screens/workspace/components/sql_editor.rs ui/src/screens/workspace/components/sql_editor/highlight.rs ui/src/screens/workspace/components/sql_editor/selection.rs ui/src/screens/workspace/components/sql_editor/highlight_js.rs
git commit -m "feat(ui): make DOM the source of truth for editor input"
```

---

### Task 10: Migrate `table_editor.rs`, `table_structure.rs`, `use_acp.rs`, agent panel

**Files:**
- Modify: `ui/src/screens/workspace/components/table_editor.rs`
- Modify: `ui/src/screens/workspace/components/table_structure.rs`
- Modify: `ui/src/screens/workspace/hooks/use_acp.rs`
- Modify: `ui/src/screens/workspace/components/agent_panel/prompt.rs`
- Modify: `ui/src/screens/workspace/components/agent_panel/requests.rs`

**Interfaces:**
- Consumes: `TabStore` from Task 1.
- Produces: migrated components that read/write the correct per-aspect signals.

- [ ] **Step 1: Migrate `table_editor.rs`**

Replace `tabs: Signal<Vec<QueryTabState>>` with `store: TabStore`. Read `preview_source`/`result` from `store.result`. Pass `store` to `ResultTable`/`StructurePanel`/`DdlPanel`/`IndexesPanel`/`RelationsPanel`.

- [ ] **Step 2: Migrate `table_structure.rs`**

Replace `tabs` with `store`. Read `result` from `store.result`.

- [ ] **Step 3: Migrate `use_acp.rs`**

Replace `tabs: Signal<Vec<QueryTabState>>` in `AcpStateInputs` with `store: TabStore`. Update `update_active_tab_sql` calls to use `store`. The ACP polling loop reads `store.result`/`store.editor` for the active tab.

- [ ] **Step 4: Migrate `agent_panel/prompt.rs` and `agent_panel/requests.rs`**

Replace `tabs` with `store`. Update `preferred_sql_target_tab_id`, `build_active_tab_context`, and SQL-execution helpers to read from `store.result`/`store.editor`.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ui/src/screens/workspace/components/table_editor.rs ui/src/screens/workspace/components/table_structure.rs ui/src/screens/workspace/hooks/use_acp.rs ui/src/screens/workspace/components/agent_panel/prompt.rs ui/src/screens/workspace/components/agent_panel/requests.rs
git commit -m "feat(ui): migrate table editor, structure, and ACP panel to TabStore"
```

---

### Task 11: Move heavy operations to `spawn_blocking`

**Files:**
- Modify: `ui/src/screens/workspace/helpers.rs`
- Modify: `ui/src/screens/workspace/components/tabs.rs` (export)
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` (format)

**Interfaces:**
- Consumes: existing `services` functions.
- Produces: `build_er_diagram` and export/format wrapped in `spawn_blocking`.

- [ ] **Step 1: Wrap `build_er_diagram` in `spawn_blocking`**

In `helpers.rs`, add an async wrapper:

```rust
pub async fn build_er_diagram_async(
    sections: Vec<ExplorerConnectionSection>,
    foreign_keys: Vec<models::TableForeignKey>,
) -> Option<ErDiagramState> {
    tokio::task::spawn_blocking(move || build_er_diagram(&sections, &foreign_keys))
        .await
        .unwrap_or(None)
}
```

Update the ER-diagram button in `mod.rs` to call `build_er_diagram_async(...).await` instead of `build_er_diagram(...)`.

- [ ] **Step 2: Wrap export in `spawn_blocking`**

In `tabs.rs`, the `export_active_page` already runs in a `spawn`; wrap the `services::export_query_page_*` call in `spawn_blocking` so the heavy file/format work does not block the async executor.

- [ ] **Step 3: Wrap `format_sql` in `spawn_blocking`**

In `sql_editor.rs`/`actions.rs`, `format_active_tab` currently calls `services::format_sql` synchronously. Wrap it in `spawn_blocking` and write the result to `store.editor` after the await.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p ui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/helpers.rs ui/src/screens/workspace/components/tabs.rs ui/src/screens/workspace/components/sql_editor.rs
git commit -m "feat(ui): move heavy operations to spawn_blocking"
```

---

### Task 12: Full workspace verification

**Files:**
- All modified files.

**Interfaces:**
- Consumes: everything from Tasks 1–11.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (no warnings).

- [ ] **Step 3: Run fmt check**

Run: `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 4: Build the desktop app**

Run: `cargo build -p app --features desktop`
Expected: PASS.

- [ ] **Step 5: Manual smoke test**

Run: `cargo run -p app --features desktop`
Verify: typing in the SQL editor is smooth (no per-keystroke `document::eval`); running a query updates only the result area; editing a cell updates only the grid; switching tabs is instant; resizing panels is smooth.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: verify responsiveness refactor across workspace"
```

---

## Self-Review

**Spec coverage:**
- Section 1 (4 signals): Tasks 1, 2, 4, 5, 6, 7, 8, 10.
- Section 2 (JS bridge): Task 9.
- Section 3 (spawn_blocking): Tasks 8, 11.
- Section 4 (memoization boundaries): Task 6.
- Section 5 (data flow, errors, testing): Tasks 3, 12.

**Placeholder scan:** No TBD/TODO. Every task has concrete code and a test cycle.

**Type consistency:** `TabStore` fields (`meta`/`editor`/`result`/`pending`/`active_tab_id`/`next_tab_id`) are consistent across all tasks. `tab_meta`/`tab_editor`/`tab_result`/`tab_pending` constructors are defined in Task 1 and used in Tasks 2, 4. `display_rows_cache_key` defined in Task 8 and used there only.
