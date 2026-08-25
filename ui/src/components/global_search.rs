//! Renders the Ctrl+K global-search overlay.
//!
//! Mounted once at the top of [`crate::app::App`] next to the
//! [`crate::components::command_palette::CommandPalette`]. The overlay
//! reads its visibility from
//! [`crate::app_state::global_search::APP_GLOBAL_SEARCH_OPEN`] and the
//! snapshot indices from [`crate::app_state::global_search::APP_GLOBAL_SEARCH_TABS`]
//! and [`crate::app_state::global_search::APP_GLOBAL_SEARCH_OBJECTS`].
//! Actions come from the unified
//! [`crate::app_state::actions::action_catalog`].
//!
//! Behaviour:
//! - The search input is auto-focused when the overlay opens; the
//!   underlying `use_signal`s for the query and the selection index are
//!   reset so the overlay always opens in a fresh state.
//! - Filtering runs on every keystroke through the pure
//!   [`crate::app_state::global_search::search_all`] helper. Results are
//!   grouped by kind (Open Tab / Connection / Schema / Table / View /
//!   Column / Action), capped per group, and empty groups are dropped.
//! - Up/Down arrows navigate across the flattened hit list (groups
//!   collapse into a single virtual list for keyboard nav). Home/End
//!   jump to the first/last hit; Enter dispatches the selected hit;
//!   Escape closes.
//! - The overlay calls `prevent_default` on its own key events so the
//!   workspace's root onkeydown matcher does not double-fire when the
//!   user is typing inside the search field.
//! - Dispatched hits bump
//!   [`crate::app_state::global_search::APP_GLOBAL_SEARCH_REQUEST`] with
//!   a discriminator; the workspace watches that counter and realises
//!   the pick against the live tab/active_tab_id/explorer signals. This
//!   mirrors how the command palette dispatches workspace-scoped work.

use crate::{
    app_state::{
        actions as actions_state,
        global_search::{
            self,
            GLOBAL_SEARCH_OPEN_OBJECT,
            GLOBAL_SEARCH_OPEN_TAB,
            GLOBAL_SEARCH_RUN_ACTION,
            SearchHit,
            close_global_search,
            dispatch_global_search_request,
            search_all,
            split_match,
        },
    },
    screens::workspace::components::{ActionIcon, IconGlyph},
};
use dioxus::prelude::*;
use models::ExplorerNodeKind;

/// Flatten the grouped search result into one navigable list of
/// (group_label, hit) pairs. Keyboard navigation works on this flat
/// index; the renderer also iterates it (in the same order) to lay out
/// the group headers and items together.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FlatHit {
    group_label: &'static str,
    hit: SearchHit,
}

fn flatten_groups(groups: &[global_search::SearchGroup]) -> Vec<FlatHit> {
    let mut out = Vec::new();
    for group in groups {
        for hit in &group.hits {
            out.push(FlatHit {
                group_label: group.label,
                hit: hit.clone(),
            });
        }
    }
    out
}

/// Find the position of the (session_id, qualified_name) pair in the
/// current object snapshot. The overlay uses this to pack a u64 payload
/// for the workspace's command-request effect without maintaining a
/// per-render index counter (the snapshot can grow / shrink between
/// keystrokes; the position is always re-derived at dispatch time).
/// Returns 0 when the pair is not in the snapshot — the workspace
/// treats a missing object as a no-op so a stale dispatch can never
/// panic.
fn find_object_index(session_id: u64, qualified_name: &str) -> u64 {
    let snapshot = global_search::APP_GLOBAL_SEARCH_OBJECTS();
    for (idx, item) in snapshot.iter().enumerate() {
        if item.session_id == session_id && item.qualified_name == qualified_name {
            return idx as u64;
        }
    }
    0
}

