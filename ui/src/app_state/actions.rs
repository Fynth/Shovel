//! Unified action catalog — the single source of truth for every
//! user-invokable action across the command palette, the toolbar,
//! context menus, and keyboard shortcuts.
//!
//! Before PHASE 3 these surfaces each carried their own registry:
//! `commands.rs` (palette), `context_menu.rs` (right-click menus) and
//! `keyboard.rs` (shortcuts) all defined labels / ids / dispatch in
//! isolation. This module is the shared catalog: an [`Action`] is plain
//! data — id, label, icon, shortcut display string, category, search
//! keywords and optional child ids — that every surface reads. The
//! execute closures themselves are *not* copied here; they stay
//! registered against the same stable [`ActionId`] via the existing
//! thread-local runner mechanism in [`crate::app_state::commands`] and
//! [`crate::app_state::context_menu`], and are invoked through
//! [`dispatch_action`].
//!
//! # Layer rule
//! `ui` may only reach `models` + `services`; this module only imports
//! from the `ui` crate itself (the [`ActionIcon`] enum lives in the
//! workspace's `icon_button`), so the rule holds.

use crate::screens::workspace::ActionIcon;

/// Stable identifier for a registered action. Identical to the palette's
/// old `CommandId` (both wrap a `u64`); the two names now alias so any
/// code that reaches for a `CommandId` gets the same stable handle the
/// Action catalog uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionId(pub u64);

impl ActionId {
    const fn new(value: u64) -> Self {
        Self(value)
    }
}

// ── Palette / workspace actions (1..=18 keep the historical numbering
//    so the persisted `APP_COMMAND_REQUEST_KIND` discriminator and any
//    already-serialized ids stay stable across the refactor). ─────────
pub const ACTION_NEW_CONNECTION: ActionId = ActionId::new(1);
pub const ACTION_OPEN_SETTINGS: ActionId = ActionId::new(2);
pub const ACTION_NEW_TAB: ActionId = ActionId::new(3);
pub const ACTION_CLOSE_TAB: ActionId = ActionId::new(4);
pub const ACTION_NEXT_TAB: ActionId = ActionId::new(5);
pub const ACTION_TOGGLE_EXPLORER: ActionId = ActionId::new(6);
pub const ACTION_TOGGLE_SAVED_QUERIES: ActionId = ActionId::new(7);
pub const ACTION_TOGGLE_HISTORY: ActionId = ActionId::new(8);
pub const ACTION_TOGGLE_SQL_EDITOR: ActionId = ActionId::new(9);
pub const ACTION_TOGGLE_AGENT_PANEL: ActionId = ActionId::new(10);
pub const ACTION_TOGGLE_CONNECTIONS: ActionId = ActionId::new(11);
pub const ACTION_REFRESH_EXPLORER: ActionId = ActionId::new(12);
pub const ACTION_RUN_QUERY: ActionId = ActionId::new(13);
pub const ACTION_FORMAT_SQL: ActionId = ActionId::new(14);
pub const ACTION_EXPLAIN_QUERY: ActionId = ActionId::new(15);
pub const ACTION_SAVE_QUERY: ActionId = ActionId::new(16);
pub const ACTION_OPEN_COMMAND_PALETTE: ActionId = ActionId::new(17);
pub const ACTION_ABOUT: ActionId = ActionId::new(18);

// ── Explorer context actions (metadata-only, id range 100+). These
//    have no palette runner: the composable context-menu builder
//    (`context_menu_for` in the explorer tree views) realises each into
//    a `ContextMenuItem` whose closure captures the live Dioxus signals.
//    The catalog is still the single place that names them. ──────────
pub const ACTION_TABLE_OPEN: ActionId = ActionId::new(100);
pub const ACTION_TABLE_SELECT_ALL: ActionId = ActionId::new(101);
pub const ACTION_TABLE_DUPLICATE: ActionId = ActionId::new(102);
pub const ACTION_TABLE_TRUNCATE: ActionId = ActionId::new(103);
pub const ACTION_TABLE_DROP: ActionId = ActionId::new(104);
pub const ACTION_TABLE_COPY_DDL: ActionId = ActionId::new(105);
pub const ACTION_TABLE_COPY_INSERT: ActionId = ActionId::new(106);
pub const ACTION_OBJECT_REFRESH: ActionId = ActionId::new(107);
pub const ACTION_OBJECT_COPY_NAME: ActionId = ActionId::new(108);
pub const ACTION_OBJECT_COPY_QUALIFIED: ActionId = ActionId::new(109);
pub const ACTION_SCHEMA_CREATE_TABLE: ActionId = ActionId::new(110);
pub const ACTION_CONNECTION_NEW_QUERY: ActionId = ActionId::new(111);
pub const ACTION_CONNECTION_DISCONNECT: ActionId = ActionId::new(112);
pub const ACTION_COLUMN_FILTER_BY_VALUE: ActionId = ActionId::new(113);
pub const ACTION_COLUMN_SORT_ASC: ActionId = ActionId::new(114);
pub const ACTION_COLUMN_SORT_DESC: ActionId = ActionId::new(115);
pub const ACTION_TABLE_RENAME: ActionId = ActionId::new(116);

