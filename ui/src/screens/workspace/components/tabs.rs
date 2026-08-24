use crate::{
    app_state::{
        APP_AI_FEATURES_ENABLED,
        APP_SHOW_SQL_EDITOR,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        context_menu::{ContextMenuItem, open_context_menu},
        open_connection_screen,
        pop_recently_closed_tab,
        push_recently_closed_tab,
    },
    screens::workspace::actions::{
        new_query_tab,
        open_structure_tab,
        read_only_mode_block_status,
        read_only_mode_enabled,
        refresh_tab_result,
        replace_active_tab_sql,
        run_explain_for_tab,
        run_query_for_tab,
        set_active_tab_status,
        tab_connection_or_error,
        toggle_execution_plan_for_tab,
    },
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use models::{
    AcpPanelState,
    QueryHistoryItem,
    QueryOutput,
    QueryTabState,
    SqlFormatSettings,
    TablePreviewSource,
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
    ensure_default_sql_agent_connected,
    send_sql_generation_request,
};

const EDITOR_MIN_HEIGHT: f64 = 160.0;
const EDITOR_MAX_HEIGHT: f64 = 720.0;
const EDITOR_DEFAULT_HEIGHT: f64 = 180.0;

#[derive(Clone, Copy, PartialEq)]
struct EditorResizeState {
    start_y: f64,
    start_height: f64,
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

#[component]
pub fn TabsManager(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    mut next_tab_id: Signal<u64>,
    history: Signal<Vec<QueryHistoryItem>>,
    next_history_id: Signal<u64>,
    explorer_sections: Signal<Vec<ExplorerConnectionSection>>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
) -> Element {
    let mut editor_height = use_signal(|| EDITOR_DEFAULT_HEIGHT);
    let mut editor_resize = use_signal(|| None::<EditorResizeState>);
    let mut show_generate_sql_window = use_signal(|| false);
    let mut generate_sql_prompt = use_signal(String::new);
    let mut generate_sql_input_revision = use_signal(|| 0_u64);
    let mut renaming_tab_id = use_signal(|| None::<u64>);
    let mut rename_value = use_signal(String::new);
    let active_tab = use_memo(move || {
        tabs.read()
            .iter()
            .find(|tab| tab.id == active_tab_id())
            .cloned()
    });

    let session_labels = {
        let app_state = APP_STATE.read();
        app_state
            .sessions
            .iter()
            .map(|session| (session.id, session.name.clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let active_actionable_source = active_tab.read().as_ref().and_then(actionable_table_source);
    let generate_sql_busy = acp_panel_state().busy;
    let generate_sql_prompt_empty = generate_sql_prompt().trim().is_empty();
    let generate_sql_input_key = format!("generate-sql-{}", generate_sql_input_revision());
    let read_only_mode = read_only_mode_enabled();

    rsx! {
        div {
            class: {
                let mut class_name = if APP_SHOW_SQL_EDITOR() {
                    "editor-shell".to_string()
                } else {
                    "editor-shell editor-shell--editor-hidden".to_string()
                };

                if editor_resize().is_some() {
                    class_name.push_str(" editor-shell--resizing");
                }

                class_name
            },
            style: if APP_SHOW_SQL_EDITOR() {
                format!("--editor-pane-height: {:.0}px;", editor_height())
            } else {
                String::new()
            },
            onmousemove: move |event| {
                let Some(resize) = editor_resize() else {
                    return;
                };

                if event.held_buttons().is_empty() {
                    editor_resize.set(None);
                    return;
                }

                let delta_y = event.client_coordinates().y - resize.start_y;
                let next_height =
                    (resize.start_height + delta_y).clamp(EDITOR_MIN_HEIGHT, EDITOR_MAX_HEIGHT);
                editor_height.set(next_height);
            },
            onmouseup: move |_| editor_resize.set(None),
            onmouseleave: move |_| editor_resize.set(None),
            div {
                class: "tabbar",
                for tab in tabs() {
                    div {
                        class: {
                            let mut class_name = if tab.id == active_tab_id() {
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
                            let tab_id = tab.id;
                            let session_id = tab.session_id;
                            move |_| {
                                active_tab_id.set(tab_id);
                                crate::app_state::activate_session(session_id);
                            }
                        },
                        onauxclick: {
                            let tab_id = tab.id;
                            move |event| {
                                if event.trigger_button() != Some(MouseButton::Auxiliary) {
                                    return;
                                }
                                close_tab_for_middle_click(tabs, active_tab_id, tab_id);
                            }
                        },
                        oncontextmenu: {
                            let tab_id = tab.id;
                            move |event| {
                                event.prevent_default();
                                let coords = event.client_coordinates();
                                let items =
                                    build_tab_context_menu(tab_id, tabs, active_tab_id, next_tab_id);
                                open_context_menu(coords.x, coords.y, items);
                            }
                        },
                        div {
                            class: "tabbar__copy",
                            if renaming_tab_id() == Some(tab.id) {
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
                                                tabs.with_mut(|all_tabs| {
                                                    if let Some(tab) = all_tabs.iter_mut().find(|t| t.id == tab_id) {
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
                                            tabs.with_mut(|all_tabs| {
                                                if let Some(tab) = all_tabs.iter_mut().find(|t| t.id == tab_id) {
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
                                        let tab_id = tab.id;
                                        move |_| {
                                            rename_value.set(tab.title.clone());
                                            renaming_tab_id.set(Some(tab_id));
                                        }
                                    },
                                    "{tab.title}"
                                }
                            }
                            if let Some(session_name) = session_labels.get(&tab.session_id) {
                                span { class: "tabbar__context", "{session_name}" }
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
                                let tab_id = tab.id;
                                move |event| {
                                    event.stop_propagation();
                                    close_tab_for_middle_click(tabs, active_tab_id, tab_id);
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

                        let new_id = next_tab_id();
                        next_tab_id += 1;
                        tabs.with_mut(|all_tabs| {
                            all_tabs.push(new_query_tab(
                                new_id,
                                session_id,
                                format!("Query {new_id}"),
                                String::new(),
                            ));
                        });
                        active_tab_id.set(new_id);
                    },
                    "+ Tab"
                }
            }

            if let Some(ref tab) = *active_tab.read() {
                if APP_SHOW_SQL_EDITOR() {
                    div {
                        class: "editor",
                        SqlEditor {
                            sql: tab.sql.clone(),
                            active_tab: tab.clone(),
                            tabs,
                            active_tab_id,
                            explorer_sections,
                        }
                    }
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
                div {
                    class: "editor__actions",
                    IconButton {
                        icon: ActionIcon::Run,
                        label: "Run SQL".to_string(),
                        primary: true,
                        onclick: move |_| {
                            let current_id = active_tab_id();
                            let current_tab = tabs
                                .read()
                                .iter()
                                .find(|tab| tab.id == current_id)
                                .cloned();

                            let Some(current_tab) = current_tab else {
                                return;
                            };

                            let sql = current_tab.sql.trim().to_string();
                            let tab_title = current_tab.title.clone();
                            let page_size = current_tab.page_size;
                            let connection_name = APP_STATE
                                .read()
                                .session(current_tab.session_id)
                                .map(|session| session.name.clone())
                                .unwrap_or_else(|| "Detached session".to_string());

                            if sql.is_empty() {
                                tabs.with_mut(|all_tabs| {
                                    if let Some(tab) = all_tabs.iter_mut().find(|tab| tab.id == current_id) {
                                        tab.status = "Query is empty".to_string();
                                    }
                                });
                                return;
                            }

                            let Some(connection) =
                                tab_connection_or_error(tabs, current_id, current_tab.session_id)
                            else {
                                return;
                            };

                            run_query_for_tab(
                                tabs,
                                current_id,
                                connection,
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
                            let current_tab = tab.clone();
                            let format_settings = APP_SQL_FORMAT_SETTINGS();
                            move |_| format_active_sql(tabs, current_tab.clone(), format_settings.clone())
                        },
                    }
                    IconButton {
                        icon: ActionIcon::Generate,
                        label: "Generate SQL".to_string(),
                        disabled: generate_sql_busy,
                        onclick: move |_| {
                            if !APP_AI_FEATURES_ENABLED() {
                                set_active_tab_status(
                                    tabs,
                                    active_tab_id(),
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
                    }
                    IconButton {
                        icon: ActionIcon::Structure,
                        label: "Open structure".to_string(),
                        disabled: active_actionable_source.is_none(),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| open_structure_for_active_preview(
                                tabs,
                                active_tab_id,
                                next_tab_id,
                                current_tab.clone(),
                            )
                        },
                    }
                    IconButton {
                        icon: ActionIcon::Explain,
                        label: "Explain Plan".to_string(),
                        onclick: {
                            move |_| {
                                let current_id = active_tab_id();
                                let Some(current_tab) = tabs
                                    .read()
                                    .iter()
                                    .find(|tab| tab.id == current_id)
                                    .cloned()
                                else {
                                    return;
                                };
                                let sql = current_tab.sql.trim().to_string();
                                if toggle_execution_plan_for_tab(tabs, current_id, &sql) {
                                    return;
                                }
                                if sql.is_empty() {
                                    tabs.with_mut(|all_tabs| {
                                        if let Some(tab) = all_tabs.iter_mut().find(|tab| tab.id == current_id) {
                                            tab.status = "Enter a query to explain".to_string();
                                        }
                                    });
                                    return;
                                }
                                if !services::is_read_only_sql(&sql) {
                                    set_active_tab_status(
                                        tabs,
                                        current_id,
                                        "Explain Plan is available only for read-only SQL.".to_string(),
                                    );
                                    return;
                                }
                                let Some(connection) =
                                    tab_connection_or_error(tabs, current_id, current_tab.session_id)
                                else {
                                    return;
                                };
                                run_explain_for_tab(tabs, current_id, connection, sql);
                            }
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportCsv,
                        label: "Export CSV".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::Csv)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportJson,
                        label: "Export JSON".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::Json)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportXlsx,
                        label: "Export XLSX".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::Xlsx)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportXml,
                        label: "Export XML".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::Xml)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportHtml,
                        label: "Export HTML".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::Html)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ExportSql,
                        label: "SQL Dump".to_string(),
                        disabled: !has_tabular_result(tab),
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| export_active_page(tabs, current_tab.clone(), ExportFormat::SqlDump)
                        },
                    }
                    IconButton {
                        icon: ActionIcon::ImportCsv,
                        label: if read_only_mode {
                            "Import CSV is blocked by read-only mode".to_string()
                        } else {
                            "Import CSV".to_string()
                        },
                        disabled: active_actionable_source.is_none() || read_only_mode,
                        onclick: {
                            let current_tab = tab.clone();
                            move |_| import_csv_into_active_table(tabs, current_tab.clone())
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
                                        key: "{generate_sql_input_key}",
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

                                            let Some(current_tab) = tabs
                                                .read()
                                                .iter()
                                                .find(|tab| tab.id == active_tab_id())
                                                .cloned()
                                            else {
                                                return;
                                            };

                                            submit_generated_sql_request(
                                                tabs,
                                                active_tab_id(),
                                                current_tab,
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
                                                let Some(current_tab) = tabs
                                                    .read()
                                                    .iter()
                                                    .find(|tab| tab.id == active_tab_id())
                                                    .cloned()
                                                else {
                                                    return;
                                                };

                                                submit_generated_sql_request(
                                                    tabs,
                                                    active_tab_id(),
                                                    current_tab,
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
                    } else if tab.batch_results.is_some() {
                        BatchResultsView {
                            tabs,
                            active_tab_id,
                        }
                    } else if tab.show_execution_plan {
                        if let Some(plan) = tab.execution_plan.clone() {
                            ExecutionPlanView {
                                plan,
                                tabs,
                                active_tab_id,
                            }
                        } else {
                            ResultTable {
                                result: tab.result.clone(),
                                tabs,
                                active_tab_id,
                            }
                        }
                    } else {
                        ResultTable {
                            result: tab.result.clone(),
                            tabs,
                            active_tab_id,
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

fn export_active_page(
    tabs: Signal<Vec<QueryTabState>>,
    current_tab: QueryTabState,
    format: ExportFormat,
) {
    let Some(QueryOutput::Table(page)) = current_tab.result.clone() else {
        set_active_tab_status(
            tabs,
            current_tab.id,
            "Nothing to export in the current tab".to_string(),
        );
        return;
    };

    let file_name = default_export_file_name(&current_tab, format);
    set_active_tab_status(
        tabs,
        current_tab.id,
        format!("Select a destination for the {} export", format.label()),
    );

    spawn(async move {
        let Some(file) = AsyncFileDialog::new()
            .set_file_name(&file_name)
            .add_filter(format.label(), &[format.extension()])
            .save_file()
            .await
        else {
            set_active_tab_status(tabs, current_tab.id, "Export cancelled".to_string());
            return;
        };

        let path = file.path().to_path_buf();
        set_active_tab_status(
            tabs,
            current_tab.id,
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
                    tabs,
                    current_tab.id,
                    format!("Exported {rows} row(s) to {destination}"),
                );
            }
            Err(err) => set_active_tab_status(
                tabs,
                current_tab.id,
                format!("{} export error: {err}", format.label()),
            ),
        }
    });
}

fn import_csv_into_active_table(tabs: Signal<Vec<QueryTabState>>, current_tab: QueryTabState) {
    if read_only_mode_enabled() {
        set_active_tab_status(
            tabs,
            current_tab.id,
            read_only_mode_block_status("CSV import"),
        );
        return;
    }

    let Some(source) = actionable_table_source(&current_tab) else {
        set_active_tab_status(
            tabs,
            current_tab.id,
            "Import CSV is available for previewed tables and simple single-table SELECT queries"
                .to_string(),
        );
        return;
    };

    let Some(connection) = tab_connection_or_error(tabs, current_tab.id, current_tab.session_id)
    else {
        return;
    };

    set_active_tab_status(
        tabs,
        current_tab.id,
        format!("Select a CSV file to import into {}", source.table_name),
    );

    spawn(async move {
        let Some(file) = AsyncFileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file()
            .await
        else {
            set_active_tab_status(tabs, current_tab.id, "CSV import cancelled".to_string());
            return;
        };

        let path = file.path().to_path_buf();
        set_active_tab_status(
            tabs,
            current_tab.id,
            format!("Importing {}...", path.to_string_lossy()),
        );

        match services::import_csv_into_table(connection, source.clone(), path).await {
            Ok(rows) => {
                set_active_tab_status(
                    tabs,
                    current_tab.id,
                    format!("Imported {rows} row(s) into {}", source.table_name),
                );
                if let Some(updated_tab) = tabs
                    .read()
                    .iter()
                    .find(|tab| tab.id == current_tab.id)
                    .cloned()
                {
                    refresh_tab_result(tabs, updated_tab, Some(source));
                }
            }
            Err(err) =>
                set_active_tab_status(tabs, current_tab.id, format!("CSV import error: {err}")),
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

fn has_tabular_result(tab: &QueryTabState) -> bool {
    matches!(tab.result.as_ref(), Some(QueryOutput::Table(_)))
}

fn format_active_sql(
    tabs: Signal<Vec<QueryTabState>>,
    current_tab: QueryTabState,
    format_settings: SqlFormatSettings,
) {
    let sql = current_tab.sql.trim();
    if sql.is_empty() {
        set_active_tab_status(
            tabs,
            current_tab.id,
            "Nothing to format in the current tab".to_string(),
        );
        return;
    }

    let session_kind = APP_STATE
        .read()
        .session(current_tab.session_id)
        .map(|session| session.kind);
    let formatted = services::format_sql(session_kind, sql, &format_settings);
    replace_active_tab_sql(tabs, current_tab.id, formatted, "SQL formatted".to_string());
}

#[allow(clippy::too_many_arguments)]
fn submit_generated_sql_request(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
    current_tab: QueryTabState,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: bool,
    prompt_draft: Signal<String>,
    mut show_generate_sql_window: Signal<bool>,
) {
    let request = prompt_draft().trim().to_string();
    if request.is_empty() {
        set_active_tab_status(
            tabs,
            current_tab.id,
            "Enter a description before generating SQL.".to_string(),
        );
        return;
    }

    let connection_label = APP_STATE
        .read()
        .session(current_tab.session_id)
        .map(|session| session.name.clone())
        .unwrap_or_else(|| "Detached session".to_string());

    set_active_tab_status(
        tabs,
        current_tab.id,
        "Generating SQL with the configured AI agent...".to_string(),
    );

    spawn(async move {
        let deepseek = crate::app_state::APP_UI_SETTINGS().deepseek;
        if let Err(err) =
            ensure_default_sql_agent_connected(acp_panel_state, chat_revision, deepseek).await
        {
            set_active_tab_status(tabs, current_tab.id, format!("Generate SQL error: {err}"));
            return;
        }

        send_sql_generation_request(
            acp_panel_state,
            tabs,
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

fn open_structure_for_active_preview(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    current_tab: QueryTabState,
) {
    let Some(source) = actionable_table_source(&current_tab) else {
        set_active_tab_status(
            tabs,
            current_tab.id,
            "Structure view is available for previewed tables and simple single-table SELECT queries"
                .to_string(),
        );
        return;
    };

    let Some(connection) = tab_connection_or_error(tabs, current_tab.id, current_tab.session_id)
    else {
        return;
    };

    open_structure_tab(
        tabs,
        active_tab_id,
        next_tab_id,
        current_tab.session_id,
        connection,
        source,
    );
}

fn actionable_table_source(tab: &QueryTabState) -> Option<TablePreviewSource> {
    tab.preview_source.clone().or_else(|| {
        tab.last_run_sql
            .as_deref()
            .and_then(services::preview_source_for_sql)
    })
}

/// Re-assign `active_tab_id` to the first remaining tab when the
/// current active tab is no longer in the list. Used by every
/// "close many" helper so the editor does not stay focused on a
/// vanished tab.
fn reassign_active_if_missing(tabs: Signal<Vec<QueryTabState>>, mut active_tab_id: Signal<u64>) {
    if tabs.read().iter().any(|t| t.id == active_tab_id()) {
        return;
    }
    if let Some(next) = tabs.read().first().cloned() {
        active_tab_id.set(next.id);
        crate::app_state::activate_session(next.session_id);
    }
}

/// Close a single tab by id. Pinned tabs are protected — this
/// helper is also the entry point for the X button and the
/// middle-click handler, both of which explicitly close the tab
/// the user targeted (so pinning never silently blocks an
/// intentional close). The closed tab is pushed onto the
/// "recently closed" stack so "Reopen Closed Tab" can restore it.
fn close_tab_for_middle_click(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    tab_id: u64,
) {
    let snapshot = tabs.read().clone();
    if snapshot.len() <= 1 {
        return;
    }
    let Some(closed) = snapshot.iter().find(|tab| tab.id == tab_id).cloned() else {
        return;
    };

    push_recently_closed_tab(closed);
    tabs.with_mut(|all_tabs| all_tabs.retain(|tab| tab.id != tab_id));

    if active_tab_id() == tab_id {
        let remaining = tabs.read().clone();
        let next_tab = remaining
            .iter()
            .find(|tab| !tab.pinned)
            .or_else(|| remaining.first())
            .cloned();
        if let Some(next_tab) = next_tab {
            active_tab_id.set(next_tab.id);
            crate::app_state::activate_session(next_tab.session_id);
        }
    }
}

/// Build the right-click menu for a tab. Mirrors DBeaver/DataGrip
/// (Close, Close Others, Close to Right, Close All, Pin/Unpin,
/// Duplicate, Reopen Closed Tab). Pinned tabs are protected from
/// "Close Others" / "Close to Right" / "Close All" — those items
/// only affect non-pinned tabs (and "Close Others" keeps all
/// pinned tabs).
#[allow(clippy::too_many_arguments)]
fn build_tab_context_menu(
    tab_id: u64,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
) -> Vec<ContextMenuItem> {
    let mut items: Vec<ContextMenuItem> = Vec::new();

    let snapshot: Option<QueryTabState> = tabs.read().iter().find(|t| t.id == tab_id).cloned();
    let Some(tab) = snapshot else {
        return items;
    };
    let total = tabs.read().len();
    let tab_index = tabs.read().iter().position(|t| t.id == tab_id);
    let non_pinned_total = tabs.read().iter().filter(|t| !t.pinned).count();
    let pinned_total = total - non_pinned_total;
    let tabs_to_right = tab_index
        .map(|idx| {
            tabs.read()
                .iter()
                .skip(idx + 1)
                .filter(|t| !t.pinned)
                .count()
        })
        .unwrap_or(0);

    // 1. Close
    {
        let mut item = ContextMenuItem::new("Close", move || {
            close_tab_for_middle_click(tabs, active_tab_id, tab_id);
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
            close_other_tabs(tabs, active_tab_id, tab_id);
        })
        .with_icon(ActionIcon::Close);
        item.disabled = closeable_others == 0;
        items.push(item);
    }

    // 3. Close to Right — non-pinned tabs to the right of this one.
    {
        let mut item = ContextMenuItem::new("Close Tabs to the Right", move || {
            close_tabs_to_the_right(tabs, active_tab_id, tab_id);
        })
        .with_icon(ActionIcon::Close);
        item.disabled = tabs_to_right == 0;
        items.push(item);
    }

    // 4. Close All — close every non-pinned tab.
    {
        let mut item = ContextMenuItem::new("Close All", move || {
            close_all_non_pinned_tabs(tabs, active_tab_id, next_tab_id, tab_id);
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
                set_tab_pinned(tabs, tab_id, false);
            }));
        } else {
            items.push(ContextMenuItem::new("Pin", move || {
                set_tab_pinned(tabs, tab_id, true);
            }));
        }
    }

    // 6. Duplicate — clone the tab into a new tab inserted after
    //    this one. The new tab gets a fresh id; title is suffixed
    //    with " copy". Result, SQL, and preview_source are copied
    //    so the duplicate is ready to run.
    {
        let source = tab.clone();
        items.push(
            ContextMenuItem::new("Duplicate", move || {
                duplicate_tab(tabs, active_tab_id, next_tab_id, source.clone());
            })
            .with_icon(ActionIcon::Duplicate)
            .separator(),
        );
    }

    // 7. Reopen Closed Tab
    {
        let mut item = ContextMenuItem::new("Reopen Closed Tab", move || {
            reopen_last_closed_tab(tabs, active_tab_id, next_tab_id);
        });
        item.disabled = crate::app_state::APP_RECENTLY_CLOSED_TABS.peek().is_empty();
        items.push(item);
    }

    items
}

fn set_tab_pinned(mut tabs: Signal<Vec<QueryTabState>>, tab_id: u64, pinned: bool) {
    tabs.with_mut(|all_tabs| {
        if let Some(tab) = all_tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.pinned = pinned;
        }
    });
}

fn duplicate_tab(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    mut next_tab_id: Signal<u64>,
    source: QueryTabState,
) {
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
    let insert_at = tabs
        .read()
        .iter()
        .position(|tab| tab.id == source_id)
        .map(|idx| idx + 1)
        .unwrap_or_else(|| tabs.read().len());
    tabs.with_mut(|all_tabs| all_tabs.insert(insert_at, clone));
    active_tab_id.set(new_id);
}

fn reopen_last_closed_tab(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    mut next_tab_id: Signal<u64>,
) {
    let Some(restored) = pop_recently_closed_tab(&mut next_tab_id) else {
        return;
    };
    let new_id = restored.id;
    let session_id = restored.session_id;
    tabs.with_mut(|all_tabs| all_tabs.push(restored));
    active_tab_id.set(new_id);
    crate::app_state::activate_session(session_id);
}

fn close_other_tabs(
    mut tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    keep_tab_id: u64,
) {
    let to_close: Vec<QueryTabState> = tabs
        .read()
        .iter()
        .filter(|t| t.id != keep_tab_id && !t.pinned)
        .cloned()
        .collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    tabs.with_mut(|all_tabs| {
        all_tabs.retain(|t| t.id == keep_tab_id || t.pinned);
    });
    reassign_active_if_missing(tabs, active_tab_id);
}

fn close_tabs_to_the_right(
    mut tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    anchor_tab_id: u64,
) {
    let snapshot = tabs.read().clone();
    let Some(anchor_idx) = snapshot.iter().position(|t| t.id == anchor_tab_id) else {
        return;
    };
    let to_close: Vec<QueryTabState> = snapshot
        .iter()
        .skip(anchor_idx + 1)
        .filter(|t| !t.pinned)
        .cloned()
        .collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    tabs.with_mut(|all_tabs| {
        let keep: std::collections::HashSet<u64> = all_tabs
            .iter()
            .take(anchor_idx + 1)
            .map(|t| t.id)
            .chain(all_tabs.iter().filter(|t| t.pinned).map(|t| t.id))
            .collect();
        all_tabs.retain(|t| keep.contains(&t.id));
    });
    reassign_active_if_missing(tabs, active_tab_id);
}

fn close_all_non_pinned_tabs(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    mut next_tab_id: Signal<u64>,
    pin_origin: u64,
) {
    let to_close: Vec<QueryTabState> = tabs.read().iter().filter(|t| !t.pinned).cloned().collect();
    if to_close.is_empty() {
        return;
    }
    for tab in to_close {
        push_recently_closed_tab(tab);
    }
    tabs.with_mut(|all_tabs| {
        all_tabs.retain(|t| t.pinned);
    });

    // Always keep at least one non-pinned tab so the editor stays
    // open. The user just closed everything, so we spin up a fresh
    // query tab for the active session (or the origin tab's session
    // when no active tab remains).
    let session_id = tabs
        .read()
        .iter()
        .find(|t| t.id == pin_origin)
        .map(|t| t.session_id)
        .or_else(|| tabs.read().iter().find(|t| t.pinned).map(|t| t.session_id))
        .or_else(|| crate::app_state::APP_STATE.read().active_session_id);

    if !tabs.read().iter().any(|t| !t.pinned) {
        if let Some(session_id) = session_id {
            let new_id = next_tab_id();
            next_tab_id.set(new_id + 1);
            tabs.with_mut(|all_tabs| {
                all_tabs.push(new_query_tab(
                    new_id,
                    session_id,
                    format!("Query {new_id}"),
                    String::new(),
                ));
            });
            active_tab_id.set(new_id);
            crate::app_state::activate_session(session_id);
        }
    } else {
        reassign_active_if_missing(tabs, active_tab_id);
    }
}
