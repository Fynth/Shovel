//! Command-palette action registry.
//!
//! The palette lists every user-invokable action the app can perform:
//! toggling tool panels, opening dialogs, dispatching workspace commands
//! (run query, format, explain), navigation, etc. The list is built
//! once at startup via [`command_list`] and rendered by
//! [`crate::components::command_palette::CommandPalette`].
//!
//! Each [`Command`] stores plain data (id, title, keywords, category,
//! icon) plus a [`CommandId`] handle that resolves to a closure stored
//! in a thread-local registry. This mirrors the
//! `CallbackId`/`ContextMenuItem` pattern used by
//! [`crate::app_state::context_menu`]: commands stay `Send`-friendly
//! plain data so they can live in a [`GlobalSignal`], while the
//! actual closures (which capture Dioxus signals) live in
//! thread-local storage and are invoked through [`dispatch`].
//!
//! All actions are real: each command's `run` either calls a global
//! setter from [`crate::app_state`] (for panel toggles, settings,
//! connection screen) or bumps a request counter that the workspace
//! watches (for run / format / explain / new-tab / etc.). That keeps
//! the palette wired into the same action surface as the toolbar and
//! context menus.

use std::collections::HashMap;

use crate::screens::workspace::ActionIcon;

/// Stable identifier for a registered command. Unlike the per-call
/// `CallbackId`, this one is stable across runs so the palette can
/// match a freshly-typed query against the catalog of commands without
/// rebuilding the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(pub u64);

impl CommandId {
    const fn new(value: u64) -> Self {
        Self(value)
    }
}

pub const CMD_NEW_CONNECTION: CommandId = CommandId::new(1);
pub const CMD_OPEN_SETTINGS: CommandId = CommandId::new(2);
pub const CMD_NEW_TAB: CommandId = CommandId::new(3);
pub const CMD_CLOSE_TAB: CommandId = CommandId::new(4);
pub const CMD_NEXT_TAB: CommandId = CommandId::new(5);
pub const CMD_TOGGLE_EXPLORER: CommandId = CommandId::new(6);
pub const CMD_TOGGLE_SAVED_QUERIES: CommandId = CommandId::new(7);
pub const CMD_TOGGLE_HISTORY: CommandId = CommandId::new(8);
pub const CMD_TOGGLE_SQL_EDITOR: CommandId = CommandId::new(9);
pub const CMD_TOGGLE_AGENT_PANEL: CommandId = CommandId::new(10);
pub const CMD_TOGGLE_CONNECTIONS: CommandId = CommandId::new(11);
pub const CMD_REFRESH_EXPLORER: CommandId = CommandId::new(12);
pub const CMD_RUN_QUERY: CommandId = CommandId::new(13);
pub const CMD_FORMAT_SQL: CommandId = CommandId::new(14);
pub const CMD_EXPLAIN_QUERY: CommandId = CommandId::new(15);
pub const CMD_SAVE_QUERY: CommandId = CommandId::new(16);
pub const CMD_OPEN_COMMAND_PALETTE: CommandId = CommandId::new(17);
pub const CMD_ABOUT: CommandId = CommandId::new(18);

/// A single entry in the palette. Plain data, no `Fn`-trait, so the
/// list can live in a [`std::sync::LazyLock`] / be cloned cheaply.
#[derive(Clone)]
pub struct Command {
    pub id: CommandId,
    pub title: &'static str,
    /// Extra search tokens. The title is always matched; keywords are
    /// things like `"toggle"`, `"panel"`, `"sql"` that the user types
    /// but that don't appear in the title.
    pub keywords: &'static [&'static str],
    pub category: &'static str,
    pub icon: Option<ActionIcon>,
}