// ── Result view-mode + quick-filter actions (PHASE 7). Toolbar buttons
//    in `result_table.rs` read their labels/icons from this catalog so
//    the segmented view-mode control and the quick-filter toggle stay
//    consistent with the rest of the registry. ─────────────────────────
pub const ACTION_VIEW_TABLE: ActionId = ActionId::new(120);
pub const ACTION_VIEW_RECORDS: ActionId = ActionId::new(121);
pub const ACTION_VIEW_SINGLE_RECORD: ActionId = ActionId::new(122);
pub const ACTION_VIEW_DETAILS: ActionId = ActionId::new(123);
pub const ACTION_TOGGLE_QUICK_FILTER: ActionId = ActionId::new(124);

// ── Composable action groups. Each group names the child actions that
//    belong together for a given object type. The explorer menu builder
//    reads these ids to decide which actions a right-click should show,
//    so the shape of every per-type menu is defined here once. ────────
pub const TABLE_ACTIONS: &[ActionId] = &[
    ACTION_TABLE_OPEN,
    ACTION_TABLE_SELECT_ALL,
    ACTION_OBJECT_COPY_NAME,
    ACTION_OBJECT_COPY_QUALIFIED,
    ACTION_TABLE_COPY_INSERT,
    ACTION_TABLE_COPY_DDL,
    ACTION_OBJECT_REFRESH,
    ACTION_TABLE_DUPLICATE,
    ACTION_TABLE_RENAME,
    ACTION_TABLE_TRUNCATE,
    ACTION_TABLE_DROP,
];
pub const COLUMN_ACTIONS: &[ActionId] = &[
    ACTION_COLUMN_FILTER_BY_VALUE,
    ACTION_COLUMN_SORT_ASC,
    ACTION_COLUMN_SORT_DESC,
    ACTION_OBJECT_COPY_NAME,
    ACTION_OBJECT_COPY_QUALIFIED,
];
pub const SCHEMA_ACTIONS: &[ActionId] = &[
    ACTION_SCHEMA_CREATE_TABLE,
    ACTION_OBJECT_REFRESH,
    ACTION_CONNECTION_DISCONNECT,
];
pub const CONNECTION_ACTIONS: &[ActionId] = &[
    ACTION_CONNECTION_DISCONNECT,
    ACTION_CONNECTION_NEW_QUERY,
    ACTION_OBJECT_REFRESH,
];

/// A single entry in the unified action catalog. Plain data, no
/// `Fn`-trait, so it can live in a leaked `&'static` array that every
/// surface (palette, keyboard, context menus) reads without locking.
#[derive(Clone)]
pub struct Action {
    pub id: ActionId,
    /// Human-readable label shown in the palette and context menus.
    pub label: &'static str,
    pub icon: Option<ActionIcon>,
    /// Display string for the keybinding, e.g. `"Ctrl+Shift+P"`. Shown
    /// next to palette entries; `None` when the action has no shortcut.
    pub shortcut: Option<&'static str>,
    /// Palette grouping (File / View / Query / Help, …).
    pub category: &'static str,
    /// Extra search tokens. The label is always matched; keywords are
    /// things like `"toggle"`, `"panel"`, `"sql"`.
    pub keywords: &'static [&'static str],
    /// Child action ids for composable sub-menus / object groups. Empty
    /// for leaf actions. Part of the public catalog contract; the concrete
    /// groups (`TABLE_ACTIONS`, …) name the same ids explicitly so this
    /// field is the forward-looking hook for nested menus.
    #[allow(dead_code)]
    pub children: &'static [ActionId],
}

