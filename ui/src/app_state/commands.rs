//! Command-palette action registry (PHASE 3 refactor).
//!
//! Before PHASE 3 this file owned both the catalog and the execute
//! closures. The catalog now lives in [`crate::app_state::actions`] as
//! the single source of truth for id/label/icon/shortcut/category/
//! keywords/children that the palette, keyboard and context menus all
//! read. This file keeps only what is still local to the palette:
//!
//! - [`CommandId`] is an alias for [`ActionId`], and [`Command`] an
//!   alias for [`Action`], so existing consumers (the palette, the
//!   workspace request dispatcher) keep compiling unchanged.
//! - [`command_list`] is the historical name the palette still calls;
//!   it guarantees the palette's execute closures are registered once
//!   against the shared [`ActionId`]s, then returns the shared catalog.
//! - The thread-local runner table + [`register_runner`] / [`dispatch`]
//!   are preserved verbatim — this is the mechanism that keeps the
//!   closures (which capture Dioxus signals) out of the plain-data
//!   catalog and `Send`-safe. [`dispatch_action`] in `actions.rs`
//!   funnels through [`dispatch`] here.
//!
//! All actions are real: each command's `run` either calls a global
//! setter from [`crate::app_state`] (for panel toggles, settings,
//! connection screen) or bumps a request counter that the workspace
//! watches (for run / format / explain / new-tab / etc.). That keeps
//! the palette wired into the same action surface as the toolbar and
//! context menus.

use std::collections::HashMap;

pub use crate::app_state::actions::{
    // Re-export the id constants under their historical `CMD_` names so
    // the workspace request dispatcher and existing call sites compile
    // unchanged against the shared registry.
    ACTION_ABOUT as CMD_ABOUT,
    ACTION_CLOSE_TAB as CMD_CLOSE_TAB,
    ACTION_ER_DIAGRAM as CMD_ER_DIAGRAM,
    ACTION_EXPLAIN_QUERY as CMD_EXPLAIN_QUERY,
    ACTION_FORMAT_SQL as CMD_FORMAT_SQL,
    ACTION_NEW_CONNECTION as CMD_NEW_CONNECTION,
    ACTION_NEW_TAB as CMD_NEW_TAB,
    ACTION_NEXT_TAB as CMD_NEXT_TAB,
    ACTION_OPEN_COMMAND_PALETTE as CMD_OPEN_COMMAND_PALETTE,
    ACTION_OPEN_SETTINGS as CMD_OPEN_SETTINGS,
    ACTION_REFRESH_EXPLORER as CMD_REFRESH_EXPLORER,
    ACTION_RUN_QUERY as CMD_RUN_QUERY,
    ACTION_SAVE_QUERY as CMD_SAVE_QUERY,
    ACTION_TOGGLE_AGENT_PANEL as CMD_TOGGLE_AGENT_PANEL,
    ACTION_TOGGLE_CONNECTIONS as CMD_TOGGLE_CONNECTIONS,
    ACTION_TOGGLE_EXPLORER as CMD_TOGGLE_EXPLORER,
    ACTION_TOGGLE_HISTORY as CMD_TOGGLE_HISTORY,
    ACTION_TOGGLE_SAVED_QUERIES as CMD_TOGGLE_SAVED_QUERIES,
    ACTION_TOGGLE_SQL_EDITOR as CMD_TOGGLE_SQL_EDITOR,
    Action as Command,
    ActionId as CommandId,
    palette_actions,
};

