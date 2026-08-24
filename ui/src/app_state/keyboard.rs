//! Centralized keyboard-shortcut dispatch.
//!
//! The Dioxus desktop UI installs two onkeydown handlers — one on
//! the workspace root and one on the SQL editor textarea. Both
//! share a single pure [`match_key_combination`] function that
//! turns a `(Key, Modifiers)` pair into a [`ShortcutAction`]. The
//! call sites then realise the action using whichever signals or
//! callbacks they have in scope, which keeps this module free of
//! Dioxus signals / closures and fully unit-testable.

use dioxus::prelude::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    FocusEditor,
    FormatSql,
    NewTab,
    CloseTab,
    NextTab,
    RefreshExplorer,
    FocusFilterPanel,
    SaveQuery,
    CloseOverlay,
    CommandPalette,
    /// Ctrl+K — global search / command palette stub. The workspace
    /// dispatcher opens the command palette (the closest existing global
    /// search surface); a dedicated search overlay can reuse this id later.
    GlobalSearch,
    /// F2 — rename the currently selected explorer object.
    RenameSelected,
    /// Delete — drop the currently selected explorer object.
    DeleteSelected,
}

impl ShortcutAction {
    /// Map a keyboard action to the stable [`ActionId`] in the unified
    /// Action catalog. This is the single place that says
    /// "this key combo = this action"; the workspace dispatcher resolves
    /// the action id through the shared registry where possible. Returns
    /// `None` for actions that are purely local (focus requests, overlay
    /// close) and have no catalog entry.
    pub fn to_action_id(self) -> Option<crate::app_state::actions::ActionId> {
        use crate::app_state::actions as acts;
        Some(match self {
            ShortcutAction::FormatSql => acts::ACTION_FORMAT_SQL,
            ShortcutAction::NewTab => acts::ACTION_NEW_TAB,
            ShortcutAction::CloseTab => acts::ACTION_CLOSE_TAB,
            ShortcutAction::NextTab => acts::ACTION_NEXT_TAB,
            ShortcutAction::RefreshExplorer => acts::ACTION_REFRESH_EXPLORER,
            ShortcutAction::SaveQuery => acts::ACTION_SAVE_QUERY,
            ShortcutAction::CommandPalette => acts::ACTION_OPEN_COMMAND_PALETTE,
            // These actions are realised against local signals (editor
            // focus, filter focus, overlay dismissal) or the selected
            // explorer node, not the global catalog — the dispatcher
            // handles them directly.
            ShortcutAction::FocusEditor
            | ShortcutAction::FocusFilterPanel
            | ShortcutAction::CloseOverlay
            | ShortcutAction::GlobalSearch
            | ShortcutAction::RenameSelected
            | ShortcutAction::DeleteSelected => return None,
        })
    }
}

/// Returns true when the Ctrl-or-Meta modifier is held. We treat
/// Cmd on macOS the same as Ctrl everywhere — Shovel is a desktop
/// client and the muscle memory should be uniform.
pub fn ctrl_or_meta(modifiers: Modifiers) -> bool {
    modifiers.contains(Modifiers::CONTROL) || modifiers.contains(Modifiers::META)
}

