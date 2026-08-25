use crate::{
    app_state::{
        APP_STATE,
        context_menu::{ContextMenuItem, open_context_menu},
        is_panel_collapsed,
        toggle_panel_collapsed,
    },
    screens::workspace::{
        actions::{append_to_tab_sql, ensure_tab_for_session, set_active_tab_sql},
        components::Chevron,
        tab_store::TabStore,
    },
};
use dioxus::prelude::*;
use models::{SavedQuery, SavedQueryKind, WorkspaceToolPanel};

#[component]
pub fn SavedQueriesPanel(
    saved_queries: Vec<SavedQuery>,
    saved_queries_signal: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    store: TabStore,
) -> Element {
    let mut save_title = use_signal(String::new);
    let mut panel_status = use_signal(String::new);

    let active_tab_id = store.active_tab_id();
    let active_tab = store
        .meta
        .read()
        .get(&active_tab_id)
        .cloned()
        .map(|meta| (meta.id, meta.session_id, meta.title));
    let active_sql = store
        .editor
        .read()
        .get(&active_tab_id)
        .map(|ed| ed.sql.trim().to_string())
        .unwrap_or_default();
    let can_save = !active_sql.is_empty();

    let sessions_by_name = APP_STATE
        .read()
        .sessions
        .iter()
        .map(|session| (session.name.clone(), session.id))
        .collect::<std::collections::HashMap<_, _>>();

    let mut items = saved_queries;
    items.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });
    let collapsed = is_panel_collapsed(WorkspaceToolPanel::SavedQueries);
    let mut class_name = "workspace__panel saved-queries".to_string();
    if collapsed {
        class_name.push_str(" workspace__panel--collapsed");
    }

    rsx! {
        section {
            class: class_name,
            div {
                class: "saved-queries__header",
                button {
                    class: "workspace__panel-collapse",
                    "aria-label": if collapsed {
                        "Expand saved queries panel"
                    } else {
                        "Collapse saved queries panel"
                    },
                    "aria-expanded": "{!collapsed}",
                    onclick: move |_| toggle_panel_collapsed(WorkspaceToolPanel::SavedQueries),
                    Chevron { open: !collapsed }
                }
                h2 { class: "workspace__section-title", "Saved Queries" }
                p {
                    class: "workspace__hint",
                    if panel_status().trim().is_empty() {
                        "Reusable queries and snippets."
                    } else {
                        "{panel_status}"
                    }
                }

                if !collapsed {
                div { class: "saved-queries__form",
                    input {
                        class: "input",
                        value: "{save_title}",
                        placeholder: active_tab
                            .as_ref()
                            .map(|(_, _, title)| title.clone())
                            .unwrap_or_else(|| "Saved Query".to_string()),
                        oninput: move |event| save_title.set(event.value()),
                    }
                    div { class: "saved-queries__form-actions",
                        button {
                            class: "button button--ghost button--small",
                            disabled: !can_save,
                            onclick: {
                                let active_tab = active_tab.clone();
                                let active_sql = active_sql.clone();
                                move |_| {
                                    save_current_sql(
                                        SavedQueryKind::Snippet,
                                        active_tab.clone(),
                                        active_sql.clone(),
                                        save_title,
                                        next_saved_query_id,
                                        saved_queries_signal,
                                        panel_status,
                                    );
                                }
                            },
                            "Save Snippet"
                        }
                        button {
                            class: "button button--primary button--small",
                            disabled: !can_save,
                            onclick: {
                                let active_tab = active_tab.clone();
                                let active_sql = active_sql.clone();
                                move |_| {
                                    save_current_sql(
                                        SavedQueryKind::Query,
                                        active_tab.clone(),
                                        active_sql.clone(),
                                        save_title,
                                        next_saved_query_id,
                                        saved_queries_signal,
                                        panel_status,
                                    );
                                }
                            },
                            "Save Query"
                        }
                    }
                }
            }

            div {
                class: "saved-queries__body",
                if items.is_empty() {
                    p { class: "empty-state", "No saved queries or snippets yet." }
                } else {
                    for item in items {
                        {
                            let source_session_id = item
                                .connection_name
                                .as_ref()
                                .and_then(|name| sessions_by_name.get(name))
                                .copied();
                            let load_label = if item.kind == SavedQueryKind::Snippet {
                                "Insert in tab"
                            } else {
                                "Load in tab"
                            };
                            let context_items = build_saved_query_context_menu(
                                item.clone(),
                                source_session_id,
                                saved_queries_signal,
                                panel_status,
                                store,
                            );

                            rsx! {
                                article {
                                    class: "saved-queries__item",
                                    oncontextmenu: move |event| {
                                        event.prevent_default();
                                        let coords = event.client_coordinates();
                                        open_context_menu(coords.x, coords.y, context_items.clone());
                                    },
                                    div { class: "saved-queries__item-top",
                                        p { class: "saved-queries__title", "{item.title}" }
                                        span { class: "saved-queries__kind", "{item.kind_label()}" }
                                    }
                                    if let Some(connection_name) = item.connection_name.clone() {
                                        p {
                                            class: "saved-queries__connection",
                                            title: {connection_name.to_string()},
                                            {connection_name.to_string()}
                                        }
                                    }
                                    pre {
                                        class: "saved-queries__sql",
                                        title: "{item.sql}",
                                        "{item.sql}"
                                    }
                                    div { class: "saved-queries__actions",
                                        button {
                                            class: "button button--ghost button--small",
                                            onclick: {
                                                let item = item.clone();
                                                move |_| {
                                                    load_saved_query_into_workspace(
                                                        item.clone(),
                                                        source_session_id,
                                                        store,
                                                    );
                                                    panel_status.set(format!(
                                                        "{} loaded into workspace.",
                                                        item.title
                                                    ));
                                                }
                                            },
                                            {load_label.to_string()}
                                        }
                                        button {
                                            class: "button button--ghost button--small",
                                            onclick: {
                                                let item_id = item.id;
                                                let item_title = item.title.clone();
                                                move |_| {
                                                    saved_queries_signal.with_mut(|items| {
                                                        items.retain(|existing| existing.id != item_id);
                                                    });
                                                    panel_status.set(format!("Deleted {item_title}."));
                                                    spawn(async move {
                                                        let _ = services::delete_saved_query(item_id).await;
                                                    });
                                                }
                                            },
                                            "Delete"
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
}

fn save_current_sql(
    kind: SavedQueryKind,
    active_tab: Option<(u64, u64, String)>,
    active_sql: String,
    mut save_title: Signal<String>,
    mut next_saved_query_id: Signal<u64>,
    mut saved_queries_signal: Signal<Vec<SavedQuery>>,
    mut panel_status: Signal<String>,
) {
    let Some((_tab_id, session_id, title)) = active_tab else {
        panel_status.set("No active SQL tab available.".to_string());
        return;
    };
    if active_sql.trim().is_empty() {
        panel_status.set("Current SQL tab is empty.".to_string());
        return;
    }

    let title = if save_title().trim().is_empty() {
        title
    } else {
        save_title().trim().to_string()
    };
    let connection_name = APP_STATE.read().session_name(session_id);
    let item = SavedQuery {
        id: next_saved_query_id(),
        title: title.clone(),
        folder: String::new(),
        sql: active_sql,
        kind,
        connection_name,
    };

    next_saved_query_id += 1;
    saved_queries_signal.with_mut(|items| {
        items.push(item.clone());
        items.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.id.cmp(&right.id))
        });
    });
    save_title.set(String::new());
    panel_status.set(format!("Saved {}.", title));

    spawn(async move {
        let _ = services::save_saved_query(item).await;
    });
}

fn load_saved_query_into_workspace(
    item: SavedQuery,
    source_session_id: Option<u64>,
    store: TabStore,
) {
    let target_tab_id = if let Some(session_id) = source_session_id {
        ensure_tab_for_session(store, session_id)
    } else {
        store.active_tab_id()
    };

    if target_tab_id == 0 {
        return;
    }

    match item.kind {
        SavedQueryKind::Query => set_active_tab_sql(
            store,
            target_tab_id,
            item.sql,
            "Loaded saved query".to_string(),
        ),
        SavedQueryKind::Snippet => append_to_tab_sql(
            store,
            target_tab_id,
            item.sql,
            "Inserted saved snippet".to_string(),
        ),
    }
}

fn build_saved_query_context_menu(
    item: SavedQuery,
    source_session_id: Option<u64>,
    mut saved_queries_signal: Signal<Vec<SavedQuery>>,
    mut panel_status: Signal<String>,
    store: TabStore,
) -> Vec<ContextMenuItem> {
    use crate::{app_state::context_menu::copy_to_clipboard, screens::workspace::ActionIcon};

    let mut items: Vec<ContextMenuItem> = Vec::new();

    let open_label = if item.kind == SavedQueryKind::Snippet {
        "Insert in tab"
    } else {
        "Load in tab"
    };

    // 1. Open in tab (mirror of the inline button).
    {
        let item = item.clone();
        items.push(
            ContextMenuItem::new(open_label, move || {
                load_saved_query_into_workspace(item.clone(), source_session_id, store);
            })
            .with_icon(ActionIcon::Run),
        );
    }

    // 2. Copy SQL to clipboard.
    {
        let sql = item.sql.clone();
        items.push(
            ContextMenuItem::new("Copy SQL", move || {
                let _ = copy_to_clipboard(sql.clone());
            })
            .with_icon(ActionIcon::Duplicate),
        );
    }

    // 3. Copy title to clipboard.
    {
        let title = item.title.clone();
        items.push(
            ContextMenuItem::new("Copy title", move || {
                let _ = copy_to_clipboard(title.clone());
            })
            .with_icon(ActionIcon::Duplicate)
            .separator(),
        );
    }

    // 4. Delete — destructive.
    {
        let item_id = item.id;
        let item_title = item.title.clone();
        items.push(
            ContextMenuItem::new("Delete", move || {
                saved_queries_signal.with_mut(|items| {
                    items.retain(|existing| existing.id != item_id);
                });
                panel_status.set(format!("Deleted {item_title}."));
                spawn(async move {
                    let _ = services::delete_saved_query(item_id).await;
                });
            })
            .with_icon(ActionIcon::Delete)
            .danger(),
        );
    }

    items
}