thread_local! {
    static RUNNERS: std::cell::RefCell<HashMap<CommandId, Box<dyn FnMut()>>> =
        std::cell::RefCell::new(HashMap::new());
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

/// Build the global command list. This is called exactly once at
/// startup; the resulting `Vec` is leaked into a `&'static` so the
/// palette can iterate it without holding a lock.
///
/// Every command registers a runner that maps to a real action:
/// either a global setter from [`crate::app_state`] or a request
/// signal that the workspace watches.
pub fn command_list() -> &'static [Command] {
    use crate::app_state as app;
    use std::sync::LazyLock;

    static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REGISTERED.swap(true, std::sync::atomic::Ordering::SeqCst) {
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

    static LIST: LazyLock<Vec<Command>> = LazyLock::new(|| {
        vec![
            Command {
                id: CMD_NEW_CONNECTION,
                title: "New Connection",
                keywords: &["connect", "add", "database"],
                category: "File",
                icon: Some(ActionIcon::NewConnection),
            },
            Command {
                id: CMD_OPEN_SETTINGS,
                title: "Open Settings",
                keywords: &["preferences", "options", "config"],
                category: "File",
                icon: Some(ActionIcon::Details),
            },
            Command {
                id: CMD_NEW_TAB,
                title: "New Query Tab",
                keywords: &["tab", "query", "new"],
                category: "File",
                icon: None,
            },
            Command {
                id: CMD_CLOSE_TAB,
                title: "Close Tab",
                keywords: &["tab", "close"],
                category: "File",
                icon: Some(ActionIcon::Close),
            },
            Command {
                id: CMD_NEXT_TAB,
                title: "Next Tab",
                keywords: &["tab", "switch"],
                category: "File",
                icon: Some(ActionIcon::Next),
            },
            Command {
                id: CMD_TOGGLE_EXPLORER,
                title: "Toggle Explorer",
                keywords: &["panel", "sidebar", "schema", "tree"],
                category: "View",
                icon: Some(ActionIcon::Explorer),
            },
            Command {
                id: CMD_TOGGLE_SAVED_QUERIES,
                title: "Toggle Saved Queries",
                keywords: &["panel", "saved", "snippets"],
                category: "View",
                icon: Some(ActionIcon::SavedQueries),
            },
            Command {
                id: CMD_TOGGLE_HISTORY,
                title: "Toggle History",
                keywords: &["panel", "history", "recent"],
                category: "View",
                icon: Some(ActionIcon::History),
            },
            Command {
                id: CMD_TOGGLE_SQL_EDITOR,
                title: "Toggle SQL Editor",
                keywords: &["panel", "editor"],
                category: "View",
                icon: Some(ActionIcon::SqlEditor),
            },
            Command {
                id: CMD_TOGGLE_AGENT_PANEL,
                title: "Toggle Agent Panel",
                keywords: &["panel", "agent", "ai", "chat"],
                category: "View",
                icon: Some(ActionIcon::Agent),
            },
            Command {
                id: CMD_TOGGLE_CONNECTIONS,
                title: "Toggle Connections",
                keywords: &["panel", "connections", "list"],
                category: "View",
                icon: Some(ActionIcon::Connections),
            },
            Command {
                id: CMD_REFRESH_EXPLORER,
                title: "Refresh Explorer",
                keywords: &["reload", "tree", "schema"],
                category: "Query",
                icon: Some(ActionIcon::Refresh),
            },
            Command {
                id: CMD_RUN_QUERY,
                title: "Run Query",
                keywords: &["execute", "run", "go"],
                category: "Query",
                icon: Some(ActionIcon::Run),
            },
            Command {
                id: CMD_FORMAT_SQL,
                title: "Format SQL",
                keywords: &["beautify", "prettify"],
                category: "Query",
                icon: Some(ActionIcon::Format),
            },
            Command {
                id: CMD_EXPLAIN_QUERY,
                title: "Explain Query",
                keywords: &["plan", "analyze", "debug"],
                category: "Query",
                icon: Some(ActionIcon::Explain),
            },
            Command {
                id: CMD_SAVE_QUERY,
                title: "Save Query",
                keywords: &["bookmark", "saved"],
                category: "Query",
                icon: Some(ActionIcon::Apply),
            },
            Command {
                id: CMD_OPEN_COMMAND_PALETTE,
                title: "Open Command Palette",
                keywords: &["palette", "search", "command", "shortcut"],
                category: "Help",
                icon: None,
            },
            Command {
                id: CMD_ABOUT,
                title: "About Shovel",
                keywords: &["version", "info"],
                category: "Help",
                icon: Some(ActionIcon::Details),
            },
        ]
    });

    &LIST
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
            assert!(!cmd.title.is_empty());
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
        ] {
            assert!(ids.contains(&required), "missing command: {required:?}");
        }
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
}