thread_local! {
    static RUNNERS: std::cell::RefCell<HashMap<CommandId, Box<dyn FnMut()>>> =
        std::cell::RefCell::new(HashMap::new());
    static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn register_runner(id: CommandId, runner: impl FnMut() + 'static) {
    RUNNERS.with(|cell| {
        cell.borrow_mut().insert(id, Box::new(runner));
    });
}

/// Run the command with the given id. Closures are registered once
/// at startup by [`command_list`] and may be invoked any number of
/// times. A missing id indicates a wiring bug (the command is in
/// the list but has no runner); we log and skip instead of
/// panicking, since panicking from a UI callback would tear down the
/// whole workspace.
pub fn dispatch(id: CommandId) {
    // Toolbar / keyboard can fire before the palette is opened.
    // command_list() is idempotent and registers the thread-local runners.
    let _ = command_list();
    let outcome = RUNNERS.with(|cell| {
        cell.borrow_mut().get_mut(&id).map(|runner| {
            // Panic-trampoline: a panic inside the runner must not
            // poison the RefCell. Errors are not propagated to the UI;
            // the runner is responsible for surfacing failures via
            // toasts / dialogs.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runner();
            }))
        })
    });
    if outcome.is_none() {
        eprintln!("command_palette: no runner registered for {id:?}");
    }
}

/// Build the global command list. Called exactly once at startup: this
/// registers the palette's execute closures against the shared
/// [`ActionId`]s, then returns the shared [`action_catalog`]. The
/// returned slice is leaked into a `&'static` so the palette can
/// iterate it without holding a lock.
///
/// Every command registers a runner that maps to a real action:
/// either a global setter from [`crate::app_state`] or a request
/// signal that the workspace watches.
pub fn command_list() -> &'static [Command] {
    use crate::app_state as app;

    let should_register = REGISTERED.with(|flag| {
        if flag.get() {
            false
        } else {
            flag.set(true);
            true
        }
    });
    if should_register {
        register_runner(CMD_NEW_CONNECTION, app::open_connection_screen);
        register_runner(CMD_OPEN_SETTINGS, || {
            // Bridge + receiver are wired in the toolbar's helper; we
            // simply delegate so the command palette keeps the same UX
            // surface as the toolbar's Settings button.
            crate::layout::toolbar_open_settings();
        });
        register_runner(CMD_NEW_TAB, || app::request_command(CMD_NEW_TAB));
        register_runner(CMD_CLOSE_TAB, || app::request_command(CMD_CLOSE_TAB));
        register_runner(CMD_NEXT_TAB, || app::request_command(CMD_NEXT_TAB));

        register_runner(CMD_TOGGLE_EXPLORER, || {
            app::set_show_explorer(!app::APP_SHOW_EXPLORER());
        });
        register_runner(CMD_TOGGLE_SAVED_QUERIES, || {
            app::set_show_saved_queries(!app::APP_SHOW_SAVED_QUERIES());
        });
        register_runner(CMD_TOGGLE_HISTORY, || {
            app::set_show_history(!app::APP_SHOW_HISTORY());
        });
        register_runner(CMD_TOGGLE_SQL_EDITOR, || {
            app::set_show_sql_editor(!app::APP_SHOW_SQL_EDITOR());
        });
        register_runner(CMD_TOGGLE_AGENT_PANEL, || {
            app::set_show_agent_panel(!app::APP_SHOW_AGENT_PANEL());
        });
        register_runner(CMD_TOGGLE_CONNECTIONS, || {
            app::set_show_connections(!app::APP_SHOW_CONNECTIONS());
        });
        register_runner(CMD_REFRESH_EXPLORER, || {
            app::request_command(CMD_REFRESH_EXPLORER);
        });
        register_runner(CMD_ER_DIAGRAM, || {
            app::request_command(CMD_ER_DIAGRAM);
        });

        register_runner(CMD_RUN_QUERY, || app::request_command(CMD_RUN_QUERY));
        register_runner(CMD_FORMAT_SQL, || app::request_command(CMD_FORMAT_SQL));
        register_runner(CMD_EXPLAIN_QUERY, || {
            app::request_command(CMD_EXPLAIN_QUERY)
        });
        register_runner(CMD_SAVE_QUERY, || app::request_command(CMD_SAVE_QUERY));

        register_runner(CMD_OPEN_COMMAND_PALETTE, app::open_command_palette);
        register_runner(CMD_ABOUT, || {
            app::show_toast(
                format!(
                    "Shovel — desktop database client (theme {})",
                    app::APP_THEME()
                ),
                app::ToastKind::Info,
            );
        });
    }

    palette_actions()
}

