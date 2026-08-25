//! Global-search (Ctrl+K) state, snapshot indexing, and pure search helpers.
//!
//! The Ctrl+K overlay lives outside the workspace tree (mounted next to the
//! command palette in `app.rs`) but its result items point into workspace-
//! scoped data: open query tabs, the explorer tree, and the unified action
//! catalog. The workspace owns those signals, so the search overlay can't
//! subscribe to them directly — instead the workspace takes a snapshot of
//! the current state when the user presses Ctrl+K and writes it into the
//! globals defined here. The overlay then reads those snapshots and runs
//! the pure [`search_all`] filter against them on every keystroke.
//!
//! When the user picks a result, the overlay bumps
//! [`APP_GLOBAL_SEARCH_REQUEST`] with a discriminator; the workspace watches
//! that counter in its command-request effect and realises the pick against
//! the live tab/active_tab_id/explorer signals. This mirrors the existing
//! command-palette request flow (`APP_COMMAND_REQUEST`).
//!
//! Pure helpers ([`search_all`], [`split_match`]) intentionally take plain
//! references and return owned vectors so they can be exercised by
//! `#[test]` functions without a Dioxus runtime.

use dioxus::prelude::*;

use crate::app_state::actions::{Action, ActionId};
use models::ExplorerNodeKind;

/// Visibility flag for the Ctrl+K overlay. The overlay component reads
/// this to decide whether to mount; the workspace toggles it via
/// [`open_global_search`] / [`close_global_search`].
pub static APP_GLOBAL_SEARCH_OPEN: GlobalSignal<bool> = Signal::global(|| false);

/// Snapshot of open query tabs taken when Ctrl+K was pressed. Each item
/// is a slimmed-down view of the tab (no result handles, no signal maps)
/// so the overlay can sort/filter cheaply without reaching back into the
/// live `tabs` signal.
pub static APP_GLOBAL_SEARCH_TABS: GlobalSignal<Vec<GlobalSearchTabItem>> =
    Signal::global(Vec::new);

/// Snapshot of explorer objects (schemas/tables/views/columns/connections)
/// taken when Ctrl+K was pressed. The workspace flattens the loaded tree
/// into this list so the search index is a flat `Vec`, not a tree the
/// overlay has to walk on every keystroke.
pub static APP_GLOBAL_SEARCH_OBJECTS: GlobalSignal<Vec<GlobalSearchObjectItem>> =
    Signal::global(Vec::new);

/// Counter bumped when the overlay dispatches an "open this result"
/// request. The workspace watches this in its command-request effect and
/// realises the pick against live state.
pub static APP_GLOBAL_SEARCH_REQUEST: GlobalSignal<u64> = Signal::global(|| 0);

/// Stable discriminator for the request. Pair of (kind, hit_index).
/// See [`GlobalSearchRequestKind`] for the small enum the workspace
/// pattern-matches on.
pub static APP_GLOBAL_SEARCH_REQUEST_KIND: GlobalSignal<u64> = Signal::global(|| 0);

/// Per-request payload slot. The overlay writes a hit-specific id
/// (tab_id / session_id / action id) here before bumping
/// [`APP_GLOBAL_SEARCH_REQUEST`]; the workspace reads it inside the
/// command-request effect.
pub static APP_GLOBAL_SEARCH_REQUEST_PAYLOAD: GlobalSignal<u64> = Signal::global(|| 0);

/// Cap on the total number of result hits returned by [`search_all`]. The
/// overlay only renders so many anyway, and bounding the per-group work
/// keeps keystroke filtering snappy on large trees.
const MAX_RESULTS_PER_GROUP: usize = 25;

/// Maximum number of tab / object snapshots retained. The workspace
/// re-snapshots on every Ctrl+K, but if a future caller wants to keep
/// a long-lived index they can; the cap is here as a safety belt.
const MAX_SNAPSHOT_ITEMS: usize = 256;

// ── Request kinds (discriminator payload) ──────────────────────────────
//
// Stable ids. The workspace's command-request effect matches on these. A
// stale or unknown id is logged and skipped (same fallback policy as the
// command-palette dispatcher) so a mis-wired overlay can never silently
// no-op into a state desync.

