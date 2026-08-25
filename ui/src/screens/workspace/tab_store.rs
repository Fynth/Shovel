use dioxus::prelude::*;
use models::{
    BatchRunState,
    ExecutionPlan,
    PendingTableChanges,
    QueryFilter,
    QueryOutput,
    QuerySort,
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
