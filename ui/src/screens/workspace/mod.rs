mod actions;
mod chat;
pub(crate) mod components;
mod context;
pub mod helpers;
mod hooks;
mod tab_store;

use crate::{
    app_state::{
        APP_AI_FEATURES_ENABLED,
        APP_BOTTOM_PANEL_HEIGHT,
        APP_COMMAND_REQUEST,
        APP_COMMAND_REQUEST_KIND,
        APP_GLOBAL_SEARCH_OBJECTS,
        APP_GLOBAL_SEARCH_REQUEST,
        APP_GLOBAL_SEARCH_REQUEST_KIND,
        APP_GLOBAL_SEARCH_REQUEST_PAYLOAD,
        APP_SHOW_AGENT_PANEL,
        APP_SHOW_BOTTOM_PANEL,
        APP_SHOW_CONNECTIONS,
        APP_SHOW_EXPLORER,
        APP_SHOW_HISTORY,
        APP_SHOW_SAVED_QUERIES,
        APP_SHOW_SQL_EDITOR,
        APP_SPLIT_MODE,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        APP_THEME,
        APP_UI_SETTINGS,
        ToastKind,
        actions as actions_state,
        close_global_search,
        commands::{
            CMD_CLOSE_TAB,
            CMD_EXPLAIN_QUERY,
            CMD_FORMAT_SQL,
            CMD_NEW_TAB,
            CMD_NEXT_TAB,
            CMD_REFRESH_EXPLORER,
            CMD_RUN_QUERY,
            CMD_SAVE_QUERY,
        },
        context_menu,
        context_menu::{ContextMenuItem, open_context_menu},
        global_search::{
            GLOBAL_SEARCH_OPEN_OBJECT,
            GLOBAL_SEARCH_OPEN_TAB,
            GLOBAL_SEARCH_RUN_ACTION,
            GlobalSearchObjectItem,
            GlobalSearchTabItem,
        },
        keyboard::{ShortcutAction, match_key_combination},
        open_connection_screen,
        open_global_search_with_snapshots,
        request_focus_agent_composer,
        request_focus_editor,
        request_focus_filter_panel,
        set_bottom_panel_height,
        set_show_agent_panel,
        set_show_bottom_panel,
        set_show_connections,
        set_show_explorer,
        set_show_history,
        set_show_saved_queries,
        set_show_sql_editor,
        set_split_mode,
        show_toast,
        update_ui_settings,
    },
    windows,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use models::{
    AcpPanelState,
    ChatThreadSummary,
    ExplorerNode,
    QueryHistoryItem,
    SavedQuery,
    TablePreviewSource,
    WorkspaceSplitMode,
    WorkspaceToolDock,
    WorkspaceToolPanel,
};

use self::{
    chat::{create_chat_thread, delete_chat_thread, select_chat_thread},
    components::{
        AcpAgentPanel,
        BottomPanelDock,
        BottomPanelTab,
        IconButton,
        QueryHistoryPanel,
        SavedQueriesPanel,
        SessionRail,
        SidebarConnectionTree,
        TabsManager,
    },
    helpers::{
        BOTTOM_PANEL_MAX_HEIGHT,
        BOTTOM_PANEL_MIN_HEIGHT,
        DockDropTarget,
        INSPECTOR_MAX_WIDTH,
        INSPECTOR_MIN_WIDTH,
        SIDEBAR_MAX_WIDTH,
        SIDEBAR_MIN_WIDTH,
        WORKSPACE_ROOT_ID,
        apply_tool_panel_drop,
        should_render_explorer_status,
        tool_panel_class,
        visible_tool_panels,
        workspace_resize_script,
        workspace_vertical_resize_script,
    },
    hooks::{
        AcpState,
        AcpStateInputs,
        ChatState,
        ExplorerState,
        QueryTabsState,
        use_acp_state,
        use_chat_state,
        use_explorer_state,
        use_query_tabs,
    },
    tab_store::TabStore,
};

// Re-export for app_state
pub use crate::screens::workspace::components::ExplorerConnectionSection;
// Re-export for context_menu (and any sibling that needs the icon enum
// without going through the private `components::icon_button` path).
pub(crate) use crate::screens::workspace::components::ActionIcon;

#[component]
fn WorkspaceDropSlot(
    dock: WorkspaceToolDock,
    index: usize,
    empty: bool,
    dragging_panel: Signal<Option<WorkspaceToolPanel>>,
    mut drop_target: Signal<Option<DockDropTarget>>,
) -> Element {
    let target = DockDropTarget { dock, index };
    let mut class_name = "workspace__dock-dropzone".to_string();
    if empty {
        class_name.push_str(" workspace__dock-dropzone--empty");
    }
    if drop_target() == Some(target) {
        class_name.push_str(" workspace__dock-dropzone--active");
    }

    rsx! {
        div {
            class: class_name,
            onmousemove: move |event| {
                if dragging_panel().is_none() {
                    return;
                }

                if event.held_buttons().is_empty() {
                    return;
                }

                if drop_target() != Some(target) {
                    drop_target.set(Some(target));
                }
            },
            if empty {
                span { class: "workspace__dock-dropzone-copy", "Drop panel here" }
            }
        }
    }
}

#[component]
fn ExplorerToolPanel(
    tree_status: Signal<String>,
    tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    tree_reload: Signal<u64>,
    store: TabStore,
) -> Element {
    rsx! {
        div {
            class: "workspace__panel",
            div {
                class: "workspace__panel-header",
                div {
                    class: "workspace__panel-header-row",
                    h2 { class: "workspace__section-title", "Explorer" }
                    IconButton {
                        icon: ActionIcon::Refresh,
                        label: "Refresh connections".to_string(),
                        small: true,
                        onclick: move |_| tree_reload += 1,
                    }
                }
                if should_render_explorer_status(&tree_status()) {
                    p { class: "workspace__hint", "{tree_status()}" }
                }
            }
            SidebarConnectionTree {
                sections: tree_sections(),
                tree_reload,
                store,
            }
        }
    }
}

#[component]
fn AgentToolPanel(
    mut acp_panel_state: Signal<AcpPanelState>,
    store: TabStore,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Signal<Vec<ChatThreadSummary>>,
    mut active_chat_thread_id: Signal<Option<i64>>,
    connection_label: String,
) -> Element {
    let active_chat_thread = use_memo(move || {
        chat_threads
            .read()
            .iter()
            .find(|thread| Some(thread.id) == active_chat_thread_id())
            .cloned()
    });
    let thread_title = active_chat_thread
        .read()
        .as_ref()
        .map(|thread| thread.title.clone())
        .unwrap_or_else(|| "New chat".to_string());
    let thread_connection_name = active_chat_thread
        .read()
        .as_ref()
        .map(|thread| thread.connection_name.clone())
        .unwrap_or_else(|| connection_label.clone());
    let new_thread_connection = connection_label.clone();
    let delete_thread_connection = connection_label.clone();
    let sql_connection_label = connection_label.clone();

    rsx! {
        AcpAgentPanel {
            panel_state: acp_panel_state,
            store,
            chat_revision,
            allow_agent_db_read,
            allow_agent_read_sql_run,
            allow_agent_write_sql_run,
            allow_agent_tool_run,
            chat_threads: chat_threads(),
            active_thread_id: active_chat_thread_id(),
            thread_title,
            thread_connection_name,
            sql_connection_label,
            on_new_thread: move |_| {
                create_chat_thread(
                    chat_threads,
                    active_chat_thread_id,
                    new_thread_connection.clone(),
                );
            },
            on_select_thread: move |thread_id| {
                select_chat_thread(active_chat_thread_id, thread_id);
            },
            on_delete_thread: move |thread_id| {
                delete_chat_thread(
                    chat_threads,
                    active_chat_thread_id,
                    delete_thread_connection.clone(),
                    thread_id,
                );
            },
        }
    }
}

#[component]
fn WorkspacePanelContent(
    panel: WorkspaceToolPanel,
    tree_status: Signal<String>,
    tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    tree_reload: Signal<u64>,
    store: TabStore,
    history: Signal<Vec<QueryHistoryItem>>,
    saved_queries: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Signal<Vec<ChatThreadSummary>>,
    active_chat_thread_id: Signal<Option<i64>>,
    connection_label: String,
) -> Element {
    match panel {
        WorkspaceToolPanel::Connections => rsx! {
            div {
                class: "workspace__panel",
                SessionRail {
                    store,
                }
            }
        },
        WorkspaceToolPanel::Explorer => rsx! {
            ExplorerToolPanel {
                tree_status,
                tree_sections,
                tree_reload,
                store,
            }
        },
        WorkspaceToolPanel::SavedQueries => rsx! {
            SavedQueriesPanel {
                saved_queries: saved_queries(),
                saved_queries_signal: saved_queries,
                next_saved_query_id,
                store,
            }
        },
        WorkspaceToolPanel::History => rsx! {
            div {
                class: "workspace__panel workspace__panel--history",
                QueryHistoryPanel {
                    history,
                    store,
                }
            }
        },
        WorkspaceToolPanel::Agent => rsx! {
            AgentToolPanel {
                acp_panel_state,
                store,
                chat_revision,
                allow_agent_db_read,
                allow_agent_read_sql_run,
                allow_agent_write_sql_run,
                allow_agent_tool_run,
                chat_threads,
                active_chat_thread_id,
                connection_label,
            }
        },
    }
}

#[component]
fn WorkspaceDockPanel(
    panel: WorkspaceToolPanel,
    dock: WorkspaceToolDock,
    index: usize,
    dragging_panel: Signal<Option<WorkspaceToolPanel>>,
    mut drop_target: Signal<Option<DockDropTarget>>,
    tree_status: Signal<String>,
    tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    tree_reload: Signal<u64>,
    store: TabStore,
    history: Signal<Vec<QueryHistoryItem>>,
    saved_queries: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Signal<Vec<ChatThreadSummary>>,
    active_chat_thread_id: Signal<Option<i64>>,
    connection_label: String,
) -> Element {
    let target = DockDropTarget { dock, index };
    let mut class_name = "workspace__tool-panel".to_string();
    class_name.push_str(tool_panel_class(panel));
    if dragging_panel() == Some(panel) {
        class_name.push_str(" workspace__tool-panel--dragging");
    }
    if drop_target() == Some(target) {
        class_name.push_str(" workspace__tool-panel--drop-target");
    }

    rsx! {
        div {
            key: "{panel.label()}",
            class: class_name,
            onmousemove: move |event| {
                if dragging_panel().is_none() {
                    return;
                }

                if event.held_buttons().is_empty() {
                    return;
                }

                if drop_target() != Some(target) {
                    drop_target.set(Some(target));
                }
            },
            div {
                class: "workspace__tool-panel-grip",
                title: format!("Drag {} panel", panel.label()),
                onmousedown: move |event| {
                    if event.trigger_button() != Some(MouseButton::Primary) {
                        return;
                    }

                    event.prevent_default();
                    event.stop_propagation();
                    dragging_panel.set(Some(panel));
                    drop_target.set(None);
                },
                span { class: "workspace__tool-panel-grip-dots" }
            }
            WorkspacePanelContent {
                panel,
                tree_status,
                tree_sections,
                tree_reload,
                store,
                history,
                saved_queries,
                next_saved_query_id,
                acp_panel_state,
                chat_revision,
                allow_agent_db_read,
                allow_agent_read_sql_run,
                allow_agent_write_sql_run,
                allow_agent_tool_run,
                chat_threads,
                active_chat_thread_id,
                connection_label,
            }
        }
    }
}

#[component]
fn WorkspaceDock(
    dock: WorkspaceToolDock,
    panels: Vec<WorkspaceToolPanel>,
    dragging_panel: Signal<Option<WorkspaceToolPanel>>,
    drop_target: Signal<Option<DockDropTarget>>,
    tree_status: Signal<String>,
    tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    tree_reload: Signal<u64>,
    store: TabStore,
    history: Signal<Vec<QueryHistoryItem>>,
    saved_queries: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Signal<Vec<ChatThreadSummary>>,
    active_chat_thread_id: Signal<Option<i64>>,
    connection_label: String,
) -> Element {
    rsx! {
        if panels.is_empty() {
            WorkspaceDropSlot {
                dock,
                index: 0,
                empty: true,
                dragging_panel,
                drop_target,
            }
        } else {
            for (index, panel) in panels.iter().copied().enumerate() {
                WorkspaceDropSlot {
                    dock,
                    index,
                    empty: false,
                    dragging_panel,
                    drop_target,
                }
                WorkspaceDockPanel {
                    panel,
                    dock,
                    index,
                    dragging_panel,
                    drop_target,
                    tree_status,
                    tree_sections,
                    tree_reload,
                    store,
                    history,
                    saved_queries,
                    next_saved_query_id,
                    acp_panel_state,
                    chat_revision,
                    allow_agent_db_read,
                    allow_agent_read_sql_run,
                    allow_agent_write_sql_run,
                    allow_agent_tool_run,
                    chat_threads,
                    active_chat_thread_id,
                    connection_label: connection_label.clone(),
                }
            }
            WorkspaceDropSlot {
                dock,
                index: panels.len(),
                empty: false,
                dragging_panel,
                drop_target,
            }
        }
    }
}

#[component]
fn WorkspaceBody(
    show_sidebar: bool,
    show_inspector: bool,
    show_bottom_panel: bool,
    sidebar_panels: Vec<WorkspaceToolPanel>,
    inspector_panels: Vec<WorkspaceToolPanel>,
    sidebar_width: Signal<f64>,
    mut sidebar_resize_active: Signal<bool>,
    inspector_width: Signal<f64>,
    mut inspector_resize_active: Signal<bool>,
    bottom_panel_height: Signal<f64>,
    mut bottom_resize_active: Signal<bool>,
    mut bottom_active_tab: Signal<BottomPanelTab>,
    store: TabStore,
    history: Signal<Vec<QueryHistoryItem>>,
    next_history_id: Signal<u64>,
    saved_queries: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    tree_status: Signal<String>,
    tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    show_saved_queries: bool,
    show_connections: bool,
    show_explorer: bool,
    ai_features_enabled: bool,
    show_agent_panel: bool,
    show_history: bool,
    tree_reload: Signal<u64>,
    dragging_panel: Signal<Option<WorkspaceToolPanel>>,
    drop_target: Signal<Option<DockDropTarget>>,
    acp_panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Signal<Vec<ChatThreadSummary>>,
    active_chat_thread_id: Signal<Option<i64>>,
    connection_label: String,
) -> Element {
    rsx! {
        div {
            class: "workspace__top-row",
        if show_sidebar {
            aside {
                class: "workspace__sidebar",
                div {
                    class: "workspace__sidebar-body",
                    WorkspaceDock {
                        dock: WorkspaceToolDock::Sidebar,
                        panels: sidebar_panels.clone(),
                        dragging_panel,
                        drop_target,
                        tree_status,
                        tree_sections,
                        tree_reload,
                        store,
                        history,
                        saved_queries,
                        next_saved_query_id,
                        acp_panel_state,
                        chat_revision,
                        allow_agent_db_read,
                        allow_agent_read_sql_run,
                        allow_agent_write_sql_run,
                        allow_agent_tool_run,
                        chat_threads,
                        active_chat_thread_id,
                        connection_label: connection_label.clone(),
                    }
                }
            }
            div {
                class: if sidebar_resize_active() {
                    "workspace__resize-handle workspace__resize-handle--active"
                } else {
                    "workspace__resize-handle"
                },
                onmousedown: move |event| {
                    if event.trigger_button() != Some(MouseButton::Primary) {
                        return;
                    }

                    event.prevent_default();
                    event.stop_propagation();

                    let start_x = event.client_coordinates().x;
                    let start_width = sidebar_width();
                    sidebar_resize_active.set(true);
                    spawn(async move {
                        let result = document::eval(&workspace_resize_script(
                            "--workspace-sidebar-width",
                            start_x,
                            start_width,
                            SIDEBAR_MIN_WIDTH,
                            SIDEBAR_MAX_WIDTH,
                            false,
                        ))
                        .join::<f64>()
                        .await;

                        match result {
                            Ok(width) => sidebar_width.set(width),
                            Err(err) => {
                                eprintln!("Failed to resize workspace sidebar: {err:?}");
                            }
                        }

                        sidebar_resize_active.set(false);
                    });
                }
            }
        }
        section {
            class: "workspace__main",
            header {
                class: "workspace__header",
                div {
                    class: "workspace__toolbar",
                    IconButton {
                        icon: ActionIcon::ViewMenu,
                        label: "View panels".to_string(),
                        // Stay "active" while any panel addressed by
                        // the menu is visible, so the button doubles
                        // as a status indicator.
                        active: show_saved_queries
                            || show_connections
                            || show_explorer
                            || show_history
                            || APP_SHOW_SQL_EDITOR()
                            || (ai_features_enabled && show_agent_panel)
                            || APP_SHOW_BOTTOM_PANEL()
                            || !matches!(APP_SPLIT_MODE(), WorkspaceSplitMode::Off),
                        small: true,
                        onclick: move |event: MouseEvent| {
                            // Menu items are snapshotted into the
                            // global signal as plain data; rebuild
                            // every open so labels reflect the live
                            // signals.
                            let mut items: Vec<ContextMenuItem> = vec![
                                ContextMenuItem::new(
                                    if APP_SHOW_SAVED_QUERIES() {
                                        "Hide saved queries"
                                    } else {
                                        "Show saved queries"
                                    },
                                    move || set_show_saved_queries(!APP_SHOW_SAVED_QUERIES()),
                                )
                                .with_icon(ActionIcon::SavedQueries)
                                .active(APP_SHOW_SAVED_QUERIES()),
                                ContextMenuItem::new(
                                    if APP_SHOW_CONNECTIONS() {
                                        "Hide connections"
                                    } else {
                                        "Show connections"
                                    },
                                    move || set_show_connections(!APP_SHOW_CONNECTIONS()),
                                )
                                .with_icon(ActionIcon::Connections)
                                .active(APP_SHOW_CONNECTIONS()),
                                ContextMenuItem::new(
                                    if APP_SHOW_EXPLORER() {
                                        "Hide explorer"
                                    } else {
                                        "Show explorer"
                                    },
                                    move || set_show_explorer(!APP_SHOW_EXPLORER()),
                                )
                                .with_icon(ActionIcon::Explorer)
                                .active(APP_SHOW_EXPLORER()),
                                ContextMenuItem::new(
                                    if APP_SHOW_HISTORY() {
                                        "Hide history"
                                    } else {
                                        "Show history"
                                    },
                                    move || set_show_history(!APP_SHOW_HISTORY()),
                                )
                                .with_icon(ActionIcon::History)
                                .active(APP_SHOW_HISTORY()),
                                ContextMenuItem::new(
                                    if APP_SHOW_SQL_EDITOR() {
                                        "Hide SQL editor"
                                    } else {
                                        "Show SQL editor"
                                    },
                                    move || set_show_sql_editor(!APP_SHOW_SQL_EDITOR()),
                                )
                                .with_icon(ActionIcon::SqlEditor)
                                .active(APP_SHOW_SQL_EDITOR()),
                            ];
                            if ai_features_enabled {
                                items.push(
                                    ContextMenuItem::new(
                                        if APP_SHOW_AGENT_PANEL() {
                                            "Hide agent panel"
                                        } else {
                                            "Show agent panel"
                                        },
                                        move || {
                                            set_show_agent_panel(!APP_SHOW_AGENT_PANEL())
                                        },
                                    )
                                    .with_icon(ActionIcon::Agent)
                                    .active(APP_SHOW_AGENT_PANEL()),
                                );
                            }
                            items.push(
                                ContextMenuItem::new(
                                    if APP_SHOW_BOTTOM_PANEL() {
                                        "Hide bottom dock"
                                    } else {
                                        "Show bottom dock"
                                    },
                                    move || {
                                        set_show_bottom_panel(!APP_SHOW_BOTTOM_PANEL())
                                    },
                                )
                                .with_icon(ActionIcon::Output)
                                .active(APP_SHOW_BOTTOM_PANEL())
                                .separator(),
                            );
                            items.push(
                                ContextMenuItem::new(
                                    format!("Editor layout: {}", APP_SPLIT_MODE().label()),
                                    move || set_split_mode(APP_SPLIT_MODE().next()),
                                )
                                .with_icon(ActionIcon::Split)
                                .active(!matches!(APP_SPLIT_MODE(), WorkspaceSplitMode::Off)),
                            );

                            let coords = event.client_coordinates();
                            open_context_menu(coords.x, coords.y, items);
                        },
                    }
                    IconButton {
                        icon: ActionIcon::Refresh,
                        label: "Refresh explorer".to_string(),
                        small: true,
                        onclick: move |_| tree_reload += 1,
                    }
                    IconButton {
                        icon: ActionIcon::Details,
                        label: "ER diagram".to_string(),
                        small: true,
                        onclick: move |_| {
                            let sections = tree_sections();
                            let connection =
                                APP_STATE.read().active_session().map(|s| s.connection.clone());
                            let Some(connection) = connection else {
                                return;
                            };
                            // Load foreign keys first so the window opens with
                            // the full diagram (tables + relationship lines).
                            // Each click opens a brand new OS window.
                            spawn(async move {
                                let fks = services::load_foreign_keys(connection)
                                    .await
                                    .unwrap_or_default();
                                if let Some(diagram) =
                                    helpers::build_er_diagram_async(sections, fks).await
                                {
                                    windows::open_er_diagram_window(diagram, APP_THEME());
                                }
                            });
                        },
                    }
                    IconButton {
                        icon: ActionIcon::NewConnection,
                        label: "New connection".to_string(),
                        primary: true,
                        small: true,
                        onclick: move |_| open_connection_screen(),
                    }
                }
            }
            div {
                class: if show_inspector {
                    "workspace__content workspace__content--with-inspector"
                } else {
                    "workspace__content"
                },
                div {
                    class: "workspace__canvas",
                    TabsManager {
                        store,
                        history,
                        next_history_id,
                        explorer_sections: tree_sections,
                        acp_panel_state,
                        chat_revision,
                        allow_agent_db_read,
                    }
                }
                if show_inspector {
                    div {
                        class: if inspector_resize_active() {
                            "workspace__resize-handle workspace__resize-handle--inspector workspace__resize-handle--active"
                        } else {
                            "workspace__resize-handle workspace__resize-handle--inspector"
                        },
                        onmousedown: move |event| {
                            if event.trigger_button() != Some(MouseButton::Primary) {
                                return;
                            }

                            event.prevent_default();
                            event.stop_propagation();

                            let start_x = event.client_coordinates().x;
                            let start_width = inspector_width();
                            inspector_resize_active.set(true);
                            spawn(async move {
                                let result = document::eval(&workspace_resize_script(
                                    "--workspace-inspector-width",
                                    start_x,
                                    start_width,
                                    INSPECTOR_MIN_WIDTH,
                                    INSPECTOR_MAX_WIDTH,
                                    true,
                                ))
                                .join::<f64>()
                                .await;

                                match result {
                                    Ok(width) => inspector_width.set(width),
                                    Err(err) => {
                                        eprintln!(
                                            "Failed to resize workspace inspector: {err:?}"
                                        );
                                    }
                                }

                                inspector_resize_active.set(false);
                            });
                        }
                    }
                    aside {
                        class: "workspace__inspector",
                        WorkspaceDock {
                            dock: WorkspaceToolDock::Inspector,
                            panels: inspector_panels,
                            dragging_panel,
                            drop_target,
                            tree_status,
                            tree_sections,
                            tree_reload,
                            store,
                            history,
                            saved_queries,
                            next_saved_query_id,
                            acp_panel_state,
                            chat_revision,
                            allow_agent_db_read,
                            allow_agent_read_sql_run,
                            allow_agent_write_sql_run,
                            allow_agent_tool_run,
                            chat_threads,
                            active_chat_thread_id,
                            connection_label: connection_label.clone(),
                        }
                    }
                }
            }
        }
        }
        if show_bottom_panel {
            div {
                class: if bottom_resize_active() {
                    "workspace__resize-handle workspace__resize-handle--bottom workspace__resize-handle--active"
                } else {
                    "workspace__resize-handle workspace__resize-handle--bottom"
                },
                onmousedown: move |event| {
                    if event.trigger_button() != Some(MouseButton::Primary) {
                        return;
                    }

                    event.prevent_default();
                    event.stop_propagation();

                    let start_y = event.client_coordinates().y;
                    let start_height = bottom_panel_height();
                    bottom_resize_active.set(true);
                    spawn(async move {
                        let result = document::eval(&workspace_vertical_resize_script(
                            "--workspace-bottom-panel-height",
                            start_y,
                            start_height,
                            BOTTOM_PANEL_MIN_HEIGHT,
                            BOTTOM_PANEL_MAX_HEIGHT,
                        ))
                        .join::<f64>()
                        .await;

                        match result {
                            Ok(height) => {
                                bottom_panel_height.set(height);
                                set_bottom_panel_height(height);
                            }
                            Err(err) => {
                                eprintln!("Failed to resize workspace bottom panel: {err:?}");
                            }
                        }

                        bottom_resize_active.set(false);
                    });
                }
            }
            BottomPanelDock {
                history,
                active_tab: bottom_active_tab,
            }
        }
    }
}

#[component]
pub fn Workspace() -> Element {
    let active_session = { APP_STATE.read().active_session().cloned() };
    let connection_label = active_session
        .as_ref()
        .map(|session| session.name.clone())
        .unwrap_or_else(|| "No connection".to_string());
    let show_history = APP_SHOW_HISTORY();

    // ── Layout signals (owned by Workspace) ────────────────────────
    let sidebar_width = use_signal(|| 320.0);
    let sidebar_resize_active = use_signal(|| false);
    let inspector_width = use_signal(|| 360.0);
    let inspector_resize_active = use_signal(|| false);
    let mut dragging_panel = use_signal(|| None::<WorkspaceToolPanel>);
    let mut drop_target = use_signal(|| None::<DockDropTarget>);
    // Bottom dock (Output / Messages / Query Log / Transactions / Problems).
    // The height signal mirrors APP_BOTTOM_PANEL_HEIGHT on first render so
    // the dock restores to the user's last size; the active tab lives in
    // memory only — switching tabs is cheap and not worth persisting.
    #[allow(clippy::redundant_closure)]
    let bottom_panel_height = use_signal(|| APP_BOTTOM_PANEL_HEIGHT());
    let bottom_resize_active = use_signal(|| false);
    let bottom_active_tab = use_signal(|| BottomPanelTab::Output);

    // ── Custom hooks ───────────────────────────────────────────────
    let ExplorerState {
        tree_status,
        tree_sections,
        mut tree_reload,
    } = use_explorer_state();

    let QueryTabsState { store } = use_query_tabs();
    let active_tab_id = store.active_tab_id;
    let next_tab_id = store.next_tab_id;

    let ChatState {
        chat_threads,
        active_chat_thread_id,
        chat_revision,
        history,
        next_history_id,
        saved_queries,
        next_saved_query_id,
        ..
    } = use_chat_state(connection_label.clone());

    let AcpState {
        acp_panel_state,
        allow_agent_db_read,
        allow_agent_read_sql_run,
        allow_agent_write_sql_run,
        allow_agent_tool_run,
        ..
    } = use_acp_state(AcpStateInputs {
        chat_threads,
        active_chat_thread_id,
        chat_revision,
        store,
        connection_label: connection_label.clone(),
    });

    context::provide_workspace_tab_context(store, active_tab_id, next_tab_id);
    context::provide_workspace_query_context(
        history,
        next_history_id,
        saved_queries,
        next_saved_query_id,
    );
    context::provide_workspace_acp_context(context::WorkspaceAcpContext {
        acp_panel_state,
        chat_revision,
        allow_agent_db_read,
        allow_agent_read_sql_run,
        allow_agent_write_sql_run,
        allow_agent_tool_run,
        chat_threads,
        active_chat_thread_id,
        connection_label: connection_label.clone(),
    });

    // ── Effect: normalize panel layout ─────────────────────────────
    use_effect(move || {
        let settings = APP_UI_SETTINGS();
        let normalized = settings.tool_panel_layout.normalized();
        if settings.tool_panel_layout != normalized {
            update_ui_settings(|current| {
                current.tool_panel_layout = normalized;
            });
        }
    });

    // ── Effect: dispatch command-palette requests ─────────────────
    // The palette lives outside the workspace tree (mounted in
    // `app.rs` next to `ContextMenu`) and can only reach workspace-
    // local state by bumping a global counter. We watch the counter
    // here and realise the request against `tabs`, `active_tab_id`,
    // `history`, etc. The discriminator (`APP_COMMAND_REQUEST_KIND`)
    // selects which action to run; any unknown id is logged and
    // dropped so an out-of-sync palette can never silently no-op.
    use_effect(move || {
        let _ = APP_COMMAND_REQUEST();
        let kind = APP_COMMAND_REQUEST_KIND();
        let mut store = store;
        match kind {
            x if x == CMD_NEW_TAB.0 => {
                if let Some(session) = APP_STATE.read().active_session().cloned() {
                    let tab_id = store.next_tab_id();
                    store.next_tab_id += 1;
                    let (meta, editor, result, pending) = actions::new_query_tab(
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
                }
            }
            x if x == CMD_CLOSE_TAB.0 =>
                if store.meta.read().len() > 1 {
                    let current_id = store.active_tab_id();
                    store.meta.with_mut(|m| {
                        m.remove(&current_id);
                    });
                    store.editor.with_mut(|m| {
                        m.remove(&current_id);
                    });
                    store.result.with_mut(|m| {
                        m.remove(&current_id);
                    });
                    store.pending.with_mut(|m| {
                        m.remove(&current_id);
                    });
                    if let Some((first_id, first_meta)) = store.meta.read().iter().next() {
                        let first_id = *first_id;
                        let session_id = first_meta.session_id;
                        store.active_tab_id.set(first_id);
                        crate::app_state::activate_session(session_id);
                    }
                },
            x if x == CMD_NEXT_TAB.0 => {
                let all_tabs: Vec<u64> = store.meta.read().keys().copied().collect();
                if all_tabs.len() > 1 {
                    let current_idx = all_tabs.iter().position(|id| *id == store.active_tab_id());
                    if let Some(idx) = current_idx {
                        let next_idx = (idx + 1) % all_tabs.len();
                        let next_id = all_tabs[next_idx];
                        let session_id = store
                            .meta
                            .read()
                            .get(&next_id)
                            .map(|m| m.session_id)
                            .unwrap_or(0);
                        store.active_tab_id.set(next_id);
                        crate::app_state::activate_session(session_id);
                    }
                }
            }
            x if x == CMD_REFRESH_EXPLORER.0 => {
                tree_reload += 1;
            }
            x if x == CMD_RUN_QUERY.0 => {
                actions::run_active_tab(store, store.active_tab_id(), (history, next_history_id));
            }
            x if x == CMD_FORMAT_SQL.0 => {
                actions::format_active_tab(store, store.active_tab_id(), APP_SQL_FORMAT_SETTINGS());
            }
            x if x == CMD_EXPLAIN_QUERY.0 => {
                actions::run_active_tab_explain(store, store.active_tab_id());
            }
            x if x == CMD_SAVE_QUERY.0 => {
                let status = actions::save_active_tab_as_saved_query(
                    store,
                    store.active_tab_id(),
                    saved_queries,
                    next_saved_query_id,
                );
                show_save_status_toast(&status);
            }
            _ => {
                eprintln!("workspace: unknown command request kind {kind}");
            }
        }
    });

    // ── Effect: dispatch global-search picks ───────────────────
    // The Ctrl+K overlay (mounted in `app.rs` outside the workspace
    // tree) bumps `APP_GLOBAL_SEARCH_REQUEST` when the user picks a
    // result. We watch the counter here and realise the pick against
    // the live tab/active_tab_id/explorer signals. The discriminator
    // tells us which kind of result it was; the payload is a u64 that
    // means tab_id for tab hits, snapshot index for object hits, and
    // action id for action hits.
    use_effect(move || {
        let _ = APP_GLOBAL_SEARCH_REQUEST();
        let kind = APP_GLOBAL_SEARCH_REQUEST_KIND();
        let payload = APP_GLOBAL_SEARCH_REQUEST_PAYLOAD();
        let mut store = store;
        match kind {
            x if x == GLOBAL_SEARCH_OPEN_TAB => {
                if let Some(meta) = store.meta.read().get(&payload).cloned() {
                    store.active_tab_id.set(payload);
                    crate::app_state::activate_session(meta.session_id);
                }
                close_global_search();
            }
            x if x == GLOBAL_SEARCH_OPEN_OBJECT => {
                let objects = APP_GLOBAL_SEARCH_OBJECTS();
                if let Some(object) = objects.get(payload as usize).cloned() {
                    open_object_hit(store, tree_reload, &object);
                }
                close_global_search();
            }
            x if x == GLOBAL_SEARCH_RUN_ACTION => {
                if let Some(action_id) = payload_to_action_id(payload) {
                    actions_state::dispatch_action(action_id);
                }
                close_global_search();
            }
            _ => {
                // Unknown discriminator: ignore so a stale dispatch
                // never tears down the workspace.
            }
        }
    });

    let tool_panel_layout = APP_UI_SETTINGS().tool_panel_layout.normalized();
    let tool_vis = helpers::ToolPanelVisibility {
        show_saved_queries: APP_SHOW_SAVED_QUERIES(),
        show_connections: APP_SHOW_CONNECTIONS(),
        show_explorer: APP_SHOW_EXPLORER(),
        show_history,
        show_agent_panel: APP_SHOW_AGENT_PANEL(),
        ai_features_enabled: APP_AI_FEATURES_ENABLED(),
    };
    let sidebar_panels = visible_tool_panels(&tool_panel_layout.sidebar, &tool_vis);
    let inspector_panels = visible_tool_panels(&tool_panel_layout.inspector, &tool_vis);
    let show_sidebar = !sidebar_panels.is_empty() || dragging_panel().is_some();
    let show_inspector = !inspector_panels.is_empty() || dragging_panel().is_some();

    rsx! {
        div {
            id: WORKSPACE_ROOT_ID,
            class: {
                let mut class_name = if show_sidebar {
                    "workspace".to_string()
                } else {
                    "workspace workspace--sidebar-hidden".to_string()
                };

                if sidebar_resize_active() || inspector_resize_active() {
                    class_name.push_str(" workspace--resizing");
                }
                if bottom_resize_active() {
                    class_name.push_str(" workspace--resizing-y");
                }
                if dragging_panel().is_some() {
                    class_name.push_str(" workspace--panel-dragging");
                }

                class_name
            },
            style: format!(
                "--workspace-sidebar-width: {:.0}px; --workspace-inspector-width: {:.0}px; --workspace-bottom-panel-height: {:.0}px;",
                sidebar_width(),
                inspector_width(),
                bottom_panel_height(),
            ),
            onmouseup: move |_| {
                if let Some(target) = drop_target() {
                    apply_tool_panel_drop(
                        dragging_panel,
                        drop_target,
                        target,
                        &tool_vis,
                    );
                } else {
                    dragging_panel.set(None);
                    drop_target.set(None);
                }
            },
            onmouseleave: move |_| {
                if dragging_panel().is_some() {
                    drop_target.set(None);
                }
            },
            onkeydown: move |event: dioxus::prelude::KeyboardEvent| {
                use dioxus::prelude::Modifiers;
                let key = event.key();
                let mods = event.modifiers();
                let ctrl = mods.contains(Modifiers::CONTROL)
                    || mods.contains(Modifiers::META);

                let Some(action) = match_key_combination(&key, mods) else {
                    return;
                };
                event.prevent_default();

                // Catalog-backed shortcuts resolve through the unified
                // Action registry: each runner bumps `APP_COMMAND_REQUEST`,
                // which the `use_effect` above realises against the local
                // tab/history signals. Local-only actions are handled here.
                if let Some(action_id) = action.to_action_id() {
                    crate::app_state::actions::dispatch_action(action_id);
                    return;
                }

                match action {
                    ShortcutAction::FocusFilterPanel => {
                        request_focus_filter_panel();
                    }
                    ShortcutAction::FocusEditor => {
                        request_focus_editor();
                    }
                    ShortcutAction::FocusAgentComposer => {
                        request_focus_agent_composer();
                    }
                    // Ctrl+K — global search overlay. We snapshot
                    // tabs + tree into the overlay's globals so the
                    // overlay can filter without reaching into the
                    // workspace's local signals.
                    ShortcutAction::GlobalSearch => {
                        let tab_snapshot: Vec<GlobalSearchTabItem> = store
                            .meta
                            .read()
                            .iter()
                            .map(|(id, meta)| GlobalSearchTabItem {
                                tab_id: *id,
                                session_id: meta.session_id,
                                title: meta.title.clone(),
                            })
                            .collect();
                        let object_snapshot: Vec<GlobalSearchObjectItem> = tree_sections
                            .read()
                            .iter()
                            .flat_map(|section| {
                                let session_id = section.session_id;
                                let session_name = section.name.clone();
                                section.nodes.iter().map(move |node| ExplorerObjectNode {
                                    session_id,
                                    session_name: session_name.clone(),
                                    node,
                                })
                            })
                            .flat_map(|item| flatten_explorer_node(&item))
                            .collect();
                        open_global_search_with_snapshots(tab_snapshot, object_snapshot);
                    }
                    // F2 rename / Delete drop act on the selected explorer
                    // object. The global [`APP_EXPLORER_SELECTED_NODE`]
                    // signal mirrors the tree's local selection and names
                    // the target; the loaded `tree_sections` supplies the
                    // metadata needed to build the rename/drop target.
                    ShortcutAction::RenameSelected => {
                        crate::screens::workspace::components::explorer::open_selected_rename(
                            tree_sections.read().to_vec(),
                            tree_reload,
                        );
                    }
                    ShortcutAction::DeleteSelected => {
                        let sections = tree_sections.read().to_vec();
                        crate::screens::workspace::components::explorer::confirm_drop_selected_table(
                            &sections,
                            store,
                            tree_reload,
                        );
                    }
                    ShortcutAction::CloseOverlay => {
                        close_topmost_overlay();
                    }
                    _ => {}
                }
                let _ = ctrl;
            },
            WorkspaceBody {
                show_sidebar,
                show_inspector,
                show_bottom_panel: APP_SHOW_BOTTOM_PANEL(),
                sidebar_panels,
                inspector_panels,
                sidebar_width,
                sidebar_resize_active,
                inspector_width,
                inspector_resize_active,
                bottom_panel_height,
                bottom_resize_active,
                bottom_active_tab,
                store,
                history,
                next_history_id,
                saved_queries,
                next_saved_query_id,
                tree_status,
                tree_sections,
                show_saved_queries: APP_SHOW_SAVED_QUERIES(),
                show_connections: APP_SHOW_CONNECTIONS(),
                show_explorer: APP_SHOW_EXPLORER(),
                ai_features_enabled: APP_AI_FEATURES_ENABLED(),
                show_agent_panel: APP_SHOW_AGENT_PANEL(),
                show_history,
                tree_reload,
                dragging_panel,
                drop_target,
                acp_panel_state,
                chat_revision,
                allow_agent_db_read,
                allow_agent_read_sql_run,
                allow_agent_write_sql_run,
                allow_agent_tool_run,
                chat_threads,
                active_chat_thread_id,
                connection_label: connection_label.clone(),
            }
        }
    }
}

pub(crate) use self::components::SqlFormatSettingsFields;

fn close_topmost_overlay() {
    // Safety net for the case where focus has drifted onto the
    // workspace root: dismiss the most recently opened overlay. Each
    // overlay also has its own Esc handler, so this is the second
    // line of defence. Z-order: palette > global search > context menu.
    use crate::app_state::{
        APP_COMMAND_PALETTE,
        APP_GLOBAL_SEARCH_OPEN,
        close_command_palette,
        close_global_search,
    };
    if APP_COMMAND_PALETTE() {
        close_command_palette();
        return;
    }
    if APP_GLOBAL_SEARCH_OPEN() {
        close_global_search();
        return;
    }
    if context_menu::CONTEXT_MENU().is_some() {
        context_menu::close_context_menu();
    }
}

fn show_save_status_toast(status: &str) {
    if let Some(title) = status
        .strip_prefix("Saved ")
        .and_then(|s| s.strip_suffix('.'))
    {
        show_toast(title.to_string(), ToastKind::Success);
    } else if !status.is_empty() {
        show_toast(status.to_string(), ToastKind::Warning);
    }
}

/// Workspace-bound reference to an explorer node. Used as the input to
/// [`flatten_explorer_node`] when the Ctrl+K handler turns a loaded
/// `Vec<ExplorerConnectionSection>` into a flat list of search index
/// entries.
struct ExplorerObjectNode<'a> {
    session_id: u64,
    session_name: String,
    node: &'a ExplorerNode,
}

/// Walk an [`ExplorerNode`] (and its children) into a flat list of
/// [`GlobalSearchObjectItem`]s. We include columns too so the user can
/// jump to "the email column" without going through the table first.
/// The recursion is bounded by the same `EXPLORER_CACHE_TTL` lifetime
/// the tree cache already enforces; nothing here issues DB calls.
fn flatten_explorer_node(item: &ExplorerObjectNode<'_>) -> Vec<GlobalSearchObjectItem> {
    let mut out = Vec::new();
    flatten_into(item, &mut out);
    out
}

fn flatten_into(item: &ExplorerObjectNode<'_>, out: &mut Vec<GlobalSearchObjectItem>) {
    out.push(GlobalSearchObjectItem {
        session_id: item.session_id,
        session_name: item.session_name.clone(),
        name: item.node.name.clone(),
        qualified_name: item.node.qualified_name.clone(),
        kind: item.node.kind,
        schema: item.node.schema.clone(),
    });
    for child in &item.node.children {
        flatten_into(
            &ExplorerObjectNode {
                session_id: item.session_id,
                session_name: item.session_name.clone(),
                node: child,
            },
            out,
        );
    }
}

/// Realise a "user picked this object" pick from the global search
/// overlay. Mirrors the explorer's double-click flow: ensure a tab
/// exists for the session, then run a table preview. Non-queryable
/// kinds (schema, function, procedure, trigger) just activate the
/// session so the explorer panel focuses on the right connection.
fn open_object_hit(store: TabStore, _tree_reload: Signal<u64>, object: &GlobalSearchObjectItem) {
    crate::app_state::activate_session(object.session_id);

    if !object.kind.is_queryable() {
        return;
    }

    let current_id = actions::ensure_tab_for_session(store, object.session_id);
    let current_tab = store.result.read().get(&current_id).cloned();
    let Some(current_tab) = current_tab else {
        return;
    };

    let session_id = store
        .meta
        .read()
        .get(&current_id)
        .map(|m| m.session_id)
        .unwrap_or(0);
    let Some(connection) = actions::tab_connection_or_error(store, current_id, session_id) else {
        return;
    };

    let source = TablePreviewSource {
        schema: object.schema.clone(),
        table_name: object.name.clone(),
        qualified_name: object.qualified_name.clone(),
    };
    actions::run_table_preview_for_tab(
        store,
        current_id,
        connection,
        source,
        0,
        current_tab.page_size,
    );
}

/// Map a payload u64 back to an action id. We only forward
/// palette-visible actions through the global search, so the candidates
/// are the 18 workspace actions. Listing them by hand keeps the lookup
/// independent of the full catalog (which includes context-menu ids we
/// never want to run from the search overlay).
fn payload_to_action_id(payload: u64) -> Option<actions_state::ActionId> {
    use actions_state as acts;
    let candidates = [
        acts::ACTION_NEW_CONNECTION,
        acts::ACTION_OPEN_SETTINGS,
        acts::ACTION_NEW_TAB,
        acts::ACTION_CLOSE_TAB,
        acts::ACTION_NEXT_TAB,
        acts::ACTION_TOGGLE_EXPLORER,
        acts::ACTION_TOGGLE_SAVED_QUERIES,
        acts::ACTION_TOGGLE_HISTORY,
        acts::ACTION_TOGGLE_SQL_EDITOR,
        acts::ACTION_TOGGLE_AGENT_PANEL,
        acts::ACTION_TOGGLE_CONNECTIONS,
        acts::ACTION_REFRESH_EXPLORER,
        acts::ACTION_RUN_QUERY,
        acts::ACTION_FORMAT_SQL,
        acts::ACTION_EXPLAIN_QUERY,
        acts::ACTION_SAVE_QUERY,
        acts::ACTION_OPEN_COMMAND_PALETTE,
        acts::ACTION_ABOUT,
    ];
    candidates.iter().find(|id| id.0 == payload).copied()
}