pub const GLOBAL_SEARCH_OPEN_TAB: u64 = 1;
pub const GLOBAL_SEARCH_OPEN_OBJECT: u64 = 2;
pub const GLOBAL_SEARCH_RUN_ACTION: u64 = 3;

/// Slimmed-down tab record exposed to the search overlay. The overlay
/// only needs the fields it filters on + the data needed to realise the
/// "open this tab" request back in the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalSearchTabItem {
    pub tab_id: u64,
    pub session_id: u64,
    pub title: String,
}

/// Slimmed-down explorer object exposed to the search overlay. We keep
/// `session_id` so the workspace can route the open request back to the
/// right connection without re-walking the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalSearchObjectItem {
    pub session_id: u64,
    pub session_name: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: ExplorerNodeKind,
    pub schema: Option<String>,
}

/// Group ordering used by both the renderer and the pure search helper.
/// Tabs first (the user's current context), then objects split by kind,
/// then actions. The overlay never shows groups that came back empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SearchGroupKind {
    OpenTab,
    Connection,
    Schema,
    Table,
    View,
    Column,
    Action,
}

impl SearchGroupKind {
    /// Human-readable label rendered above each group.
    pub fn label(self) -> &'static str {
        match self {
            SearchGroupKind::OpenTab => "Open Tab",
            SearchGroupKind::Connection => "Connection",
            SearchGroupKind::Schema => "Schema",
            SearchGroupKind::Table => "Table",
            SearchGroupKind::View => "View",
            SearchGroupKind::Column => "Column",
            SearchGroupKind::Action => "Action",
        }
    }

    fn bucket_for_object(kind: ExplorerNodeKind) -> Self {
        match kind {
            ExplorerNodeKind::Schema => SearchGroupKind::Schema,
            ExplorerNodeKind::Table => SearchGroupKind::Table,
            ExplorerNodeKind::View | ExplorerNodeKind::MaterializedView => SearchGroupKind::View,
            ExplorerNodeKind::Column => SearchGroupKind::Column,
            // Sequences, functions, procedures, triggers fall under their
            // closest stable label so they still surface in search results
            // without inventing extra group headings.
            ExplorerNodeKind::Sequence => SearchGroupKind::Table,
            ExplorerNodeKind::Function
            | ExplorerNodeKind::Procedure
            | ExplorerNodeKind::Trigger => SearchGroupKind::View,
        }
    }
}

/// A single search hit. The renderer flattens hits into one scrollable
/// list, so the kind is part of the hit (not just the group) for icon
/// picking and tab/object dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchHit {
    Tab {
        tab_id: u64,
        session_id: u64,
        title: String,
    },
    Object {
        session_id: u64,
        session_name: String,
        name: String,
        qualified_name: String,
        kind: ExplorerNodeKind,
    },
    Action {
        id: ActionId,
        label: &'static str,
        category: &'static str,
    },
}

impl SearchHit {
    /// Primary text rendered next to the icon.
    pub fn display(&self) -> String {
        match self {
            SearchHit::Tab { title, .. } => title.clone(),
            SearchHit::Object { name, .. } => name.clone(),
            SearchHit::Action { label, .. } => (*label).to_string(),
        }
    }

    /// Secondary text rendered after the primary in the muted slot.
    pub fn secondary(&self) -> Option<String> {
        match self {
            SearchHit::Tab { session_id, .. } => Some(format!("session #{session_id}")),
            SearchHit::Object {
                qualified_name,
                session_name,
                ..
            } =>
                if qualified_name.is_empty() || qualified_name == session_name {
                    Some(session_name.clone())
                } else {
                    Some(format!("{qualified_name} · {session_name}"))
                },
            SearchHit::Action { category, .. } => Some((*category).to_string()),
        }
    }
}

/// A rendered group: a label + ordered list of hits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchGroup {
    pub kind: SearchGroupKind,
    pub label: &'static str,
    pub hits: Vec<SearchHit>,
}