/// Pure key-combination matcher. Returns the abstract
/// [`ShortcutAction`] for the given key/modifier pair, or `None`
/// if the combination is not a recognized shortcut.
pub fn match_key_combination(key: &Key, modifiers: Modifiers) -> Option<ShortcutAction> {
    let ctrl = ctrl_or_meta(modifiers);
    let shift = modifiers.contains(Modifiers::SHIFT);
    let alt = modifiers.contains(Modifiers::ALT);

    if alt {
        return None;
    }

    if matches!(key, Key::Escape) && !ctrl {
        return Some(ShortcutAction::CloseOverlay);
    }

    if matches!(key, Key::F5) {
        return Some(ShortcutAction::RefreshExplorer);
    }

    // F2 (rename) and Delete (drop object) act on the selected explorer
    // object; they intentionally work without Ctrl so they read as
    // native file-manager idioms. Both are no-ops when focus is inside
    // the SQL editor (which owns its own text-editing keys).
    if matches!(key, Key::F2) && !ctrl {
        return Some(ShortcutAction::RenameSelected);
    }

    if matches!(key, Key::Delete) && !ctrl {
        return Some(ShortcutAction::DeleteSelected);
    }

    if !ctrl {
        return None;
    }

    if matches!(key, Key::Tab) {
        return Some(ShortcutAction::NextTab);
    }

    let character = match key {
        Key::Character(c) => c.as_str(),
        _ => return None,
    };

    let eq_ci = |expected: char| {
        let mut buf = [0u8; 4];
        let s = expected.encode_utf8(&mut buf);
        character.eq_ignore_ascii_case(s)
    };

    if eq_ci('F') && shift {
        return Some(ShortcutAction::FormatSql);
    }
    if eq_ci('E') && !shift {
        return Some(ShortcutAction::FocusEditor);
    }
    if eq_ci('S') && shift {
        return Some(ShortcutAction::SaveQuery);
    }
    if eq_ci('P') && shift {
        return Some(ShortcutAction::CommandPalette);
    }
    if eq_ci('K') && !shift {
        return Some(ShortcutAction::GlobalSearch);
    }
    if eq_ci('T') || eq_ci('N') {
        return Some(ShortcutAction::NewTab);
    }
    if eq_ci('W') {
        return Some(ShortcutAction::CloseTab);
    }
    if eq_ci('F') {
        return Some(ShortcutAction::FocusFilterPanel);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> Modifiers {
        Modifiers::CONTROL
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers::CONTROL | Modifiers::SHIFT
    }

    fn meta() -> Modifiers {
        Modifiers::META
    }

    #[test]
    fn ctrl_or_meta_recognises_both_modifier_keys() {
        assert!(ctrl_or_meta(ctrl()));
        assert!(ctrl_or_meta(meta()));
        assert!(!ctrl_or_meta(Modifiers::empty()));
        assert!(!ctrl_or_meta(Modifiers::SHIFT));
    }

    #[test]
    fn ctrl_enter_does_not_fall_through_to_the_root_dispatcher() {
        // Ctrl+Enter is owned by the SQL editor's textarea-local
        // handler; the root-level matcher must leave it alone so
        // the two handlers do not double-fire.
        assert_eq!(match_key_combination(&Key::Enter, ctrl()), None);
    }

    #[test]
    fn ctrl_e_maps_to_focus_editor() {
        assert_eq!(
            match_key_combination(&Key::Character("e".into()), ctrl()),
            Some(ShortcutAction::FocusEditor)
        );
        // Uppercase E with shift is a different combo — it must
        // not silently fall through to focus-editor. The
        // workspace does not currently assign Ctrl+Shift+E at
        // the root level (the editor claims it for explain);
        // the matcher just needs to return `None` so the root
        // handler does not double-fire.
        assert_eq!(
            match_key_combination(&Key::Character("E".into()), ctrl_shift()),
            None
        );
    }

    #[test]
    fn ctrl_shift_f_maps_to_format_sql() {
        assert_eq!(
            match_key_combination(&Key::Character("f".into()), ctrl_shift()),
            Some(ShortcutAction::FormatSql)
        );
        assert_eq!(
            match_key_combination(&Key::Character("F".into()), ctrl_shift()),
            Some(ShortcutAction::FormatSql)
        );
        // Plain Ctrl+F (no shift) is find-in-results, not
        // format.
        assert_eq!(
            match_key_combination(&Key::Character("f".into()), ctrl()),
            Some(ShortcutAction::FocusFilterPanel)
        );
    }

    #[test]
    fn ctrl_t_and_ctrl_n_and_their_shift_variants_open_a_new_tab() {
        for key in ["t", "T", "n", "N"] {
            assert_eq!(
                match_key_combination(&Key::Character(key.into()), ctrl()),
                Some(ShortcutAction::NewTab),
                "Ctrl+{key} should open a new tab"
            );
            assert_eq!(
                match_key_combination(&Key::Character(key.into()), ctrl_shift()),
                Some(ShortcutAction::NewTab),
                "Ctrl+Shift+{key} should also open a new tab"
            );
        }
    }

    #[test]
    fn ctrl_w_closes_the_active_tab() {
        assert_eq!(
            match_key_combination(&Key::Character("w".into()), ctrl()),
            Some(ShortcutAction::CloseTab)
        );
        // Ctrl+Shift+W also closes the active tab — we don't
        // model multi-window close, so the shift variant
        // collapses to the same action.
        assert_eq!(
            match_key_combination(&Key::Character("W".into()), ctrl_shift()),
            Some(ShortcutAction::CloseTab)
        );
    }

    #[test]
    fn ctrl_tab_advances_to_next_tab() {
        assert_eq!(
            match_key_combination(&Key::Tab, ctrl()),
            Some(ShortcutAction::NextTab)
        );
    }

    #[test]
    fn f5_refreshes_the_explorer() {
        assert_eq!(
            match_key_combination(&Key::F5, Modifiers::empty()),
            Some(ShortcutAction::RefreshExplorer)
        );
    }

    #[test]
    fn ctrl_f_focuses_the_result_filter_panel() {
        assert_eq!(
            match_key_combination(&Key::Character("f".into()), ctrl()),
            Some(ShortcutAction::FocusFilterPanel)
        );
    }

    #[test]
    fn ctrl_shift_s_saves_the_active_query() {
        assert_eq!(
            match_key_combination(&Key::Character("s".into()), ctrl_shift()),
            Some(ShortcutAction::SaveQuery)
        );
        // Plain Ctrl+S is the editor-local save (workspace does
        // not own it). The matcher should leave it alone so the
        // editor's handler keeps the muscle memory.
        assert_eq!(
            match_key_combination(&Key::Character("s".into()), ctrl()),
            None
        );
    }

    #[test]
    fn ctrl_shift_p_opens_the_command_palette() {
        assert_eq!(
            match_key_combination(&Key::Character("p".into()), ctrl_shift()),
            Some(ShortcutAction::CommandPalette)
        );
        assert_eq!(
            match_key_combination(&Key::Character("P".into()), ctrl_shift()),
            Some(ShortcutAction::CommandPalette)
        );
        // Plain Ctrl+P (no shift) is reserved for the host-level
        // print dialog and should not be hijacked by the palette.
        assert_eq!(
            match_key_combination(&Key::Character("p".into()), ctrl()),
            None
        );
    }

    #[test]
    fn escape_closes_overlay_without_any_modifier() {
        assert_eq!(
            match_key_combination(&Key::Escape, Modifiers::empty()),
            Some(ShortcutAction::CloseOverlay)
        );
        // Esc with Ctrl is reserved for host-level shortcuts and
        // should not bubble through our handler.
        assert_eq!(match_key_combination(&Key::Escape, ctrl()), None);
    }

    #[test]
    fn alt_combinations_are_left_to_the_host() {
        // Alt+letter shortcuts are owned by the desktop window
        // (e.g. menu mnemonics) and must not be hijacked.
        assert_eq!(
            match_key_combination(&Key::Character("n".into()), Modifiers::ALT),
            None
        );
        assert_eq!(
            match_key_combination(
                &Key::Character("f".into()),
                Modifiers::ALT | Modifiers::CONTROL
            ),
            None
        );
    }

    #[test]
    fn unmodified_letters_and_function_keys_do_not_match() {
        assert_eq!(
            match_key_combination(&Key::Character("a".into()), Modifiers::empty()),
            None
        );
        assert_eq!(match_key_combination(&Key::Enter, Modifiers::empty()), None);
        assert_eq!(match_key_combination(&Key::F1, Modifiers::empty()), None);
    }

    #[test]
    fn ctrl_k_maps_to_global_search() {
        assert_eq!(
            match_key_combination(&Key::Character("k".into()), ctrl()),
            Some(ShortcutAction::GlobalSearch)
        );
        assert_eq!(
            match_key_combination(&Key::Character("K".into()), ctrl()),
            Some(ShortcutAction::GlobalSearch)
        );
    }

    #[test]
    fn f2_maps_to_rename_selected_without_modifier() {
        assert_eq!(
            match_key_combination(&Key::F2, Modifiers::empty()),
            Some(ShortcutAction::RenameSelected)
        );
        // Ctrl+F2 stays unassigned so it never double-fires.
        assert_eq!(match_key_combination(&Key::F2, ctrl()), None);
    }

    #[test]
    fn delete_maps_to_delete_selected_without_modifier() {
        assert_eq!(
            match_key_combination(&Key::Delete, Modifiers::empty()),
            Some(ShortcutAction::DeleteSelected)
        );
    }

    #[test]
    fn shortcut_actions_resolve_through_the_action_registry() {
        use crate::app_state::actions as acts;
        assert_eq!(
            ShortcutAction::FormatSql.to_action_id(),
            Some(acts::ACTION_FORMAT_SQL)
        );
        assert_eq!(
            ShortcutAction::CommandPalette.to_action_id(),
            Some(acts::ACTION_OPEN_COMMAND_PALETTE)
        );
        assert_eq!(
            ShortcutAction::CloseTab.to_action_id(),
            Some(acts::ACTION_CLOSE_TAB)
        );
        // Local-only actions resolve to no catalog id.
        assert_eq!(ShortcutAction::FocusEditor.to_action_id(), None);
        assert_eq!(ShortcutAction::GlobalSearch.to_action_id(), None);
        assert_eq!(ShortcutAction::RenameSelected.to_action_id(), None);
        assert_eq!(ShortcutAction::DeleteSelected.to_action_id(), None);
    }
}
