use std::collections::HashSet;

use dioxus::prelude::*;
use models::QueryTabState;

use super::super::actions::new_query_tab;
use crate::app_state::{APP_STATE, APP_TAB_DRAFTS};

pub struct QueryTabsState {
    pub tabs: Signal<Vec<QueryTabState>>,
    pub active_tab_id: Signal<u64>,
    pub next_tab_id: Signal<u64>,
}

pub fn use_query_tabs() -> QueryTabsState {
    let mut next_tab_id = use_signal(|| 1_u64);
    let mut active_tab_id = use_signal(|| 0_u64);
    let mut tabs = use_signal(Vec::<QueryTabState>::new);

    use_effect(move || {
        let (session_ids, active_session_id) = {
            let app_state = APP_STATE.read();
            (
                app_state
                    .sessions
                    .iter()
                    .map(|session| session.id)
                    .collect::<HashSet<_>>(),
                app_state.active_session_id,
            )
        };

        tabs.with_mut(|all_tabs| all_tabs.retain(|tab| session_ids.contains(&tab.session_id)));

        if let Some(session_id) = active_session_id {
            let current_active_matches = tabs
                .read()
                .iter()
                .any(|tab| tab.id == active_tab_id() && tab.session_id == session_id);

            if current_active_matches {
                return;
            }

            if let Some(existing_tab_id) = tabs
                .read()
                .iter()
                .find(|tab| tab.session_id == session_id)
                .map(|tab| tab.id)
            {
                active_tab_id.set(existing_tab_id);
                return;
            }

            // Look up a saved tab draft for this session's connection.
            let (saved_title, saved_sql) = {
                let app_state = APP_STATE.read();
                let identity_key = app_state
                    .session(session_id)
                    .map(|session| session.request.identity_key());
                if let Some(key) = identity_key {
                    APP_TAB_DRAFTS()
                        .iter()
                        .find(|draft| draft.session_identity_key == key)
                        .map(|draft| (draft.title.clone(), draft.sql.clone()))
                        .unwrap_or_else(|| ("Query 1".to_string(), "select 1 as id;".to_string()))
                } else {
                    ("Query 1".to_string(), "select 1 as id;".to_string())
                }
            };

            let tab_id = next_tab_id();
            next_tab_id += 1;
            tabs.with_mut(|all_tabs| {
                all_tabs.push(new_query_tab(tab_id, session_id, saved_title, saved_sql));
            });
            active_tab_id.set(tab_id);
        } else {
            active_tab_id.set(0);
        }
    });

    // Persist tab drafts whenever tabs change so SQL drafts survive restarts.
    use_effect(move || {
        let _ = tabs(); // subscribe to tab changes
        let app_state = APP_STATE.read();
        let drafts: Vec<models::TabDraft> = tabs
            .read()
            .iter()
            .filter_map(|tab| {
                let session = app_state.session(tab.session_id)?;
                if tab.sql.trim().is_empty() || tab.sql.trim() == "select 1 as id;" {
                    return None;
                }
                Some(models::TabDraft {
                    session_identity_key: session.request.identity_key(),
                    title: tab.title.clone(),
                    sql: tab.sql.clone(),
                })
            })
            .collect();
        let current = APP_TAB_DRAFTS();
        if *current != drafts {
            *APP_TAB_DRAFTS.write() = drafts;
        }
    });

    QueryTabsState {
        tabs,
        active_tab_id,
        next_tab_id,
    }
}
