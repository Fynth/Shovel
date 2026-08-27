use crate::completion::{
    keyboard::CompletionKey,
    keywords::{CompletionItem, CompletionKind},
};
use dioxus::prelude::*;
use models::{ExplorerNode, ExplorerNodeKind};
use serde::Deserialize;

pub const MENU_WIDTH: f64 = 240.0;
pub const MENU_MAX_HEIGHT: f64 = 280.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaretAnchor {
    pub x: f64,
    pub y: f64,
    pub line_height: f64,
    pub editor_width: f64,
    pub editor_height: f64,
}

pub fn autocomplete_offset(
    caret_x: f64,
    caret_y: f64,
    line_height: f64,
    menu_height: f64,
    editor_height: f64,
    editor_width: f64,
    menu_width: f64,
) -> (f64, f64, bool) {
    let left = caret_x.min(editor_width - menu_width).max(0.0);
    if caret_y + line_height + menu_height > editor_height {
        (left, (caret_y - menu_height).max(0.0), true)
    } else {
        (left, caret_y + line_height, false)
    }
}

pub fn menu_height_for_items(count: usize) -> f64 {
    ((count.clamp(1, 12) as f64) * 32.0).min(MENU_MAX_HEIGHT)
}

pub fn caret_anchor_script(editor_id: &str) -> String {
    format!(
        r#"
        (() => {{
            const editor = document.getElementById({editor_id:?});
            const empty = {{x: 0, y: 0, lineHeight: 18, editorWidth: 0, editorHeight: 0}};
            if (!editor) {{
                return empty;
            }}
            const root = editor.closest(".sql-editor");
            if (!root) {{
                return empty;
            }}
            const style = window.getComputedStyle(editor);
            const pre = document.createElement("pre");
            const copy = [
                "fontFamily", "fontSize", "fontWeight", "fontStyle",
                "letterSpacing", "textTransform", "wordSpacing", "textIndent",
                "tabSize", "MozTabSize", "whiteSpace", "wordWrap", "lineHeight",
                "boxSizing", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft",
                "borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth"
            ];
            for (const prop of copy) {{
                pre.style[prop] = style[prop];
            }}
            pre.style.position = "absolute";
            pre.style.visibility = "hidden";
            pre.style.pointerEvents = "none";
            pre.style.overflow = "hidden";
            pre.style.margin = "0";
            const rootRect = root.getBoundingClientRect();
            const editorRect = editor.getBoundingClientRect();
            pre.style.left = (editorRect.left - rootRect.left) + "px";
            pre.style.top = (editorRect.top - rootRect.top) + "px";
            pre.style.width = editor.clientWidth + "px";
            pre.style.height = editor.clientHeight + "px";
            const value = editor.value ?? "";
            const caret = editor.selectionStart ?? value.length;
            pre.textContent = value.slice(0, caret);
            const marker = document.createElement("span");
            marker.textContent = "\u200b";
            pre.appendChild(marker);
            root.appendChild(pre);
            pre.scrollTop = editor.scrollTop;
            pre.scrollLeft = editor.scrollLeft;
            const markerRect = marker.getBoundingClientRect();
            const lineHeight = Number.parseFloat(style.lineHeight)
                || (Number.parseFloat(style.fontSize) * 1.64)
                || 18;
            const result = {{
                x: markerRect.left - rootRect.left,
                y: markerRect.top - rootRect.top,
                lineHeight,
                editorWidth: root.clientWidth,
                editorHeight: root.clientHeight
            }};
            pre.remove();
            return result;
        }})()
        "#
    )
}

