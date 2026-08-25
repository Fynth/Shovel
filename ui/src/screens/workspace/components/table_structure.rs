use crate::screens::workspace::{actions::tab_connection_or_error, tab_store::TabStore};
use dioxus::prelude::*;
use models::{
    DatabaseConnection,
    ExplorerNodeKind,
    QueryOutput,
    TableForeignKey,
    TablePreviewSource,
};

/// State tracked by every lazy panel: a load has been kicked off
/// (so we don't fire one per sub-tab re-select) and a status message
/// (loading / error / idle).
#[derive(Clone, Debug, PartialEq)]
enum PanelState {
    Idle,
    Loading,
    Ready,
    Error(String),
}

impl PanelState {
    fn class(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error(_) => "error",
        }
    }
}

/// Build a stable cache key for a panel so re-selecting the same
/// sub-tab on the same table does not refire the load.
fn cache_key(source: &TablePreviewSource, suffix: &str) -> String {
    format!(
        "{}|{}|{}",
        source.qualified_name,
        suffix,
        source.schema.as_deref().unwrap_or("")
    )
}

/// Lightweight "open a connection for this tab" wrapper. Surfaces
/// the standard closed-connection status message via
/// `tab_connection_or_error` and returns the live connection on
/// success. Mirrors the helpers in `actions.rs` so panels share the
/// same UX message.
fn panel_connection(store: TabStore, session_id: u64) -> Option<DatabaseConnection> {
    if tab_connection_or_error(store, store.active_tab_id(), session_id).is_some() {
        crate::app_state::session_connection(session_id)
    } else {
        None
    }
}

/// The Structure sub-tab: renders the columns of the previewed
/// table as a read-only grid. Reuses the `describe_table` output
/// that the existing `Structure` tab kind already populates into
/// `tab.result`; if the active tab is a `TablePreview` (not a
/// dedicated `Structure` tab) we spawn the load lazily on first
/// selection.
#[component]
pub fn StructurePanel(
    store: TabStore,
    source: TablePreviewSource,
    session_id: u64,
    existing_result: Option<QueryOutput>,
) -> Element {
    let mut state = use_signal(|| PanelState::Idle);
    let mut loaded_result = use_signal(|| existing_result.clone());

    // Kick off a `describe_table` load the first time the panel
    // is selected for a given table. Subsequent re-selects of the
    // same table short-circuit on the `Ready` state.
    let key = cache_key(&source, "structure");
    let mut last_key = use_signal(String::new);
    if last_key() != key {
        last_key.set(key.clone());
        if let Some(connection) = panel_connection(store, session_id) {
            state.set(PanelState::Loading);
            let mut store_sig = store;
            let source_clone = source.clone();
            spawn(async move {
                match services::describe_table(
                    connection,
                    source_clone.schema.clone(),
                    source_clone.table_name.clone(),
                )
                .await
                {
                    Ok(output) => {
                        // Write the structure into the tab's result so
                        // the dedicated Structure tab variant and the
                        // shared cache stay aligned. This matches what
                        // `open_structure_tab` already does.
                        let tab_id = store_sig.active_tab_id();
                        store_sig.result.with_mut(|m| {
                            if let Some(tab) = m.get_mut(&tab_id)
                                && matches!(tab.preview_source.as_ref(), Some(s) if s == &source_clone)
                            {
                                tab.result = Some(output.clone());
                            }
                        });
                        loaded_result.set(Some(output));
                        state.set(PanelState::Ready);
                    }
                    Err(err) => {
                        state.set(PanelState::Error(err.to_string()));
                    }
                }
            });
        } else {
            state.set(PanelState::Error("Connection closed".to_string()));
        }
    }

    let body = loaded_result();
    let _ = state().class();

    rsx! {
        div { class: "table-editor__panel table-editor__panel--structure",
            if matches!(state(), PanelState::Loading) {
                p { class: "table-editor__panel-hint", "Loading structure..." }
            } else if let PanelState::Error(err) = state() {
                p { class: "table-editor__panel-error", "Could not load structure: {err}" }
            } else if let Some(QueryOutput::Table(page)) = body.as_ref() {
                if page.columns.is_empty() {
                    p { class: "table-editor__panel-hint", "No columns reported for this table." }
                } else {
                    div { class: "table-editor__structure-grid",
                        div { class: "table-editor__structure-header",
                            for col in page.columns.iter() {
                                div { class: "table-editor__structure-cell table-editor__structure-cell--head", "{col}" }
                            }
                        }
                        for row in page.rows.iter() {
                            div { class: "table-editor__structure-row",
                                for cell in row.iter() {
                                    div { class: "table-editor__structure-cell", "{cell}" }
                                }
                            }
                        }
                    }
                }
            } else {
                p { class: "table-editor__panel-hint", "Select Structure to load columns." }
            }
        }
    }
}