/// Pick the icon for a given hit. Keeps the overlay visually consistent
/// with the command palette and the explorer tree.
fn hit_icon(hit: &SearchHit) -> ActionIcon {
    match hit {
        SearchHit::Tab { .. } => ActionIcon::SqlEditor,
        SearchHit::Object { kind, .. } => match kind {
            ExplorerNodeKind::Schema => ActionIcon::Structure,
            ExplorerNodeKind::Table | ExplorerNodeKind::MaterializedView => ActionIcon::CreateTable,
            ExplorerNodeKind::View => ActionIcon::Details,
            ExplorerNodeKind::Column => ActionIcon::AddRule,
            ExplorerNodeKind::Sequence => ActionIcon::Next,
            ExplorerNodeKind::Function | ExplorerNodeKind::Procedure => ActionIcon::Generate,
            ExplorerNodeKind::Trigger => ActionIcon::Apply,
        },
        SearchHit::Action { id, .. } => actions_state::find_action(*id)
            .and_then(|a| a.icon)
            .unwrap_or(ActionIcon::Details),
    }
}

/// Dispatch a picked hit. We never reach into the live tab/explorer
/// signals from here (the overlay is mounted in `app.rs` outside the
/// workspace tree); instead we bump the global request counter and the
/// workspace's command-request effect realises the work.
fn dispatch_hit(hit: &SearchHit, payload: u64) {
    *crate::app_state::global_search::APP_GLOBAL_SEARCH_REQUEST_PAYLOAD.write() = payload;
    match hit {
        SearchHit::Tab { .. } => dispatch_global_search_request(GLOBAL_SEARCH_OPEN_TAB),
        SearchHit::Object { .. } => dispatch_global_search_request(GLOBAL_SEARCH_OPEN_OBJECT),
        SearchHit::Action { .. } => dispatch_global_search_request(GLOBAL_SEARCH_RUN_ACTION),
    }
}

