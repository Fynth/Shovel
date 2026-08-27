use crate::{
    app_state::{
        APP_AI_FEATURES_ENABLED,
        APP_SHOW_SQL_EDITOR,
        APP_SPLIT_MODE,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        context_menu::{ContextMenuItem, open_context_menu},
        open_connection_screen,
        pop_recently_closed_tab,
        push_recently_closed_tab,
    },
    screens::workspace::{
        actions::{
            new_query_tab,
            open_structure_tab,
            read_only_mode_block_status,
            read_only_mode_enabled,
            refresh_tab_result,
            replace_active_tab_sql,
            run_explain_for_tab,
            run_query_for_tab,
            set_active_tab_status,
            tab_session_or_error,
            toggle_execution_plan_for_tab,
        },
        tab_store::{TabMeta, TabResultState, TabStore, materialize_tab_state, restore_tab_state},
    },
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use models::{
    AcpPanelState,
    BatchRunState,
    ExecutionPlan,
    QueryHistoryItem,
    QueryOutput,
    QueryTabState,
    SqlFormatSettings,
    TablePreviewSource,
    WorkspaceSplitMode,
    WorkspaceTabKind,
};
use rfd::AsyncFileDialog;

use super::{
    ActionIcon,
    BatchResultsView,
    ExecutionPlanView,
    ExplorerConnectionSection,
    IconButton,
    ResultTable,
    SqlEditor,
    TableEditor,
    ensure_default_sql_agent_connected,
    send_sql_generation_request,
};

const EDITOR_MIN_HEIGHT: f64 = 96.0;
const EDITOR_MAX_HEIGHT: f64 = 720.0;
const EDITOR_DEFAULT_HEIGHT: f64 = 120.0;
const EDITOR_MIN_WIDTH: f64 = 280.0;
const EDITOR_MAX_WIDTH: f64 = 960.0;
const EDITOR_DEFAULT_WIDTH: f64 = 520.0;

#[derive(Clone, Copy, PartialEq)]
struct EditorResizeState {
    start_y: f64,
    start_height: f64,
}

/// Snapshot of the active tab's four per-aspect states, computed once
/// per render before the `rsx!` block so the macro never has to parse
/// tuple destructuring or field access inside its body.
struct ActiveTabContext {
    id: u64,
    session_id: u64,
    sql: String,
    title: String,
    page_size: u32,
    result: Option<QueryOutput>,
    tab_kind: WorkspaceTabKind,
    preview_source: Option<TablePreviewSource>,
    batch_results: Option<BatchRunState>,
    show_execution_plan: bool,
    execution_plan: Option<ExecutionPlan>,
}

#[derive(Clone, Copy, PartialEq)]
struct EditorWidthResizeState {
    start_x: f64,
    start_width: f64,
}

#[derive(Clone, Copy)]
enum ExportFormat {
    Csv,
    Json,
    Xlsx,
    Xml,
    Html,
    SqlDump,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Xlsx => "xlsx",
            Self::Xml => "xml",
            Self::Html => "html",
            Self::SqlDump => "sql",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Xlsx => "XLSX",
            Self::Xml => "XML",
            Self::Html => "HTML",
            Self::SqlDump => "SQL Dump",
        }
    }
}

/// Icon shown next to each export format in the editor overflow menu.
fn export_icon(format: ExportFormat) -> ActionIcon {
    match format {
        ExportFormat::Csv => ActionIcon::ExportCsv,
        ExportFormat::Json => ActionIcon::ExportJson,
        ExportFormat::Xlsx => ActionIcon::ExportXlsx,
        ExportFormat::Xml => ActionIcon::ExportXml,
        ExportFormat::Html => ActionIcon::ExportHtml,
        ExportFormat::SqlDump => ActionIcon::ExportSql,
    }
}

