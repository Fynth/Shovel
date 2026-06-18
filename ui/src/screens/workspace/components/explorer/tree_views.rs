use super::{
    count_objects,
    disconnect_session,
    duplicate_table_modal::{DuplicateTableModal, DuplicateTableTarget},
    split_children,
};
use crate::{
    app_state::{
        APP_STATE,
        activate_session,
        context_menu::{ContextMenuItem, open_context_menu},
        session_connection,
    },
    screens::workspace::{
        ActionIcon,
        actions::{
            ensure_tab_for_session,
            mark_table_deleted,
            mark_table_truncated,
            read_only_mode_enabled,
            run_table_preview_for_tab,
            tab_connection_or_error,
        },
        components::IconButton,
    },
};
use dioxus::prelude::*;
use models::{DatabaseKind, ExplorerNode, ExplorerNodeKind, QueryTabState, TablePreviewSource};
use rfd::{AsyncMessageDialog, MessageButtons, MessageDialogResult, MessageLevel};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableMutationKind {
    Truncate,
    Drop,
}

#[component]
pub(super) fn ExplorerConnectionView(
    section: super::ExplorerConnectionSection,
    tree_reload: Signal<u64>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    selected_node: Signal<String>,
) -> Element {
    let mut expanded = use_signal(|| true);
    let object_count = count_objects(&section.nodes);

    rsx! {
        div { class: if section.is_active {
                "tree__connection tree__connection--active"
            } else {
                "tree__connection"
            },
            div {
                class: "tree__connection-header",
                button {
                    class: "tree__connection-toggle",
                    onclick: {
                        let session_id = section.session_id;
                        move |_| {
                            activate_session(session_id);
                            expanded.toggle();
                        }
                    },
                    span {
                        class: if expanded() {
                            "tree__chevron tree__chevron--open"
                        } else {
                            "tree__chevron"
                        },
                        ">"
                    }
                    div {
                        class: "tree__connection-copy",
                        div {
                            class: "tree__connection-topline",
                            span { class: "tree__connection-kind", "{section.kind_label}" }
                            span {
                                class: "tree__connection-title",
                                title: "{section.name}",
                                "{section.name}"
                            }
                            span {
                                class: "tree__connection-meta",
                                title: "{section.status} · {object_count} objects",
                                "{section.status} · {object_count} objects"
                            }
                        }
                    }
                }
                div {
                    class: "tree__connection-actions",
                    IconButton {
                        icon: ActionIcon::Close,
                        label: "Disconnect".to_string(),
                        small: true,
                        onclick: {
                            let session_id = section.session_id;
                            move |_| disconnect_session(tabs, active_tab_id, session_id)
                        },
                    }
                }
            }

            if expanded() {
                div { class: "tree__connection-body",
                    if section.nodes.is_empty() {
                        p { class: "empty-state", "No objects loaded for this connection." }
                    } else {
                        for node in section.nodes {
                            ExplorerSchemaView {
                                node,
                                session_id: section.session_id,
                                tree_reload,
                                tabs,
                                active_tab_id,
                                next_tab_id,
                                selected_node,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ExplorerSchemaView(
    node: ExplorerNode,
    session_id: u64,
    tree_reload: Signal<u64>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    selected_node: Signal<String>,
) -> Element {
    let mut expanded = use_signal(|| true);
    let (tables, views) = split_children(&node.children);
    let object_count = tables.len() + views.len();

    rsx! {
        div { class: "tree__schema",
            button {
                class: "tree__schema-toggle",
                onclick: move |_| expanded.toggle(),
                span {
                    class: if expanded() {
                        "tree__chevron tree__chevron--open"
                    } else {
                        "tree__chevron"
                    },
                    ">"
                }
                div {
                    class: "tree__schema-copy",
                    span { class: "tree__schema-title", "{node.name}" }
                    span {
                        class: "tree__schema-meta",
                        "{object_count} objects"
                    }
                }
            }

            if expanded() {
                div { class: "tree__schema-body",
                    if !tables.is_empty() {
                        ExplorerGroupView {
                            title: "Tables".to_string(),
                            session_id,
                            tree_reload,
                            nodes: tables,
                            tabs,
                            active_tab_id,
                            next_tab_id,
                            selected_node,
                        }
                    }
                    if !views.is_empty() {
                        ExplorerGroupView {
                            title: "Views".to_string(),
                            session_id,
                            tree_reload,
                            nodes: views,
                            tabs,
                            active_tab_id,
                            next_tab_id,
                            selected_node,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ExplorerGroupView(
    title: String,
    session_id: u64,
    tree_reload: Signal<u64>,
    nodes: Vec<ExplorerNode>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    selected_node: Signal<String>,
) -> Element {
    rsx! {
        div { class: "tree__group",
            div { class: "tree__group-header", "{title}" }
            div { class: "tree__group-items",
                for node in nodes {
                    ExplorerObjectRow {
                        node,
                        session_id,
                        tree_reload,
                        tabs,
                        active_tab_id,
                        next_tab_id,
                        selected_node,
                    }
                }
            }
        }
    }
}

#[component]
fn ExplorerObjectRow(
    node: ExplorerNode,
    session_id: u64,
    mut tree_reload: Signal<u64>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    selected_node: Signal<String>,
) -> Element {
    let table_mutation_inflight = use_signal(|| None::<TableMutationKind>);
    let mut show_duplicate_table = use_signal(|| false);
    let (connection_name, connection_kind) = APP_STATE
        .read()
        .session(session_id)
        .map(|session| (session.name.clone(), session.kind))
        .unwrap_or_else(|| ("Connection".to_string(), DatabaseKind::Sqlite));
    let preview_source = TablePreviewSource {
        schema: node.schema.clone(),
        table_name: node.name.clone(),
        qualified_name: node.qualified_name.clone(),
    };
    let selected = selected_node() == node.qualified_name;
    let can_duplicate_table = node.kind == ExplorerNodeKind::Table;
    let can_truncate_table = node.kind == ExplorerNodeKind::Table;
    let can_drop_table = node.kind == ExplorerNodeKind::Table;
    let read_only_mode = read_only_mode_enabled();
    let kind_badge = match node.kind {
        ExplorerNodeKind::Table => "T",
        ExplorerNodeKind::View => "V",
        ExplorerNodeKind::Schema => "",
    };
    let kind_label = match node.kind {
        ExplorerNodeKind::Table => "Table",
        ExplorerNodeKind::View => "View",
        ExplorerNodeKind::Schema => "Schema",
    };

    let (_read_only_mode_for_menu, items) = build_explorer_context_menu(
        connection_name.clone(),
        preview_source.clone(),
        node.kind,
        read_only_mode,
        show_duplicate_table,
        tabs,
        active_tab_id,
        next_tab_id,
        selected_node,
        session_id,
        connection_kind,
        table_mutation_inflight,
        tree_reload,
    );

    rsx! {
        div {
            class: if selected {
                "tree__object-row tree__object-row--selected"
            } else {
                "tree__object-row"
            },
            oncontextmenu: move |event| {
                event.prevent_default();
                let coords = event.client_coordinates();
                open_context_menu(coords.x, coords.y, items.clone());
            },
            button {
                class: if selected {
                    "tree__object tree__object--selected"
                } else {
                    "tree__object"
                },
                onclick: {
                    let qualified_name = node.qualified_name.clone();
                    move |_| {
                        selected_node.set(qualified_name.clone());
                        activate_session(session_id);
                    }
                },
                ondoubleclick: {
                    let source = preview_source.clone();
                    let qualified_name = node.qualified_name.clone();
                    move |_| {
                        selected_node.set(qualified_name.clone());
                        let current_id =
                            ensure_tab_for_session(tabs, active_tab_id, next_tab_id, session_id);
                        let current_tab = tabs
                            .read()
                            .iter()
                            .find(|tab| tab.id == current_id)
                            .cloned();
                        let Some(current_tab) = current_tab else {
                            return;
                        };

                        let Some(connection) =
                            tab_connection_or_error(tabs, current_id, current_tab.session_id)
                        else {
                            return;
                        };

                        run_table_preview_for_tab(
                            tabs,
                            current_id,
                            connection,
                            source.clone(),
                            0,
                            current_tab.page_size,
                        );
                    }
                },
                div {
                    class: "tree__object-badge",
                    "{kind_badge}"
                }
                div {
                    class: "tree__object-copy",
                    div {
                        class: "tree__object-name",
                        title: "{node.qualified_name}",
                        "{node.name}"
                    }
                    div { class: "tree__object-kind", "{kind_label}" }
                }
            }
            if can_duplicate_table || can_truncate_table || can_drop_table {
                div { class: "tree__object-actions",
                    if can_duplicate_table {
                        IconButton {
                            icon: ActionIcon::Duplicate,
                            label: if read_only_mode {
                                format!("Duplicate table {} is blocked by read-only mode", node.name)
                            } else {
                                format!("Duplicate table {}", node.name)
                            },
                            small: true,
                            disabled: table_mutation_inflight().is_some() || read_only_mode,
                            onclick: {
                                move |event: MouseEvent| {
                                    event.stop_propagation();
                                    if read_only_mode_enabled() {
                                        return;
                                    }
                                    show_duplicate_table.set(true);
                                }
                            },
                        }
                    }
                    if can_truncate_table {
                        IconButton {
                            icon: ActionIcon::Truncate,
                            label: if read_only_mode {
                                format!("Truncate table {} is blocked by read-only mode", node.name)
                            } else {
                                table_mutation_button_label(
                                    TableMutationKind::Truncate,
                                    &node.name,
                                    table_mutation_inflight() == Some(TableMutationKind::Truncate),
                                )
                            },
                            small: true,
                            disabled: table_mutation_inflight().is_some() || read_only_mode,
                            onclick: {
                                let source = preview_source.clone();
                                move |event: MouseEvent| {
                                    event.stop_propagation();
                                    if table_mutation_inflight().is_some()
                                        || read_only_mode_enabled()
                                    {
                                        return;
                                    }
                                    spawn(confirm_and_truncate_table(
                                        source.clone(),
                                        session_id,
                                        connection_kind,
                                        tabs,
                                        table_mutation_inflight,
                                        selected_node,
                                        tree_reload,
                                    ));
                                }
                            },
                        }
                    }
                    IconButton {
                        icon: ActionIcon::Delete,
                        label: if read_only_mode {
                            format!("Drop table {} is blocked by read-only mode", node.name)
                        } else {
                            table_mutation_button_label(
                                TableMutationKind::Drop,
                                &node.name,
                                table_mutation_inflight() == Some(TableMutationKind::Drop),
                            )
                        },
                        small: true,
                        disabled: table_mutation_inflight().is_some() || read_only_mode,
                        onclick: {
                            let source = preview_source.clone();
                            let selected_qualified_name = node.qualified_name.clone();
                            move |event: MouseEvent| {
                                event.stop_propagation();
                                if table_mutation_inflight().is_some() || read_only_mode_enabled() {
                                    return;
                                }
                                spawn(confirm_and_drop_table(
                                    source.clone(),
                                    selected_qualified_name.clone(),
                                    session_id,
                                    connection_kind,
                                    tabs,
                                    table_mutation_inflight,
                                    selected_node,
                                    tree_reload,
                                ));
                            }
                        },
                    }
                }
            }
            if show_duplicate_table() {
                DuplicateTableModal {
                    target: DuplicateTableTarget {
                        session_id,
                        connection_name: connection_name.clone(),
                        kind: connection_kind,
                        source: preview_source.clone(),
                    },
                    tree_reload,
                    selected_node,
                    show_duplicate_table,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Table mutation helpers
// ---------------------------------------------------------------------------

fn table_mutation_button_label(
    action: TableMutationKind,
    table_name: &str,
    inflight: bool,
) -> String {
    match (action, inflight) {
        (TableMutationKind::Truncate, true) => "Truncating table".to_string(),
        (TableMutationKind::Truncate, false) => format!("Truncate table {table_name}"),
        (TableMutationKind::Drop, true) => "Dropping table".to_string(),
        (TableMutationKind::Drop, false) => format!("Drop table {table_name}"),
    }
}

fn table_mutation_dialog_title(action: TableMutationKind) -> &'static str {
    match action {
        TableMutationKind::Truncate => "Truncate table",
        TableMutationKind::Drop => "Drop table",
    }
}

fn table_mutation_error_title(action: TableMutationKind) -> &'static str {
    match action {
        TableMutationKind::Truncate => "Truncate table failed",
        TableMutationKind::Drop => "Drop table failed",
    }
}

fn table_mutation_connection_closed_description(action: TableMutationKind) -> &'static str {
    match action {
        TableMutationKind::Truncate =>
            "The connection was closed before the table could be truncated.",
        TableMutationKind::Drop => "The connection was closed before the table could be dropped.",
    }
}

fn table_mutation_confirmation_description(
    action: TableMutationKind,
    kind: DatabaseKind,
    source: &TablePreviewSource,
) -> String {
    match action {
        TableMutationKind::Truncate => {
            let sql = match kind {
                DatabaseKind::Sqlite => format!("DELETE FROM {}", source.qualified_name),
                DatabaseKind::Postgres | DatabaseKind::MySql | DatabaseKind::ClickHouse => {
                    format!("TRUNCATE TABLE {}", source.qualified_name)
                }
            };
            format!(
                "Truncate {}?\n\nThis removes all rows but keeps the table structure by running {}.",
                source.table_name, sql,
            )
        }
        TableMutationKind::Drop => format!(
            "Drop {}?\n\nThis permanently removes the table by running DROP TABLE IF EXISTS {}. Dependent objects may prevent the operation.",
            source.table_name, source.qualified_name,
        ),
    }
}

// ---------------------------------------------------------------------------
// Context-menu builder for table/view rows in the Explorer
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn build_explorer_context_menu(
    _connection_name: String,
    preview_source: TablePreviewSource,
    kind: ExplorerNodeKind,
    read_only_mode: bool,
    mut show_duplicate_table: Signal<bool>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
    selected_node: Signal<String>,
    session_id: u64,
    connection_kind: DatabaseKind,
    table_mutation_inflight: Signal<Option<TableMutationKind>>,
    mut tree_reload: Signal<u64>,
) -> (bool, Vec<ContextMenuItem>) {
    use crate::app_state::context_menu::copy_to_clipboard;

    let mut items: Vec<ContextMenuItem> = Vec::new();

    // 1. Open in editor (preview) — only for tables and views.
    if matches!(kind, ExplorerNodeKind::Table | ExplorerNodeKind::View) {
        let source = preview_source.clone();
        items.push(
            ContextMenuItem::new("Open in editor", move || {
                let source = source.clone();
                let current_id =
                    ensure_tab_for_session(tabs, active_tab_id, next_tab_id, session_id);
                let Some(current_tab) =
                    tabs.read().iter().find(|tab| tab.id == current_id).cloned()
                else {
                    return;
                };
                let Some(connection) =
                    tab_connection_or_error(tabs, current_id, current_tab.session_id)
                else {
                    return;
                };
                run_table_preview_for_tab(
                    tabs,
                    current_id,
                    connection,
                    source,
                    0,
                    current_tab.page_size,
                );
            })
            .with_icon(ActionIcon::Run),
        );
    }

    // 2. Select as query — generate "SELECT * FROM qualified" in the active tab.
    if matches!(kind, ExplorerNodeKind::Table | ExplorerNodeKind::View) {
        let qualified = preview_source.qualified_name.clone();
        items.push(
            ContextMenuItem::new("Select all rows", move || {
                let sql = format!("SELECT * FROM {qualified}");
                crate::screens::workspace::actions::set_active_tab_sql(
                    tabs,
                    active_tab_id(),
                    sql,
                    "Loaded query from explorer".to_string(),
                );
            })
            .with_icon(ActionIcon::Details),
        );
    }

    // 3. Copy name (short).
    {
        let name = preview_source.table_name.clone();
        items.push(
            ContextMenuItem::new("Copy name", move || {
                let _ = copy_to_clipboard(name.clone());
            })
            .with_icon(ActionIcon::Duplicate),
        );
    }

    // 4. Copy qualified name.
    {
        let qualified = preview_source.qualified_name.clone();
        items.push(
            ContextMenuItem::new("Copy qualified name", move || {
                let _ = copy_to_clipboard(qualified.clone());
            })
            .with_icon(ActionIcon::Duplicate),
        );
    }

    // 5. Copy as INSERT template — only meaningful for tables.
    if kind == ExplorerNodeKind::Table {
        let qualified = preview_source.qualified_name.clone();
        items.push(
            ContextMenuItem::new("Copy as INSERT template", move || {
                let _ = copy_to_clipboard(format!("INSERT INTO {qualified} VALUES (...);"));
            })
            .with_icon(ActionIcon::ExportSql),
        );
    }

    // 6. Refresh — always available.
    items.push(
        ContextMenuItem::new("Refresh", move || {
            tree_reload += 1;
        })
        .with_icon(ActionIcon::Refresh)
        .separator(),
    );

    // 7. Duplicate — only for tables. Disables in read-only mode.
    //    For views, surface a "View DDL" placeholder for symmetry —
    //    the actual DDL extraction lives in the DescribeTable pipeline
    //    and is left for a follow-up.
    if kind == ExplorerNodeKind::Table {
        let mut item = ContextMenuItem::new("Duplicate table…", move || {
            show_duplicate_table.set(true);
        })
        .with_icon(ActionIcon::Duplicate);
        if read_only_mode {
            item = item.disabled();
        }
        items.push(item);
    }

    // 8. Truncate / Drop — only for tables. Disabled in read-only mode.
    //    Both reuse the same async helpers as the inline IconButton
    //    onclick handlers above, so the rfd confirmation flow, the
    //    mark_table_* signal updates, and the error handling all stay
    //    in one place. We only check read-only mode here at the menu
    //    level; the async helpers themselves are private to this
    //    module and never called by a non-table source.
    if kind == ExplorerNodeKind::Table {
        let source = preview_source.clone();
        let mut truncate_item = ContextMenuItem::new("Truncate table", move || {
            spawn(confirm_and_truncate_table(
                source.clone(),
                session_id,
                connection_kind,
                tabs,
                table_mutation_inflight,
                selected_node,
                tree_reload,
            ));
        })
        .with_icon(ActionIcon::Truncate)
        .danger();
        if read_only_mode {
            truncate_item = truncate_item.disabled();
        }
        items.push(truncate_item);

        let source = preview_source.clone();
        let qualified = preview_source.qualified_name.clone();
        let mut drop_item = ContextMenuItem::new("Drop table", move || {
            spawn(confirm_and_drop_table(
                source.clone(),
                qualified.clone(),
                session_id,
                connection_kind,
                tabs,
                table_mutation_inflight,
                selected_node,
                tree_reload,
            ));
        })
        .with_icon(ActionIcon::Delete)
        .danger();
        if read_only_mode {
            drop_item = drop_item.disabled();
        }
        items.push(drop_item);
    }

    (read_only_mode, items)
}

// ---------------------------------------------------------------------------
// Async helpers for table-mutation flows (used by both inline buttons and the
// context menu). Hoisted from the inline `IconButton` onclick handlers in
// `ExplorerObjectRow` so that the same flow can be triggered from
// different surfaces without duplicating the rfd confirmation + signal
// orchestration logic.
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn confirm_and_truncate_table(
    source: TablePreviewSource,
    session_id: u64,
    connection_kind: DatabaseKind,
    tabs: Signal<Vec<QueryTabState>>,
    mut table_mutation_inflight: Signal<Option<TableMutationKind>>,
    _selected_node: Signal<String>,
    _tree_reload: Signal<u64>,
) {
    let confirmation = AsyncMessageDialog::new()
        .set_title(table_mutation_dialog_title(TableMutationKind::Truncate))
        .set_description(table_mutation_confirmation_description(
            TableMutationKind::Truncate,
            connection_kind,
            &source,
        ))
        .set_buttons(MessageButtons::YesNo)
        .set_level(MessageLevel::Warning)
        .show()
        .await;

    if confirmation != MessageDialogResult::Yes {
        return;
    }

    let Some(connection) = session_connection(session_id) else {
        let _ = AsyncMessageDialog::new()
            .set_title(table_mutation_error_title(TableMutationKind::Truncate))
            .set_description(table_mutation_connection_closed_description(
                TableMutationKind::Truncate,
            ))
            .set_buttons(MessageButtons::Ok)
            .set_level(MessageLevel::Error)
            .show()
            .await;
        return;
    };

    let refresh_connection = connection.clone();
    table_mutation_inflight.set(Some(TableMutationKind::Truncate));
    let result = services::truncate_table(connection, source.clone()).await;
    table_mutation_inflight.set(None);

    match result {
        Ok(()) => {
            mark_table_truncated(tabs, session_id, refresh_connection, source.clone());
        }
        Err(err) => {
            let _ = AsyncMessageDialog::new()
                .set_title(table_mutation_error_title(TableMutationKind::Truncate))
                .set_description(format!(
                    "Failed to truncate {}.\n\n{}",
                    source.qualified_name, err
                ))
                .set_buttons(MessageButtons::Ok)
                .set_level(MessageLevel::Error)
                .show()
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn confirm_and_drop_table(
    source: TablePreviewSource,
    selected_qualified_name: String,
    session_id: u64,
    connection_kind: DatabaseKind,
    tabs: Signal<Vec<QueryTabState>>,
    mut table_mutation_inflight: Signal<Option<TableMutationKind>>,
    mut selected_node: Signal<String>,
    mut tree_reload: Signal<u64>,
) {
    let confirmation = AsyncMessageDialog::new()
        .set_title(table_mutation_dialog_title(TableMutationKind::Drop))
        .set_description(table_mutation_confirmation_description(
            TableMutationKind::Drop,
            connection_kind,
            &source,
        ))
        .set_buttons(MessageButtons::YesNo)
        .set_level(MessageLevel::Warning)
        .show()
        .await;

    if confirmation != MessageDialogResult::Yes {
        return;
    }

    let Some(connection) = session_connection(session_id) else {
        let _ = AsyncMessageDialog::new()
            .set_title(table_mutation_error_title(TableMutationKind::Drop))
            .set_description(table_mutation_connection_closed_description(
                TableMutationKind::Drop,
            ))
            .set_buttons(MessageButtons::Ok)
            .set_level(MessageLevel::Error)
            .show()
            .await;
        return;
    };

    table_mutation_inflight.set(Some(TableMutationKind::Drop));
    let result = services::drop_table(connection, source.clone()).await;
    table_mutation_inflight.set(None);

    match result {
        Ok(()) => {
            if selected_node() == selected_qualified_name {
                selected_node.set(String::new());
            }
            mark_table_deleted(tabs, session_id, source.clone());
            tree_reload += 1;
        }
        Err(err) => {
            let _ = AsyncMessageDialog::new()
                .set_title(table_mutation_error_title(TableMutationKind::Drop))
                .set_description(format!(
                    "Failed to drop {}.\n\n{}",
                    source.qualified_name, err
                ))
                .set_buttons(MessageButtons::Ok)
                .set_level(MessageLevel::Error)
                .show()
                .await;
        }
    }
}