#[cfg(test)]
fn has_runner(id: CommandId) -> bool {
    RUNNERS.with(|cell| cell.borrow().contains_key(&id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_list_is_stable_and_unique() {
        let list = command_list();
        assert!(list.len() >= 15, "spec requires 15+ commands");
        let mut seen = std::collections::HashSet::new();
        for cmd in list {
            assert!(seen.insert(cmd.id), "duplicate command id");
            assert!(!cmd.label.is_empty());
            assert!(!cmd.category.is_empty());
        }
    }

    #[test]
    fn command_list_contains_required_actions() {
        let list = command_list();
        let ids: std::collections::HashSet<_> = list.iter().map(|c| c.id).collect();
        for required in [
            CMD_NEW_CONNECTION,
            CMD_OPEN_SETTINGS,
            CMD_NEW_TAB,
            CMD_CLOSE_TAB,
            CMD_TOGGLE_EXPLORER,
            CMD_TOGGLE_SAVED_QUERIES,
            CMD_TOGGLE_HISTORY,
            CMD_TOGGLE_SQL_EDITOR,
            CMD_TOGGLE_AGENT_PANEL,
            CMD_TOGGLE_CONNECTIONS,
            CMD_REFRESH_EXPLORER,
            CMD_RUN_QUERY,
            CMD_FORMAT_SQL,
            CMD_EXPLAIN_QUERY,
            CMD_SAVE_QUERY,
            CMD_OPEN_COMMAND_PALETTE,
            CMD_ABOUT,
            CMD_ER_DIAGRAM,
            CMD_REFRESH_EXPLORER,
        ] {
            assert!(ids.contains(&required), "missing command: {required:?}");
        }
    }

    #[test]
    fn command_list_registers_er_diagram_and_refresh_runners() {
        let _ = command_list();
        assert!(
            has_runner(CMD_ER_DIAGRAM),
            "ER Diagram must have a palette runner"
        );
        assert!(
            has_runner(CMD_REFRESH_EXPLORER),
            "Refresh explorer must have a palette runner"
        );
    }

    #[test]
    fn command_ids_are_distinct() {
        let ids = [
            CMD_NEW_CONNECTION,
            CMD_OPEN_SETTINGS,
            CMD_NEW_TAB,
            CMD_CLOSE_TAB,
            CMD_NEXT_TAB,
            CMD_TOGGLE_EXPLORER,
            CMD_TOGGLE_SAVED_QUERIES,
            CMD_TOGGLE_HISTORY,
            CMD_TOGGLE_SQL_EDITOR,
            CMD_TOGGLE_AGENT_PANEL,
            CMD_TOGGLE_CONNECTIONS,
            CMD_REFRESH_EXPLORER,
            CMD_RUN_QUERY,
            CMD_FORMAT_SQL,
            CMD_EXPLAIN_QUERY,
            CMD_SAVE_QUERY,
            CMD_OPEN_COMMAND_PALETTE,
            CMD_ABOUT,
        ];
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(seen.insert(id), "duplicate id: {id:?}");
        }
    }

    #[test]
    fn command_shortcuts_are_available_for_rendering() {
        // The palette renders the shortcut next to each entry; the
        // catalog must populate it for the key actions.
        use crate::app_state::actions::find_action;
        for required in [
            CMD_RUN_QUERY,
            CMD_FORMAT_SQL,
            CMD_EXPLAIN_QUERY,
            CMD_SAVE_QUERY,
            CMD_OPEN_COMMAND_PALETTE,
            CMD_CLOSE_TAB,
            CMD_NEXT_TAB,
            CMD_REFRESH_EXPLORER,
            CMD_NEW_TAB,
        ] {
            let action = find_action(required).expect("action present");
            assert!(action.shortcut.is_some(), "missing shortcut: {required:?}");
        }
    }
}