/// Run the pure search across all three indices. Takes plain references
/// (no Dioxus signals) so the function is fully unit-testable: callers
/// read `APP_GLOBAL_SEARCH_TABS()` / `APP_GLOBAL_SEARCH_OBJECTS()` /
/// `action_catalog()` and pass the slices in. The action catalog is the
/// only `&'static` slice; tabs/objects come from the snapshot globals.
pub fn search_all(
    query: &str,
    tabs: &[GlobalSearchTabItem],
    objects: &[GlobalSearchObjectItem],
    actions: &[Action],
) -> Vec<SearchGroup> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let needle = trimmed.to_ascii_lowercase();

    // Pre-allocate one bucket per group kind. Vec ordering matches the
    // renderer header order (tabs first, then connections/schemas, then
    // tables/views/columns, then actions).
    let mut groups: Vec<SearchGroup> = vec![
        SearchGroup {
            kind: SearchGroupKind::OpenTab,
            label: SearchGroupKind::OpenTab.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::Connection,
            label: SearchGroupKind::Connection.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::Schema,
            label: SearchGroupKind::Schema.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::Table,
            label: SearchGroupKind::Table.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::View,
            label: SearchGroupKind::View.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::Column,
            label: SearchGroupKind::Column.label(),
            hits: Vec::new(),
        },
        SearchGroup {
            kind: SearchGroupKind::Action,
            label: SearchGroupKind::Action.label(),
            hits: Vec::new(),
        },
    ];

    // Connection objects are first-class here so the user can search for
    // a saved connection name ("prod-db") and jump to it. Connections are
    // rendered as a flat list even though they live inside the same
    // explorer tree; the snapshot flattening is the workspace's job.
    for object in objects {
        if object.kind != ExplorerNodeKind::Schema
            && !matches!(
                object.kind,
                ExplorerNodeKind::Table
                    | ExplorerNodeKind::View
                    | ExplorerNodeKind::MaterializedView
                    | ExplorerNodeKind::Column
                    | ExplorerNodeKind::Sequence
                    | ExplorerNodeKind::Function
                    | ExplorerNodeKind::Procedure
                    | ExplorerNodeKind::Trigger
            )
        {
            // Unknown / future kinds are skipped rather than mis-bucketed.
            continue;
        }
        if !object_matches(object, &needle) {
            continue;
        }
        let bucket = if connection_bucket_for(object, &needle) {
            // The match came from the connection name (or the object
            // is the connection's top-level node). Surface it as a
            // Connection hit so the user sees a "jump to my
            // connection" entry without having to scroll the Table
            // group.
            SearchGroupKind::Connection
        } else {
            SearchGroupKind::bucket_for_object(object.kind)
        };
        if let Some(group) = groups.iter_mut().find(|g| g.kind == bucket) {
            if group.hits.len() >= MAX_RESULTS_PER_GROUP {
                continue;
            }
            group.hits.push(SearchHit::Object {
                session_id: object.session_id,
                session_name: object.session_name.clone(),
                name: object.name.clone(),
                qualified_name: object.qualified_name.clone(),
                kind: object.kind,
            });
        }
    }

    for tab in tabs {
        if !tab.title.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g.kind == SearchGroupKind::OpenTab)
        {
            if group.hits.len() >= MAX_RESULTS_PER_GROUP {
                continue;
            }
            group.hits.push(SearchHit::Tab {
                tab_id: tab.tab_id,
                session_id: tab.session_id,
                title: tab.title.clone(),
            });
        }
    }

    for action in actions {
        if !action_matches(action, &needle) {
            continue;
        }
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g.kind == SearchGroupKind::Action)
        {
            if group.hits.len() >= MAX_RESULTS_PER_GROUP {
                continue;
            }
            group.hits.push(SearchHit::Action {
                id: action.id,
                label: action.label,
                category: action.category,
            });
        }
    }

    groups.into_iter().filter(|g| !g.hits.is_empty()).collect()
}

fn object_matches(object: &GlobalSearchObjectItem, needle: &str) -> bool {
    object.name.to_ascii_lowercase().contains(needle)
        || object.qualified_name.to_ascii_lowercase().contains(needle)
        || object.session_name.to_ascii_lowercase().contains(needle)
        || object
            .schema
            .as_deref()
            .map(|schema| schema.to_ascii_lowercase().contains(needle))
            .unwrap_or(false)
}

