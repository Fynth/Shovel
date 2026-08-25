use dioxus::prelude::*;
use models::{
    BatchRunState,
    ExecutionPlan,
    PendingTableChanges,
    QueryFilter,
    QueryOutput,
    QuerySort,
    QueryTabState,
    TablePreviewSource,
    WorkspaceTabKind,
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
    TabMeta {
        id,
        session_id,
        title,
        tab_kind,
        pinned,
    }
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
    TabPendingState {
        pending_table_changes: PendingTableChanges::default(),
    }
}

/// The four independent signals backing every tab. Writing to one signal
/// notifies only that signal's subscribers.
#[derive(Clone, Copy, PartialEq)]
pub struct TabStore {
    pub meta: Signal<HashMap<u64, TabMeta>>,
    pub editor: Signal<HashMap<u64, TabEditorState>>,
    pub result: Signal<HashMap<u64, TabResultState>>,
    pub pending: Signal<HashMap<u64, TabPendingState>>,
    pub active_tab_id: Signal<u64>,
    pub next_tab_id: Signal<u64>,
}

impl TabStore {
    /// Current active tab id.
    pub fn active_tab_id(&self) -> u64 {
        (self.active_tab_id)()
    }

    /// Current next-tab counter value.
    pub fn next_tab_id(&self) -> u64 {
        (self.next_tab_id)()
    }
}

/// Materialize a full [`QueryTabState`] snapshot for a tab id from the
/// four per-aspect maps. Returns `None` when the tab's meta is missing.
/// Used by the few remaining call sites that still operate on the
/// aggregate snapshot (recently-closed stack, `refresh_tab_result`,
/// `load_tab_page`, `append_next_tab_page`).
pub fn materialize_tab_state(store: TabStore, tab_id: u64) -> Option<QueryTabState> {
    let meta = store.meta.read().get(&tab_id).cloned()?;
    let editor = store.editor.read().get(&tab_id).cloned();
    let result = store.result.read().get(&tab_id).cloned();
    let pending = store.pending.read().get(&tab_id).cloned();

    Some(QueryTabState {
        id: tab_id,
        session_id: meta.session_id,
        title: meta.title,
        sql: editor.map(|e| e.sql).unwrap_or_default(),
        status: result
            .as_ref()
            .map(|r| r.status.clone())
            .unwrap_or_default(),
        result: result.as_ref().and_then(|r| r.result.clone()),
        current_offset: result.as_ref().map(|r| r.current_offset).unwrap_or(0),
        page_size: result.as_ref().map(|r| r.page_size).unwrap_or(0),
        last_run_sql: result.as_ref().and_then(|r| r.last_run_sql.clone()),
        preview_source: result.as_ref().and_then(|r| r.preview_source.clone()),
        filter: result.as_ref().and_then(|r| r.filter.clone()),
        sort: result.as_ref().and_then(|r| r.sort.clone()),
        tab_kind: meta.tab_kind,
        is_loading_more: result.as_ref().map(|r| r.is_loading_more).unwrap_or(false),
        pending_table_changes: pending.map(|p| p.pending_table_changes).unwrap_or_default(),
        execution_plan: result.as_ref().and_then(|r| r.execution_plan.clone()),
        show_execution_plan: result
            .as_ref()
            .map(|r| r.show_execution_plan)
            .unwrap_or(false),
        batch_results: result.as_ref().and_then(|r| r.batch_results.clone()),
        batch_outputs: result
            .as_ref()
            .map(|r| r.batch_outputs.clone())
            .unwrap_or_default(),
        last_duration_ms: result.as_ref().and_then(|r| r.last_duration_ms),
        pinned: meta.pinned,
    })
}

/// Write a full [`QueryTabState`] snapshot back into the four per-aspect
/// maps. Used by the recently-closed "Reopen" flow and tab duplication,
/// which operate on the aggregate snapshot.
pub fn restore_tab_state(mut store: TabStore, tab: QueryTabState) {
    store.meta.with_mut(|m| {
        m.insert(
            tab.id,
            TabMeta {
                id: tab.id,
                session_id: tab.session_id,
                title: tab.title,
                tab_kind: tab.tab_kind,
                pinned: tab.pinned,
            },
        );
    });
    store.editor.with_mut(|m| {
        m.insert(tab.id, TabEditorState { sql: tab.sql });
    });
    store.result.with_mut(|m| {
        m.insert(
            tab.id,
            TabResultState {
                result: tab.result,
                status: tab.status,
                current_offset: tab.current_offset,
                page_size: tab.page_size,
                last_run_sql: tab.last_run_sql,
                preview_source: tab.preview_source,
                filter: tab.filter,
                sort: tab.sort,
                is_loading_more: tab.is_loading_more,
                execution_plan: tab.execution_plan,
                show_execution_plan: tab.show_execution_plan,
                batch_results: tab.batch_results,
                batch_outputs: tab.batch_outputs,
                last_duration_ms: tab.last_duration_ms,
            },
        );
    });
    store.pending.with_mut(|m| {
        m.insert(
            tab.id,
            TabPendingState {
                pending_table_changes: tab.pending_table_changes,
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::WorkspaceTabKind;

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
    }

    #[test]
    fn tab_pending_defaults_empty() {
        let pending = tab_pending();
        assert!(pending.pending_table_changes.is_empty());
    }
}