/// The single source of truth for the action catalog. Called once at
/// startup; the resulting slice is leaked into a `&'static` so every
/// surface iterates it cheaply.
pub fn action_catalog() -> &'static [Action] {
    use std::sync::LazyLock;

    static CATALOG: LazyLock<Vec<Action>> = LazyLock::new(|| {
        vec![
            Action {
                id: ACTION_NEW_CONNECTION,
                label: "New Connection",
                shortcut: Some("Ctrl+Shift+N"),
                keywords: &["connect", "add", "database"],
                category: "File",
                icon: Some(ActionIcon::NewConnection),
                children: &[],
            },
            Action {
                id: ACTION_OPEN_SETTINGS,
                label: "Open Settings",
                shortcut: Some("Ctrl+,"),
                keywords: &["preferences", "options", "config"],
                category: "File",
                icon: Some(ActionIcon::Details),
                children: &[],
            },
            Action {
                id: ACTION_NEW_TAB,
                label: "New Query Tab",
                shortcut: Some("Ctrl+T"),
                keywords: &["tab", "query", "new"],
                category: "File",
                icon: None,
                children: &[],
            },
            Action {
                id: ACTION_CLOSE_TAB,
                label: "Close Tab",
                shortcut: Some("Ctrl+W"),
                keywords: &["tab", "close"],
                category: "File",
                icon: Some(ActionIcon::Close),
                children: &[],
            },
            Action {
                id: ACTION_NEXT_TAB,
                label: "Next Tab",
                shortcut: Some("Ctrl+Tab"),
                keywords: &["tab", "switch"],
                category: "File",
                icon: Some(ActionIcon::Next),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_EXPLORER,
                label: "Toggle Explorer",
                shortcut: None,
                keywords: &["panel", "sidebar", "schema", "tree"],
                category: "View",
                icon: Some(ActionIcon::Explorer),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_SAVED_QUERIES,
                label: "Toggle Saved Queries",
                shortcut: None,
                keywords: &["panel", "saved", "snippets"],
                category: "View",
                icon: Some(ActionIcon::SavedQueries),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_HISTORY,
                label: "Toggle History",
                shortcut: None,
                keywords: &["panel", "history", "recent"],
                category: "View",
                icon: Some(ActionIcon::History),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_SQL_EDITOR,
                label: "Toggle SQL Editor",
                shortcut: None,
                keywords: &["panel", "editor"],
                category: "View",
                icon: Some(ActionIcon::SqlEditor),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_AGENT_PANEL,
                label: "Toggle Agent Panel",
                shortcut: None,
                keywords: &["panel", "agent", "ai", "chat"],
                category: "View",
                icon: Some(ActionIcon::Agent),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_CONNECTIONS,
                label: "Toggle Connections",
                shortcut: None,
                keywords: &["panel", "connections", "list"],
                category: "View",
                icon: Some(ActionIcon::Connections),
                children: &[],
            },
            Action {
                id: ACTION_REFRESH_EXPLORER,
                label: "Refresh Explorer",
                shortcut: Some("F5"),
                keywords: &["reload", "tree", "schema"],
                category: "Query",
                icon: Some(ActionIcon::Refresh),
                children: &[],
            },
            Action {
                id: ACTION_RUN_QUERY,
                label: "Run Query",
                shortcut: Some("Ctrl+Enter"),
                keywords: &["execute", "run", "go"],
                category: "Query",
                icon: Some(ActionIcon::Run),
                children: &[],
            },
            Action {
                id: ACTION_FORMAT_SQL,
                label: "Format SQL",
                shortcut: Some("Ctrl+Shift+F"),
                keywords: &["beautify", "prettify"],
                category: "Query",
                icon: Some(ActionIcon::Format),
                children: &[],
            },
            Action {
                id: ACTION_EXPLAIN_QUERY,
                label: "Explain Query",
                shortcut: Some("Ctrl+Shift+E"),
                keywords: &["plan", "analyze", "debug"],
                category: "Query",
                icon: Some(ActionIcon::Explain),
                children: &[],
            },
            Action {
                id: ACTION_SAVE_QUERY,
                label: "Save Query",
                shortcut: Some("Ctrl+Shift+S"),
                keywords: &["bookmark", "saved"],
                category: "Query",
                icon: Some(ActionIcon::Apply),
                children: &[],
            },
            Action {
                id: ACTION_OPEN_COMMAND_PALETTE,
                label: "Open Command Palette",
                shortcut: Some("Ctrl+Shift+P"),
                keywords: &["palette", "search", "command", "shortcut"],
                category: "Help",
                icon: None,
                children: &[],
            },
            Action {
                id: ACTION_ABOUT,
                label: "About Shovel",
                shortcut: None,
                keywords: &["version", "info"],
                category: "Help",
                icon: Some(ActionIcon::Details),
                children: &[],
            },
            // ── Context (explorer) actions ──────────────────────
            // These have no palette entry (the palette iterates
            // `palette_actions()` below, which stops at id 100); they
            // exist in the catalog so `find_action` / the group arrays
            // stay the single source of truth for labels + icons too.
            Action {
                id: ACTION_TABLE_OPEN,
                label: "Open in editor",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Run),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_SELECT_ALL,
                label: "Select all rows",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Details),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_DUPLICATE,
                label: "Duplicate table",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Duplicate),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_RENAME,
                label: "Rename table",
                shortcut: Some("F2"),
                keywords: &["rename"],
                category: "Object",
                icon: Some(ActionIcon::Duplicate),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_TRUNCATE,
                label: "Truncate table",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Truncate),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_DROP,
                label: "Drop table",
                shortcut: Some("Delete"),
                keywords: &["drop"],
                category: "Object",
                icon: Some(ActionIcon::Delete),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_COPY_DDL,
                label: "Copy DDL",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::ExportSql),
                children: &[],
            },
            Action {
                id: ACTION_TABLE_COPY_INSERT,
                label: "Copy as INSERT template",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::ExportSql),
                children: &[],
            },
            Action {
                id: ACTION_OBJECT_REFRESH,
                label: "Refresh",
                shortcut: None,
                keywords: &["reload"],
                category: "Object",
                icon: Some(ActionIcon::Refresh),
                children: &[],
            },
            Action {
                id: ACTION_OBJECT_COPY_NAME,
                label: "Copy name",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Duplicate),
                children: &[],
            },
            Action {
                id: ACTION_OBJECT_COPY_QUALIFIED,
                label: "Copy qualified name",
                shortcut: None,
                keywords: &[],
                category: "Object",
                icon: Some(ActionIcon::Duplicate),
                children: &[],
            },
            Action {
                id: ACTION_SCHEMA_CREATE_TABLE,
                label: "Create table",
                shortcut: None,
                keywords: &["new", "create"],
                category: "Object",
                icon: Some(ActionIcon::CreateTable),
                children: &[],
            },
            Action {
                id: ACTION_CONNECTION_NEW_QUERY,
                label: "New Query",
                shortcut: None,
                keywords: &["query", "new"],
                category: "Connection",
                icon: Some(ActionIcon::SqlEditor),
                children: &[],
            },
            Action {
                id: ACTION_CONNECTION_DISCONNECT,
                label: "Disconnect",
                shortcut: None,
                keywords: &["close", "connection"],
                category: "Connection",
                icon: Some(ActionIcon::Close),
                children: &[],
            },
            Action {
                id: ACTION_COLUMN_FILTER_BY_VALUE,
                label: "Filter by value",
                shortcut: None,
                keywords: &["filter"],
                category: "Object",
                icon: Some(ActionIcon::Filter),
                children: &[],
            },
            Action {
                id: ACTION_COLUMN_SORT_ASC,
                label: "Sort ascending",
                shortcut: None,
                keywords: &["sort", "asc"],
                category: "Object",
                icon: Some(ActionIcon::Previous),
                children: &[],
            },
            Action {
                id: ACTION_COLUMN_SORT_DESC,
                label: "Sort descending",
                shortcut: None,
                keywords: &["sort", "desc"],
                category: "Object",
                icon: Some(ActionIcon::Next),
                children: &[],
            },
            Action {
                id: ACTION_VIEW_TABLE,
                label: "Table view",
                shortcut: None,
                keywords: &["view", "grid", "rows"],
                category: "View",
                icon: Some(ActionIcon::Details),
                children: &[],
            },
            Action {
                id: ACTION_VIEW_RECORDS,
                label: "Records view",
                shortcut: None,
                keywords: &["view", "records", "list", "dense"],
                category: "View",
                icon: Some(ActionIcon::History),
                children: &[],
            },
            Action {
                id: ACTION_VIEW_SINGLE_RECORD,
                label: "Single record view",
                shortcut: None,
                keywords: &["view", "single", "card", "detail"],
                category: "View",
                icon: Some(ActionIcon::Apply),
                children: &[],
            },
            Action {
                id: ACTION_VIEW_DETAILS,
                label: "Details view",
                shortcut: None,
                keywords: &["view", "details", "sidebar", "panel"],
                category: "View",
                icon: Some(ActionIcon::Details),
                children: &[],
            },
            Action {
                id: ACTION_TOGGLE_QUICK_FILTER,
                label: "Toggle quick filter",
                shortcut: Some("Ctrl+F"),
                keywords: &["filter", "where", "search", "quick"],
                category: "Query",
                icon: Some(ActionIcon::Filter),
                children: &[],
            },
        ]
    });

    &CATALOG
}