pub fn map_completion_key(event: &KeyboardEvent) -> CompletionKey {
    let mods = event.modifiers();
    let ctrl = mods.contains(Modifiers::CONTROL) || mods.contains(Modifiers::META);
    let alt = mods.contains(Modifiers::ALT);
    let shift = mods.contains(Modifiers::SHIFT);
    let key = event.key();
    let code = event.code();

    if ctrl && (code == Code::Space || matches!(key, Key::Character(ref ch) if ch == " ")) {
        return CompletionKey::CtrlSpace;
    }
    if alt && (code == Code::BracketRight || matches!(key, Key::Character(ref ch) if ch == "]")) {
        return CompletionKey::AltRBracket;
    }
    if alt && (code == Code::BracketLeft || matches!(key, Key::Character(ref ch) if ch == "[")) {
        return CompletionKey::AltLBracket;
    }
    if matches!(key, Key::Tab) || code == Code::Tab {
        return if shift {
            CompletionKey::ShiftTab
        } else {
            CompletionKey::Tab
        };
    }

    match key {
        Key::Escape => CompletionKey::Escape,
        Key::Enter => CompletionKey::Enter,
        Key::ArrowUp => CompletionKey::ArrowUp,
        Key::ArrowDown => CompletionKey::ArrowDown,
        Key::Character(ch) => ch
            .chars()
            .next()
            .filter(|_| ch.chars().count() == 1)
            .map(CompletionKey::Character)
            .unwrap_or(CompletionKey::Other),
        _ => CompletionKey::Other,
    }
}

pub fn table_missing_columns(nodes: &[ExplorerNode], schema: Option<&str>, table: &str) -> bool {
    find_relation(nodes, schema, table).is_some_and(|node| {
        !node
            .children
            .iter()
            .any(|child| child.kind == ExplorerNodeKind::Column)
    })
}

fn find_relation<'a>(
    nodes: &'a [ExplorerNode],
    schema: Option<&str>,
    table: &str,
) -> Option<&'a ExplorerNode> {
    for node in nodes {
        if is_relation(node.kind)
            && node.name.eq_ignore_ascii_case(table)
            && schema_matches(node.schema.as_deref(), schema)
        {
            return Some(node);
        }
        if let Some(found) = find_relation(&node.children, schema, table) {
            return Some(found);
        }
    }
    None
}

fn is_relation(kind: ExplorerNodeKind) -> bool {
    matches!(
        kind,
        ExplorerNodeKind::Table | ExplorerNodeKind::View | ExplorerNodeKind::MaterializedView
    )
}

fn schema_matches(got: Option<&str>, expected: Option<&str>) -> bool {
    match expected {
        None => true,
        Some(expected) => got.is_some_and(|got| got.eq_ignore_ascii_case(expected)),
    }
}

fn kind_label(kind: CompletionKind) -> &'static str {
    match kind {
        CompletionKind::Keyword => "keyword",
        CompletionKind::Schema => "schema",
        CompletionKind::Table => "table",
        CompletionKind::View => "view",
        CompletionKind::Column => "column",
        CompletionKind::Function => "function",
        CompletionKind::Procedure => "procedure",
    }
}

#[component]
pub fn SqlCompletionMenu(
    items: Vec<CompletionItem>,
    active_index: usize,
    left: f64,
    top: f64,
    max_height: f64,
    on_accept: EventHandler<usize>,
) -> Element {
    rsx! {
        div {
            class: "sql-editor__autocomplete",
            style: format!("left: {left}px; top: {top}px; max-height: {max_height}px;"),
            role: "listbox",
            for (index, item) in items.iter().enumerate() {
                button {
                    class: if index == active_index {
                        "sql-editor__autocomplete-item sql-editor__autocomplete-item--active"
                    } else {
                        "sql-editor__autocomplete-item"
                    },
                    r#type: "button",
                    onmousedown: move |event| event.prevent_default(),
                    onclick: move |_| on_accept.call(index),
                    div {
                        class: "sql-editor__autocomplete-copy",
                        span { class: "sql-editor__autocomplete-label", "{item.label}" }
                        if !item.detail.is_empty() {
                            span { class: "sql-editor__autocomplete-detail", "{item.detail}" }
                        }
                    }
                    span { class: "sql-editor__autocomplete-kind", "{kind_label(item.kind)}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::autocomplete_offset;

    #[test]
    fn autocomplete_offset_flips_above_when_clipped() {
        let (left, top, flip) = autocomplete_offset(40.0, 180.0, 18.0, 120.0, 220.0, 400.0, 240.0);
        assert!(flip);
        assert!(top < 180.0);
        assert!(left >= 0.0);
    }

    #[test]
    fn autocomplete_offset_stays_below_when_space() {
        let (_, top, flip) = autocomplete_offset(10.0, 20.0, 18.0, 80.0, 400.0, 400.0, 200.0);
        assert!(!flip);
        assert!(top > 20.0);
    }
}