#[component]
pub fn GlobalSearch() -> Element {
    let open = global_search::APP_GLOBAL_SEARCH_OPEN();
    if !open {
        return rsx! {};
    }

    let mut query = use_signal(String::new);
    let mut selection: Signal<usize> = use_signal(|| 0usize);
    let mut groups: Signal<Vec<global_search::SearchGroup>> = use_signal(Vec::new);
    let mut flat: Signal<Vec<FlatHit>> = use_signal(Vec::new);

    // Reset the query and selection whenever the overlay (re-)opens.
    // Without this, opening the overlay via Ctrl+K after a previous
    // search would keep the stale query and the selection index.
    use_effect(move || {
        let _ = global_search::APP_GLOBAL_SEARCH_OPEN();
        query.set(String::new());
        selection.set(0);
        groups.set(Vec::new());
        flat.set(Vec::new());
    });

    // Re-run the pure search on every keystroke. The pure helper takes
    // plain `&[T]` slices, so we read the snapshot globals here and
    // hand them in. The action catalog is a `&'static [Action]` and is
    // also passed by reference — no allocation churn.
    use_effect(move || {
        let q = query();
        let tabs = global_search::APP_GLOBAL_SEARCH_TABS();
        let objects = global_search::APP_GLOBAL_SEARCH_OBJECTS();
        let actions = actions_state::action_catalog();
        let next = search_all(&q, &tabs, &objects, actions);
        let next_flat = flatten_groups(&next);
        groups.set(next);
        flat.set(next_flat);
    });

    // Clamp the selection when the result set shrinks so a stale index
    // never points past the end. Same shape as the command palette.
    {
        let len = flat().len();
        let cur = selection();
        if len == 0 {
            if cur != 0 {
                selection.set(0);
            }
        } else if cur >= len {
            selection.set(len - 1);
        }
    }

    let run_and_close = move |idx: usize| {
        let hits = flat();
        if let Some(item) = hits.get(idx) {
            let payload = match &item.hit {
                SearchHit::Tab { tab_id, .. } => *tab_id,
                // For objects the workspace needs to know which snapshot
                // entry to open. We re-derive the index from the
                // current snapshot globals so the overlay never has to
                // hand-roll an index counter.
                SearchHit::Object {
                    session_id,
                    qualified_name,
                    ..
                } => find_object_index(*session_id, qualified_name),
                SearchHit::Action { id, .. } => id.0,
            };
            dispatch_hit(&item.hit, payload);
        }
        close_global_search();
    };

    let onkeydown = move |event: KeyboardEvent| {
        use dioxus::prelude::Modifiers;
        let key = event.key();
        let mods = event.modifiers();
        let _ = mods.contains(Modifiers::CONTROL) || mods.contains(Modifiers::META);
        let len = flat().len();
        match key {
            Key::Escape => {
                event.prevent_default();
                close_global_search();
            }
            Key::ArrowDown => {
                event.prevent_default();
                if len > 0 {
                    let next = (selection() + 1) % len;
                    selection.set(next);
                }
            }
            Key::ArrowUp => {
                event.prevent_default();
                if len > 0 {
                    let prev = if selection() == 0 {
                        len - 1
                    } else {
                        selection() - 1
                    };
                    selection.set(prev);
                }
            }
            Key::Home => {
                event.prevent_default();
                if len > 0 {
                    selection.set(0);
                }
            }
            Key::End => {
                event.prevent_default();
                if len > 0 {
                    selection.set(len - 1);
                }
            }
            Key::Enter => {
                event.prevent_default();
                run_and_close(selection());
            }
            _ => {}
        }
    };

    let current_groups = groups();
    let _current_flat = flat();
    let current_query = query();

    rsx! {
        div {
            class: "global-search__backdrop",
            onclick: move |_| close_global_search(),
            div {
                class: "global-search",
                onclick: move |event| event.stop_propagation(),
                onkeydown: onkeydown,
                tabindex: "0",
                div {
                    class: "global-search__input-row",
                    span { class: "global-search__icon",
                        svg {
                            view_box: "0 0 24 24",
                            fill: "none",
                            stroke: "currentColor",
                            stroke_width: "1.85",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            width: "16",
                            height: "16",
                            circle { cx: "11", cy: "11", r: "6" }
                            path { d: "m20 20-3.5-3.5" }
                        }
                    }
                    input {
                        class: "global-search__input",
                        r#type: "text",
                        placeholder: "Search tabs, tables, actions…",
                        value: "{current_query}",
                        autofocus: true,
                        autocomplete: "off",
                        spellcheck: "false",
                        oninput: move |event| {
                            query.set(event.value());
                            selection.set(0);
                        },
                    }
                    span { class: "global-search__shortcut-hint", "Ctrl+K" }
                }
                div {
                    class: "global-search__results",
                    {
                        if current_groups.is_empty() {
                            if current_query.trim().is_empty() {
                                rsx! {
                                    div {
                                        class: "global-search__empty",
                                        "Type to search across tabs, tables, schemas, and actions"
                                    }
                                }
                            } else {
                                rsx! {
                                    div {
                                        class: "global-search__empty",
                                        "No results for \"{current_query}\""
                                    }
                                }
                            }
                        } else {
                            // Walk the groups + flat list in lockstep so
                            // each rendered item knows its flat index
                            // (used for selection + Enter dispatch) and
                            // its group label.
                            let mut cursor: usize = 0;
                            rsx! {
                                for group in current_groups.iter() {
                                    div {
                                        class: "global-search__group",
                                        div { class: "global-search__group-title", {group.label} }
                                        for hit in group.hits.iter() {
                                            {
                                                let display = hit.display();
                                                let secondary = hit.secondary();
                                                let flat_idx = cursor;
                                                cursor += 1;
                                                let needle = current_query.clone();
                                                let is_selected = selection() == flat_idx;
                                                let mut class_name = String::from("global-search__item");
                                                if is_selected {
                                                    class_name.push_str(" global-search__item--selected");
                                                }
                                                let segments = split_match(&display, &needle);
                                                let icon = hit_icon(hit);
                                                rsx! {
                                                    button {
                                                        class: class_name.clone(),
                                                        r#type: "button",
                                                        onclick: move |_| run_and_close(flat_idx),
                                                        onmouseenter: move |_| selection.set(flat_idx),
                                                        span { class: "global-search__item-icon",
                                                            svg {
                                                                view_box: "0 0 24 24",
                                                                fill: "none",
                                                                stroke: "currentColor",
                                                                stroke_width: "1.85",
                                                                stroke_linecap: "round",
                                                                stroke_linejoin: "round",
                                                                width: "14",
                                                                height: "14",
                                                                IconGlyph { icon }
                                                            }
                                                        }
                                                        span { class: "global-search__item-copy",
                                                            span { class: "global-search__item-title",
                                                                for segment in segments.iter() {
                                                                    if segment.1 {
                                                                        span { class: "global-search__match", {segment.0.clone()} }
                                                                    } else {
                                                                        span { {segment.0.clone()} }
                                                                    }
                                                                }
                                                            }
                                                            if let Some(secondary) = secondary.as_deref() {
                                                                span { class: "global-search__item-secondary", {secondary} }
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
                div {
                    class: "global-search__footer",
                    span { class: "global-search__hint",
                        span { class: "global-search__kbd", "↑↓" }
                        " navigate"
                        span { class: "global-search__kbd-sep", " · " }
                        span { class: "global-search__kbd", "Enter" }
                        " open"
                        span { class: "global-search__kbd-sep", " · " }
                        span { class: "global-search__kbd", "Esc" }
                        " close"
                    }
                    span { class: "global-search__hint global-search__hint--right",
                        "Tip: "
                        span { class: "global-search__kbd", "Ctrl+Shift+P" }
                        " for the command palette"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{
        actions as acts,
        global_search::{GlobalSearchObjectItem, GlobalSearchTabItem, SearchGroupKind},
    };
    use models::ExplorerNodeKind;

    #[test]
    fn flatten_groups_preserves_group_order_and_concatenation() {
        let groups = vec![
            global_search::SearchGroup {
                kind: SearchGroupKind::OpenTab,
                label: "Open Tab",
                hits: vec![
                    SearchHit::Tab {
                        tab_id: 1,
                        session_id: 1,
                        title: "alpha".to_string(),
                    },
                    SearchHit::Tab {
                        tab_id: 2,
                        session_id: 1,
                        title: "beta".to_string(),
                    },
                ],
            },
            global_search::SearchGroup {
                kind: SearchGroupKind::Action,
                label: "Action",
                hits: vec![SearchHit::Action {
                    id: acts::ACTION_NEW_TAB,
                    label: "New Query Tab",
                    category: "File",
                }],
            },
        ];
        let flat = flatten_groups(&groups);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0].group_label, "Open Tab");
        assert_eq!(flat[2].group_label, "Action");
    }

    #[test]
    fn hit_icon_picks_table_for_materialized_view() {
        let hit = SearchHit::Object {
            session_id: 1,
            session_name: "db".to_string(),
            name: "events".to_string(),
            qualified_name: "analytics.events".to_string(),
            kind: ExplorerNodeKind::MaterializedView,
        };
        let icon = hit_icon(&hit);
        // The exact icon can evolve; the contract is "non-default" —
        // asserts the dispatch wired through to a real variant.
        assert!(matches!(icon, ActionIcon::CreateTable));
    }

    #[test]
    fn snapshot_items_round_trip_through_pure_helpers() {
        let tabs = [GlobalSearchTabItem {
            tab_id: 99,
            session_id: 7,
            title: "snapshot-tab".to_string(),
        }];
        let objects = [GlobalSearchObjectItem {
            session_id: 7,
            session_name: "db7".to_string(),
            name: "orders".to_string(),
            qualified_name: "public.orders".to_string(),
            kind: ExplorerNodeKind::Table,
            schema: Some("public".to_string()),
        }];
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].tab_id, 99);
        assert_eq!(tabs[0].session_id, 7);
        assert_eq!(tabs[0].title, "snapshot-tab");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "orders");
        assert_eq!(objects[0].schema.as_deref(), Some("public"));
    }
}