/// The palette-visible subset of the catalog: every action with an id
/// below `CONTEXT_ACTION_BASE` (i.e. the 18 workspace actions). Context
/// actions (id 100+) stay out of the palette but remain in the catalog
/// for lookup.
pub fn palette_actions() -> &'static [Action] {
    use std::sync::LazyLock;
    static PALETTE: LazyLock<Vec<Action>> = LazyLock::new(|| {
        action_catalog()
            .iter()
            .filter(|a| a.id.0 < CONTEXT_ACTION_BASE)
            .cloned()
            .collect()
    });
    &PALETTE
}

/// First id of the context-action range. Actions at or above this id are
/// explorer/menu actions, not palette entries.
pub const CONTEXT_ACTION_BASE: u64 = 100;

/// Dispatch an action by id. The execute closures are registered once at
/// startup against the same stable [`ActionId`] (see
/// [`crate::app_state::commands::command_list`]); this is the single
/// entry point the palette, toolbar and keyboard use to run an action.
pub fn dispatch_action(id: ActionId) {
    crate::app_state::commands::dispatch(id);
}

/// Look up an [`Action`] by id. Returns `None` for unknown ids so a
/// stale caller can degrade gracefully instead of panicking. Public
/// catalog lookup surface; currently exercised by tests and reserved for
/// toolbar / future menu consumers.
#[allow(dead_code)]
pub fn find_action(id: ActionId) -> Option<&'static Action> {
    action_catalog().iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_and_well_formed() {
        let catalog = action_catalog();
        assert!(catalog.len() >= 18, "spec requires 18+ palette actions");
        let mut seen = std::collections::HashSet::new();
        for action in catalog {
            assert!(seen.insert(action.id), "duplicate action id");
            assert!(!action.label.is_empty());
            assert!(!action.category.is_empty());
        }
    }

    #[test]
    fn catalog_contains_required_actions() {
        let ids: std::collections::HashSet<_> = action_catalog().iter().map(|a| a.id).collect();
        for required in [
            ACTION_NEW_CONNECTION,
            ACTION_OPEN_SETTINGS,
            ACTION_NEW_TAB,
            ACTION_CLOSE_TAB,
            ACTION_TOGGLE_EXPLORER,
            ACTION_TOGGLE_SAVED_QUERIES,
            ACTION_TOGGLE_HISTORY,
            ACTION_TOGGLE_SQL_EDITOR,
            ACTION_TOGGLE_AGENT_PANEL,
            ACTION_TOGGLE_CONNECTIONS,
            ACTION_REFRESH_EXPLORER,
            ACTION_RUN_QUERY,
            ACTION_FORMAT_SQL,
            ACTION_EXPLAIN_QUERY,
            ACTION_SAVE_QUERY,
            ACTION_OPEN_COMMAND_PALETTE,
            ACTION_ABOUT,
        ] {
            assert!(ids.contains(&required), "missing action: {required:?}");
        }
    }

    #[test]
    fn palette_shortcuts_are_populated_for_key_actions() {
        for id in [
            ACTION_RUN_QUERY,
            ACTION_FORMAT_SQL,
            ACTION_EXPLAIN_QUERY,
            ACTION_SAVE_QUERY,
            ACTION_OPEN_COMMAND_PALETTE,
            ACTION_CLOSE_TAB,
            ACTION_NEXT_TAB,
            ACTION_REFRESH_EXPLORER,
            ACTION_NEW_TAB,
        ] {
            let action = find_action(id).expect("action in catalog");
            assert!(
                action.shortcut.is_some(),
                "{label} should advertise its keybinding",
                label = action.label
            );
        }
    }

    #[test]
    fn object_groups_are_non_empty_and_reference_catalog() {
        let catalog_ids: std::collections::HashSet<_> =
            action_catalog().iter().map(|a| a.id).collect();
        for group in [
            TABLE_ACTIONS,
            COLUMN_ACTIONS,
            SCHEMA_ACTIONS,
            CONNECTION_ACTIONS,
        ] {
            assert!(!group.is_empty());
            for id in group {
                assert!(
                    catalog_ids.contains(id),
                    "group references an id not in the catalog: {id:?}"
                );
            }
        }
    }
}
