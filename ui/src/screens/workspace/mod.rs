mod actions;
mod chat;
pub(crate) mod components;
mod context;
pub mod helpers;
mod hooks;

use crate::{
    app_state::{
        APP_AI_FEATURES_ENABLED,
        APP_COMMAND_REQUEST,
        APP_COMMAND_REQUEST_KIND,
        APP_SHOW_AGENT_PANEL,
        APP_SHOW_CONNECTIONS,
        APP_SHOW_EXPLORER,
        APP_SHOW_HISTORY,
        APP_SHOW_SAVED_QUERIES,
        APP_SHOW_SQL_EDITOR,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        APP_THEME,
        APP_UI_SETTINGS,
        ToastKind,
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
        keyboard::{ShortcutAction, match_key_combination},
        open_connection_screen,
        request_focus_editor,
        request_focus_filter_panel,
        set_show_agent_panel,
        set_show_connections,
        set_show_explorer,
        set_show_history,
        set_show_saved_queries,
        set_show_sql_editor,
        show_toast,
        toggle_command_palette,
        update_ui_settings,
    },
    windows,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use models::{
    AcpPanelState,
    ChatThreadSummary,
    QueryHistoryItem,
    QueryTabState,
    SavedQuery,
    WorkspaceToolDock,
    WorkspaceToolPanel,
};

use self::{
    chat::{create_chat_thread, delete_chat_thread, select_chat_thread},
    components::{
        AcpAgentPanel,
        IconButton,
        QueryHistoryPanel,
        SavedQueriesPanel,
        SessionRail,
        SidebarConnectionTree,
        TabsManager,
    },
    helpers::{
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
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
                tabs,
                active_tab_id,
                next_tab_id,
            }
        }
    }
}

