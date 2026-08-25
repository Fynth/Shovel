use std::collections::{HashMap, HashSet};

use crate::{
    app_state::{
        APP_THEME,
        actions::find_action,
        context_menu::{ContextMenuItem, open_context_menu},
    },
    screens::workspace::{
        actions::{
            append_next_tab_page,
            apply_active_tab_filter,
            clear_active_tab_filter,
            load_tab_page,
            read_only_mode_block_status,
            read_only_mode_enabled,
            refresh_tab_result,
            rows_toolbar_summary,
            set_active_tab_status,
            tab_connection_or_error,
            toggle_active_tab_sort,
        },
        components::{
            ActionIcon,
            IconButton,
            IconGlyph,
            ResultChart,
            ValueEditor,
            ValueEditorMode,
            ValueEditorState,
            copy_formats::{
                format_all_rows_csv,
                format_all_rows_json,
                format_all_rows_markdown,
                format_row_csv,
                format_row_json,
                format_row_tsv,
            },
        },
        helpers::format_duration,
        tab_store::{TabResultState, TabStore},
    },
    windows,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use models::{
    EditableTableContext,
    PendingCellChange,
    PendingDeleteRow,
    PendingInsertRow,
    PendingTableChanges,
    QueryFilter,
    QueryFilterMode,
    QueryFilterOperator,
    QueryFilterRule,
    QueryOutput,
    QuerySort,
};

/// Resolve the qualified table name backing the active tab's result, if any.
/// Falls back to the `<table>` placeholder so generated INSERT statements stay
/// editable when the result comes from an arbitrary query.
fn active_table_name(store: TabStore) -> String {
    store
        .result
        .read()
        .get(&store.active_tab_id())
        .and_then(|tab| tab.preview_source.as_ref())
        .map(|source| source.qualified_name.clone())
        .unwrap_or_else(|| "<table>".to_string())
}

#[derive(Clone, PartialEq)]
struct EditingCell {
    row_ref: EditableRowRef,
    col_index: usize,
    value: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowDetailsView {
    Fields,
    Json,
}

#[derive(Clone, PartialEq, Eq)]
enum EditableRowRef {
    Existing(String),
    PendingInsert(u64),
}

#[derive(Clone, PartialEq)]
struct DisplayRow {
    row_ref: EditableRowRef,
    values: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultsStateVariant {
    Empty,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultsStateAction {
    None,
    RunAgain,
    Retry,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResultViewMode {
    Table,
    Records,
    Single,
    Details,
}

impl ResultViewMode {
    fn label(self) -> &'static str {
        match self {
            ResultViewMode::Table => "Table",
            ResultViewMode::Records => "Records",
            ResultViewMode::Single => "Single",
            ResultViewMode::Details => "Details",
        }
    }

    fn catalog_id(self) -> crate::app_state::actions::ActionId {
        use crate::app_state::actions::{
            ACTION_VIEW_DETAILS,
            ACTION_VIEW_RECORDS,
            ACTION_VIEW_SINGLE_RECORD,
            ACTION_VIEW_TABLE,
        };
        match self {
            ResultViewMode::Table => ACTION_VIEW_TABLE,
            ResultViewMode::Records => ACTION_VIEW_RECORDS,
            ResultViewMode::Single => ACTION_VIEW_SINGLE_RECORD,
            ResultViewMode::Details => ACTION_VIEW_DETAILS,
        }
    }
}

/// Coherent empty/error state block for the result grid. Renders an icon + title + body and, when applicable, a Retry action that calls `refresh_tab_result`.
#[component]
fn ResultsStateBlock(
    variant: ResultsStateVariant,
    title: String,
    body: Option<String>,
    action: ResultsStateAction,
    store: TabStore,
) -> Element {
    let mut class_name = match variant {
        ResultsStateVariant::Empty => "results__state results__state--empty".to_string(),
        ResultsStateVariant::Error => "results__state results__state--error".to_string(),
    };
    let outer_class = match variant {
        ResultsStateVariant::Empty => "results".to_string(),
        ResultsStateVariant::Error => "results results--error".to_string(),
    };
    let icon = match variant {
        ResultsStateVariant::Empty => ActionIcon::Details,
        ResultsStateVariant::Error => ActionIcon::Delete,
    };

    let can_retry = can_retry_active_tab(store);
    let show_retry_button = match action {
        ResultsStateAction::None => false,
        ResultsStateAction::RunAgain | ResultsStateAction::Retry => can_retry,
    };
    let retry_label = match action {
        ResultsStateAction::RunAgain => "Run again",
        ResultsStateAction::Retry => "Retry",
        ResultsStateAction::None => "Run again",
    };
    let retry_aria = match action {
        ResultsStateAction::RunAgain => "Run the query again".to_string(),
        ResultsStateAction::Retry => "Retry the query".to_string(),
        ResultsStateAction::None => String::new(),
    };
    if show_retry_button {
        class_name.push_str(" results__state--actionable");
    }

    rsx! {
        div { class: "{outer_class}",
            div { class: "{class_name}",
                div { class: "results__state-inner",
                    div { class: "results__state-icon",
                        IconGlyph { icon }
                    }
                    p { class: "results__state-title", "{title}" }
                    if let Some(body_text) = body.as_ref() {
                        p { class: "results__state-body", "{body_text}" }
                    }
                    if show_retry_button {
                        button {
                            class: "button button--primary button--small",
                            "aria-label": retry_aria,
                            onclick: move |_| {
                                let Some(current_tab) =
                                    crate::screens::workspace::tab_store::materialize_tab_state(
                                        store,
                                        store.active_tab_id(),
                                    )
                                else {
                                    return;
                                };
                                refresh_tab_result(store, current_tab, None);
                            },
                            "{retry_label}"
                        }
                    }
                }
            }
        }
    }
}

/// True when the active tab has a SQL preview or a last-run query — i.e. the standard `refresh_tab_result` entry point has work to do.
fn can_retry_active_tab(store: TabStore) -> bool {
    store
        .result
        .read()
        .get(&store.active_tab_id())
        .is_some_and(|tab| tab.preview_source.is_some() || tab.last_run_sql.is_some())
}

fn is_empty_table_result(page: &models::QueryPage, display_rows: &[DisplayRow]) -> bool {
    page.columns.is_empty() && page.rows.is_empty() && display_rows.is_empty()
}

#[component]
pub fn ResultTable(
    result: Option<QueryOutput>,
    store: TabStore,
) -> Element {
    let mut editing_cell = use_signal(|| None::<EditingCell>);
    let mut filter_draft = use_signal(|| QueryFilter {
        mode: QueryFilterMode::And,
        rules: Vec::new(),
    });
    let mut filter_sync_key = use_signal(String::new);
    let mut filter_panel_open = use_signal(|| false);
    let mut selected_row_index = use_signal(|| None::<usize>);
    let mut selected_row_sync_key = use_signal(String::new);
    let mut show_row_details = use_signal(|| false);
    let mut row_details_view = use_signal(|| RowDetailsView::Fields);
    let mut editing_row_values = use_signal(Vec::<(usize, String)>::new);
    let mut editing_row_ref = use_signal(|| None::<EditableRowRef>);
    let mut view_mode = use_signal(|| ResultViewMode::Table);
    let mut quick_filter_open = use_signal(|| false);
    let mut quick_filter_column = use_signal(String::new);
    let mut quick_filter_operator = use_signal(|| QueryFilterOperator::Contains);
    let mut quick_filter_value = use_signal(String::new);
    // Materialized only when the active tab's result or pending changes change,
    // not after every render (scroll, selection, hover). `use_memo` auto-tracks
    // the `store.result`/`store.pending` reads, so this recomputes on dependency change
    // instead of on every render cycle like a `use_effect` would.
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
    let mut details_width = use_signal(|| 360.0);
    let mut details_resize_active = use_signal(|| false);
    let mut resize_start_x = use_signal(|| 0.0_f64);
    let mut resize_start_width = use_signal(|| 0.0_f64);
    let mut scroll_offset = use_signal(|| 0.0_f64);
    let mut viewport_height = use_signal(|| 600.0_f64);
    let mut show_chart = use_signal(|| false);
    let mut pinned_result = use_signal(|| None::<models::QueryPage>);
    let mut value_editor = use_signal(|| None::<ValueEditorState>);
    let mut value_editor_target = use_signal(|| None::<(EditableRowRef, usize)>);
    let mut column_widths = use_signal(HashMap::<String, f64>::new);
    let hidden_columns = use_signal(Vec::<String>::new);
    let mut column_resize_active = use_signal(|| None::<(String, f64, f64)>);

    let current_editing = editing_cell();
    let active_tab = store.result.read().get(&store.active_tab_id()).cloned();
    let active_filter = active_tab.as_ref().and_then(|tab| tab.filter.clone());
    let has_active_filter = active_filter.is_some();
    let active_sort = active_tab.as_ref().and_then(|tab| tab.sort.clone());
    let active_error = active_tab
        .as_ref()
        .and_then(|tab| result_error_message(&tab.status));
    let pending_changes = store
        .pending
        .read()
        .get(&store.active_tab_id())
        .map(|p| p.pending_table_changes.clone())
        .unwrap_or_default();
    let has_pending_changes = !pending_changes.is_empty();
    let is_loading_more = active_tab.as_ref().is_some_and(|tab| tab.is_loading_more);
    let sort_enabled = active_tab.as_ref().is_some_and(can_sort_tab);
    let filter_enabled = active_tab.as_ref().is_some_and(can_filter_tab);
    let current_columns = result_columns(result.as_ref());
    let next_filter_draft = filter_draft_from_state(active_filter.as_ref(), &current_columns);
    let next_filter_sync_key = filter_sync_key_for_tab(active_tab.as_ref(), &current_columns);
    let next_row_sync_key = row_sync_key_for_tab(
        active_tab.as_ref(),
        result.as_ref(),
        pending_changes.inserted_rows.len(),
    );

    use_effect(move || {
        if filter_sync_key() != next_filter_sync_key {
            filter_sync_key.set(next_filter_sync_key.clone());
            filter_draft.set(next_filter_draft.clone());
            filter_panel_open.set(has_active_filter);
        }

        if filter_panel_should_auto_open(has_active_filter, &filter_draft()) && !filter_panel_open()
        {
            filter_panel_open.set(true);
        }
    });

    use_effect(move || {
        if selected_row_sync_key() != next_row_sync_key {
            selected_row_sync_key.set(next_row_sync_key.clone());
            selected_row_index.set(None);
            row_details_view.set(RowDetailsView::Fields);
        }
    });

    use_effect(move || {
        let _ = crate::app_state::APP_FOCUS_FILTER_PANEL_REQUEST();
        if filter_enabled {
            filter_panel_open.set(true);
        }
    });

    use_effect(move || {
        if view_mode() == ResultViewMode::Details && !show_row_details() {
            show_row_details.set(true);
        }
    });

    rsx! {
        match result {
            Some(QueryOutput::AffectedRows(rows)) => {
                let summary = match active_tab.as_ref().and_then(|t| t.last_duration_ms) {
                    Some(ms) => format!("Rows affected: {rows} · {}", format_duration(ms)),
                    None => format!("Rows affected: {rows}"),
                };
                rsx! {
                    div {
                        class: "results",
                        p { class: "results__summary", "{summary}" }
                    }
                }
            }
            Some(QueryOutput::Table(page)) => {
                let is_loading = active_tab
                    .as_ref()
                    .map(|tab| {
                        tab.status.starts_with("Loading")
                            || tab.status.starts_with("Running")
                            || tab.status.starts_with("Preview")
                    })
                    .unwrap_or(false);

                if is_loading && page.columns.is_empty() && page.rows.is_empty() {
                    return rsx! {
                        div {
                            class: "results",
                            div {
                                class: "results__state results__state--loading",
                                div {
                                    class: "results__state-skeleton",
                                    div { class: "skeleton skeleton-bar" }
                                    div { class: "skeleton skeleton-table-header" }
                                    for _ in 0..6 {
                                        div { class: "skeleton skeleton-row" }
                                    }
                                }
                                p { class: "results__state-title", "Loading data..." }
                                p {
                                    class: "results__state-body",
                                    "Running the query and materializing the result page."
                                }
                            }
                        }
                    };
                }

                let display_rows = display_rows_cache();
                let virtual_row_height: f64 = 28.0;
                let virtual_buffer: usize = 10;
                let virtual_first = ((scroll_offset() / virtual_row_height) as usize).saturating_sub(virtual_buffer);
                let virtual_last = {
                    let raw = (((scroll_offset() + viewport_height()) / virtual_row_height + 1.0) as usize).saturating_add(virtual_buffer);
                    raw.min(display_rows.len())
                };
                let virtual_top_height = virtual_first as f64 * virtual_row_height;
                let virtual_bottom_height = (display_rows.len().saturating_sub(virtual_last)) as f64 * virtual_row_height;
                // Pre-compute O(1) lookup set for cell_class (avoids linear scan per visible cell).
                let updated_cells_set: HashSet<(String, String)> = pending_changes
                    .updated_cells
                    .iter()
                    .map(|c| (c.locator.clone(), c.column_name.clone()))
                    .collect();
                let draft_rows = pending_changes.inserted_rows.len();
                let selected_row = selected_row_index().and_then(|index| {
                    display_rows
                        .get(index)
                        .cloned()
                        .map(|row| (index, row))
                });
                let details_visible = show_row_details() && selected_row.is_some();
                let has_selected_row = selected_row.is_some();
                let selected_row_label = selected_row
                    .as_ref()
                    .map(|(row_index, row)| display_row_label(page.offset, draft_rows, *row_index, row));
                let details_json = selected_row
                    .as_ref()
                    .map(|(_, row)| format_row_json(&page.columns, &row.values))
                    .unwrap_or_default();
                let status_text = active_tab
                    .as_ref()
                    .map(|tab| tab.status.clone())
                    .unwrap_or_else(|| "Ready".to_string());
                let can_paginate = active_tab
                    .as_ref()
                    .is_some_and(|tab| tab.last_run_sql.is_some() || tab.preview_source.is_some());
                let has_previous_page =
                    page.has_previous && can_paginate && !is_loading_more && !has_pending_changes;
                let has_next_page =
                    page.has_next && can_paginate && !is_loading_more && !has_pending_changes;
                let read_only_mode = read_only_mode_enabled();
                let table_cells_editable = page.editable.is_some() && !read_only_mode;
                let hidden_columns_vec = hidden_columns();
                let visible_columns: Vec<(usize, String)> =
                    filter_visible_columns(&page.columns, &hidden_columns_vec);
                let visible_column_names: Vec<String> = visible_columns
                    .iter()
                    .map(|(_, name)| name.clone())
                    .collect();
                let column_widths_map = column_widths();
                let visible_column_count = visible_columns.len();

                let on_value_editor_mode_change = move |next_mode: ValueEditorMode| {
                    let mut current = value_editor();
                    if let Some(state) = current.as_mut() {
                        state.mode = next_mode;
                    }
                    value_editor.set(current);
                };
                let on_value_editor_apply = move |new_value: String| {
                    let target = value_editor_target();
                    if let Some((row_ref, col_index)) = target {
                        let editing = EditingCell {
                            row_ref,
                            col_index,
                            value: new_value,
                        };
                        commit_cell_edit(editing_cell, store, editing);
                    }
                    value_editor_target.set(None);
                    value_editor.set(None);
                };
                let on_value_editor_close = move |_| {
                    value_editor_target.set(None);
                    value_editor.set(None);
                };
                let on_value_editor_change = |_: String| {};

                rsx! {
                    if is_empty_table_result(&page, &display_rows) {
                        ResultsStateBlock {
                            variant: ResultsStateVariant::Empty,
                            title: "Query returned no rows.".to_string(),
                            body: None,
                            action: ResultsStateAction::RunAgain,
                            store,
                        }
                    } else {
                        div {
                            class: "results",
                            div {
                                class: if details_visible {
                                    "results__layout results__layout--with-details"
                                } else {
                                    "results__layout"
                                },
                                style: if details_visible {
                                    format!("--results-details-width: {}px;", details_width())
                                } else {
                                    String::new()
                                },
                                div {
                                    class: "results__main",
                                    div {
                                        class: "results__toolbar",
                                        div {
                                            class: "results__toolbar-copy",
                                            span {
                                                class: "results__toolbar-chip",
                                                "{rows_toolbar_summary(page.offset, page.rows.len(), page.page_size)}"
                                            }
                                            if should_render_result_status_chip(&status_text, has_pending_changes) {
                                                span {
                                                    class: "results__toolbar-chip",
                                                    "{result_status_text_for_display(&status_text)}"
                                                }
                                            }
                                            p {
                                                class: "results__toolbar-meta",
                                                if let Some(row_label) = selected_row_label.as_ref() {
                                                    "{row_label} selected"
                                                } else if has_pending_changes {
                                                    "{pending_changes_summary(&pending_changes)}"
                                                } else {
                                                    "Select a row for details."
                                                }
                                            }
                                        }
                                        div {
                                        class: "results__toolbar-actions",
                                        div {
                                            class: "results__view-mode",
                                            role: "group",
                                            "aria-label": "Result view mode",
                                            for mode in [ResultViewMode::Table, ResultViewMode::Records, ResultViewMode::Single, ResultViewMode::Details] {
                                                {
                                                    let catalog_entry = find_action(mode.catalog_id());
                                                    let label = catalog_entry.map(|a| a.label).unwrap_or(mode.label());
                                                    let active = view_mode() == mode;
                                                    rsx! {
                                                        button {
                                                            class: if active {
                                                                "button button--ghost button--small button--active results__view-mode-button"
                                                            } else {
                                                                "button button--ghost button--small results__view-mode-button"
                                                            },
                                                            "aria-label": label,
                                                            "aria-pressed": "{active}",
                                                            title: "{label}",
                                                            onclick: move |_| {
                                                                view_mode.set(mode);
                                                                if mode == ResultViewMode::Details {
                                                                    show_row_details.set(true);
                                                                }
                                                            },
                                                            "{mode.label()}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if filter_enabled {
                                            IconButton {
                                                icon: ActionIcon::Filter,
                                                label: "Quick filter".to_string(),
                                                active: quick_filter_open(),
                                                small: true,
                                                onclick: move |_| {
                                                    let next = !quick_filter_open();
                                                    quick_filter_open.set(next);
                                                    if next {
                                                        filter_panel_open.set(true);
                                                    }
                                                },
                                            }
                                            IconButton {
                                                icon: ActionIcon::AddRule,
                                                label: "Filters".to_string(),
                                                active: filter_panel_open(),
                                                small: true,
                                                onclick: move |_| filter_panel_open.toggle(),
                                            }
                                        }
                                        IconButton {
                                            icon: ActionIcon::Previous,
                                            label: "Previous page".to_string(),
                                            small: true,
                                            disabled: !has_previous_page,
                                            onclick: {
                                                let current_id = store.active_tab_id();
                                                move |_| {
                                                    let Some(current_tab) =
                                                        crate::screens::workspace::tab_store::materialize_tab_state(store, current_id)
                                                    else {
                                                        return;
                                                    };
                                                    load_tab_page(
                                                        store,
                                                        current_tab.clone(),
                                                        page.offset.saturating_sub(current_tab.page_size as u64),
                                                    );
                                                }
                                            },
                                        }
                                        IconButton {
                                            icon: ActionIcon::Next,
                                            label: "Next page".to_string(),
                                            small: true,
                                            disabled: !has_next_page,
                                            onclick: {
                                                let current_id = store.active_tab_id();
                                                move |_| {
                                                    let Some(current_tab) =
                                                        crate::screens::workspace::tab_store::materialize_tab_state(store, current_id)
                                                    else {
                                                        return;
                                                    };
                                                    append_next_tab_page(store, current_tab);
                                                }
                                            },
                                        }
                                        if page.editable.is_some() {
                                            IconButton {
                                                icon: ActionIcon::InsertRow,
                                                label: if read_only_mode {
                                                    "Insert draft row is blocked by read-only mode".to_string()
                                                } else {
                                                    "Insert draft row".to_string()
                                                },
                                                small: true,
                                                disabled: read_only_mode,
                                                onclick: move |_| insert_empty_row(store),
                                            }
                                            IconButton {
                                                icon: ActionIcon::Apply,
                                                label: if read_only_mode {
                                                    "Apply pending changes is blocked by read-only mode".to_string()
                                                } else {
                                                    "Apply pending changes".to_string()
                                                },
                                                small: true,
                                                disabled: !has_pending_changes || read_only_mode,
                                                onclick: move |_| apply_pending_changes(store),
                                            }
                                            IconButton {
                                                icon: ActionIcon::Undo,
                                                label: "Discard pending changes".to_string(),
                                                small: true,
                                                disabled: !has_pending_changes,
                                                onclick: move |_| discard_pending_changes(store),
                                            }
                                            IconButton {
                                                icon: ActionIcon::Delete,
                                                label: if read_only_mode {
                                                    "Delete selected row is blocked by read-only mode".to_string()
                                                } else {
                                                    "Delete selected row".to_string()
                                                },
                                                small: true,
                                                disabled: !has_selected_row || read_only_mode,
                                                onclick: {
                                                    let selected_row_index = selected_row_index();
                                                    move |_| {
                                                        if let Some(row_index) = selected_row_index {
                                                            delete_selected_row(store, row_index);
                                                        }
                                                    }
                                                },
                                            }
                                        }
                                        IconButton {
                                            icon: ActionIcon::Details,
                                            label: if details_visible {
                                                "Hide row details".to_string()
                                            } else {
                                                "Show row details".to_string()
                                            },
                                            active: details_visible,
                                            small: true,
                                            disabled: !has_selected_row,
                                            onclick: move |_| show_row_details.toggle(),
                                        }
                                        button {
                                            class: if show_chart() {
                                                "button button--ghost button--small button--active"
                                            } else {
                                                "button button--ghost button--small"
                                            },
                                            onclick: move |_| show_chart.toggle(),
                                            "Chart"
                                        }
                                        button {
                                            class: if pinned_result().is_some() {
                                                "button button--ghost button--small button--active"
                                            } else {
                                                "button button--ghost button--small"
                                            },
                                            onclick: {
                                                let pin_snapshot = page.clone();
                                                move |_| {
                                                    pinned_result.set(Some(pin_snapshot.clone()));
                                                }
                                            },
                                            "Pin for compare"
                                        }
                                        button {
                                            class: "button button--ghost button--small",
                                            disabled: pinned_result().is_none(),
                                            onclick: {
                                                let compare_snapshot = page;
                                                move |_| {
                                                    let Some(pinned) = pinned_result() else {
                                                        return;
                                                    };
                                                    windows::open_data_diff_window(
                                                        Some(pinned),
                                                        Some(compare_snapshot.clone()),
                                                        "Pinned".to_string(),
                                                        "Current".to_string(),
                                                        APP_THEME(),
                                                    );
                                                }
                                            },
                                            "Compare with pinned"
                                        }
                                    }
                                    }

                                    if filter_enabled && filter_panel_open() {
                                        div {
                                            class: "results__filters",
                                            if filter_enabled && quick_filter_open() {
                                                div {
                                                    class: "results__quick-filter",
                                                    "aria-label": "Quick filter",
                                                    span {
                                                        class: "results__quick-filter-keyword",
                                                        "WHERE"
                                                    }
                                                    select {
                                                        class: "input results__quick-filter-column",
                                                        value: "{quick_filter_column()}",
                                                        oninput: move |event| quick_filter_column.set(event.value()),
                                                        for column in page.columns.iter().cloned() {
                                                            option { value: column.clone(), "{column}" }
                                                        }
                                                    }
                                                    select {
                                                        class: "input results__quick-filter-operator",
                                                        value: filter_operator_value(quick_filter_operator()),
                                                        oninput: move |event| {
                                                            quick_filter_operator.set(parse_filter_operator(&event.value()));
                                                        },
                                                        for operator in supported_filter_operators() {
                                                            option {
                                                                value: filter_operator_value(operator),
                                                                "{filter_operator_label(operator)}"
                                                            }
                                                        }
                                                    }
                                                    if quick_filter_operator().is_nullary() {
                                                        div {
                                                            class: "results__filter-null",
                                                            "No value required"
                                                        }
                                                    } else {
                                                        input {
                                                            class: "input results__quick-filter-value",
                                                            value: "{quick_filter_value()}",
                                                            placeholder: "Filter value",
                                                            oninput: move |event| quick_filter_value.set(event.value()),
                                                            onkeydown: move |event| {
                                                                if event.key() == Key::Enter {
                                                                    apply_quick_filter(
                                                                        store,
                                                                        filter_draft,
                                                                        quick_filter_column,
                                                                        quick_filter_operator,
                                                                        quick_filter_value,
                                                                    );
                                                                }
                                                            },
                                                        }
                                                    }
                                                    IconButton {
                                                        icon: ActionIcon::FilterApply,
                                                        label: "Apply quick filter".to_string(),
                                                        small: true,
                                                        onclick: {
                                                            let columns = page.columns.clone();
                                                            move |_| {
                                                                apply_quick_filter_with_columns(
                                                                    store,
                                                                    filter_draft,
                                                                    quick_filter_column,
                                                                    quick_filter_operator,
                                                                    quick_filter_value,
                                                                    &columns,
                                                                );
                                                            }
                                                        },
                                                        disabled: !quick_filter_is_meaningful(quick_filter_operator(), &quick_filter_value()),
                                                    }
                                                    IconButton {
                                                        icon: ActionIcon::FilterClear,
                                                        label: "Clear quick filter".to_string(),
                                                        small: true,
                                                        onclick: move |_| {
                                                            quick_filter_value.set(String::new());
                                                            clear_active_tab_filter(store, store.active_tab_id());
                                                        },
                                                        disabled: !has_active_filter && quick_filter_value().is_empty(),
                                                    }
                                                }
                                            }
                                            div {
                                                class: "results__filters-topbar",
                                                select {
                                                    class: "input results__filter-mode",
                                                    value: filter_mode_value(filter_draft().mode),
                                                    oninput: move |event| update_filter_mode(filter_draft, event.value()),
                                                    option { value: "and", "Match all (AND)" }
                                                    option { value: "or", "Match any (OR)" }
                                                }
                                                IconButton {
                                                    icon: ActionIcon::AddRule,
                                                    label: "Add filter rule".to_string(),
                                                    small: true,
                                                    onclick: {
                                                        let columns = page.columns.clone();
                                                        move |_| add_filter_rule(filter_draft, &columns)
                                                    },
                                                }
                                                IconButton {
                                                    icon: ActionIcon::FilterApply,
                                                    label: "Apply filters".to_string(),
                                                    small: true,
                                                    onclick: move |_| {
                                                        apply_active_tab_filter(store, store.active_tab_id(), filter_draft());
                                                    },
                                                    disabled: !has_meaningful_rules(&filter_draft()),
                                                }
                                                IconButton {
                                                    icon: ActionIcon::FilterClear,
                                                    label: "Clear filters".to_string(),
                                                    small: true,
                                                    onclick: {
                                                        let columns = page.columns.clone();
                                                        move |_| {
                                                            filter_draft.set(blank_filter(&columns));
                                                            clear_active_tab_filter(store, store.active_tab_id());
                                                            filter_panel_open.set(false);
                                                        }
                                                    },
                                                    disabled: !has_active_filter && !has_meaningful_rules(&filter_draft()),
                                                }
                                            }

                                            div {
                                                class: "results__filters-body",
                                                for (rule_index, rule) in filter_draft().rules.iter().cloned().enumerate() {
                                                    div {
                                                        class: "results__filter-row",
                                                        select {
                                                            class: "input results__filter-select",
                                                            value: "{rule.column_name}",
                                                            oninput: move |event| {
                                                                update_filter_rule_column(
                                                                    filter_draft,
                                                                    rule_index,
                                                                    event.value(),
                                                                );
                                                            },
                                                            for column in page.columns.iter().cloned() {
                                                                option { value: column.clone(), "{column}" }
                                                            }
                                                        }
                                                        select {
                                                            class: "input results__filter-operator",
                                                            value: filter_operator_value(rule.operator),
                                                            oninput: move |event| {
                                                                update_filter_rule_operator(
                                                                    filter_draft,
                                                                    rule_index,
                                                                    event.value(),
                                                                );
                                                            },
                                                            for operator in supported_filter_operators() {
                                                                option {
                                                                    value: filter_operator_value(operator),
                                                                    "{filter_operator_label(operator)}"
                                                                }
                                                            }
                                                        }
                                                        if rule.operator.is_nullary() {
                                                            div {
                                                                class: "results__filter-null",
                                                                "No value required"
                                                            }
                                                        } else {
                                                            input {
                                                                class: "input results__filter-input",
                                                                value: "{rule.value}",
                                                                placeholder: "Enter filter value",
                                                                oninput: move |event| {
                                                                    update_filter_rule_value(
                                                                        filter_draft,
                                                                        rule_index,
                                                                        event.value(),
                                                                    );
                                                                },
                                                            }
                                                        }
                                                        IconButton {
                                                            icon: ActionIcon::Clear,
                                                            label: "Remove filter rule".to_string(),
                                                            small: true,
                                                            onclick: {
                                                                let columns = page.columns.clone();
                                                                move |_| remove_filter_rule(
                                                                    filter_draft,
                                                                    rule_index,
                                                                    &columns,
                                                                )
                                                            },
                                                            disabled: filter_draft().rules.len() <= 1,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if view_mode() == ResultViewMode::Records {
                                        div {
                                            class: "results__records",
                                            "aria-label": "Records list",
                                            if display_rows.is_empty() {
                                                p {
                                                    class: "results__records-empty",
                                                    "No rows to display."
                                                }
                                            } else {
                                                for (row_index, display_row) in display_rows.iter().cloned().enumerate() {
                                                    {
                                                        let row_label = display_row_label(page.offset, draft_rows, row_index, &display_row);
                                                        let is_selected = selected_row_index() == Some(row_index);
                                                        let is_draft = matches!(display_row.row_ref, EditableRowRef::PendingInsert(_));
                                                        rsx! {
                                                            button {
                                                                class: if is_selected {
                                                                    "button button--ghost results__records-row results__records-row--selected"
                                                                } else {
                                                                    "button button--ghost results__records-row"
                                                                },
                                                                key: "{display_row_key(&display_row)}",
                                                                "aria-label": "{row_label}",
                                                                onclick: {
                                                                    let row_ref = display_row.row_ref.clone();
                                                                    let values: Vec<(usize, String)> = display_row.values.iter().cloned().enumerate().collect();
                                                                    move |_| {
                                                                        selected_row_index.set(Some(row_index));
                                                                        editing_row_values.set(values.clone());
                                                                        editing_row_ref.set(Some(row_ref.clone()));
                                                                        if view_mode() == ResultViewMode::Details {
                                                                            show_row_details.set(true);
                                                                        }
                                                                    }
                                                                },
                                                                div {
                                                                    class: "results__records-row-label",
                                                                    span { class: "results__records-row-index", "{row_index + 1}" }
                                                                    span { class: "results__records-row-name", "{row_label}" }
                                                                    if is_draft {
                                                                        span { class: "results__records-row-draft", "draft" }
                                                                    }
                                                                }
                                                                div {
                                                                    class: "results__records-row-cells",
                                                                    for (col_index, column_name) in visible_columns.iter().cloned() {
                                                                        {
                                                                            let cell_value = display_row.values.get(col_index).cloned().unwrap_or_default();
                                                                            rsx! {
                                                                                span {
                                                                                    class: "results__records-row-cell",
                                                                                    span { class: "results__records-row-cell-label", "{column_name}" }
                                                                                    span { class: "results__records-row-cell-value", "{cell_value}" }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else if view_mode() == ResultViewMode::Single {
                                        {
                                            let total_rows = display_rows.len();
                                            let current_index = selected_row_index().unwrap_or(0);
                                            let safe_index = if total_rows == 0 { 0 } else { current_index.min(total_rows - 1) };
                                            let single_row = display_rows.get(safe_index).cloned();
                                            let single_label = single_row
                                                .as_ref()
                                                .map(|row| display_row_label(page.offset, draft_rows, safe_index, row));
                                            let single_json = single_row
                                                .as_ref()
                                                .map(|row| format_row_json(&page.columns, &row.values))
                                                .unwrap_or_default();
                                            let has_prev = safe_index > 0;
                                            let has_next = total_rows > 0 && safe_index + 1 < total_rows;
                                            rsx! {
                                                div {
                                                    class: "results__single",
                                                    "aria-label": "Single record view",
                                                    div {
                                                        class: "results__single-toolbar",
                                                        button {
                                                            class: "button button--ghost button--small",
                                                            "aria-label": "Previous record",
                                                            disabled: !has_prev,
                                                            onclick: move |_| {
                                                                if let Some(prev) = selected_row_index() {
                                                                    if prev > 0 {
                                                                        selected_row_index.set(Some(prev - 1));
                                                                    }
                                                                } else if total_rows > 0 {
                                                                    selected_row_index.set(Some(total_rows - 1));
                                                                }
                                                            },
                                                            IconGlyph { icon: ActionIcon::Previous }
                                                            span { "Prev" }
                                                        }
                                                        span {
                                                            class: "results__single-counter",
                                                            if total_rows == 0 {
                                                                "0 / 0"
                                                            } else {
                                                                "{safe_index + 1} / {total_rows}"
                                                            }
                                                        }
                                                        button {
                                                            class: "button button--ghost button--small",
                                                            "aria-label": "Next record",
                                                            disabled: !has_next,
                                                            onclick: move |_| {
                                                                if let Some(curr) = selected_row_index() {
                                                                    if curr + 1 < total_rows {
                                                                        selected_row_index.set(Some(curr + 1));
                                                                    }
                                                                } else if total_rows > 0 {
                                                                    selected_row_index.set(Some(0));
                                                                }
                                                            },
                                                            IconGlyph { icon: ActionIcon::Next }
                                                            span { "Next" }
                                                        }
                                                    }
                                                    if let Some(row) = single_row.as_ref() {
                                                        {
                                                            let is_draft = matches!(row.row_ref, EditableRowRef::PendingInsert(_));
                                                            rsx! {
                                                                div {
                                                                    class: "results__single-card",
                                                                    div {
                                                                        class: "results__single-header",
                                                                        h3 {
                                                                            class: "results__single-title",
                                                                            if let Some(label) = single_label.as_ref() {
                                                                                "{label}"
                                                                            } else {
                                                                                "Row"
                                                                            }
                                                                        }
                                                                        if is_draft {
                                                                            span { class: "results__single-draft", "draft" }
                                                                        }
                                                                    }
                                                                    div {
                                                                        class: "results__single-fields",
                                                                        for (col_index, column_name) in visible_columns.iter().cloned() {
                                                                            {
                                                                                let cell_value = row.values.get(col_index).cloned().unwrap_or_default();
                                                                                rsx! {
                                                                                    div {
                                                                                        class: "results__single-field",
                                                                                        p { class: "results__single-field-label", "{column_name}" }
                                                                                        p { class: "results__single-field-value", "{cell_value}" }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                    div {
                                                                        class: "results__single-json",
                                                                        h4 { "JSON" }
                                                                        pre { "{single_json}" }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        p {
                                                            class: "results__single-empty",
                                                            "No row selected."
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else {

                                    div {
                                        class: "results__table-wrap",
                                        onscroll: move |event| {
                                            let scroll_state = event.data();
                                            scroll_offset.set(scroll_state.scroll_top());
                                            viewport_height.set(scroll_state.client_height() as f64);
                                            let remaining_scroll = scroll_state.scroll_height() as f64
                                                - (scroll_state.scroll_top()
                                                    + scroll_state.client_height() as f64);

                                            if remaining_scroll > 96.0 {
                                                return;
                                            }

                                            let current_id = store.active_tab_id();
                                            let Some(current_tab) =
                                                crate::screens::workspace::tab_store::materialize_tab_state(store, current_id)
                                            else {
                                                return;
                                            };

                                            append_next_tab_page(store, current_tab);
                                        },
                                        table {
                                            class: "results__table",
                                            thead {
                                                tr {
                                                    for column in visible_column_names.iter().cloned() {
                                                        th {
                                                            class: "results__head",
                                                            style: column_widths_map
                                                                .get(&column)
                                                                .copied()
                                                                .map(|width| format!("width: {width}px; min-width: {width}px; max-width: {width}px;"))
                                                                .unwrap_or_default(),
                                                            oncontextmenu: {
                                                                let column_name = column.clone();
                                                                let hidden_columns_for_menu = hidden_columns;
                                                                let column_widths_for_menu = column_widths;
                                                                move |event| {
                                                                    event.prevent_default();
                                                                    let coords = event.client_coordinates();
                                                                    let items = build_header_context_menu(
                                                                        column_name.clone(),
                                                                        hidden_columns_for_menu,
                                                                        column_widths_for_menu,
                                                                        store,
                                                                    );
                                                                    open_context_menu(coords.x, coords.y, items);
                                                                }
                                                            },
                                                            if sort_enabled {
                                                                button {
                                                                    class: sort_button_class(active_sort.as_ref(), &column),
                                                                    disabled: has_pending_changes,
                                                                    onclick: {
                                                                        let column_name = column.clone();
                                                                        move |_| toggle_active_tab_sort(
                                                                            store,
                                                                            store.active_tab_id(),
                                                                            column_name.clone(),
                                                                        )
                                                                    },
                                                                    span { class: "results__head-label", "{column}" }
                                                                    span {
                                                                        class: "results__sort-indicator",
                                                                        "{sort_indicator(active_sort.as_ref(), &column)}"
                                                                    }
                                                                }
                                                            } else {
                                                                span { class: "results__head-label", "{column}" }
                                                            }
                                                            div {
                                                                class: "results__head-resize",
                                                                onmousedown: {
                                                                    let column_name = column.clone();
                                                                    let start_width = column_widths_map
                                                                        .get(&column_name)
                                                                        .copied()
                                                                        .unwrap_or(160.0);
                                                                    move |event| {
                                                                        if event.trigger_button() != Some(MouseButton::Primary) {
                                                                            return;
                                                                        }
                                                                        event.prevent_default();
                                                                        event.stop_propagation();
                                                                        column_resize_active.set(Some((
                                                                            column_name.clone(),
                                                                            event.client_coordinates().x,
                                                                            start_width,
                                                                        )));
                                                                    }
                                                                },
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            tbody {
                                                if virtual_first > 0 {
                                                    tr {
                                                        key: "spacer-top-{virtual_first}",
                                                        td {
                                                            colspan: "{visible_column_count}",
                                                            style: "height: {virtual_top_height}px; padding: 0; border: none;",
                                                            div { style: "height: {virtual_top_height}px;" }
                                                        }
                                                    }
                                                }

                                                for visible_idx in virtual_first..virtual_last {
                                                    if let Some(row) = display_rows.get(visible_idx) {
                                                        tr {
                                                            class: row_class(selected_row_index() == Some(visible_idx), row),
                                                            key: "{display_row_key(row)}",
                                                            onclick: move |_| {
                                                                selected_row_index.set(Some(visible_idx));
                                                                show_row_details.set(true);
                                                                let rows = display_rows_cache.read();
                                                                if let Some(r) = rows.get(visible_idx) {
                                                                    let values: Vec<(usize, String)> = r.values.iter()
                                                                        .enumerate()
                                                                        .map(|(i, v)| (i, v.clone()))
                                                                        .collect();
                                                                    editing_row_values.set(values);
                                                                    editing_row_ref.set(Some(r.row_ref.clone()));
                                                                }
                                                            },
                                                            oncontextmenu: {
                                                                let columns_for_row_menu = page.columns.clone();
                                                                let row_values = row.values.clone();
                                                                let has_pending_changes_for_menu = has_pending_changes;
                                                                let table_name_for_menu = active_table_name(store);
                                                                let all_rows_for_menu = page.rows.clone();
                                                                move |event| {
                                                                    event.prevent_default();
                                                                    let coords = event.client_coordinates();
                                                                    let items = build_row_context_menu(
                                                                        columns_for_row_menu.clone(),
                                                                        row_values.clone(),
                                                                        store,
                                                                        has_pending_changes_for_menu,
                                                                        table_name_for_menu.clone(),
                                                                        all_rows_for_menu.clone(),
                                                                    );
                                                                    open_context_menu(coords.x, coords.y, items);
                                                                }
                                                            },
                                                            for (col_index, column_name) in visible_columns.iter().cloned() {
                                                                td {
                                                                    class: cell_class(
                                                                        table_cells_editable,
                                                                        row,
                                                                        page.columns.get(col_index),
                                                                        &updated_cells_set,
                                                                    ),
                                                                    style: column_widths_map
                                                                        .get(&column_name)
                                                                        .copied()
                                                                        .map(|width| format!("width: {width}px; min-width: {width}px; max-width: {width}px;"))
                                                                        .unwrap_or_default(),
                                                                    oncontextmenu: {
                                                                        let columns_for_cell_menu = page.columns.clone();
                                                                        let row_values = row.values.clone();
                                                                        let cell_value = row.values.get(col_index).cloned().unwrap_or_default();
                                                                        let col = col_index;
                                                                        let column_name_for_menu = column_name.clone();
                                                                        let row_ref_for_menu = row.row_ref.clone();
                                                                        let editable_for_menu = table_cells_editable;
                                                                        let editing_cell_for_menu = editing_cell;
                                                                        let value_editor_for_menu = value_editor;
                                                                        let value_editor_target_for_menu = value_editor_target;
                                                                        move |event| {
                                                                            event.prevent_default();
                                                                            let coords = event.client_coordinates();
                                                                            let items = build_cell_context_menu(
                                                                                columns_for_cell_menu.clone(),
                                                                                row_values.clone(),
                                                                                col,
                                                                                cell_value.clone(),
                                                                                column_name_for_menu.clone(),
                                                                                row_ref_for_menu.clone(),
                                                                                editable_for_menu,
                                                                                editing_cell_for_menu,
                                                                                value_editor_for_menu,
                                                                                value_editor_target_for_menu,
                                                                                store,
                                                                            );
                                                                            open_context_menu(coords.x, coords.y, items);
                                                                        }
                                                                    },
                                                                    ondoubleclick: {
                                                                        let cell_value = row.values.get(col_index).cloned().unwrap_or_default();
                                                                        let editable = table_cells_editable;
                                                                        let row_ref = row.row_ref.clone();
                                                                        move |_| {
                                                                            if editable {
                                                                                editing_cell.set(Some(EditingCell {
                                                                                    row_ref: row_ref.clone(),
                                                                                    col_index,
                                                                                    value: cell_value.clone(),
                                                                                }));
                                                                            }
                                                                        }
                                                                    },
                                                                    if let Some(current_edit) = current_editing.clone() {
                                                                        if current_edit.row_ref == row.row_ref && current_edit.col_index == col_index {
                                                                            input {
                                                                                class: "results__cell-input",
                                                                                value: "{current_edit.value}",
                                                                                oninput: move |event| {
                                                                                    let value = event.value();
                                                                                    editing_cell.with_mut(|editing| {
                                                                                        if let Some(editing) = editing.as_mut() {
                                                                                            editing.value = value;
                                                                                        }
                                                                                    });
                                                                                },
                                                                                onkeydown: {
                                                                                    let visible_columns_for_nav = visible_columns.clone();
                                                                                    let display_rows_for_nav = display_rows_cache;
                                                                                    move |event| {
                                                                                        let key = event.key();
                                                                                        if key == Key::Enter {
                                                                                            if let Some(editing) = editing_cell() {
                                                                                                commit_cell_edit(
                                                                                                    editing_cell,
                                                                                                    store,
                                                                                                    editing,
                                                                                                );
                                                                                            }
                                                                                            return;
                                                                                        }
                                                                                        if key == Key::Escape {
                                                                                            editing_cell.set(None);
                                                                                            return;
                                                                                        }
                                                                                        // Arrow / Tab navigation between cells.
                                                                                        let is_nav = matches!(
                                                                                            key,
                                                                                            Key::ArrowUp
                                                                                                | Key::ArrowDown
                                                                                                | Key::ArrowLeft
                                                                                                | Key::ArrowRight
                                                                                                | Key::Tab
                                                                                        );
                                                                                        if !is_nav {
                                                                                            return;
                                                                                        }
                                                                                        event.prevent_default();
                                                                                        let Some(current) = editing_cell() else {
                                                                                            return;
                                                                                        };
                                                                                        let rows = display_rows_for_nav();
                                                                                        let row_count = rows.len();
                                                                                        let col_count = visible_columns_for_nav.len();
                                                                                        if row_count == 0 || col_count == 0 {
                                                                                            return;
                                                                                        }
                                                                                        let Some(current_row) = rows
                                                                                            .iter()
                                                                                            .position(|r| r.row_ref == current.row_ref)
                                                                                        else {
                                                                                            return;
                                                                                        };
                                                                                        let Some(current_col) = visible_columns_for_nav
                                                                                            .iter()
                                                                                            .position(|(i, _)| *i == current.col_index)
                                                                                        else {
                                                                                            return;
                                                                                        };
                                                                                        let (target_row, target_col) = match key {
                                                                                            Key::ArrowDown => (current_row + 1, current_col),
                                                                                            Key::ArrowUp => (current_row.saturating_sub(1), current_col),
                                                                                            Key::ArrowRight | Key::Tab => {
                                                                                                if current_col + 1 < col_count {
                                                                                                    (current_row, current_col + 1)
                                                                                                } else if current_row + 1 < row_count {
                                                                                                    (current_row + 1, 0)
                                                                                                } else {
                                                                                                    return;
                                                                                                }
                                                                                            }
                                                                                            Key::ArrowLeft => {
                                                                                                if current_col > 0 {
                                                                                                    (current_row, current_col - 1)
                                                                                                } else if current_row > 0 {
                                                                                                    (current_row - 1, col_count - 1)
                                                                                                } else {
                                                                                                    return;
                                                                                                }
                                                                                            }
                                                                                            _ => return,
                                                                                        };
                                                                                        if target_row >= row_count || target_col >= col_count {
                                                                                            return;
                                                                                        }
                                                                                        let (target_col_index, _) =
                                                                                            visible_columns_for_nav[target_col];
                                                                                        let new_value = rows
                                                                                            .get(target_row)
                                                                                            .and_then(|r| r.values.get(target_col_index))
                                                                                            .cloned()
                                                                                            .unwrap_or_default();
                                                                                        let new_row_ref = rows[target_row].row_ref.clone();
                                                                                        editing_cell.set(Some(EditingCell {
                                                                                            row_ref: new_row_ref,
                                                                                            col_index: target_col_index,
                                                                                            value: new_value,
                                                                                        }));
                                                                                    }
                                                                                },
                                                                                onblur: move |_| {
                                                                                    if let Some(editing) = editing_cell() {
                                                                                        commit_cell_edit(
                                                                                            editing_cell,
                                                                                            store,
                                                                                            editing,
                                                                                        );
                                                                                    }
                                                                                }
                                                                            }
                                                                        } else {
                                                                            div {
                                                                                class: "results__cell-content",
                                                                                title: "{row.values.get(col_index).cloned().unwrap_or_default()}",
                                                                                "{row.values.get(col_index).cloned().unwrap_or_default()}"
                                                                            }
                                                                        }
                                                                    } else {
                                                                        div {
                                                                            class: "results__cell-content",
                                                                            title: "{row.values.get(col_index).cloned().unwrap_or_default()}",
                                                                            "{row.values.get(col_index).cloned().unwrap_or_default()}"
                                                                        }
                                                                    }
                                                                    if should_show_cell_filter(&row.values.get(col_index).cloned().unwrap_or_default()) {
                                                                        button {
                                                                            class: "results__cell-filter",
                                                                            title: "Filter by this value",
                                                                            "aria-label": "Filter by this value",
                                                                            tabindex: "-1",
                                                                            onclick: {
                                                                                let cell_value = row.values.get(col_index).cloned().unwrap_or_default();
                                                                                let column_name = column_name.clone();
                                                                                move |event| {
                                                                                    event.stop_propagation();
                                                                                    apply_filter_for_value(
                                                                                        column_name.clone(),
                                                                                        cell_value.clone(),
                                                                                        QueryFilterOperator::Contains,
                                                                                        store,
                                                                                    );
                                                                                }
                                                                            },
                                                                            IconGlyph { icon: ActionIcon::Filter }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            }
                                                        }
                                                    }
                                                }

                                                if virtual_bottom_height > 0.0 {
                                                    tr {
                                                        key: "spacer-bottom-{virtual_last}",
                                                        td {
                                                            colspan: "{visible_column_count}",
                                                            style: "height: {virtual_bottom_height}px; padding: 0; border: none;",
                                                            div { style: "height: {virtual_bottom_height}px;" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    }

                                    if is_loading_more {
                                        div {
                                            class: "results__load-more",
                                            "Loading more rows..."
                                        }
                                    }

                                    if let Some((resize_column, resize_start_x_val, resize_start_width_val)) = column_resize_active() {
                                        div {
                                            style: "position:fixed;inset:0;z-index:9999;cursor:col-resize;",
                                            onmousemove: move |event| {
                                                let delta = event.client_coordinates().x - resize_start_x_val;
                                                let new_width = (resize_start_width_val + delta).clamp(60.0, 800.0);
                                                column_widths.with_mut(|widths| {
                                                    widths.insert(resize_column.clone(), new_width);
                                                });
                                            },
                                            onmouseup: move |_| {
                                                column_resize_active.set(None);
                                            },
                                        }
                                    }

                                    if let Some(editor_state) = value_editor() {
                                        ValueEditor {
                                            state: editor_state,
                                            on_value_change: on_value_editor_change,
                                            on_mode_change: on_value_editor_mode_change,
                                            on_apply: on_value_editor_apply,
                                            on_close: on_value_editor_close,
                                        }
                                    }

                                    if details_visible {
                                    aside {
                                        class: if details_resize_active() {
                                            "results__details results__details--resizing"
                                        } else {
                                            "results__details"
                                        },
                                        div {
                                            class: "results__details-header",
                                            div {
                                                class: "results__details-copy",
                                                h3 {
                                                    class: "results__details-title",
                                                    if let Some(row_label) = selected_row_label.as_ref() {
                                                        "{row_label}"
                                                    } else {
                                                        "Row Details"
                                                    }
                                                }
                                                p {
                                                    class: "results__details-hint",
                                                    "Full values for the selected row."
                                                }
                                            }
                                            IconButton {
                                                icon: ActionIcon::Close,
                                                label: "Close row details".to_string(),
                                                small: true,
                                                onclick: move |_| show_row_details.set(false),
                                            }
                                        }
                                        div {
                                            class: "results__details-actions",
                                            button {
                                                class: if row_details_view() == RowDetailsView::Fields {
                                                    "button button--ghost button--small button--active"
                                                } else {
                                                    "button button--ghost button--small"
                                                },
                                                onclick: move |_| row_details_view.set(RowDetailsView::Fields),
                                                "Fields"
                                            }
                                            button {
                                                class: if row_details_view() == RowDetailsView::Json {
                                                    "button button--ghost button--small button--active"
                                                } else {
                                                    "button button--ghost button--small"
                                                },
                                                onclick: move |_| row_details_view.set(RowDetailsView::Json),
                                                "JSON"
                                            }
                                            button {
                                                class: "button button--primary button--small",
                                                onclick: move |_| {
                                                    let editing_values = editing_row_values();
                                                    let editing_ref = editing_row_ref();
                                                    if let Some(row_ref) = editing_ref.clone() {
                                                        for (col_index, value) in editing_values.iter().cloned() {
                                                            let cell_edit = EditingCell {
                                                                row_ref: row_ref.clone(),
                                                                col_index,
                                                                value: value.clone(),
                                                            };
                                                            commit_cell_edit(
                                                                editing_cell,
                                                                store,
                                                                cell_edit,
                                                            );
                                                        }
                                                    }
                                                },
                                                "Save"
                                            }
                                        }
                                        if let Some((_, _row)) = selected_row.as_ref() {
                                            if row_details_view() == RowDetailsView::Fields {
                                                div {
                                                    class: "results__details-list",
                                                    for (col_index, value) in editing_row_values().iter().cloned() {
                                                        div {
                                                            class: "results__details-field",
                                                            p { class: "results__details-label", "{page.columns.get(col_index).unwrap_or(&\"?\".to_string())}" }
                                                            input {
                                                                class: "input results__details-input",
                                                                value: "{value}",
                                                                oninput: move |event| {
                                                                    editing_row_values.with_mut(|values| {
                                                                        if let Some(v) = values.iter_mut().find(|(i, _)| *i == col_index) {
                                                                            v.1 = event.value();
                                                                        }
                                                                    });
                                                                },
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                pre {
                                                    class: "results__details-json",
                                                    "{details_json}"
                                                }
                                            }
                                        }
                                        div {
                                            class: if details_resize_active() {
                                                "results__details-resize results__details-resize--active"
                                            } else {
                                                "results__details-resize"
                                            },
                                            onmousedown: move |event| {
                                                if event.trigger_button() != Some(MouseButton::Primary) {
                                                    return;
                                                }
                                                event.prevent_default();
                                                event.stop_propagation();
                                                resize_start_x.set(event.client_coordinates().x);
                                                resize_start_width.set(details_width());
                                                details_resize_active.set(true);
                                            },
                                        }
                                        if details_resize_active() {
                                            div {
                                                style: "position:fixed;inset:0;z-index:9999;cursor:col-resize;",
                                                onmousemove: move |event| {
                                                    let delta = event.client_coordinates().x - resize_start_x();
                                                    let new_width = (resize_start_width() - delta).clamp(280.0, 1200.0);
                                                    details_width.set(new_width);
                                                },
                                                onmouseup: move |_| {
                                                    details_resize_active.set(false);
                                                },
                                            }
                                        }
                                    }
                                }
                            }
                            ResultChart {
                                columns: page.columns.clone(),
                                rows: page.rows.clone(),
                                visible: show_chart,
                            }
                        }
                    }
                }
            }
            None => rsx! {
                if let Some(error) = active_error {
                    ResultsStateBlock {
                        variant: ResultsStateVariant::Error,
                        title: "Query failed".to_string(),
                        body: Some(error),
                        action: ResultsStateAction::Retry,
                        store,
                    }
                } else {
                    ResultsStateBlock {
                        variant: ResultsStateVariant::Empty,
                        title: "No results yet".to_string(),
                        body: Some(
                            "Double-click a table in Explorer or run SQL to see rows here."
                                .to_string(),
                        ),
                        action: ResultsStateAction::None,
                        store,
                    }
                }
            },
        }
    }
}

fn result_error_message(status: &str) -> Option<String> {
    [
        "Error: ",
        "Preview error: ",
        "Structure error: ",
        "Load more error: ",
    ]
    .iter()
    .find_map(|prefix| status.strip_prefix(prefix))
    .map(str::trim)
    .filter(|message| !message.is_empty())
    .map(ToOwned::to_owned)
}

pub fn should_render_result_status_chip(status: &str, has_pending_changes: bool) -> bool {
    let status = status.trim();
    if status.is_empty() {
        return has_pending_changes;
    }

    let is_loading = status.starts_with("Loading") || status.starts_with("Running");
    let is_error = status.starts_with("Error:")
        || status.starts_with("Preview error:")
        || status.starts_with("Structure error:")
        || status.starts_with("Load more error:");
    let is_ready = status == "Ready";
    let is_loaded = status.starts_with("Loaded rows");

    is_loading || is_error || has_pending_changes || (!is_ready && !is_loaded)
}

pub fn result_status_text_for_display(status: &str) -> &str {
    status.strip_prefix("Status: ").unwrap_or(status)
}

pub fn format_row_edit_error(operation: &str, err: impl std::fmt::Display) -> String {
    format!("{operation} error: {err}")
}

// ---------------------------------------------------------------------------
// Context-menu builders for the result table.
//
// Three surfaces are wired up to the global context menu:
//   - column headers (`<th>`) — sort/filter actions on a column
//   - table rows (`<tr>`) — copy / filter / sort / select actions
//   - table cells (`<td>`) — copy value / filter by this value
//
// Each builder returns a `Vec<ContextMenuItem>` that the parent
// component passes to `open_context_menu`. The builders deliberately
// stay close to the existing `actions` API so that the menu does
// not introduce new persistence or side-effect surface area.
// ---------------------------------------------------------------------------

fn build_header_context_menu(
    column_name: String,
    hidden_columns: Signal<Vec<String>>,
    column_widths: Signal<HashMap<String, f64>>,
    store: TabStore,
) -> Vec<ContextMenuItem> {
    let mut items: Vec<ContextMenuItem> = Vec::new();

    {
        let column_name = column_name.clone();
        items.push(
            ContextMenuItem::new("Sort ascending", move || {
                sort_by_column(&column_name, false, store);
            })
            .with_icon(ActionIcon::Previous),
        );
    }

    {
        let column_name = column_name.clone();
        items.push(
            ContextMenuItem::new("Sort descending", move || {
                sort_by_column(&column_name, true, store);
            })
            .with_icon(ActionIcon::Next),
        );
    }

    {
        let column_name = column_name.clone();
        items.push(
            ContextMenuItem::new("Filter by this column…", move || {
                apply_filter_for_value(
                    column_name.clone(),
                    String::new(),
                    QueryFilterOperator::Contains,
                    store,
                );
            })
            .with_icon(ActionIcon::Filter)
            .separator(),
        );
    }

    {
        let active_id = store.active_tab_id();
        let has_filter = store
            .result
            .read()
            .get(&active_id)
            .and_then(|tab| tab.filter.as_ref())
            .is_some();
        if has_filter {
            items.push(
                ContextMenuItem::new("Clear filter", move || {
                    clear_active_tab_filter(store, store.active_tab_id());
                })
                .with_icon(ActionIcon::FilterClear),
            );
        }
    }

    {
        let column_name = column_name.clone();
        let mut hidden_columns = hidden_columns;
        items.push(
            ContextMenuItem::new("Hide column", move || {
                hidden_columns.with_mut(|hidden| {
                    if !hidden.contains(&column_name) {
                        hidden.push(column_name.clone());
                    }
                });
            })
            .with_icon(ActionIcon::Close)
            .separator(),
        );
    }

    {
        let column_name = column_name.clone();
        let mut column_widths = column_widths;
        items.push(
            ContextMenuItem::new("Reset column width", move || {
                column_widths.with_mut(|widths| {
                    widths.remove(&column_name);
                });
            })
            .with_icon(ActionIcon::Format),
        );
    }

    let hidden_list = hidden_columns();
    for hidden_name in hidden_list {
        let mut hidden_columns = hidden_columns;
        let target = hidden_name.clone();
        items.push(
            ContextMenuItem::new(format!("Show \"{hidden_name}\""), move || {
                let target = target.clone();
                hidden_columns.with_mut(|hidden| {
                    hidden.retain(|current| current != &target);
                });
            })
            .with_icon(ActionIcon::Details),
        );
    }

    items
}

fn build_row_context_menu(
    columns: Vec<String>,
    row_values: Vec<String>,
    store: TabStore,
    has_pending_changes: bool,
    table_name: String,
    all_rows: Vec<Vec<String>>,
) -> Vec<ContextMenuItem> {
    use crate::app_state::context_menu::copy_to_clipboard;
    use services::format_insert_statements;

    let mut items: Vec<ContextMenuItem> = Vec::new();

    // 1. Copy row as JSON.
    {
        let columns = columns.clone();
        let row_values = row_values.clone();
        items.push(
            ContextMenuItem::new("Copy row as JSON", move || {
                let _ = copy_to_clipboard(format_row_json(&columns, &row_values));
            })
            .with_icon(ActionIcon::ExportJson),
        );
    }

    // 2. Copy row as TSV (header + tab-separated values).
    {
        let columns = columns.clone();
        let row_values = row_values.clone();
        items.push(
            ContextMenuItem::new("Copy row as TSV", move || {
                let _ = copy_to_clipboard(format_row_tsv(&columns, &row_values));
            })
            .with_icon(ActionIcon::ExportCsv),
        );
    }

    // 2b. Copy row as CSV (RFC 4180 quoted values, no header).
    {
        let columns = columns.clone();
        let row_values = row_values.clone();
        items.push(
            ContextMenuItem::new("Copy row as CSV", move || {
                let _ = copy_to_clipboard(format_row_csv(&columns, &row_values));
            })
            .with_icon(ActionIcon::ExportCsv),
        );
    }

    // 3. Copy row as INSERT — a real, escaped `INSERT INTO <table> (cols) VALUES (...)`
    //    using the active tab's source table when known. DBeaver-class data
    //    migration clipboard action; the previous placeholder did not quote
    //    values or include the column list.
    {
        let table = table_name.clone();
        let columns = columns.clone();
        let row = row_values.clone();
        items.push(
            ContextMenuItem::new("Copy row as INSERT", move || {
                let _ = copy_to_clipboard(format_insert_statements(
                    &table,
                    &columns,
                    std::slice::from_ref(&row),
                ));
            })
            .with_icon(ActionIcon::ExportSql),
        );
    }

    // 3b. Bulk-row copy actions. The closing separator is attached to the
    //    last bulk item when bulk is present, or moved to the previous
    //    single-row item when bulk is not, so the divider between the copy
    //    group and the filter/sort group stays in either layout.
    let mut bulk_items: Vec<ContextMenuItem> = Vec::new();
    {
        let table = table_name.clone();
        let columns = columns.clone();
        let rows = all_rows.clone();
        bulk_items.push(
            ContextMenuItem::new("Copy all rows as INSERT", move || {
                let _ = copy_to_clipboard(format_insert_statements(&table, &columns, &rows));
            })
            .with_icon(ActionIcon::ExportSql),
        );
    }
    {
        let columns = columns.clone();
        let rows = all_rows.clone();
        bulk_items.push(
            ContextMenuItem::new("Copy all rows as CSV", move || {
                let _ = copy_to_clipboard(format_all_rows_csv(&columns, &rows));
            })
            .with_icon(ActionIcon::ExportCsv),
        );
    }
    {
        let columns = columns.clone();
        let rows = all_rows.clone();
        bulk_items.push(
            ContextMenuItem::new("Copy all rows as JSON", move || {
                let _ = copy_to_clipboard(format_all_rows_json(&columns, &rows));
            })
            .with_icon(ActionIcon::ExportJson),
        );
    }
    {
        let columns = columns.clone();
        let rows = all_rows.clone();
        bulk_items.push(
            ContextMenuItem::new("Copy all rows as Markdown", move || {
                let _ = copy_to_clipboard(format_all_rows_markdown(&columns, &rows));
            })
            .with_icon(ActionIcon::ExportHtml),
        );
    }
    if all_rows.len() > 1 {
        if let Some(last) = bulk_items.pop() {
            items.push(last.separator());
        }
        items.extend(bulk_items);
    } else if let Some(last) = items.pop() {
        items.push(last.separator());
    }

    // 4. Filter by every column whose value is non-empty. The
    //    first match wins — a single "Filter row" entry opens the
    //    filter panel with a draft pointing at the first column.
    let first_non_empty: Option<(usize, String)> =
        row_values.iter().enumerate().find_map(|(idx, v)| {
            if v.is_empty() {
                None
            } else {
                Some((idx, v.clone()))
            }
        });
    if let Some((idx, value)) = first_non_empty {
        // Two-step pattern: the inner `column` is not always present
        // for the picked `idx`, so this cannot collapse into a tuple
        // `if let` without losing the early-out.
        #[allow(clippy::collapsible_if)]
        if let Some(column) = columns.get(idx).cloned() {
            items.push(
                ContextMenuItem::new("Filter by this row", move || {
                    apply_filter_for_value(
                        column.clone(),
                        value.clone(),
                        QueryFilterOperator::Contains,
                        store,
                    );
                })
                .with_icon(ActionIcon::Filter),
            );
        }
    }

    // 5. Sort by first column. Sorting is a per-column action, but
    //    surfacing it on the row makes it discoverable for users who
    //    do not realise the column header is interactive.
    if let Some(first) = columns.first().cloned() {
        items.push(
            ContextMenuItem::new("Sort by first column", move || {
                sort_by_column(&first, false, store);
            })
            .with_icon(ActionIcon::Previous),
        );
    }

    // 6. Edit row is only meaningful for editable cells and not
    //    while there are unsaved changes that would conflict.
    if !has_pending_changes {
        items.push(
            ContextMenuItem::new("Edit row details", move || {
                // The user can also click the row to open the details
                // aside. We just make sure the existing `click` flow
                // is also available from the menu.
                set_active_tab_status(store, store.active_tab_id(), "Row selected.".to_string());
            })
            .with_icon(ActionIcon::Details),
        );
    }

    items
}

#[allow(clippy::too_many_arguments)]
fn build_cell_context_menu(
    columns: Vec<String>,
    row_values: Vec<String>,
    col_index: usize,
    cell_value: String,
    column_name: String,
    row_ref: EditableRowRef,
    editable: bool,
    mut editing_cell: Signal<Option<EditingCell>>,
    mut value_editor: Signal<Option<ValueEditorState>>,
    mut value_editor_target: Signal<Option<(EditableRowRef, usize)>>,
    store: TabStore,
) -> Vec<ContextMenuItem> {
    use crate::app_state::context_menu::copy_to_clipboard;

    let mut items: Vec<ContextMenuItem> = Vec::new();
    let resolved_column = if column_name.is_empty() {
        columns
            .get(col_index)
            .cloned()
            .unwrap_or_else(|| format!("col_{col_index}"))
    } else {
        column_name
    };

    if editable {
        let row_ref = row_ref.clone();
        let col = col_index;
        let cell_value = cell_value.clone();
        items.push(
            ContextMenuItem::new("Edit", move || {
                editing_cell.set(Some(EditingCell {
                    row_ref: row_ref.clone(),
                    col_index: col,
                    value: cell_value.clone(),
                }));
            })
            .with_icon(ActionIcon::Format)
            .separator(),
        );
    }

    {
        let cell_value = cell_value.clone();
        items.push(
            ContextMenuItem::new("Copy value", move || {
                let _ = copy_to_clipboard(cell_value.clone());
            })
            .with_icon(ActionIcon::Duplicate),
        );
    }

    {
        let cell_value = cell_value.clone();
        items.push(
            ContextMenuItem::new("Copy as JSON literal", move || {
                let literal = serde_json::to_string(&cell_value)
                    .unwrap_or_else(|_| format!("\"{}\"", cell_value));
                let _ = copy_to_clipboard(literal);
            })
            .with_icon(ActionIcon::ExportJson),
        );
    }

    {
        let column_name = resolved_column.clone();
        let cell_value = cell_value.clone();
        let row_ref_for_target = row_ref.clone();
        let col = col_index;
        items.push(
            ContextMenuItem::new("Open value editor", move || {
                value_editor_target.set(Some((row_ref_for_target.clone(), col)));
                value_editor.set(Some(ValueEditorState {
                    column_name: column_name.clone(),
                    value: cell_value.clone(),
                    editable: true,
                    mode: ValueEditorMode::Text,
                    width: 520.0,
                }));
            })
            .with_icon(ActionIcon::Details),
        );
    }

    if is_valid_cell_json(&cell_value) {
        let column_name = resolved_column.clone();
        let cell_value = cell_value.clone();
        let row_ref_for_target = row_ref.clone();
        let col = col_index;
        items.push(
            ContextMenuItem::new("JSON viewer", move || {
                value_editor_target.set(Some((row_ref_for_target.clone(), col)));
                value_editor.set(Some(ValueEditorState {
                    column_name: column_name.clone(),
                    value: cell_value.clone(),
                    editable: false,
                    mode: ValueEditorMode::Json,
                    width: 520.0,
                }));
            })
            .with_icon(ActionIcon::Explain)
            .separator(),
        );
    }

    if editable {
        let row_ref_null = row_ref.clone();
        let col = col_index;
        items.push(
            ContextMenuItem::new("Set NULL", move || {
                editing_cell.set(Some(EditingCell {
                    row_ref: row_ref_null.clone(),
                    col_index: col,
                    value: "NULL".to_string(),
                }));
            })
            .with_icon(ActionIcon::Truncate),
        );
        let row_ref_empty = row_ref.clone();
        let col = col_index;
        items.push(
            ContextMenuItem::new("Set empty", move || {
                editing_cell.set(Some(EditingCell {
                    row_ref: row_ref_empty.clone(),
                    col_index: col,
                    value: String::new(),
                }));
            })
            .with_icon(ActionIcon::Clear)
            .separator(),
        );
    }

    if !cell_value.trim().is_empty() {
        let cell_value_contains = cell_value.clone();
        let column_name_contains = resolved_column.clone();
        items.push(
            ContextMenuItem::new("Filter by this value", move || {
                apply_filter_for_value(
                    column_name_contains.clone(),
                    cell_value_contains.clone(),
                    QueryFilterOperator::Contains,
                    store,
                );
            })
            .with_icon(ActionIcon::Filter),
        );
        let cell_value_equals = cell_value.clone();
        let column_name_equals = resolved_column.clone();
        items.push(
            ContextMenuItem::new("Filter by selection", move || {
                apply_filter_for_value(
                    column_name_equals.clone(),
                    cell_value_equals.clone(),
                    QueryFilterOperator::Equals,
                    store,
                );
            })
            .with_icon(ActionIcon::FilterApply)
            .separator(),
        );
    }

    {
        let col_asc = resolved_column.clone();
        items.push(
            ContextMenuItem::new("Sort ascending", move || {
                sort_by_column(&col_asc, false, store);
            })
            .with_icon(ActionIcon::Previous),
        );
        let col_desc = resolved_column;
        items.push(
            ContextMenuItem::new("Sort descending", move || {
                sort_by_column(&col_desc, true, store);
            })
            .with_icon(ActionIcon::Next),
        );
    }

    {
        let row_values = row_values.clone();
        let columns = columns.clone();
        items.push(
            ContextMenuItem::new("Copy entire row as TSV", move || {
                let _ = copy_to_clipboard(format_row_tsv(&columns, &row_values));
            })
            .with_icon(ActionIcon::ExportCsv),
        );
    }

    items
}

fn is_valid_cell_json(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let first = trimmed.chars().next().expect("trimmed is non-empty");
    if first != '{' && first != '[' {
        return false;
    }
    crate::screens::workspace::components::value_editor::is_valid_json(trimmed)
}

/// Returns `(original_index, column_name)` pairs for columns that
/// are not in `hidden_columns`. The original index is preserved so
/// `row.values[col_index]` lookups stay valid after columns hide.
fn filter_visible_columns(columns: &[String], hidden_columns: &[String]) -> Vec<(usize, String)> {
    columns
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, name)| !hidden_columns.contains(name))
        .collect()
}

/// Set the active tab's sort to `column_name` in the given direction.
/// Going from descending to `None` (i.e. clear sort) is handled by
/// the existing `toggle_active_tab_sort` state machine — calling
/// that helper three times cycles ascending → descending → none.
fn sort_by_column(
    column_name: &str,
    descending: bool,
    store: TabStore,
) {
    // Inspect the current sort. If it matches the requested
    // direction, no-op. Otherwise walk the state machine by calling
    // `toggle_active_tab_sort` until the desired state is reached.
    for _ in 0..2 {
        let current = store
            .result
            .read()
            .get(&store.active_tab_id())
            .and_then(|tab| tab.sort.clone());
        let matches = match (&current, descending) {
            (Some(sort), false) => sort.column_name == column_name && !sort.descending,
            (Some(sort), true) => sort.column_name == column_name && sort.descending,
            _ => false,
        };
        if matches {
            return;
        }
        toggle_active_tab_sort(store, store.active_tab_id(), column_name.to_string());
    }
}

/// Decide whether the hover-revealed cell-level filter affordance should
/// appear for a given cell value. Mirrors the gating used by the cell
/// context menu's "Filter by this value" item, so the affordance is only
/// shown when there is an actual value to filter on (trim-aware, so a
/// whitespace-only cell stays hidden too).
fn should_show_cell_filter(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Apply a filter on `column_name` matching `value` with the
/// given operator. An empty `value` plus `Contains` opens a blank
/// filter (the user can then type the value in the panel).
fn apply_filter_for_value(
    column_name: String,
    value: String,
    operator: QueryFilterOperator,
    store: TabStore,
) {
    let filter = QueryFilter {
        mode: QueryFilterMode::And,
        rules: vec![QueryFilterRule {
            column_name,
            operator,
            value,
        }],
    };
    apply_active_tab_filter(store, store.active_tab_id(), filter);
}

fn quick_filter_is_meaningful(operator: QueryFilterOperator, value: &str) -> bool {
    if operator.is_nullary() {
        true
    } else {
        !value.is_empty()
    }
}

fn build_quick_filter(column: String, operator: QueryFilterOperator, value: String) -> QueryFilter {
    QueryFilter {
        mode: QueryFilterMode::And,
        rules: vec![QueryFilterRule {
            column_name: column,
            operator,
            value,
        }],
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_quick_filter(
    store: TabStore,
    mut filter_draft: Signal<QueryFilter>,
    quick_filter_column: Signal<String>,
    quick_filter_operator: Signal<QueryFilterOperator>,
    quick_filter_value: Signal<String>,
) {
    let column = quick_filter_column();
    if column.is_empty() {
        return;
    }
    let operator = quick_filter_operator();
    let value = quick_filter_value();
    if !quick_filter_is_meaningful(operator, &value) {
        return;
    }
    filter_draft.with_mut(|filter| {
        *filter = build_quick_filter(column.clone(), operator, value.clone());
    });
    let filter = build_quick_filter(column, operator, value);
    apply_active_tab_filter(store, store.active_tab_id(), filter);
}

#[allow(clippy::too_many_arguments)]
fn apply_quick_filter_with_columns(
    store: TabStore,
    mut filter_draft: Signal<QueryFilter>,
    quick_filter_column: Signal<String>,
    quick_filter_operator: Signal<QueryFilterOperator>,
    quick_filter_value: Signal<String>,
    columns: &[String],
) {
    let column = quick_filter_column();
    let column = if column.is_empty() && !columns.is_empty() {
        columns[0].clone()
    } else {
        column
    };
    if column.is_empty() {
        return;
    }
    let operator = quick_filter_operator();
    let value = quick_filter_value();
    if !quick_filter_is_meaningful(operator, &value) {
        return;
    }
    filter_draft.with_mut(|filter| {
        *filter = build_quick_filter(column.clone(), operator, value.clone());
    });
    let filter = build_quick_filter(column, operator, value);
    apply_active_tab_filter(store, store.active_tab_id(), filter);
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        filter_panel_should_auto_open,
        filter_panel_should_collapse_after_clear,
        filter_visible_columns,
        format_row_edit_error,
        result_error_message,
        result_status_text_for_display,
        should_render_result_status_chip,
        should_show_cell_filter,
    };
    use crate::screens::workspace::actions::rows_toolbar_summary;
    use models::{QueryFilter, QueryFilterMode, QueryFilterOperator, QueryFilterRule};

    #[test]
    fn extracts_query_error_from_status() {
        assert_eq!(
            result_error_message("Error: SQLite error: near \"from\": syntax error"),
            Some("SQLite error: near \"from\": syntax error".to_string())
        );
    }

    #[test]
    fn ignores_non_error_status() {
        assert_eq!(result_error_message("Loaded rows 1-10"), None);
    }

    #[test]
    fn summarizes_empty_page_without_invalid_range() {
        assert_eq!(rows_toolbar_summary(0, 0, 100), "0 rows · page size 100");
    }

    #[test]
    fn keeps_filters_collapsed_without_active_filter_or_meaningful_draft() {
        let filter = QueryFilter {
            mode: QueryFilterMode::And,
            rules: vec![QueryFilterRule {
                column_name: "name".to_string(),
                operator: QueryFilterOperator::Contains,
                value: String::new(),
            }],
        };

        assert!(!filter_panel_should_auto_open(false, &filter));
        assert!(filter_panel_should_collapse_after_clear(false, &filter));
    }

    #[test]
    fn opens_filters_for_active_filter_or_meaningful_draft() {
        let meaningful_filter = QueryFilter {
            mode: QueryFilterMode::And,
            rules: vec![QueryFilterRule {
                column_name: "name".to_string(),
                operator: QueryFilterOperator::Contains,
                value: "Ada".to_string(),
            }],
        };

        assert!(filter_panel_should_auto_open(true, &meaningful_filter));
        assert!(filter_panel_should_auto_open(false, &meaningful_filter));
    }

    #[test]
    fn compact_layout_supports_25_rows_at_default_window() {
        const WINDOW_HEIGHT: i32 = 920;
        const APP_TOOLBAR: i32 = 44;
        const STATUSBAR: i32 = 26;
        const WORKSPACE_PADDING: i32 = 12;
        const WORKSPACE_HEADER: i32 = 32;
        const TABBAR: i32 = 34;
        const RESULTS_TOOLBAR: i32 = 30;
        const ROW_HEIGHT_PX: i32 = 22;

        let chrome_height = APP_TOOLBAR
            + STATUSBAR
            + WORKSPACE_PADDING
            + WORKSPACE_HEADER
            + TABBAR
            + RESULTS_TOOLBAR;
        let available_height = WINDOW_HEIGHT - chrome_height;
        let visible_rows = available_height / ROW_HEIGHT_PX;

        assert!(
            visible_rows >= 25,
            "Expected >= 25 visible rows (got {visible_rows}) with {available_height}px available"
        );
    }

    #[test]
    fn status_chip_visible_for_loading_states() {
        assert!(should_render_result_status_chip("Loading rows...", false));
        assert!(should_render_result_status_chip("Running query...", false));
    }

    #[test]
    fn status_chip_visible_for_error_states() {
        assert!(should_render_result_status_chip(
            "Error: connection failed",
            false
        ));
        assert!(should_render_result_status_chip(
            "Preview error: timeout",
            false
        ));
    }

    #[test]
    fn status_chip_visible_for_pending_changes() {
        assert!(should_render_result_status_chip("Ready", true));
        assert!(should_render_result_status_chip("", true));
    }

    #[test]
    fn status_chip_hidden_for_ready_and_loaded_states() {
        assert!(!should_render_result_status_chip("Ready", false));
        assert!(!should_render_result_status_chip(
            "Loaded rows 1-10 of 100",
            false
        ));
    }

    #[test]
    fn status_chip_hidden_for_empty_status() {
        assert!(!should_render_result_status_chip("", false));
        assert!(!should_render_result_status_chip("   ", false));
    }

    #[test]
    fn status_text_removes_status_prefix() {
        assert_eq!(
            result_status_text_for_display("Status: Loading..."),
            "Loading..."
        );
        assert_eq!(result_status_text_for_display("Status: Ready"), "Ready");
    }

    #[test]
    fn status_text_preserves_text_without_prefix() {
        assert_eq!(result_status_text_for_display("Loading..."), "Loading...");
        assert_eq!(
            result_status_text_for_display("Error: failed"),
            "Error: failed"
        );
    }

    #[test]
    fn row_edit_error_uses_display_not_debug() {
        let formatted = format_row_edit_error("Row insert", "constraint violation");
        assert_eq!(formatted, "Row insert error: constraint violation");
        assert!(!formatted.contains(":?"));
    }

    #[test]
    fn cell_filter_affordance_visible_for_non_empty_values() {
        assert!(should_show_cell_filter("Ada"));
        assert!(should_show_cell_filter("0"));
        assert!(should_show_cell_filter(" hello "));
    }

    #[test]
    fn cell_filter_affordance_hidden_for_empty_values() {
        assert!(!should_show_cell_filter(""));
        assert!(!should_show_cell_filter("   "));
    }

    #[test]
    fn filter_visible_columns_returns_all_when_none_hidden() {
        let columns = vec!["id".to_string(), "name".to_string(), "email".to_string()];
        let visible = filter_visible_columns(&columns, &[]);
        assert_eq!(
            visible,
            vec![
                (0, "id".to_string()),
                (1, "name".to_string()),
                (2, "email".to_string()),
            ]
        );
    }

    #[test]
    fn filter_visible_columns_preserves_original_indices_after_hide() {
        let columns = vec!["id".to_string(), "name".to_string(), "email".to_string()];
        let hidden = vec!["name".to_string()];
        let visible = filter_visible_columns(&columns, &hidden);
        // "email" must keep original_index 2 so row.values[2] still
        // resolves to the right cell value after the hide.
        assert_eq!(
            visible,
            vec![(0, "id".to_string()), (2, "email".to_string())]
        );
    }

    #[test]
    fn filter_visible_columns_hides_multiple_non_contiguous_columns() {
        let columns = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
            "e".to_string(),
        ];
        let hidden = vec!["b".to_string(), "d".to_string()];
        let visible = filter_visible_columns(&columns, &hidden);
        assert_eq!(
            visible,
            vec![
                (0, "a".to_string()),
                (2, "c".to_string()),
                (4, "e".to_string()),
            ]
        );
    }

    #[test]
    fn filter_visible_columns_handles_unknown_hidden_name() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let hidden = vec!["does_not_exist".to_string()];
        let visible = filter_visible_columns(&columns, &hidden);
        assert_eq!(
            visible,
            vec![(0, "id".to_string()), (1, "name".to_string())]
        );
    }
}

fn can_sort_tab(tab: &TabResultState) -> bool {
    tab.preview_source.is_some() || tab.last_run_sql.as_deref().is_some_and(is_sortable_sql)
}

fn can_filter_tab(tab: &TabResultState) -> bool {
    can_sort_tab(tab)
}

fn is_sortable_sql(sql: &str) -> bool {
    matches!(
        sql.split_whitespace().next(),
        Some("select" | "SELECT" | "with" | "WITH")
    )
}

fn sort_button_class(active_sort: Option<&QuerySort>, column: &str) -> &'static str {
    match active_sort {
        Some(sort) if sort.column_name == column =>
            "results__sort-button results__sort-button--active",
        _ => "results__sort-button",
    }
}

fn sort_indicator(active_sort: Option<&QuerySort>, column: &str) -> &'static str {
    match active_sort {
        Some(sort) if sort.column_name == column && sort.descending => "↓",
        Some(sort) if sort.column_name == column => "↑",
        _ => "↕",
    }
}

fn result_columns(result: Option<&QueryOutput>) -> Vec<String> {
    match result {
        Some(QueryOutput::Table(page)) => page.columns.clone(),
        _ => Vec::new(),
    }
}

fn materialize_display_rows(
    page: &models::QueryPage,
    pending_changes: &PendingTableChanges,
) -> Vec<DisplayRow> {
    let mut rows = pending_changes
        .inserted_rows
        .iter()
        .map(|row| DisplayRow {
            row_ref: EditableRowRef::PendingInsert(row.id),
            values: row
                .values
                .iter()
                .map(|value| value.clone().unwrap_or_default())
                .collect(),
        })
        .collect::<Vec<_>>();

    // Build O(1) lookup structures instead of linear scans per row/cell.
    let deleted_set: HashSet<&str> = pending_changes
        .deleted_rows
        .iter()
        .map(|d| d.locator.as_str())
        .collect();
    let updated_map: HashMap<(&str, &str), &str> = pending_changes
        .updated_cells
        .iter()
        .map(|c| {
            (
                (c.locator.as_str(), c.column_name.as_str()),
                c.value.as_str(),
            )
        })
        .collect();

    if let Some(editable) = page.editable.as_ref() {
        rows.extend(page.rows.iter().enumerate().filter_map(|(row_index, row)| {
            let locator = editable
                .row_locators
                .get(row_index)
                .cloned()
                .unwrap_or_default();
            if deleted_set.contains(locator.as_str()) {
                return None;
            }
            Some(DisplayRow {
                row_ref: EditableRowRef::Existing(locator),
                values: page
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(col_index, column_name)| {
                        existing_cell_value_fast(
                            &updated_map,
                            editable,
                            row_index,
                            col_index,
                            column_name,
                            row,
                        )
                    })
                    .collect(),
            })
        }));
    } else {
        rows.extend(
            page.rows
                .iter()
                .enumerate()
                .map(|(row_index, row)| DisplayRow {
                    row_ref: EditableRowRef::Existing(format!("result-{row_index}")),
                    values: row.clone(),
                }),
        );
    }

    rows
}

/// O(1) replacement for the old `existing_cell_value` linear scan.
/// Uses a pre-built HashMap keyed by (locator, column_name).
fn existing_cell_value_fast(
    updated_map: &HashMap<(&str, &str), &str>,
    editable: &EditableTableContext,
    row_index: usize,
    col_index: usize,
    column_name: &str,
    row: &[String],
) -> String {
    let base_value = row.get(col_index).cloned().unwrap_or_default();
    let Some(locator) = editable.row_locators.get(row_index) else {
        return base_value;
    };

    updated_map
        .get(&(locator.as_str(), column_name))
        .map(|v| v.to_string())
        .unwrap_or(base_value)
}

fn display_row_label(offset: u64, draft_rows: usize, row_index: usize, row: &DisplayRow) -> String {
    match row.row_ref {
        EditableRowRef::PendingInsert(insert_id) => format!("Draft Row {insert_id}"),
        EditableRowRef::Existing(_) => {
            let persisted_index = row_index.saturating_sub(draft_rows);
            format!("Row {}", offset + persisted_index as u64 + 1)
        }
    }
}

fn display_row_key(row: &DisplayRow) -> String {
    match &row.row_ref {
        EditableRowRef::Existing(locator) => format!("row-{locator}"),
        EditableRowRef::PendingInsert(insert_id) => format!("draft-{insert_id}"),
    }
}

fn row_class(is_selected: bool, row: &DisplayRow) -> &'static str {
    match (&row.row_ref, is_selected) {
        (EditableRowRef::PendingInsert(_), true) =>
            "results__row results__row--draft results__row--selected",
        (EditableRowRef::PendingInsert(_), false) => "results__row results__row--draft",
        (_, true) => "results__row results__row--selected",
        (_, false) => "results__row",
    }
}

/// O(1) cell-class lookup using a pre-built HashSet of (locator, column_name).
fn cell_class(
    editable: bool,
    row: &DisplayRow,
    column_name: Option<&String>,
    updated_cells_set: &HashSet<(String, String)>,
) -> &'static str {
    let mut is_pending = matches!(row.row_ref, EditableRowRef::PendingInsert(_));
    if let (EditableRowRef::Existing(locator), Some(column_name)) = (&row.row_ref, column_name) {
        is_pending = updated_cells_set.contains(&(locator.clone(), column_name.clone()));
    }

    match (editable, is_pending) {
        (true, true) => "results__cell results__cell--editable results__cell--pending",
        (true, false) => "results__cell results__cell--editable",
        (false, true) => "results__cell results__cell--pending",
        (false, false) => "results__cell",
    }
}

fn pending_changes_summary(pending_changes: &PendingTableChanges) -> String {
    let inserts = pending_changes.inserted_rows.len();
    let updates = pending_changes.updated_cells.len();
    let deletes = pending_changes.deleted_rows.len();
    let mut parts = Vec::new();
    if inserts > 0 {
        parts.push(if inserts == 1 {
            "1 insert".to_string()
        } else {
            format!("{inserts} inserts")
        });
    }
    if updates > 0 {
        parts.push(if updates == 1 {
            "1 update".to_string()
        } else {
            format!("{updates} updates")
        });
    }
    if deletes > 0 {
        parts.push(if deletes == 1 {
            "1 delete".to_string()
        } else {
            format!("{deletes} deletes")
        });
    }
    if parts.is_empty() {
        "No pending changes".to_string()
    } else {
        format!("{} pending", parts.join(", "))
    }
}

fn filter_draft_from_state(active_filter: Option<&QueryFilter>, columns: &[String]) -> QueryFilter {
    let mut filter = active_filter
        .cloned()
        .unwrap_or_else(|| blank_filter(columns));

    if filter.rules.is_empty() {
        filter
            .rules
            .push(blank_rule(default_filter_column(columns)));
    }

    for rule in &mut filter.rules {
        if rule.column_name.trim().is_empty()
            || !columns.iter().any(|column| column == &rule.column_name)
        {
            rule.column_name = default_filter_column(columns);
        }
    }

    filter
}

fn filter_sync_key_for_tab(active_tab: Option<&TabResultState>, columns: &[String]) -> String {
    match active_tab {
        Some(tab) => format!("{:?}|{:?}", tab.filter.as_ref(), columns),
        None => "no-tab".to_string(),
    }
}

fn row_sync_key_for_tab(
    active_tab: Option<&TabResultState>,
    result: Option<&QueryOutput>,
    inserted_rows: usize,
) -> String {
    match (active_tab, result) {
        (Some(tab), Some(QueryOutput::Table(page))) => format!(
            "{:?}|{:?}|{}|{}|{}|{}",
            tab.preview_source
                .as_ref()
                .map(|source| &source.qualified_name),
            tab.last_run_sql.as_ref(),
            page.offset,
            page.rows.len(),
            page.columns.len(),
            inserted_rows
        ),
        (Some(_), _) => "no-table".to_string(),
        _ => "no-tab".to_string(),
    }
}

fn blank_filter(columns: &[String]) -> QueryFilter {
    QueryFilter {
        mode: QueryFilterMode::And,
        rules: vec![blank_rule(default_filter_column(columns))],
    }
}

fn blank_rule(default_column: String) -> QueryFilterRule {
    QueryFilterRule {
        column_name: default_column,
        operator: QueryFilterOperator::Contains,
        value: String::new(),
    }
}

fn default_filter_column(columns: &[String]) -> String {
    columns.first().cloned().unwrap_or_default()
}

fn has_meaningful_rules(filter: &QueryFilter) -> bool {
    filter.rules.iter().any(|rule| {
        !rule.column_name.trim().is_empty()
            && (!rule.value.trim().is_empty() || rule.operator.is_nullary())
    })
}

fn filter_panel_should_auto_open(active_filter_present: bool, filter_draft: &QueryFilter) -> bool {
    active_filter_present || has_meaningful_rules(filter_draft)
}

#[cfg(test)]
fn filter_panel_should_collapse_after_clear(
    active_filter_present: bool,
    filter_draft: &QueryFilter,
) -> bool {
    !active_filter_present && !has_meaningful_rules(filter_draft)
}

fn update_filter_mode(mut filter_draft: Signal<QueryFilter>, value: String) {
    filter_draft.with_mut(|filter| {
        filter.mode = if value.eq_ignore_ascii_case("or") {
            QueryFilterMode::Or
        } else {
            QueryFilterMode::And
        };
    });
}

fn add_filter_rule(mut filter_draft: Signal<QueryFilter>, columns: &[String]) {
    filter_draft.with_mut(|filter| {
        filter
            .rules
            .push(blank_rule(default_filter_column(columns)));
    });
}

fn remove_filter_rule(mut filter_draft: Signal<QueryFilter>, index: usize, columns: &[String]) {
    filter_draft.with_mut(|filter| {
        if index < filter.rules.len() {
            filter.rules.remove(index);
        }
        if filter.rules.is_empty() {
            filter
                .rules
                .push(blank_rule(default_filter_column(columns)));
        }
    });
}

fn update_filter_rule_column(
    mut filter_draft: Signal<QueryFilter>,
    index: usize,
    column_name: String,
) {
    filter_draft.with_mut(|filter| {
        if let Some(rule) = filter.rules.get_mut(index) {
            rule.column_name = column_name;
        }
    });
}

fn update_filter_rule_operator(
    mut filter_draft: Signal<QueryFilter>,
    index: usize,
    operator_value: String,
) {
    filter_draft.with_mut(|filter| {
        if let Some(rule) = filter.rules.get_mut(index) {
            rule.operator = parse_filter_operator(&operator_value);
            if rule.operator.is_nullary() {
                rule.value.clear();
            }
        }
    });
}

fn update_filter_rule_value(mut filter_draft: Signal<QueryFilter>, index: usize, value: String) {
    filter_draft.with_mut(|filter| {
        if let Some(rule) = filter.rules.get_mut(index) {
            rule.value = value;
        }
    });
}

fn supported_filter_operators() -> [QueryFilterOperator; 8] {
    [
        QueryFilterOperator::Contains,
        QueryFilterOperator::NotContains,
        QueryFilterOperator::Equals,
        QueryFilterOperator::NotEquals,
        QueryFilterOperator::StartsWith,
        QueryFilterOperator::EndsWith,
        QueryFilterOperator::IsNull,
        QueryFilterOperator::IsNotNull,
    ]
}

fn filter_mode_value(mode: QueryFilterMode) -> &'static str {
    match mode {
        QueryFilterMode::And => "and",
        QueryFilterMode::Or => "or",
    }
}

fn filter_operator_value(operator: QueryFilterOperator) -> &'static str {
    match operator {
        QueryFilterOperator::Contains => "contains",
        QueryFilterOperator::NotContains => "not_contains",
        QueryFilterOperator::Equals => "equals",
        QueryFilterOperator::NotEquals => "not_equals",
        QueryFilterOperator::StartsWith => "starts_with",
        QueryFilterOperator::EndsWith => "ends_with",
        QueryFilterOperator::IsNull => "is_null",
        QueryFilterOperator::IsNotNull => "is_not_null",
    }
}

fn filter_operator_label(operator: QueryFilterOperator) -> &'static str {
    match operator {
        QueryFilterOperator::Contains => "Contains",
        QueryFilterOperator::NotContains => "Does not contain",
        QueryFilterOperator::Equals => "Equals",
        QueryFilterOperator::NotEquals => "Does not equal",
        QueryFilterOperator::StartsWith => "Starts with",
        QueryFilterOperator::EndsWith => "Ends with",
        QueryFilterOperator::IsNull => "Is null",
        QueryFilterOperator::IsNotNull => "Is not null",
    }
}

fn parse_filter_operator(value: &str) -> QueryFilterOperator {
    match value {
        "not_contains" => QueryFilterOperator::NotContains,
        "equals" => QueryFilterOperator::Equals,
        "not_equals" => QueryFilterOperator::NotEquals,
        "starts_with" => QueryFilterOperator::StartsWith,
        "ends_with" => QueryFilterOperator::EndsWith,
        "is_null" => QueryFilterOperator::IsNull,
        "is_not_null" => QueryFilterOperator::IsNotNull,
        _ => QueryFilterOperator::Contains,
    }
}

fn original_cell_value(
    page: &models::QueryPage,
    locator: &str,
    col_index: usize,
) -> Option<String> {
    let editable = page.editable.as_ref()?;
    let row_index = editable
        .row_locators
        .iter()
        .position(|current_locator| current_locator == locator)?;
    page.rows.get(row_index)?.get(col_index).cloned()
}

fn commit_cell_edit(
    mut editing_cell: Signal<Option<EditingCell>>,
    mut store: TabStore,
    editing: EditingCell,
) {
    let current_id = store.active_tab_id();
    if read_only_mode_enabled() {
        editing_cell.set(None);
        set_active_tab_status(store, current_id, read_only_mode_block_status("cell edit"));
        return;
    }

    let current_tab = store.result.read().get(&current_id).cloned();
    let Some(current_tab) = current_tab else {
        editing_cell.set(None);
        return;
    };
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        editing_cell.set(None);
        return;
    };
    if page.editable.is_none() {
        editing_cell.set(None);
        return;
    }
    let Some(column_name) = page.columns.get(editing.col_index).cloned() else {
        editing_cell.set(None);
        return;
    };

    editing_cell.set(None);
    store.pending.with_mut(|pending| {
        let Some(tab) = pending.get_mut(&current_id) else {
            return;
        };

        match editing.row_ref {
            EditableRowRef::PendingInsert(insert_id) => {
                if let Some(row) = tab
                    .pending_table_changes
                    .inserted_rows
                    .iter_mut()
                    .find(|row| row.id == insert_id)
                    && let Some(value) = row.values.get_mut(editing.col_index)
                {
                    *value = Some(editing.value);
                }
            }
            EditableRowRef::Existing(locator) => {
                let original_value =
                    original_cell_value(&page, locator.as_str(), editing.col_index)
                        .unwrap_or_default();

                if original_value == editing.value {
                    tab.pending_table_changes.updated_cells.retain(|change| {
                        !(change.locator == locator && change.column_name == column_name)
                    });
                } else if let Some(change) = tab
                    .pending_table_changes
                    .updated_cells
                    .iter_mut()
                    .find(|change| change.locator == locator && change.column_name == column_name)
                {
                    change.value = editing.value;
                } else {
                    tab.pending_table_changes
                        .updated_cells
                        .push(PendingCellChange {
                            locator,
                            column_name,
                            value: editing.value,
                        });
                }
            }
        }

        let summary = pending_changes_summary(&tab.pending_table_changes);
        store.result.with_mut(|r| {
            if let Some(res) = r.get_mut(&current_id) {
                res.status = summary;
            }
        });
    });
}

fn insert_empty_row(mut store: TabStore) {
    let current_id = store.active_tab_id();
    if read_only_mode_enabled() {
        set_active_tab_status(
            store,
            current_id,
            read_only_mode_block_status("draft row insert"),
        );
        return;
    }

    let current_tab = store.result.read().get(&current_id).cloned();
    let Some(current_tab) = current_tab else {
        return;
    };
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        set_active_tab_status(store, current_id, "No editable table is open".to_string());
        return;
    };
    let Some(_) = page.editable.clone() else {
        set_active_tab_status(
            store,
            current_id,
            "Row insert is available only for editable table views".to_string(),
        );
        return;
    };
    let editable = page.editable.clone();
    let page_columns = page.columns.clone();
    let mut inserted_row_id = None;
    store.pending.with_mut(|pending| {
        if let Some(tab) = pending.get_mut(&current_id) {
            let insert_id = tab.pending_table_changes.next_insert_id;
            tab.pending_table_changes.next_insert_id += 1;
            tab.pending_table_changes.inserted_rows.insert(
                0,
                PendingInsertRow {
                    id: insert_id,
                    values: vec![None; page.columns.len()],
                },
            );
            let summary = pending_changes_summary(&tab.pending_table_changes);
            store.result.with_mut(|r| {
                if let Some(res) = r.get_mut(&current_id) {
                    res.status = summary;
                }
            });
            inserted_row_id = Some(insert_id);
        }
    });
    let (Some(editable), Some(inserted_row_id)) = (editable, inserted_row_id) else {
        return;
    };
    let session_id = store
        .meta
        .read()
        .get(&current_id)
        .map(|m| m.session_id)
        .unwrap_or(0);
    let Some(connection) = tab_connection_or_error(store, current_id, session_id) else {
        return;
    };

    spawn(async move {
        match services::next_table_primary_key_id(connection, editable.source.clone()).await {
            Ok(Some((column_name, remote_next_id))) => {
                store.pending.with_mut(|pending| {
                    let Some(tab) = pending.get_mut(&current_id) else {
                        return;
                    };
                    let Some(column_index) = page_columns
                        .iter()
                        .position(|column| column.eq_ignore_ascii_case(&column_name))
                    else {
                        return;
                    };
                    let next_id = next_pending_auto_id(
                        &tab.pending_table_changes,
                        column_index,
                        remote_next_id,
                    );
                    let Some(row) = tab
                        .pending_table_changes
                        .inserted_rows
                        .iter_mut()
                        .find(|row| row.id == inserted_row_id)
                    else {
                        return;
                    };
                    let Some(value) = row.values.get_mut(column_index) else {
                        return;
                    };
                    if value.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                        return;
                    }
                    *value = Some(next_id.to_string());
                });
            }
            Ok(None) => {}
            Err(err) => {
                set_active_tab_status(
                    store,
                    current_id,
                    format_row_edit_error("Draft row added without auto id", err),
                );
            }
        }
    });
}

fn apply_pending_changes(mut store: TabStore) {
    let current_id = store.active_tab_id();
    if read_only_mode_enabled() {
        set_active_tab_status(
            store,
            current_id,
            read_only_mode_block_status("pending table changes"),
        );
        return;
    }

    let current_tab = store.result.read().get(&current_id).cloned();
    let Some(current_tab) = current_tab else {
        return;
    };
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        set_active_tab_status(store, current_id, "No editable table is open".to_string());
        return;
    };
    let Some(editable) = page.editable.clone() else {
        set_active_tab_status(
            store,
            current_id,
            "Changes can be applied only for editable table views".to_string(),
        );
        return;
    };
    let pending_changes = store
        .pending
        .read()
        .get(&current_id)
        .map(|p| p.pending_table_changes.clone())
        .unwrap_or_default();
    if pending_changes.is_empty() {
        set_active_tab_status(store, current_id, "No pending changes".to_string());
        return;
    }

    let session_id = store
        .meta
        .read()
        .get(&current_id)
        .map(|m| m.session_id)
        .unwrap_or(0);
    let Some(connection) = tab_connection_or_error(store, current_id, session_id) else {
        return;
    };

    let columns = page.columns.clone();
    let summary = pending_changes_summary(&pending_changes);
    set_active_tab_status(store, current_id, format!("Applying {summary}..."));

    spawn(async move {
        for row in pending_changes.inserted_rows {
            let column_values = columns
                .iter()
                .cloned()
                .zip(row.values)
                .filter_map(|(column_name, value)| value.map(|value| (column_name, value)))
                .collect::<Vec<_>>();

            if let Err(err) = services::insert_table_row_with_values(
                connection.clone(),
                editable.source.clone(),
                column_values,
            )
            .await
            {
                set_active_tab_status(store, current_id, format_row_edit_error("Row insert", err));
                return;
            }
        }

        for change in pending_changes.updated_cells {
            if let Err(err) = services::update_table_cell(
                connection.clone(),
                editable.source.clone(),
                change.locator,
                change.column_name,
                change.value,
            )
            .await
            {
                set_active_tab_status(store, current_id, format_row_edit_error("Cell update", err));
                return;
            }
        }

        for delete in pending_changes.deleted_rows {
            if let Err(err) = services::delete_table_row(
                connection.clone(),
                editable.source.clone(),
                delete.locator,
            )
            .await
            {
                set_active_tab_status(store, current_id, format_row_edit_error("Row delete", err));
                return;
            }
        }

        store.pending.with_mut(|pending| {
            if let Some(tab) = pending.get_mut(&current_id) {
                tab.pending_table_changes = PendingTableChanges::default();
            }
        });
        store.result.with_mut(|r| {
            if let Some(res) = r.get_mut(&current_id) {
                res.status = format!("Applied changes to {}", editable.source.table_name);
            }
        });

        if let Some(updated_tab) =
            crate::screens::workspace::tab_store::materialize_tab_state(store, current_id)
        {
            refresh_tab_result(store, updated_tab, Some(editable.source));
        }
    });
}

fn discard_pending_changes(mut store: TabStore) {
    let current_id = store.active_tab_id();
    store.pending.with_mut(|pending| {
        if let Some(tab) = pending.get_mut(&current_id) {
            tab.pending_table_changes = PendingTableChanges::default();
        }
    });
    store.result.with_mut(|r| {
        if let Some(res) = r.get_mut(&current_id) {
            res.status = "Discarded pending changes".to_string();
        }
    });
}

fn delete_selected_row(mut store: TabStore, row_index: usize) {
    let current_id = store.active_tab_id();
    if read_only_mode_enabled() {
        set_active_tab_status(store, current_id, read_only_mode_block_status("row delete"));
        return;
    }

    let current_tab = store.result.read().get(&current_id).cloned();
    let Some(current_tab) = current_tab else {
        return;
    };
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        set_active_tab_status(store, current_id, "No editable table is open".to_string());
        return;
    };
    let Some(_editable) = page.editable.clone() else {
        set_active_tab_status(
            store,
            current_id,
            "Row delete is available only for editable table views".to_string(),
        );
        return;
    };
    let pending_changes = store
        .pending
        .read()
        .get(&current_id)
        .map(|p| p.pending_table_changes.clone())
        .unwrap_or_default();
    let display_rows = materialize_display_rows(&page, &pending_changes);
    let Some(row) = display_rows.get(row_index).cloned() else {
        set_active_tab_status(
            store,
            current_id,
            "The selected row is no longer available".to_string(),
        );
        return;
    };

    if let EditableRowRef::PendingInsert(insert_id) = row.row_ref {
        store.pending.with_mut(|pending| {
            if let Some(tab) = pending.get_mut(&current_id) {
                tab.pending_table_changes
                    .inserted_rows
                    .retain(|row| row.id != insert_id);
                let summary = pending_changes_summary(&tab.pending_table_changes);
                store.result.with_mut(|r| {
                    if let Some(res) = r.get_mut(&current_id) {
                        res.status = summary;
                    }
                });
            }
        });
        return;
    }

    let EditableRowRef::Existing(locator) = row.row_ref else {
        return;
    };

    store.pending.with_mut(|pending| {
        if let Some(tab) = pending.get_mut(&current_id) {
            tab.pending_table_changes
                .deleted_rows
                .push(PendingDeleteRow {
                    locator: locator.clone(),
                });
            tab.pending_table_changes
                .updated_cells
                .retain(|change| change.locator != locator);
            let summary = pending_changes_summary(&tab.pending_table_changes);
            store.result.with_mut(|r| {
                if let Some(res) = r.get_mut(&current_id) {
                    res.status = summary;
                }
            });
        }
    });
}

fn next_pending_auto_id(
    pending_changes: &PendingTableChanges,
    column_index: usize,
    remote_next_id: i64,
) -> i64 {
    let pending_next_id = pending_changes
        .inserted_rows
        .iter()
        .filter_map(|row| row.values.get(column_index))
        .filter_map(|value| value.as_ref())
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .max()
        .map(|max_id| max_id + 1)
        .unwrap_or(remote_next_id);

    pending_next_id.max(remote_next_id)
}
