use super::{
    count_objects,
    disconnect_session,
    duplicate_table_modal::DuplicateTableTarget,
    highlight_match_segments,
    rename_table_modal::RenameTableTarget,
    split_children,
};
use crate::{
    app_state::{
        APP_STATE,
        activate_session,
        context_menu::{ContextMenuItem, open_confirm_dialog, open_context_menu},
        live_session_id,
    },
    screens::workspace::{
        ActionIcon,
        actions::{
            ensure_tab_for_table_preview,
            mark_table_deleted,
            mark_table_truncated,
            read_only_mode_enabled,
            run_table_preview_for_tab,
            tab_session_or_error,
        },
        components::{Chevron, IconButton, ObjectIcon, send_describe_object_request},
        context::WorkspaceAcpContext,
    },
};
use dioxus::prelude::*;
use models::{
    DatabaseKind,
    ExplorerNode,
    ExplorerNodeKind,
    ExplorerViewSettings,
    TablePreviewSource,
};

use super::super::super::tab_store::TabStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableMutationKind {
    Truncate,
    Drop,
}

#[component]
pub(super) fn ExplorerConnectionView(
    section: super::ExplorerConnectionSection,
    tree_reload: Signal<u64>,
    store: TabStore,
    selected_node: Signal<String>,
    query: String,
    view: ExplorerViewSettings,
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
                oncontextmenu: move |event| {
                    event.prevent_default();
                    event.stop_propagation();
                    let coords = event.client_coordinates();
                    let connection_menu = connection_actions_context_menu(
                        section.session_id,
                        store,
                        tree_reload,
                    );
                    open_context_menu(coords.x, coords.y, connection_menu);
                },
                button {
                    class: "tree__connection-toggle",
                    onclick: {
                        let session_id = section.session_id;
                        move |_| {
                            activate_session(session_id);
                            expanded.toggle();
                        }
                    },
                    Chevron { open: expanded() }
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
                            move |_| disconnect_session(store, session_id)
                        },
                    }
                }
            }

            if expanded() {
                div { class: "tree__connection-body",
                    if section.nodes.is_empty() {
                        p { class: "empty-state", "No objects loaded for this connection." }
                    } else if view.show_schemas {
                        for node in section.nodes {
                            ExplorerSchemaView {
                                node,
                                session_id: section.session_id,
                                tree_reload,
                                store,
                                selected_node,
                                query: query.clone(),
                                view,
                            }
                        }
                    } else {
                        // Schemas are hidden — flatten and render every schema's
                        // children directly under the connection body. This
                        // keeps object access intact without re-querying.
                        for schema in section.nodes {
                            for child in schema.children {
                                ExplorerObjectRow {
                                    node: child,
                                    session_id: section.session_id,
                                    tree_reload,
                                    store,
                                    selected_node,
                                    query: query.clone(),
                                    view,
                                }
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
    store: TabStore,
    selected_node: Signal<String>,
    query: String,
    view: ExplorerViewSettings,
) -> Element {
    let mut expanded = use_signal(|| true);
    let groups = split_children(&node.children, view.sort_alphabetical);
    let object_count = groups.total();
    let non_empty = groups.non_empty(&view);

    rsx! {
        div { class: "tree__schema",
            button {
                class: "tree__schema-toggle",
                onclick: move |_| expanded.toggle(),
                Chevron { open: expanded() }
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
                    for (title, nodes) in non_empty.into_iter() {
                        ExplorerGroupView {
                            key: "{title}",
                            title: title.to_string(),
                            session_id,
                            tree_reload,
                            nodes: nodes.clone(),
                            store,
                            selected_node,
                            query: query.clone(),
                            view,
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
    store: TabStore,
    selected_node: Signal<String>,
    query: String,
    view: ExplorerViewSettings,
) -> Element {
    rsx! {
        div { class: "tree__group",
            div { class: "tree__group-header", {title.to_string()} }
            div { class: "tree__group-items",
                for node in nodes {
                    ExplorerObjectRow {
                        node,
                        session_id,
                        tree_reload,
                        store,
                        selected_node,
                        query: query.clone(),
                        view,
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
    store: TabStore,
    mut selected_node: Signal<String>,
    query: String,
    view: ExplorerViewSettings,
) -> Element {
    let acp_ctx = use_context::<WorkspaceAcpContext>();
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
    let kind_label = node.kind.display_label();
    let preview_source_for_menu = preview_source.clone();
    let preview_source_for_click = preview_source.clone();

    rsx! {
        div {
            class: if selected {
                "tree__object-row tree__object-row--selected"
            } else {
                "tree__object-row"
            },
            oncontextmenu: move |event| {
                event.prevent_default();
                event.stop_propagation();
                let coords = event.client_coordinates();
                let mut menu_items = build_explorer_context_menu(
                    connection_name.clone(),
                    preview_source_for_menu.clone(),
                    node.kind,
                    read_only_mode_enabled(),
                    store,
                    selected_node,
                    session_id,
                    connection_kind,
                    tree_reload,
                );
                if matches!(node.kind, ExplorerNodeKind::Table | ExplorerNodeKind::View)
                    && crate::app_state::APP_AI_FEATURES_ENABLED()
                {
                    let qualified = preview_source_for_menu.qualified_name.clone();
                    let panel_state = acp_ctx.acp_panel_state;
                    let chat_revision = acp_ctx.chat_revision;
                    let allow_db_read = acp_ctx.allow_agent_db_read;
                    let label = acp_ctx.connection_label.clone();
                    menu_items.push(
                        ContextMenuItem::new("Describe with AI", move || {
                            send_describe_object_request(
                                panel_state,
                                store,
                                label.clone(),
                                chat_revision,
                                allow_db_read(),
                                qualified.clone(),
                            );
                        })
                        .with_icon(ActionIcon::Agent)
                        .separator(),
                    );
                }
                open_context_menu(coords.x, coords.y, menu_items);
            },
            button {
                class: if selected {
                    "tree__object tree__object--selected"
                } else {
                    "tree__object"
                },
                onclick: {
                    let source = preview_source_for_click.clone();
                    let qualified_name = node.qualified_name.clone();
                    move |_| {
                        selected_node.set(qualified_name.clone());
                        crate::app_state::set_explorer_selected_node(qualified_name.clone());
                        activate_session(session_id);
                        let current_id =
                            ensure_tab_for_table_preview(store, session_id, &source);
                        let current_tab = store
                            .result
                            .read()
                            .get(&current_id)
                            .cloned()
                            .map(|r| (r, store.meta.read().get(&current_id).cloned()));
                        let Some((current_tab, meta)) = current_tab else {
                            return;
                        };
                        let Some(meta) = meta else {
                            return;
                        };

                        let Some(session_id) =
                            tab_session_or_error(store, current_id, meta.session_id)
                        else {
                            return;
                        };

                        run_table_preview_for_tab(
                            store,
                            current_id,
                            session_id,
                            source.clone(),
                            0,
                            current_tab.page_size,
                        );
                    }
                },
                ondoubleclick: {
                    let source = preview_source_for_click.clone();
                    let qualified_name = node.qualified_name.clone();
                    move |_| {
                        selected_node.set(qualified_name.clone());
                        crate::app_state::set_explorer_selected_node(qualified_name.clone());
                        let current_id =
                            ensure_tab_for_table_preview(store, session_id, &source);
                        let current_tab = store
                            .result
                            .read()
                            .get(&current_id)
                            .cloned()
                            .map(|r| (r, store.meta.read().get(&current_id).cloned()));
                        let Some((current_tab, meta)) = current_tab else {
                            return;
                        };
                        let Some(meta) = meta else {
                            return;
                        };

                        let Some(session_id) =
                            tab_session_or_error(store, current_id, meta.session_id)
                        else {
                            return;
                        };

                        run_table_preview_for_tab(
                            store,
                            current_id,
                            session_id,
                            source.clone(),
                            0,
                            current_tab.page_size,
                        );
                    }
                },
                div {
                    class: "tree__object-badge tree__object-badge--{node.kind.badge_class()}",
                    ObjectIcon { kind: node.kind }
                }
                div {
                    class: "tree__object-copy",
                    div {
                        class: "tree__object-name",
                        title: "{node.qualified_name}",
                        {highlight_match_segments(&node.name, &query)}
                    }
                    if view.show_row_counts
                        && matches!(
                            node.kind,
                            ExplorerNodeKind::Table | ExplorerNodeKind::MaterializedView
                        )
                        && let Some(row_count) = node.row_count
                    {
                        span {
                            class: "tree__row-count",
                            title: "≈ {row_count} rows (estimated)",
                            "({row_count})"
                        }
                    }
                    div { class: "tree__object-kind", {kind_label.to_string()} }
                }
            }
        }
    }
}

fn should_prompt_table_mutation(
    kind: TableMutationKind,
    behavior: &models::AppBehaviorSettings,
) -> bool {
    match kind {
        TableMutationKind::Drop => behavior.confirm_before_drop,
        TableMutationKind::Truncate => behavior.confirm_before_truncate,
    }
}

fn table_mutation_dialog_title(action: TableMutationKind) -> &'static str {
    match action {
        TableMutationKind::Truncate => "Truncate table",
        TableMutationKind::Drop => "Drop table",
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

/// Composable per-object-type context-menu builder (PHASE 3).
///
/// The action *set* for each object type is defined once in
/// [`crate::app_state::actions`] (the `TABLE_ACTIONS` / `COLUMN_ACTIONS`
/// / `SCHEMA_ACTIONS` groups). This function reads the group for `kind`,
/// iterates it and realises each [`ActionId`] into a [`ContextMenuItem`]
/// via [`menu_item_for_action`]. The execute closures stay here where the
/// private signal helpers (`confirm_and_truncate_table`, etc.) live.
#[allow(clippy::too_many_arguments)]
fn build_explorer_context_menu(
    connection_name: String,
    preview_source: TablePreviewSource,
    kind: ExplorerNodeKind,
    read_only_mode: bool,
    store: TabStore,
    selected_node: Signal<String>,
    session_id: u64,
    connection_kind: DatabaseKind,
    tree_reload: Signal<u64>,
) -> Vec<ContextMenuItem> {
    use crate::app_state::actions::{self as actions, ActionId};

    // Which actions a node of this kind gets is defined by the shared
    // group; non-queryable "other" object kinds reuse the table's
    // read-ish subset (open/select/copy/ddl/refresh).
    let group: &[ActionId] = match kind {
        ExplorerNodeKind::Table => actions::TABLE_ACTIONS,
        ExplorerNodeKind::View
        | ExplorerNodeKind::MaterializedView
        | ExplorerNodeKind::Sequence
        | ExplorerNodeKind::Function
        | ExplorerNodeKind::Procedure
        | ExplorerNodeKind::Trigger => &[
            actions::ACTION_TABLE_OPEN,
            actions::ACTION_TABLE_SELECT_ALL,
            actions::ACTION_OBJECT_COPY_NAME,
            actions::ACTION_OBJECT_COPY_QUALIFIED,
            actions::ACTION_TABLE_COPY_DDL,
            actions::ACTION_OBJECT_REFRESH,
        ],
        ExplorerNodeKind::Column => actions::COLUMN_ACTIONS,
        ExplorerNodeKind::Schema => actions::SCHEMA_ACTIONS,
    };

    group
        .iter()
        .filter_map(|id| {
            menu_item_for_action(
                *id,
                &connection_name,
                &preview_source,
                kind,
                read_only_mode,
                store,
                selected_node,
                session_id,
                connection_kind,
                tree_reload,
            )
        })
        .collect()
}

/// Connection-level context menu (Disconnect / New Query / Refresh),
/// driven by the shared `CONNECTION_ACTIONS` group from the Action
/// catalog. The three items are built directly here — they need only the
/// session/tab signals, not the table-mutation helpers.
fn connection_actions_context_menu(
    session_id: u64,
    mut store: TabStore,
    mut tree_reload: Signal<u64>,
) -> Vec<ContextMenuItem> {
    use crate::app_state::actions::{self as actions};

    let mut items = Vec::new();
    for id in actions::CONNECTION_ACTIONS {
        match *id {
            actions::ACTION_CONNECTION_DISCONNECT => items.push(
                ContextMenuItem::new("Disconnect", move || {
                    disconnect_session(store, session_id);
                })
                .with_icon(ActionIcon::Close)
                .danger(),
            ),
            actions::ACTION_CONNECTION_NEW_QUERY => {
                items.push(
                    ContextMenuItem::new("New Query", move || {
                        let Some(session) = crate::app_state::APP_STATE
                            .read()
                            .session(session_id)
                            .cloned()
                        else {
                            return;
                        };
                        let tab_id = store.next_tab_id();
                        store.next_tab_id += 1;
                        let (meta, editor, result, pending) =
                            crate::screens::workspace::actions::new_query_tab(
                                tab_id,
                                session.id,
                                format!("Query {}", store.meta.read().len() + 1),
                                String::new(),
                            );
                        store.meta.with_mut(|m| {
                            m.insert(tab_id, meta);
                        });
                        store.editor.with_mut(|m| {
                            m.insert(tab_id, editor);
                        });
                        store.result.with_mut(|m| {
                            m.insert(tab_id, result);
                        });
                        store.pending.with_mut(|m| {
                            m.insert(tab_id, pending);
                        });
                        store.active_tab_id.set(tab_id);
                        crate::app_state::activate_session(session.id);
                    })
                    .with_icon(ActionIcon::SqlEditor),
                );
            }
            actions::ACTION_OBJECT_REFRESH => items.push(
                ContextMenuItem::new("Refresh", move || {
                    tree_reload += 1;
                })
                .with_icon(ActionIcon::Refresh),
            ),
            _ => {}
        }
    }
    items
}

/// Realise a single [`ActionId`] from the shared catalog into a concrete
/// [`ContextMenuItem`]. Returns `None` for actions that do not apply to
/// the given `kind` (e.g. table-only truncate on a view) so the composable
/// builder can skip them by id.
#[allow(clippy::too_many_arguments)]
fn menu_item_for_action(
    id: crate::app_state::actions::ActionId,
    connection_name: &str,
    preview_source: &TablePreviewSource,
    kind: ExplorerNodeKind,
    read_only_mode: bool,
    mut store: TabStore,
    selected_node: Signal<String>,
    session_id: u64,
    connection_kind: DatabaseKind,
    mut tree_reload: Signal<u64>,
) -> Option<ContextMenuItem> {
    use crate::app_state::context_menu::copy_to_clipboard;

    let source = preview_source.clone();
    let qualified = preview_source.qualified_name.clone();

    match id {
        crate::app_state::actions::ACTION_TABLE_OPEN if kind.is_queryable() => Some(
            ContextMenuItem::new("Open in editor", move || {
                let source = source.clone();
                let current_id = ensure_tab_for_table_preview(store, session_id, &source);
                let Some(current_tab) = store.result.read().get(&current_id).cloned() else {
                    return;
                };
                let Some(meta) = store.meta.read().get(&current_id).cloned() else {
                    return;
                };
                let Some(session_id) = tab_session_or_error(store, current_id, meta.session_id)
                else {
                    return;
                };
                run_table_preview_for_tab(
                    store,
                    current_id,
                    session_id,
                    source,
                    0,
                    current_tab.page_size,
                );
            })
            .with_icon(ActionIcon::Run),
        ),

        crate::app_state::actions::ACTION_TABLE_SELECT_ALL if kind.is_queryable() => Some(
            ContextMenuItem::new("Select all rows", move || {
                let sql = format!("SELECT * FROM {qualified}");
                crate::screens::workspace::actions::set_active_tab_sql(
                    store,
                    store.active_tab_id(),
                    sql,
                    "Loaded query from explorer".to_string(),
                );
            })
            .with_icon(ActionIcon::Details),
        ),

        crate::app_state::actions::ACTION_OBJECT_COPY_NAME => Some(
            ContextMenuItem::new("Copy name", move || {
                match copy_to_clipboard(source.table_name.clone()) {
                    Ok(()) => crate::app_state::toast_success("Copied name"),
                    Err(err) => crate::app_state::toast_error(err),
                }
            })
            .with_icon(ActionIcon::Duplicate),
        ),

        crate::app_state::actions::ACTION_OBJECT_COPY_QUALIFIED => Some(
            ContextMenuItem::new("Copy qualified name", move || {
                match copy_to_clipboard(qualified.clone()) {
                    Ok(()) => crate::app_state::toast_success("Copied qualified name"),
                    Err(err) => crate::app_state::toast_error(err),
                }
            })
            .with_icon(ActionIcon::Duplicate),
        ),

        crate::app_state::actions::ACTION_TABLE_COPY_INSERT if kind == ExplorerNodeKind::Table =>
            Some(
                ContextMenuItem::new("Copy as INSERT template", move || {
                    let _ = copy_to_clipboard(format!("INSERT INTO {qualified} VALUES (...);"));
                })
                .with_icon(ActionIcon::ExportSql),
            ),

        crate::app_state::actions::ACTION_TABLE_COPY_DDL
            if matches!(
                kind,
                ExplorerNodeKind::Table
                    | ExplorerNodeKind::View
                    | ExplorerNodeKind::MaterializedView
                    | ExplorerNodeKind::Sequence
                    | ExplorerNodeKind::Function
                    | ExplorerNodeKind::Procedure
                    | ExplorerNodeKind::Trigger
            ) =>
        {
            let node_kind = kind;
            Some(
                ContextMenuItem::new("Copy DDL", move || {
                    let source = source.clone();
                    let Some(session_id) = live_session_id(session_id) else {
                        crate::app_state::toast_error("Active connection not available");
                        return;
                    };
                    spawn(async move {
                        match services::load_object_ddl(
                            session_id,
                            source.schema.clone(),
                            source.table_name.clone(),
                            node_kind,
                        )
                        .await
                        {
                            Ok(Some(ddl)) => {
                                let _ = copy_to_clipboard(ddl.clone());
                                crate::app_state::toast_success(format!(
                                    "DDL copied ({} chars)",
                                    ddl.chars().count()
                                ));
                            }
                            Ok(None) => {
                                crate::app_state::toast_error("DDL not found for this object");
                            }
                            Err(err) => {
                                crate::app_state::toast_error(format!("Failed to load DDL: {err}"));
                            }
                        }
                    });
                })
                .with_icon(ActionIcon::ExportSql),
            )
        }

        crate::app_state::actions::ACTION_OBJECT_REFRESH => Some(
            ContextMenuItem::new("Refresh", move || {
                tree_reload += 1;
            })
            .with_icon(ActionIcon::Refresh)
            .separator(),
        ),

        crate::app_state::actions::ACTION_TABLE_DUPLICATE if kind == ExplorerNodeKind::Table => {
            let target = DuplicateTableTarget {
                session_id,
                connection_name: connection_name.to_string(),
                kind: connection_kind,
                source: preview_source.clone(),
            };
            let mut tree_reload_signal = tree_reload;
            let mut selected_node_signal = selected_node;
            let mut item = ContextMenuItem::new("Duplicate table…", move || {
                let session_id = live_session_id(target.session_id);
                let (bridge, mut rx) = crate::windows::create_duplicate_table_bridge();
                spawn(async move {
                    while let Some(result) = rx.recv().await {
                        selected_node_signal.set(result.new_qualified_name);
                        tree_reload_signal += 1;
                    }
                });
                crate::windows::open_duplicate_table_window(
                    bridge,
                    target.clone(),
                    session_id,
                    read_only_mode_enabled(),
                    crate::app_state::APP_THEME(),
                );
            })
            .with_icon(ActionIcon::Duplicate);
            if read_only_mode {
                item = item.disabled();
            }
            Some(item)
        }

        crate::app_state::actions::ACTION_TABLE_RENAME if kind == ExplorerNodeKind::Table => {
            let target = RenameTableTarget {
                session_id,
                connection_name: connection_name.to_string(),
                kind: connection_kind,
                source: preview_source.clone(),
            };
            let mut tree_reload_signal = tree_reload;
            let mut selected_node_signal = selected_node;
            let mut item = ContextMenuItem::new("Rename table…", move || {
                let session_id = live_session_id(target.session_id);
                let (bridge, mut rx) = crate::windows::create_rename_table_bridge();
                spawn(async move {
                    while let Some(result) = rx.recv().await {
                        selected_node_signal.set(result.new_qualified_name);
                        tree_reload_signal += 1;
                    }
                });
                crate::windows::open_rename_table_window(
                    bridge,
                    target.clone(),
                    session_id,
                    read_only_mode_enabled(),
                    crate::app_state::APP_THEME(),
                );
            })
            .with_icon(ActionIcon::Duplicate);
            if read_only_mode {
                item = item.disabled();
            }
            Some(item)
        }

        crate::app_state::actions::ACTION_TABLE_TRUNCATE if kind == ExplorerNodeKind::Table => {
            let source = preview_source.clone();
            let mut truncate_item = ContextMenuItem::new("Truncate table", move || {
                confirm_and_truncate_table(source.clone(), session_id, connection_kind, store);
            })
            .with_icon(ActionIcon::Truncate)
            .danger();
            if read_only_mode {
                truncate_item = truncate_item.disabled();
            }
            Some(truncate_item)
        }

        crate::app_state::actions::ACTION_TABLE_DROP if kind == ExplorerNodeKind::Table => {
            let source = preview_source.clone();
            let mut drop_item = ContextMenuItem::new("Drop table", move || {
                confirm_and_drop_table(
                    source.clone(),
                    qualified.clone(),
                    session_id,
                    connection_kind,
                    store,
                    Some(selected_node),
                    tree_reload,
                );
            })
            .with_icon(ActionIcon::Delete)
            .danger();
            if read_only_mode {
                drop_item = drop_item.disabled();
            }
            Some(drop_item)
        }

        crate::app_state::actions::ACTION_SCHEMA_CREATE_TABLE => {
            let live_id = live_session_id(session_id);
            let conn_name = connection_name.to_string();
            let target_schemas = vec![
                preview_source
                    .schema
                    .clone()
                    .unwrap_or_else(|| super::default_schema_name(connection_kind)),
            ];
            Some(ContextMenuItem::new("Create table", move || {
                let target = crate::screens::workspace::components::explorer::create_table_modal::CreateTableTarget {
                    session_id,
                    connection_name: conn_name.clone(),
                    kind: connection_kind,
                    schemas: target_schemas.clone(),
                };
                let (bridge, mut rx) = crate::windows::create_table_bridge();
                spawn(async move {
                    while rx.recv().await.is_some() {
                        tree_reload += 1;
                    }
                });
                crate::windows::open_create_table_window(
                    bridge,
                    target,
                    live_id,
                    read_only_mode,
                    crate::app_state::APP_THEME(),
                );
            })
            .with_icon(ActionIcon::CreateTable))
        }

        crate::app_state::actions::ACTION_CONNECTION_NEW_QUERY => Some(
            ContextMenuItem::new("New Query", move || {
                let Some(session) = crate::app_state::APP_STATE
                    .read()
                    .session(session_id)
                    .cloned()
                else {
                    return;
                };
                let tab_id = store.next_tab_id();
                store.next_tab_id += 1;
                let (meta, editor, result, pending) =
                    crate::screens::workspace::actions::new_query_tab(
                        tab_id,
                        session.id,
                        format!("Query {}", store.meta.read().len() + 1),
                        String::new(),
                    );
                store.meta.with_mut(|m| {
                    m.insert(tab_id, meta);
                });
                store.editor.with_mut(|m| {
                    m.insert(tab_id, editor);
                });
                store.result.with_mut(|m| {
                    m.insert(tab_id, result);
                });
                store.pending.with_mut(|m| {
                    m.insert(tab_id, pending);
                });
                store.active_tab_id.set(tab_id);
                crate::app_state::activate_session(session.id);
            })
            .with_icon(ActionIcon::SqlEditor),
        ),

        crate::app_state::actions::ACTION_CONNECTION_DISCONNECT => Some(
            ContextMenuItem::new("Disconnect", move || {
                disconnect_session(store, session_id);
            })
            .with_icon(ActionIcon::Close)
            .danger(),
        ),

        crate::app_state::actions::ACTION_COLUMN_FILTER_BY_VALUE => Some(
            ContextMenuItem::new("Filter by value", move || {
                crate::app_state::show_toast(
                    format!("Filter on {qualified}"),
                    crate::app_state::ToastKind::Info,
                );
            })
            .with_icon(ActionIcon::Filter),
        ),

        crate::app_state::actions::ACTION_COLUMN_SORT_ASC => Some(
            ContextMenuItem::new("Sort ascending", move || {
                crate::app_state::show_toast(
                    "Sort ascending (column)".to_string(),
                    crate::app_state::ToastKind::Info,
                );
            })
            .with_icon(ActionIcon::Previous),
        ),

        crate::app_state::actions::ACTION_COLUMN_SORT_DESC => Some(
            ContextMenuItem::new("Sort descending", move || {
                crate::app_state::show_toast(
                    "Sort descending (column)".to_string(),
                    crate::app_state::ToastKind::Info,
                );
            })
            .with_icon(ActionIcon::Next),
        ),

        // Unknown / non-matching guards fall through to no item.
        _ => None,
    }
}

pub(super) fn confirm_and_truncate_table(
    source: TablePreviewSource,
    session_id: u64,
    connection_kind: DatabaseKind,
    store: TabStore,
) {
    let run = {
        let source = source.clone();
        move || {
            let source = source.clone();
            spawn(async move {
                if live_session_id(session_id).is_none() {
                    crate::app_state::toast_error(table_mutation_connection_closed_description(
                        TableMutationKind::Truncate,
                    ));
                    return;
                }
                match services::truncate_table(session_id, source.clone()).await {
                    Ok(()) => {
                        mark_table_truncated(store, session_id, source.clone());
                        crate::app_state::toast_success(format!("Truncated {}", source.table_name));
                    }
                    Err(err) => {
                        crate::app_state::toast_error(format!(
                            "Failed to truncate {}: {err}",
                            source.qualified_name
                        ));
                    }
                }
            });
        }
    };
    let behavior = crate::app_state::APP_APP_BEHAVIOR.peek().clone();
    if should_prompt_table_mutation(TableMutationKind::Truncate, &behavior) {
        open_confirm_dialog(
            table_mutation_dialog_title(TableMutationKind::Truncate),
            table_mutation_confirmation_description(
                TableMutationKind::Truncate,
                connection_kind,
                &source,
            ),
            "Truncate",
            true,
            run,
        );
    } else {
        run();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn confirm_and_drop_table(
    source: TablePreviewSource,
    selected_qualified_name: String,
    session_id: u64,
    connection_kind: DatabaseKind,
    store: TabStore,
    local_selected_node: Option<Signal<String>>,
    mut tree_reload: Signal<u64>,
) {
    let run = {
        let source = source.clone();
        let selected_qualified_name = selected_qualified_name.clone();
        move || {
            let source = source.clone();
            let selected_qualified_name = selected_qualified_name.clone();
            spawn(async move {
                if live_session_id(session_id).is_none() {
                    crate::app_state::toast_error(table_mutation_connection_closed_description(
                        TableMutationKind::Drop,
                    ));
                    return;
                }
                match services::drop_table(session_id, source.clone()).await {
                    Ok(()) => {
                        if let Some(mut local_selected_node) = local_selected_node
                            && local_selected_node() == selected_qualified_name
                        {
                            local_selected_node.set(String::new());
                            crate::app_state::set_explorer_selected_node(String::new());
                        }
                        mark_table_deleted(store, session_id, source.clone());
                        tree_reload += 1;
                        crate::app_state::toast_success(format!("Dropped {}", source.table_name));
                    }
                    Err(err) => {
                        crate::app_state::toast_error(format!(
                            "Failed to drop {}: {err}",
                            source.qualified_name
                        ));
                    }
                }
            });
        }
    };
    let behavior = crate::app_state::APP_APP_BEHAVIOR.peek().clone();
    if should_prompt_table_mutation(TableMutationKind::Drop, &behavior) {
        open_confirm_dialog(
            table_mutation_dialog_title(TableMutationKind::Drop),
            table_mutation_confirmation_description(
                TableMutationKind::Drop,
                connection_kind,
                &source,
            ),
            "Drop",
            true,
            run,
        );
    } else {
        run();
    }
}

#[cfg(test)]
mod tests {
    use super::{TableMutationKind, should_prompt_table_mutation};

    #[test]
    fn should_prompt_table_mutation_honors_flags() {
        let mut behavior = models::AppBehaviorSettings::default();
        assert!(should_prompt_table_mutation(
            TableMutationKind::Drop,
            &behavior
        ));
        assert!(should_prompt_table_mutation(
            TableMutationKind::Truncate,
            &behavior
        ));
        behavior.confirm_before_drop = false;
        assert!(!should_prompt_table_mutation(
            TableMutationKind::Drop,
            &behavior
        ));
        assert!(should_prompt_table_mutation(
            TableMutationKind::Truncate,
            &behavior
        ));
    }
}