/// The DDL sub-tab: renders the CREATE TABLE statement (or view
/// definition) as read-only monospace text. The default node kind
/// is `Table` since every previewed object today is a table; the
/// DDL loader treats the kind as a hint and falls back gracefully
/// for views / materialized views.
#[component]
pub fn DdlPanel(store: TabStore, source: TablePreviewSource, session_id: u64) -> Element {
    let mut state = use_signal(|| PanelState::Idle);
    let mut ddl_text = use_signal(String::new);

    let key = cache_key(&source, "ddl");
    let mut last_key = use_signal(String::new);
    if last_key() != key {
        last_key.set(key.clone());
        if let Some(connection) = panel_connection(store, session_id) {
            state.set(PanelState::Loading);
            let source_clone = source.clone();
            spawn(async move {
                match services::load_object_ddl(
                    connection,
                    source_clone.schema.clone(),
                    source_clone.table_name.clone(),
                    ExplorerNodeKind::Table,
                )
                .await
                {
                    Ok(Some(text)) => {
                        ddl_text.set(text);
                        state.set(PanelState::Ready);
                    }
                    Ok(None) => {
                        state.set(PanelState::Error(
                            "No DDL available for this object".to_string(),
                        ));
                    }
                    Err(err) => {
                        state.set(PanelState::Error(err.to_string()));
                    }
                }
            });
        } else {
            state.set(PanelState::Error("Connection closed".to_string()));
        }
    }

    rsx! {
        div { class: "table-editor__panel table-editor__panel--ddl",
            if matches!(state(), PanelState::Loading) {
                p { class: "table-editor__panel-hint", "Loading CREATE TABLE statement..." }
            } else if let PanelState::Error(err) = state() {
                p { class: "table-editor__panel-error", "DDL error: {err}" }
            } else {
                pre { class: "table-editor__ddl", "{ddl_text}" }
            }
        }
    }
}

/// The Indexes sub-tab. The current `services` surface does not
/// expose a table-index API for every driver, so this panel shows a
/// graceful empty state instead of inventing a fake one. When a
/// real index API is added in a follow-up, this component is the
/// one place that needs to change.
#[component]
pub fn IndexesPanel(source: TablePreviewSource) -> Element {
    rsx! {
        div { class: "table-editor__panel table-editor__panel--indexes",
            div { class: "table-editor__empty",
                p { class: "table-editor__panel-hint",
                    "Index information is not available for {source.table_name} on this connection."
                }
                p { class: "table-editor__panel-hint table-editor__panel-hint--muted",
                    "Use the DDL tab to inspect the CREATE TABLE statement, including index clauses."
                }
            }
        }
    }
}

/// The Relations sub-tab: lists foreign keys that reference this
/// table (or that this table declares). Backed by
/// `services::load_foreign_keys`, which is already used by the
/// ER-diagram component. We filter server-side results down to
/// rows where either end matches the active table.
#[component]
pub fn RelationsPanel(store: TabStore, source: TablePreviewSource, session_id: u64) -> Element {
    let mut state = use_signal(|| PanelState::Idle);
    let mut relations = use_signal(Vec::<TableForeignKey>::new);

    let key = cache_key(&source, "relations");
    let mut last_key = use_signal(String::new);
    if last_key() != key {
        last_key.set(key.clone());
        if let Some(connection) = panel_connection(store, session_id) {
            state.set(PanelState::Loading);
            let source_clone = source.clone();
            spawn(async move {
                match services::load_foreign_keys(connection).await {
                    Ok(all_keys) => {
                        let table_name = source_clone.table_name.as_str();
                        let schema_name = source_clone.schema.as_deref().unwrap_or("");
                        let filtered: Vec<TableForeignKey> = all_keys
                            .into_iter()
                            .filter(|fk| {
                                fk.from_table == table_name
                                    || fk.to_table == table_name
                                    || (fk.from_table.eq_ignore_ascii_case(table_name)
                                        && (fk.from_schema.is_empty()
                                            || fk.from_schema == schema_name))
                            })
                            .collect();
                        relations.set(filtered);
                        state.set(PanelState::Ready);
                    }
                    Err(err) => {
                        state.set(PanelState::Error(err.to_string()));
                    }
                }
            });
        } else {
            state.set(PanelState::Error("Connection closed".to_string()));
        }
    }

    rsx! {
        div { class: "table-editor__panel table-editor__panel--relations",
            if matches!(state(), PanelState::Loading) {
                p { class: "table-editor__panel-hint", "Loading foreign keys..." }
            } else if let PanelState::Error(err) = state() {
                p { class: "table-editor__panel-error", "Relations error: {err}" }
            } else if relations().is_empty() {
                div { class: "table-editor__empty",
                    p { class: "table-editor__panel-hint",
                        "No foreign keys reference or are declared by {source.table_name}."
                    }
                }
            } else {
                div { class: "table-editor__relations-grid",
                    div { class: "table-editor__relations-header",
                        span { class: "table-editor__relations-cell table-editor__relations-cell--head", "Constraint" }
                        span { class: "table-editor__relations-cell table-editor__relations-cell--head", "From" }
                        span { class: "table-editor__relations-cell table-editor__relations-cell--head", "To" }
                    }
                    for fk in relations().iter() {
                        div { class: "table-editor__relations-row",
                            span { class: "table-editor__relations-cell table-editor__relations-cell--name", "{fk.name}" }
                            span { class: "table-editor__relations-cell", "{fk.from_table}.{fk.from_column}" }
                            span { class: "table-editor__relations-cell", "{fk.to_table}.{fk.to_column}" }
                        }
                    }
                }
            }
        }
    }
}
