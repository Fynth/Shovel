use dioxus::prelude::*;

use super::super::{
    components::ExplorerConnectionSection,
    helpers::{load_explorer_section, unloaded_explorer_section},
};
use crate::app_state::{APP_SHOW_EXPLORER, APP_STATE};

pub struct ExplorerState {
    pub tree_status: Signal<String>,
    pub tree_sections: Signal<Vec<ExplorerConnectionSection>>,
    pub tree_reload: Signal<u64>,
}

pub fn use_explorer_state() -> ExplorerState {
    let mut tree_status = use_signal(|| "Loading explorer...".to_string());
    let mut tree_sections = use_signal(Vec::<ExplorerConnectionSection>::new);
    let tree_reload = use_signal(|| 0_u64);
    let mut last_handled_reload_tick = use_signal(|| 0_u64);

    use_effect(move || {
        let reload_tick = tree_reload();
        let force_reload =
            should_force_explorer_reload(reload_tick, *last_handled_reload_tick.peek());
        let explorer_visible = APP_SHOW_EXPLORER();
        let (sessions, active_session_id) = {
            let app_state = APP_STATE.read();
            (app_state.sessions.clone(), app_state.active_session_id)
        };

        spawn(async move {
            let _ = reload_tick;
            if sessions.is_empty() {
                tree_sections.set(Vec::new());
                tree_status.set("Select or create a connection".to_string());
                return;
            }

            if !explorer_visible {
                tree_sections.set(
                    sessions
                        .iter()
                        .map(|session| {
                            unloaded_explorer_section(session, active_session_id, "Explorer hidden")
                        })
                        .collect(),
                );
                tree_status.set("Explorer hidden".to_string());
                return;
            }

            let active_index = sessions
                .iter()
                .position(|session| Some(session.id) == active_session_id)
                .unwrap_or(0);
            let mut sections = sessions
                .iter()
                .map(|session| {
                    unloaded_explorer_section(
                        session,
                        active_session_id,
                        "Activate this connection to load explorer",
                    )
                })
                .collect::<Vec<_>>();

            tree_status.set("Loading explorer...".to_string());
            if force_reload {
                last_handled_reload_tick.set(reload_tick);
            }
            let active_section = load_explorer_section(
                sessions[active_index].clone(),
                active_session_id.or(Some(sessions[active_index].id)),
                !force_reload,
            )
            .await;
            let active_failed = active_section.status.starts_with("Error:");
            sections[active_index] = active_section;

            tree_sections.set(sections);
            if active_failed {
                tree_status.set("Explorer failed for the active connection".to_string());
            } else {
                tree_status.set("Explorer ready for the active connection".to_string());
            }
        });
    });

    ExplorerState {
        tree_status,
        tree_sections,
        tree_reload,
    }
}

fn should_force_explorer_reload(reload_tick: u64, last_handled_reload_tick: u64) -> bool {
    reload_tick != last_handled_reload_tick
}

#[cfg(test)]
mod tests {
    use super::should_force_explorer_reload;

    #[test]
    fn manual_reload_tick_bypasses_explorer_cache_once() {
        assert!(!should_force_explorer_reload(0, 0));
        assert!(should_force_explorer_reload(1, 0));
        assert!(!should_force_explorer_reload(1, 1));
        assert!(should_force_explorer_reload(2, 1));
    }
}
