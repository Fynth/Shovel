use std::collections::HashMap;

use dioxus::prelude::*;
use models::TabDraft;

use super::super::tab_store::{
    TabEditorState,
    TabMeta,
    TabPendingState,
    TabResultState,
    TabStore,
    tab_editor,
    tab_meta,
    tab_pending,
    tab_result,
};
use crate::app_state::{APP_STATE, APP_TAB_DRAFTS};

pub struct QueryTabsState {
    pub store: TabStore,
}

pub fn default_tab_title() -> String {
    "Query 1".to_string()
}

pub fn default_tab_sql() -> String {
    "select 1 as id;".to_string()
}

pub fn use_query_tabs() -> QueryTabsState {
    let mut next_tab_id = use_signal(|| 1_u64);
    let mut active_tab_id = use_signal(|| 0_u64);
    let mut meta = use_signal(HashMap::<u64, TabMeta>::new);
    let mut editor = use_signal(HashMap::<u64, TabEditorState>::new);
    let mut result = use_signal(HashMap::<u64, TabResultState>::new);
    let mut pending = use_signal(HashMap::<u64, TabPendingState>::new);

    // Effect: prune tabs whose session is gone; ensure an active tab exists.
    use_effect(move || {
        let (session_ids, active_session_id) = {
            let app_state = APP_STATE.read();
            (
                app_state
                    .sessions
                    .iter()
                    .map(|s| s.id)
                    .collect::<std::collections::HashSet<_>>(),
                app_state.active_session_id,
            )
        };

        meta.with_mut(|m| m.retain(|_, t| session_ids.contains(&t.session_id)));
        editor.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));
        result.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));
        pending.with_mut(|m| m.retain(|id, _| meta.read().contains_key(id)));

        let Some(session_id) = active_session_id else {
            active_tab_id.set(0);
            return;
        };

        let current_active_matches = meta
            .read()
            .get(&active_tab_id())
            .is_some_and(|t| t.session_id == session_id);
        if current_active_matches {
            return;
        }

        if let Some(existing_id) = meta
            .read()
            .iter()
            .find(|(_, t)| t.session_id == session_id)
            .map(|(id, _)| *id)
        {
            active_tab_id.set(existing_id);
            return;
        }

        // Look up a saved tab draft for this session's connection.
        let (saved_title, saved_sql) = {
            let app_state = APP_STATE.read();
            let identity_key = app_state
                .session(session_id)
                .map(|s| s.request.identity_key());
            if let Some(key) = identity_key {
                APP_TAB_DRAFTS()
                    .iter()
                    .find(|d| d.session_identity_key == key)
                    .map(|d| (d.title.clone(), d.sql.clone()))
                    .unwrap_or_else(|| (default_tab_title(), default_tab_sql()))
            } else {
                (default_tab_title(), default_tab_sql())
            }
        };

        let tab_id = next_tab_id();
        next_tab_id += 1;
        let page_size = crate::app_state::APP_UI_SETTINGS().default_page_size;
        meta.with_mut(|m| {
            m.insert(
                tab_id,
                tab_meta(
                    tab_id,
                    session_id,
                    saved_title,
                    models::WorkspaceTabKind::Query,
                    false,
                ),
            );
        });
        editor.with_mut(|m| {
            m.insert(tab_id, tab_editor(saved_sql));
        });
        result.with_mut(|m| {
            m.insert(tab_id, tab_result(page_size));
        });
        pending.with_mut(|m| {
            m.insert(tab_id, tab_pending());
        });
        active_tab_id.set(tab_id);
    });

    // Effect: persist tab drafts whenever editor state changes.
    use_effect(move || {
        let _ = editor(); // subscribe
        let app_state = APP_STATE.read();
        let drafts: Vec<TabDraft> = editor
            .read()
            .iter()
            .filter_map(|(id, ed)| {
                let (session_id, title) = {
                    let meta_guard = meta.read();
                    let t = meta_guard.get(id)?;
                    (t.session_id, t.title.clone())
                };
                let session = app_state.session(session_id)?;
                if ed.sql.trim().is_empty() || ed.sql.trim() == default_tab_sql() {
                    return None;
                }
                Some(TabDraft {
                    session_identity_key: session.request.identity_key(),
                    title,
                    sql: ed.sql.clone(),
                })
            })
            .collect();
        let current = APP_TAB_DRAFTS();
        if *current != drafts {
            *APP_TAB_DRAFTS.write() = drafts;
        }
    });

    QueryTabsState {
        store: TabStore {
            meta,
            editor,
            result,
            pending,
            active_tab_id,
            next_tab_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tab_sql_is_select_one() {
        assert_eq!(default_tab_sql(), "select 1 as id;");
    }

    #[test]
    fn default_tab_title_is_query_one() {
        assert_eq!(default_tab_title(), "Query 1");
    }
}