#[component]
fn AgentToolPanel(
    mut acp_panel_state: Signal<AcpPanelState>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
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
            tabs,
            active_tab_id,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
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
                    tabs,
                    active_tab_id,
                }
            }
        },
        WorkspaceToolPanel::Explorer => rsx! {
            ExplorerToolPanel {
                tree_status,
                tree_sections,
                tree_reload,
                tabs,
                active_tab_id,
                next_tab_id,
            }
        },
        WorkspaceToolPanel::SavedQueries => rsx! {
            SavedQueriesPanel {
                saved_queries: saved_queries(),
                saved_queries_signal: saved_queries,
                next_saved_query_id,
                tabs,
                active_tab_id,
                next_tab_id,
            }
        },
        WorkspaceToolPanel::History => rsx! {
            div {
                class: "workspace__panel workspace__panel--history",
                QueryHistoryPanel {
                    history,
                    tabs,
                    active_tab_id,
                }
            }
        },
        WorkspaceToolPanel::Agent => rsx! {
            AgentToolPanel {
                acp_panel_state,
                tabs,
                active_tab_id,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
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
                tabs,
                active_tab_id,
                next_tab_id,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
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
                    tabs,
                    active_tab_id,
                    next_tab_id,
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
    sidebar_panels: Vec<WorkspaceToolPanel>,
    inspector_panels: Vec<WorkspaceToolPanel>,
    sidebar_width: Signal<f64>,
    mut sidebar_resize_active: Signal<bool>,
    inspector_width: Signal<f64>,
    mut inspector_resize_active: Signal<bool>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
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
                        tabs,
                        active_tab_id,
                        next_tab_id,
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
                        icon: ActionIcon::SavedQueries,
                        label: if show_saved_queries {
                            "Hide saved queries".to_string()
                        } else {
                            "Show saved queries".to_string()
                        },
                        active: show_saved_queries,
                        small: true,
                        onclick: move |_| set_show_saved_queries(!APP_SHOW_SAVED_QUERIES()),
                    }
                    IconButton {
                        icon: ActionIcon::Connections,
                        label: if show_connections {
                            "Hide connections".to_string()
                        } else {
                            "Show connections".to_string()
                        },
                        active: show_connections,
                        small: true,
                        onclick: move |_| set_show_connections(!APP_SHOW_CONNECTIONS()),
                    }
                    IconButton {
                        icon: ActionIcon::Explorer,
                        label: if show_explorer {
                            "Hide explorer".to_string()
                        } else {
                            "Show explorer".to_string()
                        },
                        active: show_explorer,
                        small: true,
                        onclick: move |_| set_show_explorer(!APP_SHOW_EXPLORER()),
                    }
                    IconButton {
                        icon: ActionIcon::History,
                        label: if show_history {
                            "Hide history".to_string()
                        } else {
                            "Show history".to_string()
                        },
                        active: show_history,
                        small: true,
                        onclick: move |_| set_show_history(!APP_SHOW_HISTORY()),
                    }
                    IconButton {
                        icon: ActionIcon::SqlEditor,
                        label: if APP_SHOW_SQL_EDITOR() {
                            "Hide SQL editor".to_string()
                        } else {
                            "Show SQL editor".to_string()
                        },
                        active: APP_SHOW_SQL_EDITOR(),
                        small: true,
                        onclick: move |_| set_show_sql_editor(!APP_SHOW_SQL_EDITOR()),
                    }
                    if ai_features_enabled {
                        IconButton {
                            icon: ActionIcon::Agent,
                            label: if show_agent_panel {
                                "Hide agent panel".to_string()
                            } else {
                                "Show agent panel".to_string()
                            },
                            active: show_agent_panel,
                            small: true,
                            onclick: move |_| set_show_agent_panel(!APP_SHOW_AGENT_PANEL()),
                        }
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
                                if let Some(diagram) = helpers::build_er_diagram(&sections, &fks) {
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
                        tabs,
                        active_tab_id,
                        next_tab_id,
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
                            tabs,
                            active_tab_id,
                            next_tab_id,
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

    // ── Custom hooks ───────────────────────────────────────────────
    let ExplorerState {
        tree_status,
        tree_sections,
        mut tree_reload,
    } = use_explorer_state();

    let QueryTabsState {
        mut tabs,
        mut active_tab_id,
        mut next_tab_id,
    } = use_query_tabs();

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
        tabs,
        active_tab_id,
        connection_label: connection_label.clone(),
    });

    context::provide_workspace_tab_context(tabs, active_tab_id, next_tab_id);
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
        match kind {
            x if x == CMD_NEW_TAB.0 => {
                if let Some(session) = APP_STATE.read().active_session().cloned() {
                    let tab_id = next_tab_id();
                    next_tab_id += 1;
                    let tab = actions::new_query_tab(
                        tab_id,
                        session.id,
                        format!("Query {}", tabs.read().len() + 1),
                        String::new(),
                    );
                    tabs.with_mut(|all_tabs| all_tabs.push(tab));
                    active_tab_id.set(tab_id);
                }
            }
            x if x == CMD_CLOSE_TAB.0 =>
                if tabs.read().len() > 1 {
                    let current_id = active_tab_id();
                    tabs.with_mut(|all_tabs| all_tabs.retain(|t| t.id != current_id));
                    if let Some(first) = tabs.read().first() {
                        active_tab_id.set(first.id);
                        crate::app_state::activate_session(first.session_id);
                    }
                },
            x if x == CMD_NEXT_TAB.0 => {
                let all_tabs = tabs.read();
                if all_tabs.len() > 1 {
                    let current_idx = all_tabs.iter().position(|t| t.id == active_tab_id());
                    if let Some(idx) = current_idx {
                        let next_idx = (idx + 1) % all_tabs.len();
                        let next_tab = &all_tabs[next_idx];
                        active_tab_id.set(next_tab.id);
                        crate::app_state::activate_session(next_tab.session_id);
                    }
                }
            }
            x if x == CMD_REFRESH_EXPLORER.0 => {
                tree_reload += 1;
            }
            x if x == CMD_RUN_QUERY.0 => {
                actions::run_active_tab(tabs, active_tab_id(), (history, next_history_id));
            }
            x if x == CMD_FORMAT_SQL.0 => {
                actions::format_active_tab(tabs, active_tab_id(), APP_SQL_FORMAT_SETTINGS());
            }
            x if x == CMD_EXPLAIN_QUERY.0 => {
                actions::run_active_tab_explain(tabs, active_tab_id());
            }
            x if x == CMD_SAVE_QUERY.0 => {
                let status = actions::save_active_tab_as_saved_query(
                    tabs,
                    active_tab_id(),
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
                if dragging_panel().is_some() {
                    class_name.push_str(" workspace--panel-dragging");
                }

                class_name
            },
            style: format!(
                "--workspace-sidebar-width: {:.0}px; --workspace-inspector-width: {:.0}px;",
                sidebar_width(),
                inspector_width(),
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

                match action {
                    ShortcutAction::NewTab => {
                        if let Some(session) = APP_STATE.read().active_session().cloned() {
                            let tab_id = next_tab_id();
                            next_tab_id += 1;
                            let tab = actions::new_query_tab(
                                tab_id,
                                session.id,
                                format!("Query {}", tabs.read().len() + 1),
                                String::new(),
                            );
                            tabs.with_mut(|all_tabs| all_tabs.push(tab));
                            active_tab_id.set(tab_id);
                        }
                    }
                    ShortcutAction::CloseTab => {
                        if tabs.read().len() > 1 {
                            let current_id = active_tab_id();
                            tabs.with_mut(|all_tabs| all_tabs.retain(|t| t.id != current_id));
                            if let Some(first) = tabs.read().first() {
                                active_tab_id.set(first.id);
                                crate::app_state::activate_session(first.session_id);
                            }
                        }
                    }
                    ShortcutAction::NextTab => {
                        let all_tabs = tabs.read();
                        if all_tabs.len() > 1 {
                            let current_idx =
                                all_tabs.iter().position(|t| t.id == active_tab_id());
                            if let Some(idx) = current_idx {
                                let next_idx = (idx + 1) % all_tabs.len();
                                let next_tab = &all_tabs[next_idx];
                                active_tab_id.set(next_tab.id);
                                crate::app_state::activate_session(next_tab.session_id);
                            }
                        }
                    }
                    ShortcutAction::RefreshExplorer => {
                        tree_reload += 1;
                    }
                    ShortcutAction::SaveQuery => {
                        let status = actions::save_active_tab_as_saved_query(
                            tabs,
                            active_tab_id(),
                            saved_queries,
                            next_saved_query_id,
                        );
                        show_save_status_toast(&status);
                    }
                    ShortcutAction::FocusFilterPanel => {
                        request_focus_filter_panel();
                    }
                    ShortcutAction::FocusEditor => {
                        request_focus_editor();
                    }
                    ShortcutAction::FormatSql => {
                        actions::format_active_tab(
                            tabs,
                            active_tab_id(),
                            APP_SQL_FORMAT_SETTINGS(),
                        );
                    }
                    ShortcutAction::CommandPalette => {
                        toggle_command_palette();
                    }
                    ShortcutAction::CloseOverlay => {
                        close_topmost_overlay();
                    }
                }
                let _ = ctrl;
            },
            WorkspaceBody {
                show_sidebar,
                show_inspector,
                sidebar_panels,
                inspector_panels,
                sidebar_width,
                sidebar_resize_active,
                inspector_width,
                inspector_resize_active,
                tabs,
                active_tab_id,
                next_tab_id,
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
    // The settings surface lives in its own native window now, so the Esc
    // shortcut on the main window only needs to clear the context menu.
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