#[component]
pub fn TabsManager(
    store: TabStore,
    history: Signal<Vec<QueryHistoryItem>>,
    next_history_id: Signal<u64>,
    explorer_sections: Signal<Vec<ExplorerConnectionSection>>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
) -> Element {
    let mut editor_height = use_signal(|| EDITOR_DEFAULT_HEIGHT);
    let mut editor_resize = use_signal(|| None::<EditorResizeState>);
    let mut editor_width = use_signal(|| EDITOR_DEFAULT_WIDTH);
    let mut editor_width_resize = use_signal(|| None::<EditorWidthResizeState>);
    let mut show_generate_sql_window = use_signal(|| false);
    let mut generate_sql_prompt = use_signal(String::new);
    let generate_sql_input_revision = use_signal(|| 0_u64);
    let mut renaming_tab_id = use_signal(|| None::<u64>);
    let mut rename_value = use_signal(String::new);
    let active_tab = use_memo(move || {
        let id = store.active_tab_id();
        let meta = store.meta.read().get(&id).cloned();
        let editor = store.editor.read().get(&id).cloned();
        let result = store.result.read().get(&id).cloned();
        let pending = store.pending.read().get(&id).cloned();
        meta.map(|m| (m, editor, result, pending))
    });

    let session_labels = {
        let app_state = APP_STATE.read();
        app_state
            .sessions
            .iter()
            .map(|session| (session.id, session.name.clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let active_actionable_source = active_tab.read().as_ref().and_then(|(m, e, r, p)| {
        let _ = (m, e, p);
        r.as_ref().and_then(actionable_table_source)
    });
    let generate_sql_busy = acp_panel_state().busy;
    let generate_sql_prompt_empty = generate_sql_prompt().trim().is_empty();
    let read_only_mode = read_only_mode_enabled();
    let tab_list: Vec<(u64, TabMeta)> = store
        .meta
        .read()
        .iter()
        .map(|(id, meta)| (*id, meta.clone()))
        .collect();
    let active_ctx = active_tab.read().as_ref().map(|(m, e, r, p)| {
        let _ = p;
        ActiveTabContext {
            id: m.id,
            session_id: m.session_id,
            sql: e.as_ref().map(|e| e.sql.clone()).unwrap_or_default(),
            title: m.title.clone(),
            page_size: r.as_ref().map(|r| r.page_size).unwrap_or(0),
            result: r.as_ref().and_then(|r| r.result.clone()),
            tab_kind: m.tab_kind,
            preview_source: r.as_ref().and_then(|r| r.preview_source.clone()),
            batch_results: r.as_ref().and_then(|r| r.batch_results.clone()),
            show_execution_plan: r.as_ref().map(|r| r.show_execution_plan).unwrap_or(false),
            execution_plan: r.as_ref().and_then(|r| r.execution_plan.clone()),
        }
    });
    let active_tab_id_value = active_ctx.as_ref().map(|a| a.id).unwrap_or(0);
    let active_session_id = active_ctx.as_ref().map(|a| a.session_id).unwrap_or(0);
    let active_sql = active_ctx
        .as_ref()
        .map(|a| a.sql.clone())
        .unwrap_or_default();
    let active_sql_run = active_sql.clone();
    let active_sql_explain = active_sql.clone();
    let active_title = active_ctx
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_default();
    let active_page_size = active_ctx.as_ref().map(|a| a.page_size).unwrap_or(0);
    let active_result = active_ctx.as_ref().and_then(|a| a.result.clone());
    let active_tab_kind = active_ctx
        .as_ref()
        .map(|a| a.tab_kind)
        .unwrap_or(WorkspaceTabKind::Query);
    let active_preview_source = active_ctx.as_ref().and_then(|a| a.preview_source.clone());
    let active_batch_results = active_ctx.as_ref().and_then(|a| a.batch_results.clone());
    let active_show_execution_plan = active_ctx
        .as_ref()
        .map(|a| a.show_execution_plan)
        .unwrap_or(false);
    let active_execution_plan = active_ctx.as_ref().and_then(|a| a.execution_plan.clone());

    rsx! {
        div {
            class: {
                let mut class_name = if APP_SHOW_SQL_EDITOR() {
                    "editor-shell".to_string()
                } else {
                    "editor-shell editor-shell--editor-hidden".to_string()
                };

                match APP_SPLIT_MODE() {
                    WorkspaceSplitMode::Horizontal => {
                        class_name.push_str(" editor-shell--split-horizontal");
                    }
                    WorkspaceSplitMode::Vertical => {
                        class_name.push_str(" editor-shell--split-vertical");
                    }
                    WorkspaceSplitMode::Off => {}
                }

                if editor_resize().is_some() {
                    class_name.push_str(" editor-shell--resizing");
                }
                if editor_width_resize().is_some() {
                    class_name.push_str(" editor-shell--resizing-x");
                }

                class_name
            },
            style: {
                let mut style = String::new();
                if APP_SHOW_SQL_EDITOR() {
                    match APP_SPLIT_MODE() {
                        WorkspaceSplitMode::Horizontal => {
                            style.push_str(&format!(
                                "--editor-pane-width: {:.0}px;",
                                editor_width()
                            ));
                        }
                        _ => {
                            style.push_str(&format!(
                                "--editor-pane-height: {:.0}px;",
                                editor_height()
                            ));
                        }
                    }
                }
                style
            },
            onmousemove: move |event| {
                if let Some(resize) = editor_resize() {
                    if event.held_buttons().is_empty() {
                        editor_resize.set(None);
                        return;
                    }

                    let delta_y = event.client_coordinates().y - resize.start_y;
                    let next_height = (resize.start_height + delta_y)
                        .clamp(EDITOR_MIN_HEIGHT, EDITOR_MAX_HEIGHT);
                    editor_height.set(next_height);
                    return;
                }

                if let Some(resize) = editor_width_resize() {
                    if event.held_buttons().is_empty() {
                        editor_width_resize.set(None);
                        return;
                    }

                    let delta_x = event.client_coordinates().x - resize.start_x;
                    let next_width = (resize.start_width + delta_x)
                        .clamp(EDITOR_MIN_WIDTH, EDITOR_MAX_WIDTH);
                    editor_width.set(next_width);
                }
            },
            onmouseup: move |_| {
                if editor_resize().is_some() {
                    editor_resize.set(None);
                }
                if editor_width_resize().is_some() {
                    editor_width_resize.set(None);
                }
            },
            onmouseleave: move |_| {
                if editor_resize().is_some() {
                    editor_resize.set(None);
                }
                if editor_width_resize().is_some() {
                    editor_width_resize.set(None);
                }
            },
            div {
                class: "tabbar",
                for (tab_id, tab) in tab_list.iter().cloned() {
                    div {
                        class: {
                            let mut class_name = if tab_id == store.active_tab_id() {
                                "tabbar__tab tabbar__tab--active".to_string()
                            } else {
                                "tabbar__tab".to_string()
                            };
                            if tab.pinned {
                                class_name.push_str(" tabbar__tab--pinned");
                            }
                            class_name
                        },
                        onclick: {
                            let session_id = tab.session_id;
                            move |_| {
                                store.active_tab_id.set(tab_id);
                                crate::app_state::activate_session(session_id);
                            }
                        },
                        onauxclick: {
                            move |event| {
                                if event.trigger_button() != Some(MouseButton::Auxiliary) {
                                    return;
                                }
                                close_tab_for_middle_click(store, tab_id);
                            }
                        },
                        oncontextmenu: {
                            move |event| {
                                event.prevent_default();
                                let coords = event.client_coordinates();
                                let items = build_tab_context_menu(tab_id, store);
                                open_context_menu(coords.x, coords.y, items);
                            }
                        },
                        div {
                            class: "tabbar__copy",
                            if renaming_tab_id() == Some(tab_id) {
                                input {
                                    class: "tabbar__rename-input",
                                    value: "{rename_value}",
                                    oninput: move |event| rename_value.set(event.value()),
                                    onkeydown: move |event| {
                                        if event.key() == Key::Enter {
                                            let new_title = rename_value().trim().to_string();
                                            if !new_title.is_empty()
                                                && let Some(tab_id) = renaming_tab_id()
                                            {
                                                store.meta.with_mut(|m| {
                                                    if let Some(tab) = m.get_mut(&tab_id) {
                                                        tab.title = new_title;
                                                    }
                                                });
                                            }
                                            renaming_tab_id.set(None);
                                        } else if event.key() == Key::Escape {
                                            renaming_tab_id.set(None);
                                        }
                                    },
                                    onblur: move |_| {
                                        let new_title = rename_value().trim().to_string();
                                        if !new_title.is_empty()
                                            && let Some(tab_id) = renaming_tab_id()
                                        {
                                            store.meta.with_mut(|m| {
                                                if let Some(tab) = m.get_mut(&tab_id) {
                                                    tab.title = new_title;
                                                }
                                            });
                                        }
                                        renaming_tab_id.set(None);
                                    },
                                }
                            } else {
                                span {
                                    class: "tabbar__label",
                                    ondoubleclick: {
                                        move |_| {
                                            rename_value.set(tab.title.clone());
                                            renaming_tab_id.set(Some(tab_id));
                                        }
                                    },
                                    "{tab.title}"
                                }
                            }
                            if let Some(session_name) = session_labels.get(&tab.session_id) {
                                span { class: "tabbar__context", {session_name.to_string()} }
                            }
                        }
                        if tab.pinned {
                            span {
                                class: "tabbar__pin",
                                "aria-label": "Pinned tab",
                                title: "Pinned",
                                "📌"
                            }
                        }
                        button {
                            class: "tabbar__close",
                            onclick: {
                                move |event| {
                                    event.stop_propagation();
                                    close_tab_for_middle_click(store, tab_id);
                                }
                            },
                            "x"
                        }
                    }
                }
                button {
                    class: "tabbar__add",
                    onclick: move |_| {
                        let Some(session_id) = APP_STATE.read().active_session_id else {
                            open_connection_screen();
                            return;
                        };

                        let new_id = store.next_tab_id();
                        store.next_tab_id += 1;
                        let (meta, editor, result, pending) = new_query_tab(
                            new_id,
                            session_id,
                            format!("Query {new_id}"),
                            String::new(),
                        );
                        store.meta.with_mut(|m| { m.insert(new_id, meta); });
                        store.editor.with_mut(|m| { m.insert(new_id, editor); });
                        store.result.with_mut(|m| { m.insert(new_id, result); });
                        store.pending.with_mut(|m| { m.insert(new_id, pending); });
                        store.active_tab_id.set(new_id);
                    },
                    "+ Tab"
                }
            }

            if active_ctx.is_some() {
                if APP_SHOW_SQL_EDITOR() {
                    div {
                        class: "editor",
                        SqlEditor {
                            sql: active_sql.clone(),
                            active_tab_id: active_tab_id_value,
                            active_session_id,
                            store,
                            explorer_sections,
                        }
                    }
                    if matches!(APP_SPLIT_MODE(), WorkspaceSplitMode::Horizontal) {
                        div {
                            class: if editor_width_resize().is_some() {
                                "editor-shell__col-resize editor-shell__col-resize--active"
                            } else {
                                "editor-shell__col-resize"
                            },
                            onmousedown: move |event| {
                                event.prevent_default();
                                editor_width_resize.set(Some(EditorWidthResizeState {
                                    start_x: event.client_coordinates().x,
                                    start_width: editor_width(),
                                }));
                            }
                        }
                    } else {
                        div {
                            class: if editor_resize().is_some() {
                                "editor-shell__resize-handle editor-shell__resize-handle--active"
                            } else {
                                "editor-shell__resize-handle"
                            },
                            onmousedown: move |event| {
                                event.prevent_default();
                                editor_resize.set(Some(EditorResizeState {
                                    start_y: event.client_coordinates().y,
                                    start_height: editor_height(),
                                }));
                            }
                        }
                    }
                }
                if matches!(APP_SPLIT_MODE(), WorkspaceSplitMode::Horizontal) {
                    div {
                        class: "editor-shell__bottom",
                        div {
                            class: "editor__actions",
                    IconButton {
                        icon: ActionIcon::Run,
                        label: "Run SQL".to_string(),
                        primary: true,
                        onclick: move |_| {
                            let current_id = active_tab_id_value;
                            let sql = active_sql_run.trim().to_string();
                            let tab_title = active_title.clone();
                            let page_size = active_page_size;
                            let connection_name = APP_STATE
                                .read()
                                .session(active_session_id)
                                .map(|session| session.name.clone())
                                .unwrap_or_else(|| "Detached session".to_string());

                            if sql.is_empty() {
                                set_active_tab_status(
                                    store,
                                    current_id,
                                    "Query is empty".to_string(),
                                );
                                return;
                            }

                            let Some(session_id) =
                                tab_session_or_error(store, current_id, active_session_id)
                            else {
                                return;
                            };

                            run_query_for_tab(
                                store,
                                current_id,
                                session_id,
                                sql,
                                0,
                                page_size,
                                Some((history, next_history_id, tab_title, connection_name)),
                            );
                        },
                    }
                    IconButton {
                        icon: ActionIcon::Format,
                        label: "Format SQL".to_string(),
                        onclick: {
                            let format_settings = APP_SQL_FORMAT_SETTINGS();
                            move |_| format_active_sql(store, active_tab_id_value, format_settings.clone())
                        },
                    }
                    IconButton {
                        icon: ActionIcon::Explain,
                        label: "Explain Plan".to_string(),
                        onclick: {
                            move |_| {
                                let current_id = active_tab_id_value;
                                let sql = active_sql_explain.trim().to_string();
                                if toggle_execution_plan_for_tab(store, current_id, &sql) {
                                    return;
                                }
                                if sql.is_empty() {
                                    set_active_tab_status(
                                        store,
                                        current_id,
                                        "Enter a query to explain".to_string(),
                                    );
                                    return;
                                }
                                if !services::is_read_only_sql(&sql) {
                                    set_active_tab_status(
                                        store,
                                        current_id,
                                        "Explain Plan is available only for read-only SQL.".to_string(),
                                    );
                                    return;
                                }
                                let Some(session_id) =
                                    tab_session_or_error(store, current_id, active_session_id)
                                else {
                                    return;
                                };
                                run_explain_for_tab(store, current_id, session_id, sql);
                            }
                        },
                    }
                    IconButton {
                        icon: ActionIcon::More,
                        label: "More actions".to_string(),
                        onclick: {
                            let active_actionable_source = active_actionable_source.clone();
                            let active_result_for_menu = active_result.clone();
                            let mut show_generate_sql_window = show_generate_sql_window;
                            let mut generate_sql_prompt = generate_sql_prompt;
                            let mut generate_sql_input_revision = generate_sql_input_revision;
                            move |event: MouseEvent| {
                                let coords = event.client_coordinates();
                                let has_tabular = has_tabular_result(&active_result_for_menu);
                                let mut generate_item = ContextMenuItem::new(
                                    if show_generate_sql_window() {
                                        "Close Generate SQL"
                                    } else {
                                        "Generate SQL"
                                    },
                                    move || {
                                        if !APP_AI_FEATURES_ENABLED() {
                                            set_active_tab_status(
                                                store,
                                                active_tab_id_value,
                                                "Enable AI features in Settings to use Generate SQL."
                                                    .to_string(),
                                            );
                                            return;
                                        }
                                        if show_generate_sql_window() {
                                            show_generate_sql_window.set(false);
                                        } else {
                                            generate_sql_prompt.set(String::new());
                                            generate_sql_input_revision += 1;
                                            show_generate_sql_window.set(true);
                                        }
                                    },
                                )
                                .with_icon(ActionIcon::Generate);
                                if generate_sql_busy {
                                    generate_item = generate_item.disabled();
                                }
                                let mut items: Vec<ContextMenuItem> = vec![generate_item];
                                let mut structure_item = ContextMenuItem::new(
                                    "Open structure",
                                    move || {
                                        open_structure_for_active_preview(
                                            store,
                                            active_tab_id_value,
                                        )
                                    },
                                )
                                .with_icon(ActionIcon::Structure);
                                if active_actionable_source.is_none() {
                                    structure_item = structure_item.disabled();
                                }
                                items.push(structure_item);
                                for format in [
                                    ExportFormat::Csv,
                                    ExportFormat::Json,
                                    ExportFormat::Xlsx,
                                    ExportFormat::Xml,
                                    ExportFormat::Html,
                                    ExportFormat::SqlDump,
                                ] {
                                    let mut item = ContextMenuItem::new(
                                        format!("Export {}", format.label()),
                                        move || {
                                            export_active_page(
                                                store,
                                                active_tab_id_value,
                                                format,
                                            )
                                        },
                                    )
                                    .with_icon(export_icon(format))
                                    .separator();
                                    if !has_tabular {
                                        item = item.disabled();
                                    }
                                    items.push(item);
                                }
                                let mut import_item = ContextMenuItem::new(
                                    if read_only_mode {
                                        "Import CSV (blocked by read-only mode)"
                                    } else {
                                        "Import CSV"
                                    },
                                    move || {
                                        import_csv_into_active_table(
                                            store,
                                            active_tab_id_value,
                                        )
                                    },
                                )
                                .with_icon(ActionIcon::ImportCsv)
                                .separator();
                                if active_actionable_source.is_none() || read_only_mode {
                                    import_item = import_item.disabled();
                                }
                                items.push(import_item);
                                open_context_menu(coords.x, coords.y, items);
                            }
                        },
                    }
                }
                div {
                    class: "workspace__results",
                    if show_generate_sql_window() {
                        div { class: "editor__context-window editor__context-window--fill",
                            div { class: "editor__format-settings editor__generate-sql-window editor__generate-sql-window--fill",
                                div {
                                    class: "editor__format-settings-header",
                                    div { class: "editor__format-settings-copy",
                                        h3 { class: "editor__format-settings-title", "Generate SQL" }
                                        p {
                                            class: "editor__format-settings-hint",
                                            "Describe the query you want. The configured AI agent will generate SQL and insert it into the active editor."
                                        }
                                    }
                                    button {
                                        class: "button button--ghost button--small",
                                        onclick: move |_| show_generate_sql_window.set(false),
                                        "Close"
                                    }
                                }
                                div { class: "field",
                                    span { class: "field__label", "Query description" }
                                    textarea {
                                        key: "generate-sql-{generate_sql_input_revision}",
                                        class: "input editor__generate-sql-input",
                                        placeholder: "For example: show failed payments from the last 7 days grouped by provider",
                                        initial_value: "{generate_sql_prompt}",
                                        oninput: move |event| generate_sql_prompt.set(event.value()),
                                        onkeydown: move |event| {
                                            if event.key() != Key::Enter
                                                || event.modifiers().contains(Modifiers::SHIFT)
                                                || generate_sql_busy
                                                || generate_sql_prompt_empty
                                            {
                                                return;
                                            }
                                            event.prevent_default();

                                            submit_generated_sql_request(
                                                store,
                                                active_tab_id_value,
                                                acp_panel_state,
                                                chat_revision,
                                                allow_agent_db_read(),
                                                generate_sql_prompt,
                                                show_generate_sql_window,
                                            );
                                        },
                                    }
                                }
                                div { class: "editor__generate-sql-actions",
                                    button {
                                        class: "button button--ghost button--small",
                                        disabled: generate_sql_busy,
                                        onclick: move |_| show_generate_sql_window.set(false),
                                        "Cancel"
                                    }
                                    button {
                                        class: "button button--primary button--small",
                                        disabled: generate_sql_busy || generate_sql_prompt_empty,
                                        onclick: {
                                            move |_| {
                                                submit_generated_sql_request(
                                                    store,
                                                    active_tab_id_value,
                                                    acp_panel_state,
                                                    chat_revision,
                                                    allow_agent_db_read(),
                                                    generate_sql_prompt,
                                                    show_generate_sql_window,
                                                );
                                            }
                                        },
                                        if generate_sql_busy { "Generating..." } else { "Generate SQL" }
                                    }
                                }
                            }
                        }
                    } else if active_batch_results.is_some() {
                        BatchResultsView {
                            store,
                        }
                    } else if active_show_execution_plan {
                        if let Some(plan) = active_execution_plan.clone() {
                            ExecutionPlanView {
                                plan,
                                store,
                            }
                        } else {
                            ResultTable {
                                result: active_result.clone(),
                                store,
                            }
                        }
                    } else if active_tab_kind == WorkspaceTabKind::TablePreview
                        && active_preview_source.is_some()
                    {
                        TableEditor {
                            store,
                        }
                    } else {
                        ResultTable {
                            result: active_result.clone(),
                            store,
                        }
                    }
                }
                }
                }
            } else {
                div {
                    class: "workspace__empty",
                    p { class: "empty-state", "No active tab for the selected connection." }
                }
            }
        }
    }
}

fn export_active_page(store: TabStore, current_id: u64, format: ExportFormat) {
    let Some(current_tab) = materialize_tab_state(store, current_id) else {
        return;
    };
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        set_active_tab_status(
            store,
            current_id,
            "Nothing to export in the current tab".to_string(),
        );
        return;
    };

    let file_name = default_export_file_name(&current_tab, format);
    set_active_tab_status(
        store,
        current_id,
        format!("Select a destination for the {} export", format.label()),
    );

    spawn(async move {
        let Some(file) = AsyncFileDialog::new()
            .set_file_name(&file_name)
            .add_filter(format.label(), &[format.extension()])
            .save_file()
            .await
        else {
            set_active_tab_status(store, current_id, "Export cancelled".to_string());
            return;
        };

        let path = file.path().to_path_buf();
        set_active_tab_status(
            store,
            current_id,
            format!(
                "Exporting {} rows to {}...",
                page.rows.len(),
                format.label()
            ),
        );

        let export_result = match format {
            ExportFormat::Csv => services::export_query_page_csv(page, path.clone()).await,
            ExportFormat::Json => services::export_query_page_json(page, path.clone()).await,
            ExportFormat::Xlsx => services::export_query_page_xlsx(page, path.clone()).await,
            ExportFormat::Xml => services::export_query_page_xml(page, path.clone()).await,
            ExportFormat::Html => services::export_query_page_html(page, path.clone()).await,
            ExportFormat::SqlDump => {
                let table_name = current_tab
                    .preview_source
                    .as_ref()
                    .map(|s| s.table_name.clone())
                    .unwrap_or_else(|| "exported_table".to_string());
                services::export_query_page_sql_dump(page, path.clone(), table_name).await
            }
        };

        match export_result {
            Ok(rows) => {
                let destination = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                set_active_tab_status(
                    store,
                    current_id,
                    format!("Exported {rows} row(s) to {destination}"),
                );
            }
            Err(err) => set_active_tab_status(
                store,
                current_id,
                format!("{} export error: {err}", format.label()),
            ),
        }
    });
}

fn import_csv_into_active_table(store: TabStore, current_id: u64) {
    let Some(current_tab) = materialize_tab_state(store, current_id) else {
        return;
    };
    if read_only_mode_enabled() {
        set_active_tab_status(store, current_id, read_only_mode_block_status("CSV import"));
        return;
    }

    let result_state = TabResultState {
        result: current_tab.result.clone(),
        status: current_tab.status.clone(),
        current_offset: current_tab.current_offset,
        page_size: current_tab.page_size,
        last_run_sql: current_tab.last_run_sql.clone(),
        preview_source: current_tab.preview_source.clone(),
        filter: current_tab.filter.clone(),
        sort: current_tab.sort.clone(),
        is_loading_more: current_tab.is_loading_more,
        execution_plan: current_tab.execution_plan.clone(),
        show_execution_plan: current_tab.show_execution_plan,
        batch_results: current_tab.batch_results.clone(),
        batch_outputs: current_tab.batch_outputs.clone(),
        last_duration_ms: current_tab.last_duration_ms,
        optimizer_result: current_tab.optimizer_result.clone(),
        optimizer_raw_response: current_tab.optimizer_raw_response.clone(),
    };
    let Some(source) = actionable_table_source(&result_state) else {
        set_active_tab_status(
            store,
            current_id,
            "Import CSV is available for previewed tables and simple single-table SELECT queries"
                .to_string(),
        );
        return;
    };

    let Some(session_id) = tab_session_or_error(store, current_id, current_tab.session_id) else {
        return;
    };

    set_active_tab_status(
        store,
        current_id,
        format!("Select a CSV file to import into {}", source.table_name),
    );

    spawn(async move {
        let Some(file) = AsyncFileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file()
            .await
        else {
            set_active_tab_status(store, current_id, "CSV import cancelled".to_string());
            return;
        };

        let path = file.path().to_path_buf();
        set_active_tab_status(
            store,
            current_id,
            format!("Importing {}...", path.to_string_lossy()),
        );

        match services::import_csv_into_table(session_id, source.clone(), path).await {
            Ok(rows) => {
                set_active_tab_status(
                    store,
                    current_id,
                    format!("Imported {rows} row(s) into {}", source.table_name),
                );
                if let Some(updated_tab) = materialize_tab_state(store, current_id) {
                    refresh_tab_result(store, updated_tab, Some(source));
                }
            }
            Err(err) =>
                set_active_tab_status(store, current_id, format!("CSV import error: {err}")),
        }
    });
}

fn default_export_file_name(tab: &QueryTabState, format: ExportFormat) -> String {
    let base = tab
        .preview_source
        .as_ref()
        .map(|source| source.table_name.clone())
        .unwrap_or_else(|| tab.title.clone());
    let sanitized = sanitize_file_name(&base);
    format!("{sanitized}.{}", format.extension())
}

fn sanitize_file_name(value: &str) -> String {
    let candidate = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();

    if candidate.is_empty() {
        "query_result".to_string()
    } else {
        candidate
    }
}

fn has_tabular_result(result: &Option<QueryOutput>) -> bool {
    matches!(result.as_ref(), Some(QueryOutput::Table(_)))
}

fn format_active_sql(store: TabStore, current_id: u64, format_settings: SqlFormatSettings) {
    let Some(current_tab) = materialize_tab_state(store, current_id) else {
        return;
    };
    let sql = current_tab.sql.trim();
    if sql.is_empty() {
        set_active_tab_status(
            store,
            current_id,
            "Nothing to format in the current tab".to_string(),
        );
        return;
    }

    let session_id = current_tab.session_id;
    let sql = sql.to_string();
    let fallback_sql = sql.clone();
    spawn(async move {
        let formatted = tokio::task::spawn_blocking(move || {
            services::format_sql_for_session(session_id, &sql, &format_settings).unwrap_or(sql)
        })
        .await
        .unwrap_or(fallback_sql);
        replace_active_tab_sql(store, current_id, formatted, "SQL formatted".to_string());
    });
}

#[allow(clippy::too_many_arguments)]
fn submit_generated_sql_request(
    store: TabStore,
    active_tab_id: u64,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: bool,
    prompt_draft: Signal<String>,
    mut show_generate_sql_window: Signal<bool>,
) {
    let request = prompt_draft().trim().to_string();
    if request.is_empty() {
        set_active_tab_status(
            store,
            active_tab_id,
            "Enter a description before generating SQL.".to_string(),
        );
        return;
    }

    let connection_label = APP_STATE
        .read()
        .session(
            store
                .meta
                .read()
                .get(&active_tab_id)
                .map(|m| m.session_id)
                .unwrap_or(0),
        )
        .map(|session| session.name.clone())
        .unwrap_or_else(|| "Detached session".to_string());

    set_active_tab_status(
        store,
        active_tab_id,
        "Generating SQL with the configured AI agent...".to_string(),
    );

    spawn(async move {
        let settings = crate::app_state::APP_UI_SETTINGS();
        let deepseek = settings.deepseek;
        let ollama = settings.ollama;
        if let Err(err) =
            ensure_default_sql_agent_connected(acp_panel_state, chat_revision, deepseek, ollama)
                .await
        {
            set_active_tab_status(store, active_tab_id, format!("Generate SQL error: {err}"));
            return;
        }

        send_sql_generation_request(
            acp_panel_state,
            store,
            active_tab_id,
            connection_label,
            chat_revision,
            allow_agent_db_read,
            request,
            Some(prompt_draft),
            false,
        );
        show_generate_sql_window.set(false);
    });
}

fn open_structure_for_active_preview(store: TabStore, current_id: u64) {
    let Some(current_tab) = materialize_tab_state(store, current_id) else {
        return;
    };
    let result_state = TabResultState {
        result: current_tab.result.clone(),
        status: current_tab.status.clone(),
        current_offset: current_tab.current_offset,
        page_size: current_tab.page_size,
        last_run_sql: current_tab.last_run_sql.clone(),
        preview_source: current_tab.preview_source.clone(),
        filter: current_tab.filter.clone(),
        sort: current_tab.sort.clone(),
        is_loading_more: current_tab.is_loading_more,
        execution_plan: current_tab.execution_plan.clone(),
        show_execution_plan: current_tab.show_execution_plan,
        batch_results: current_tab.batch_results.clone(),
        batch_outputs: current_tab.batch_outputs.clone(),
        last_duration_ms: current_tab.last_duration_ms,
        optimizer_result: current_tab.optimizer_result.clone(),
        optimizer_raw_response: current_tab.optimizer_raw_response.clone(),
    };
    let Some(source) = actionable_table_source(&result_state) else {
        set_active_tab_status(
            store,
            current_id,
            "Structure view is available for previewed tables and simple single-table SELECT queries"
                .to_string(),
        );
        return;
    };

    if tab_session_or_error(store, current_id, current_tab.session_id).is_none() {
        return;
    }

    open_structure_tab(store, current_tab.session_id, source);
}

fn actionable_table_source(result: &TabResultState) -> Option<TablePreviewSource> {
    result.preview_source.clone().or_else(|| {
        result
            .last_run_sql
            .as_deref()
            .and_then(services::preview_source_for_sql)
    })
}

/// Re-assign `active_tab_id` to the first remaining tab when the
/// current active tab is no longer in the list. Used by every
/// "close many" helper so the editor does not stay focused on a
/// vanished tab.
fn reassign_active_if_missing(store: TabStore) {
    let mut active_tab_id = store.active_tab_id;
    if store.meta.read().contains_key(&active_tab_id()) {
        return;
    }
    if let Some((next_id, next_meta)) = store.meta.read().iter().next() {
        let next_id = *next_id;
        let session_id = next_meta.session_id;
        active_tab_id.set(next_id);
        crate::app_state::activate_session(session_id);
    }
}

/// Close a single tab by id. Pinned tabs are protected — this
/// helper is also the entry point for the X button and the
/// middle-click handler, both of which explicitly close the tab
/// the user targeted (so pinning never silently blocks an
/// intentional close). The closed tab is pushed onto the
/// "recently closed" stack so "Reopen Closed Tab" can restore it.
fn close_tab_for_middle_click(mut store: TabStore, tab_id: u64) {
    let mut active_tab_id = store.active_tab_id;
    if store.meta.read().len() <= 1 {
        return;
    }
    let Some(closed) = materialize_tab_state(store, tab_id) else {
        return;
    };

    push_recently_closed_tab(closed);
    store.meta.with_mut(|m| {
        m.remove(&tab_id);
    });
    store.editor.with_mut(|m| {
        m.remove(&tab_id);
    });
    store.result.with_mut(|m| {
        m.remove(&tab_id);
    });
    store.pending.with_mut(|m| {
        m.remove(&tab_id);
    });

    if active_tab_id() == tab_id {
        let next_tab = {
            let meta = store.meta.read();
            meta.iter()
                .find(|(_, t)| !t.pinned)
                .or_else(|| meta.iter().next())
                .map(|(id, t)| (*id, t.session_id))
        };
        if let Some((next_id, session_id)) = next_tab {
            active_tab_id.set(next_id);
            crate::app_state::activate_session(session_id);
        }
    }
}

/// Build the right-click menu for a tab. Mirrors DBeaver/DataGrip
/// (Close, Close Others, Close to Right, Close All, Pin/Unpin,
/// Duplicate, Reopen Closed Tab). Pinned tabs are protected from
/// "Close Others" / "Close to Right" / "Close All" — those items
/// only affect non-pinned tabs (and "Close Others" keeps all
/// pinned tabs).
fn build_tab_context_menu(tab_id: u64, store: TabStore) -> Vec<ContextMenuItem> {
    let mut items: Vec<ContextMenuItem> = Vec::new();

    let Some(tab) = store.meta.read().get(&tab_id).cloned() else {
        return items;
    };
    let total = store.meta.read().len();
    let tab_index = store.meta.read().iter().position(|(id, _)| *id == tab_id);
    let non_pinned_total = store.meta.read().iter().filter(|(_, t)| !t.pinned).count();
    let pinned_total = total - non_pinned_total;
    let tabs_to_right = tab_index
        .map(|idx| {
            store
                .meta
                .read()
                .iter()
                .skip(idx + 1)
                .filter(|(_, t)| !t.pinned)
                .count()
        })
        .unwrap_or(0);

    // 1. Close
    {
        let mut item = ContextMenuItem::new("Close", move || {
            close_tab_for_middle_click(store, tab_id);
        })
        .with_icon(ActionIcon::Close);
        item.disabled = total <= 1;
        items.push(item);
    }

    // 2. Close Others — keep this tab + every other pinned tab.
    {
        let closeable_others =
            non_pinned_total.saturating_sub(1) + (if tab.pinned { 0 } else { pinned_total });
        let mut item = ContextMenuItem::new("Close Others", move || {
            close_other_tabs(store, tab_id);
        })
        .with_icon(ActionIcon::Close);
        item.disabled = closeable_others == 0;
        items.push(item);
    }

    // 3. Close to Right — non-pinned tabs to the right of this one.
    {
        let mut item = ContextMenuItem::new("Close Tabs to the Right", move || {
            close_tabs_to_the_right(store, tab_id);
        })
        .with_icon(ActionIcon::Close);
        item.disabled = tabs_to_right == 0;
        items.push(item);
    }

    // 4. Close All — close every non-pinned tab.
    {
        let mut item = ContextMenuItem::new("Close All", move || {
            close_all_non_pinned_tabs(store, tab_id);
        })
        .with_icon(ActionIcon::Close)
        .separator();
        item.disabled = non_pinned_total == 0;
        items.push(item);
    }

    // 5. Pin / Unpin
    {
        if tab.pinned {
            items.push(ContextMenuItem::new("Unpin", move || {
                set_tab_pinned(store, tab_id, false);
            }));
        } else {
            items.push(ContextMenuItem::new("Pin", move || {
                set_tab_pinned(store, tab_id, true);
            }));
        }
    }

    // 6. Duplicate — clone the tab into a new tab inserted after
    //    this one. The new tab gets a fresh id; title is suffixed
    //    with " copy". Result, SQL, and preview_source are copied
    //    so the duplicate is ready to run.
    {
        let source = materialize_tab_state(store, tab_id);
        if let Some(source) = source {
            items.push(
                ContextMenuItem::new("Duplicate", move || {
                    duplicate_tab(store, source.clone());
                })
                .with_icon(ActionIcon::Duplicate)
                .separator(),
            );
        }
    }

    // 7. Reopen Closed Tab
    {
        let mut item = ContextMenuItem::new("Reopen Closed Tab", move || {
            reopen_last_closed_tab(store);
        });
        item.disabled = crate::app_state::APP_RECENTLY_CLOSED_TABS.peek().is_empty();
        items.push(item);
    }

    items
}

fn set_tab_pinned(mut store: TabStore, tab_id: u64, pinned: bool) {
    store.meta.with_mut(|m| {
        if let Some(tab) = m.get_mut(&tab_id) {
            tab.pinned = pinned;
        }
    });
}

fn duplicate_tab(store: TabStore, source: QueryTabState) {
    let mut active_tab_id = store.active_tab_id;
    let mut next_tab_id = store.next_tab_id;
    let source_id = source.id;
    let new_id = next_tab_id();
    next_tab_id.set(new_id + 1);
    let mut clone = source;
    clone.id = new_id;
    clone.pinned = false;
    let trimmed_lower = clone.title.trim_end().to_lowercase();
    if !trimmed_lower.ends_with(" copy") {
        clone.title.push_str(" copy");
    }
    restore_tab_state(store, clone);
    active_tab_id.set(new_id);
    let _ = source_id;
}

fn reopen_last_closed_tab(store: TabStore) {
    let mut active_tab_id = store.active_tab_id;
    let mut next_tab_id = store.next_tab_id;
    let Some(restored) = pop_recently_closed_tab(&mut next_tab_id) else {
        return;
    };
    let new_id = restored.id;
    let session_id = restored.session_id;
    restore_tab_state(store, restored);
    active_tab_id.set(new_id);
    crate::app_state::activate_session(session_id);
}

fn close_other_tabs(mut store: TabStore, keep_tab_id: u64) {
    let to_close: Vec<QueryTabState> = store
        .meta
        .read()
        .iter()
        .filter(|(id, t)| **id != keep_tab_id && !t.pinned)
        .filter_map(|(id, _)| materialize_tab_state(store, *id))
        .collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    store.meta.with_mut(|m| {
        m.retain(|id, t| *id == keep_tab_id || t.pinned);
    });
    store.editor.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.result.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.pending.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    reassign_active_if_missing(store);
}

fn close_tabs_to_the_right(mut store: TabStore, anchor_tab_id: u64) {
    let snapshot: Vec<u64> = store.meta.read().keys().copied().collect();
    let Some(anchor_idx) = snapshot.iter().position(|id| *id == anchor_tab_id) else {
        return;
    };
    let to_close: Vec<QueryTabState> = snapshot
        .iter()
        .skip(anchor_idx + 1)
        .filter(|id| {
            store
                .meta
                .read()
                .get(id)
                .map(|t| !t.pinned)
                .unwrap_or(false)
        })
        .filter_map(|id| materialize_tab_state(store, *id))
        .collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    let keep: std::collections::HashSet<u64> = snapshot
        .iter()
        .take(anchor_idx + 1)
        .copied()
        .chain(
            store
                .meta
                .read()
                .iter()
                .filter(|(_, t)| t.pinned)
                .map(|(id, _)| *id),
        )
        .collect();
    store.meta.with_mut(|m| {
        m.retain(|id, _| keep.contains(id));
    });
    store.editor.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.result.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.pending.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    reassign_active_if_missing(store);
}

fn close_all_non_pinned_tabs(mut store: TabStore, pin_origin: u64) {
    let mut active_tab_id = store.active_tab_id;
    let mut next_tab_id = store.next_tab_id;
    let to_close: Vec<QueryTabState> = store
        .meta
        .read()
        .iter()
        .filter(|(_, t)| !t.pinned)
        .filter_map(|(id, _)| materialize_tab_state(store, *id))
        .collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    store.meta.with_mut(|m| {
        m.retain(|_, t| t.pinned);
    });
    store.editor.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.result.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });
    store.pending.with_mut(|m| {
        m.retain(|id, _| store.meta.read().contains_key(id));
    });

    // Always keep at least one non-pinned tab so the editor stays
    // open. The user just closed everything, so we spin up a fresh
    // query tab for the active session (or the origin tab's session
    // when no active tab remains).
    let session_id = store
        .meta
        .read()
        .get(&pin_origin)
        .map(|t| t.session_id)
        .or_else(|| store.meta.read().iter().next().map(|(_, t)| t.session_id))
        .or_else(|| crate::app_state::APP_STATE.read().active_session_id);

    if !store.meta.read().iter().any(|(_, t)| !t.pinned) {
        if let Some(session_id) = session_id {
            let new_id = next_tab_id();
            next_tab_id.set(new_id + 1);
            let (meta, editor, result, pending) =
                new_query_tab(new_id, session_id, format!("Query {new_id}"), String::new());
            store.meta.with_mut(|m| {
                m.insert(new_id, meta);
            });
            store.editor.with_mut(|m| {
                m.insert(new_id, editor);
            });
            store.result.with_mut(|m| {
                m.insert(new_id, result);
            });
            store.pending.with_mut(|m| {
                m.insert(new_id, pending);
            });
            active_tab_id.set(new_id);
            crate::app_state::activate_session(session_id);
        }
    } else {
        reassign_active_if_missing(store);
    }
}
