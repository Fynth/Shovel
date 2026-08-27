#![cfg_attr(not(test), allow(dead_code))]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKey {
    Escape,
    Tab,
    ShiftTab,
    Enter,
    ArrowUp,
    ArrowDown,
    Character(char),
    CtrlSpace,
    AltRBracket,
    AltLBracket,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeyAction {
    Pass,
    CloseMenu,
    DismissGhost,
    CycleGhostNext,
    CycleGhostPrev,
    MenuMove(i32),
    AcceptMenu,
    AcceptGhost,
    Indent { shift: bool },
    ForceMenu,
}

pub fn editor_completion_action(
    key: CompletionKey,
    menu_open: bool,
    ghost_visible: bool,
) -> EditorKeyAction {
    match key {
        CompletionKey::Escape =>
            if menu_open {
                EditorKeyAction::CloseMenu
            } else if ghost_visible {
                EditorKeyAction::DismissGhost
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::AltRBracket =>
            if ghost_visible {
                EditorKeyAction::CycleGhostNext
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::AltLBracket =>
            if ghost_visible {
                EditorKeyAction::CycleGhostPrev
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::ArrowUp =>
            if menu_open {
                EditorKeyAction::MenuMove(-1)
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::ArrowDown =>
            if menu_open {
                EditorKeyAction::MenuMove(1)
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::Tab =>
            if menu_open {
                EditorKeyAction::AcceptMenu
            } else if ghost_visible {
                EditorKeyAction::AcceptGhost
            } else {
                EditorKeyAction::Indent { shift: false }
            },
        CompletionKey::ShiftTab => EditorKeyAction::Indent { shift: true },
        CompletionKey::Enter =>
            if menu_open {
                EditorKeyAction::AcceptMenu
            } else {
                EditorKeyAction::Pass
            },
        CompletionKey::CtrlSpace => EditorKeyAction::ForceMenu,
        CompletionKey::Character(_) | CompletionKey::Other => EditorKeyAction::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::{CompletionKey, EditorKeyAction, editor_completion_action};

    #[test]
    fn keyboard_priority_table() {
        use CompletionKey::*;
        use EditorKeyAction::*;
        assert_eq!(editor_completion_action(Escape, true, true), CloseMenu);
        assert_eq!(editor_completion_action(Escape, false, true), DismissGhost);
        assert_eq!(editor_completion_action(Tab, true, true), AcceptMenu);
        assert_eq!(editor_completion_action(Tab, false, true), AcceptGhost);
        assert_eq!(
            editor_completion_action(Tab, false, false),
            Indent { shift: false }
        );
        assert_eq!(editor_completion_action(Enter, true, true), AcceptMenu);
        assert_eq!(editor_completion_action(Enter, false, true), Pass);
        assert_eq!(editor_completion_action(ArrowUp, true, false), MenuMove(-1));
        assert_eq!(
            editor_completion_action(ArrowDown, true, false),
            MenuMove(1)
        );
        assert_eq!(
            editor_completion_action(AltRBracket, true, true),
            CycleGhostNext
        );
        assert_eq!(
            editor_completion_action(AltLBracket, false, true),
            CycleGhostPrev
        );
        assert_eq!(editor_completion_action(AltRBracket, true, false), Pass);
        assert_eq!(editor_completion_action(CtrlSpace, false, false), ForceMenu);
        assert_eq!(
            editor_completion_action(ShiftTab, true, true),
            Indent { shift: true }
        );
        assert_eq!(editor_completion_action(Character('a'), true, true), Pass);
        assert_eq!(editor_completion_action(Other, false, false), Pass);
    }
}