/// Decide whether an object hit should be surfaced in the
/// [`SearchGroupKind::Connection`] bucket rather than its natural
/// kind-based bucket. We bucket to Connection when the user's query
/// matched the *connection* itself rather than the object path — i.e.
/// either the object is the connection's top-level node (no schema in
/// its qualified name) or the needle is contained in the connection
/// name but not in the qualified name. That keeps "prod-pg" → the
/// prod-pg connection while "public.users" still routes to Table.
fn connection_bucket_for(object: &GlobalSearchObjectItem, needle: &str) -> bool {
    if object.qualified_name.is_empty() || object.qualified_name == object.session_name {
        return true;
    }
    let needle_in_session = object.session_name.to_ascii_lowercase().contains(needle);
    let needle_in_qualified = object.qualified_name.to_ascii_lowercase().contains(needle);
    needle_in_session && !needle_in_qualified
}

fn action_matches(action: &Action, needle: &str) -> bool {
    if action.label.to_ascii_lowercase().contains(needle) {
        return true;
    }
    action
        .keywords
        .iter()
        .any(|kw| kw.to_ascii_lowercase().contains(needle))
}

/// Highlight the matched substring inside `text`. Returns a sequence of
/// `(text, matched)` pairs so the renderer can wrap the matched fragment
/// in a styled span. Mirrors the explorer tree's `split_match` helper so
/// the search overlay's highlight behaviour is consistent with the
/// in-tree match-highlight.
pub fn split_match(text: &str, query: &str) -> Vec<(String, bool)> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let needle = trimmed.to_ascii_lowercase();
    let haystack = text.to_ascii_lowercase();

    let mut out: Vec<(String, bool)> = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = haystack[cursor..].find(&needle) {
        let start = cursor + rel;
        let end = start + needle.len();
        if start > cursor {
            out.push((text[cursor..start].to_string(), false));
        }
        out.push((text[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < text.len() {
        out.push((text[cursor..].to_string(), false));
    }
    if out.is_empty() {
        out.push((text.to_string(), false));
    }
    out
}

// ── Workspace-facing API ───────────────────────────────────────────────

pub fn open_global_search() {
    *APP_GLOBAL_SEARCH_OPEN.write() = true;
}

pub fn close_global_search() {
    if APP_GLOBAL_SEARCH_OPEN() {
        *APP_GLOBAL_SEARCH_OPEN.write() = false;
    }
}

/// Pure helper: cap a snapshot `Vec` at [`MAX_SNAPSHOT_ITEMS`] in place
/// (keeps the first N items, drops the tail). Extracted so callers
/// without a Dioxus runtime — unit tests in particular — can exercise
/// the truncation logic on a plain `&mut Vec<T>`.
fn truncate_snapshot<T>(items: &mut Vec<T>) {
    if items.len() > MAX_SNAPSHOT_ITEMS {
        items.truncate(MAX_SNAPSHOT_ITEMS);
    }
}

/// Snapshot the current open tabs into the search overlay's global. The
/// workspace calls this right before opening the overlay so the index
/// reflects whatever was loaded at the moment Ctrl+K was pressed. The
/// snapshot is bounded by [`MAX_SNAPSHOT_ITEMS`].
pub fn set_global_search_tabs(tabs: Vec<GlobalSearchTabItem>) {
    let mut truncated = tabs;
    truncate_snapshot(&mut truncated);
    *APP_GLOBAL_SEARCH_TABS.write() = truncated;
}

/// Snapshot the current explorer objects into the search overlay's
/// global. Same lifecycle as [`set_global_search_tabs`].
pub fn set_global_search_objects(objects: Vec<GlobalSearchObjectItem>) {
    let mut truncated = objects;
    truncate_snapshot(&mut truncated);
    *APP_GLOBAL_SEARCH_OBJECTS.write() = truncated;
}

/// Bump the open-request counter with the given discriminator. The
/// workspace's command-request effect watches this and reacts.
pub fn dispatch_global_search_request(kind: u64) {
    *APP_GLOBAL_SEARCH_REQUEST_KIND.write() = kind;
    let mut counter = APP_GLOBAL_SEARCH_REQUEST.write();
    *counter = counter.wrapping_add(1);
}

/// Convenience wrapper: snapshot the live state the workspace has at
/// hand and open the overlay in one step. The workspace calls this from
/// its Ctrl+K handler so the snapshot is taken in the same effect that
/// realises the shortcut.
pub fn open_global_search_with_snapshots(
    tabs: Vec<GlobalSearchTabItem>,
    objects: Vec<GlobalSearchObjectItem>,
) {
    set_global_search_tabs(tabs);
    set_global_search_objects(objects);
    open_global_search();
}

// ── Unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{actions as acts, actions::action_catalog};
    use models::ExplorerNodeKind;

    fn tab(id: u64, session_id: u64, title: &str) -> GlobalSearchTabItem {
        GlobalSearchTabItem {
            tab_id: id,
            session_id,
            title: title.to_string(),
        }
    }

    fn obj(
        session_id: u64,
        session_name: &str,
        name: &str,
        qualified_name: &str,
        kind: ExplorerNodeKind,
    ) -> GlobalSearchObjectItem {
        GlobalSearchObjectItem {
            session_id,
            session_name: session_name.to_string(),
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            kind,
            schema: None,
        }
    }

    fn catalog() -> Vec<Action> {
        action_catalog()
            .iter()
            .filter(|a| a.id.0 < acts::CONTEXT_ACTION_BASE)
            .cloned()
            .collect()
    }

    #[test]
    fn empty_query_returns_no_groups() {
        let tabs = vec![tab(1, 1, "Q1")];
        let objects = vec![obj(
            1,
            "db",
            "users",
            "public.users",
            ExplorerNodeKind::Table,
        )];
        let groups = search_all("", &tabs, &objects, &catalog());
        assert!(groups.is_empty(), "empty query must yield no groups");
    }

    #[test]
    fn whitespace_only_query_returns_no_groups() {
        let tabs = vec![tab(1, 1, "Q1")];
        let groups = search_all("   \t  ", &tabs, &[], &catalog());
        assert!(groups.is_empty(), "whitespace-only query yields no groups");
    }

    #[test]
    fn query_matches_open_tab_by_title_case_insensitively() {
        let tabs = vec![tab(7, 1, "Find recent orders")];
        let groups = search_all("RECENT", &tabs, &[], &catalog());
        let tab_group = groups
            .iter()
            .find(|g| g.kind == SearchGroupKind::OpenTab)
            .expect("open tab group present");
        assert_eq!(tab_group.hits.len(), 1);
        match &tab_group.hits[0] {
            SearchHit::Tab { tab_id, title, .. } => {
                assert_eq!(*tab_id, 7);
                assert_eq!(title, "Find recent orders");
            }
            _ => panic!("expected tab hit"),
        }
    }

    #[test]
    fn query_matches_object_by_qualified_name() {
        let objects = vec![obj(
            1,
            "db",
            "users",
            "public.users",
            ExplorerNodeKind::Table,
        )];
        let groups = search_all("public.user", &[], &objects, &catalog());
        let table_group = groups
            .iter()
            .find(|g| g.kind == SearchGroupKind::Table)
            .expect("table group present");
        assert_eq!(table_group.hits.len(), 1);
    }

    #[test]
    fn query_matches_object_by_session_name() {
        let objects = vec![obj(
            1,
            "prod-pg",
            "users",
            "public.users",
            ExplorerNodeKind::Table,
        )];
        let groups = search_all("prod-pg", &[], &objects, &catalog());
        let conn_group = groups
            .iter()
            .find(|g| g.kind == SearchGroupKind::Connection)
            .expect("connection group present");
        assert_eq!(conn_group.hits.len(), 1);
    }

    #[test]
    fn action_group_matches_label_and_keywords() {
        let groups = search_all("format", &[], &[], &catalog());
        let action_group = groups
            .iter()
            .find(|g| g.kind == SearchGroupKind::Action)
            .expect("action group present");
        let has_format = action_group.hits.iter().any(|hit| match hit {
            SearchHit::Action { id, .. } => *id == acts::ACTION_FORMAT_SQL,
            _ => false,
        });
        assert!(has_format, "Format SQL action should appear for 'format'");

        let by_keyword = search_all("panel", &[], &[], &catalog());
        let action_group = by_keyword
            .iter()
            .find(|g| g.kind == SearchGroupKind::Action)
            .expect("action group present");
        // Toggle Explorer / Saved Queries / History all share the "panel"
        // keyword — at least one should appear.
        assert!(!action_group.hits.is_empty());
    }

    #[test]
    fn groups_are_ordered_with_tabs_first_and_actions_last() {
        let tabs = vec![tab(1, 1, "recent orders")];
        let objects = vec![obj(
            1,
            "db",
            "users",
            "public.users",
            ExplorerNodeKind::Table,
        )];
        let groups = search_all("e", &tabs, &objects, &catalog());
        let ordering: Vec<SearchGroupKind> = groups.iter().map(|g| g.kind).collect();
        // Tabs always come before tables / actions, and actions always
        // come last. The exact mid-ordering can shift as the buckets
        // move, so we only assert the meaningful boundary invariants.
        if let Some(tab_idx) = groups
            .iter()
            .position(|g| g.kind == SearchGroupKind::OpenTab)
            && let Some(table_idx) = groups.iter().position(|g| g.kind == SearchGroupKind::Table)
        {
            assert!(tab_idx < table_idx, "tabs must come before tables");
        }
        if let Some(action_idx) = groups
            .iter()
            .position(|g| g.kind == SearchGroupKind::Action)
        {
            let last = ordering.len().saturating_sub(1);
            assert_eq!(action_idx, last, "actions must be the last group");
        }
    }

    #[test]
    fn empty_groups_are_dropped_from_the_result() {
        let tabs = vec![tab(1, 1, "alpha")];
        let groups = search_all("alpha", &tabs, &[], &catalog());
        // No objects indexed, no connection match → those groups must
        // not appear in the output even though they're allocated in the
        // working `Vec`.
        assert!(
            groups
                .iter()
                .all(|g| g.kind != SearchGroupKind::Table && g.kind != SearchGroupKind::Connection)
        );
    }

    #[test]
    fn per_group_cap_bounds_hits() {
        let mut objects = Vec::new();
        for i in 0..(MAX_RESULTS_PER_GROUP + 5) {
            objects.push(obj(
                1,
                "db",
                &format!("users{i}"),
                &format!("public.users{i}"),
                ExplorerNodeKind::Table,
            ));
        }
        let groups = search_all("users", &[], &objects, &catalog());
        let table_group = groups
            .iter()
            .find(|g| g.kind == SearchGroupKind::Table)
            .expect("table group present");
        assert_eq!(table_group.hits.len(), MAX_RESULTS_PER_GROUP);
    }

    #[test]
    fn split_match_preserves_text_and_marks_matches() {
        let segs = split_match("Find recent orders", "recent");
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "Find recent orders");
        // The matched span should be exactly "recent".
        let matched: String = segs
            .iter()
            .filter(|(_, m)| *m)
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(matched, "recent");
    }

    #[test]
    fn split_match_returns_single_unmatched_segment_on_empty_needle() {
        let segs = split_match("hello", "");
        assert_eq!(segs.len(), 1);
        assert!(!segs[0].1);
        assert_eq!(segs[0].0, "hello");
    }

    #[test]
    fn split_match_returns_single_unmatched_segment_on_no_hit() {
        let segs = split_match("hello", "zzz");
        assert_eq!(segs.len(), 1);
        assert!(!segs[0].1);
        assert_eq!(segs[0].0, "hello");
    }

    #[test]
    fn split_match_handles_multiple_occurrences() {
        let segs = split_match("user_user", "user");
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "user_user");
        let matched_count = segs.iter().filter(|(_, m)| *m).count();
        assert_eq!(matched_count, 2, "should mark both occurrences");
    }

    #[test]
    fn snapshot_writes_truncate_at_cap() {
        let mut tabs = Vec::new();
        for i in 0..(MAX_SNAPSHOT_ITEMS + 10) {
            tabs.push(tab(i as u64, 1, &format!("Q{i}")));
        }
        truncate_snapshot(&mut tabs);
        assert_eq!(tabs.len(), MAX_SNAPSHOT_ITEMS);
        // First tab should be the oldest (0) — we never sorted, only
        // truncated, so the first MAX_SNAPSHOT_ITEMS survive.
        assert_eq!(tabs[0].tab_id, 0);
        assert_eq!(tabs.last().unwrap().tab_id, (MAX_SNAPSHOT_ITEMS - 1) as u64);
    }

    #[test]
    fn snapshot_truncate_is_a_noop_under_the_cap() {
        let mut tabs = vec![tab(1, 1, "alpha"), tab(2, 1, "beta")];
        truncate_snapshot(&mut tabs);
        assert_eq!(tabs.len(), 2);
    }
}
