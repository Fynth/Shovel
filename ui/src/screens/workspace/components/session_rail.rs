use crate::{
    app_state::{
        APP_STATE,
        activate_session,
        is_panel_collapsed,
        open_connection_screen,
        remove_session,
        toggle_panel_collapsed,
    },
    screens::workspace::components::{ActionIcon, IconButton},
};
use dioxus::prelude::*;
use models::{ConnectionRequest, QueryTabState, WorkspaceToolPanel};

fn open_context_menu(mut context_menu: Signal<Option<u64>>, session_id: u64, event: MouseEvent) {
    event.prevent_default();
    event.stop_propagation();
    context_menu.set(Some(session_id));
}

#[component]
pub fn SessionRail(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
) -> Element {
    let mut context_menu = use_signal(|| None::<u64>);
    let (sessions, active_session_id) = {
        let app_state = APP_STATE.read();
        (app_state.sessions.clone(), app_state.active_session_id)
    };
    let session_cards = sessions
        .into_iter()
        .map(|session| {
            let kind_label = match session.kind {
                models::DatabaseKind::Sqlite => "SQLite",
                models::DatabaseKind::Postgres => "PostgreSQL",
                models::DatabaseKind::MySql => "MySQL",
                models::DatabaseKind::ClickHouse => "ClickHouse",
            };
            let target_label = session_target_label(&session.request);
            (session, kind_label, target_label)
        })
        .collect::<Vec<_>>();
    let collapsed = is_panel_collapsed(WorkspaceToolPanel::Connections);
    let mut class_name = "session-list".to_string();
    if collapsed {
        class_name.push_str(" session-list--collapsed");
    }

    rsx! {
        section {
            class: class_name,
            div {
                class: "session-list__header",
                button {
                    class: "workspace__panel-collapse",
                    "aria-label": if collapsed {
                        "Expand connections panel"
                    } else {
                        "Collapse connections panel"
                    },
                    "aria-expanded": "{!collapsed}",
                    onclick: move |_| toggle_panel_collapsed(WorkspaceToolPanel::Connections),
                    span {
                        class: if collapsed {
                            "workspace__panel-chevron"
                        } else {
                            "workspace__panel-chevron workspace__panel-chevron--open"
                        },
                        ">"
                    }
                }
                h2 { class: "workspace__section-title", "Connections" }
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| open_connection_screen(),
                    "Add"
                }
            }

            if !collapsed {
                div {
                    class: "session-list__body",
                if session_cards.is_empty() {
                    p { class: "empty-state", "No active connections." }
                } else {
                    for (session, kind_label, target_label) in session_cards {
                        div {
                            class: if Some(session.id) == active_session_id {
                                "session-list__item session-list__item--active"
                            } else {
                                "session-list__item"
                            },
                            oncontextmenu: {
                                let session_id = session.id;
                                move |event| {
                                    open_context_menu(context_menu, session_id, event);
                                }
                            },
                            div {
                                class: "session-list__main",
                                role: "button",
                                tabindex: "0",
                                onclick: {
                                    let session_id = session.id;
                                    move |_| {
                                        context_menu.set(None);
                                        activate_session(session_id);
                                    }
                                },
                                onkeydown: {
                                    let session_id = session.id;
                                    move |event| match event.key() {
                                        Key::Enter => {
                                            event.prevent_default();
                                            context_menu.set(None);
                                            activate_session(session_id);
                                        }
                                        Key::Character(text) if text == " " => {
                                            event.prevent_default();
                                            context_menu.set(None);
                                            activate_session(session_id);
                                        }
                                        _ => {}
                                    }
                                },
                                span { class: "session-list__kind", {kind_label.to_string()} }
                                strong {
                                    class: "session-list__name",
                                    title: target_label.to_string(),
                                    {target_label.to_string()}
                                }
                            }
                            IconButton {
                                icon: ActionIcon::Close,
                                label: "Disconnect".to_string(),
                                small: true,
                                onclick: {
                                    let session_id = session.id;
                                    move |_| {
                                        context_menu.set(None);
                                        disconnect_session(tabs, active_tab_id, session_id);
                                    }
                                },
                            }

                            if context_menu() == Some(session.id) {
                                div {
                                    class: "session-list__context-menu",
                                    onmousedown: move |event| event.stop_propagation(),
                                    onclick: move |event| event.stop_propagation(),
                                    button {
                                        class: "session-list__context-action",
                                        onclick: move |_| {
                                            context_menu.set(None);
                                            disconnect_session(tabs, active_tab_id, session.id);
                                        },
                                        "Disconnect"
                                    }
                                }
                            }
                        }
                    }
                }
            }
            }

            if context_menu().is_some() {
                div {
                    class: "session-list__context-backdrop",
                    onmousedown: move |_| context_menu.set(None),
                    onclick: move |_| context_menu.set(None),
                }
            }
        }
    }
}

fn session_target_label(request: &ConnectionRequest) -> String {
    request.short_name()
}

fn disconnect_session(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    session_id: u64,
) {
    tabs.with_mut(|all_tabs| all_tabs.retain(|tab| tab.session_id != session_id));
    if let Some(first_tab) = tabs.read().first() {
        active_tab_id.set(first_tab.id);
        activate_session(first_tab.session_id);
    } else {
        active_tab_id.set(0);
    }
    remove_session(session_id);
}
