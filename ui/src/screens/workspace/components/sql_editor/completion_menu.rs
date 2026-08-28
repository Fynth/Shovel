use crate::completion::{
    keyboard::CompletionKey,
    keywords::{CompletionItem, CompletionKind},
    rank::apply_menu_item,
};
use dioxus::prelude::*;
use models::{ExplorerNode, ExplorerNodeKind};
use serde::Deserialize;
use std::ops::Range;

pub const MENU_ROW_HEIGHT: f64 = 26.0;
pub const MENU_VISIBLE_ROWS: usize = 9;
// 4px padding top + bottom and a 1px border on each side.
pub const MENU_VERTICAL_CHROME: f64 = 10.0;
pub const MENU_MIN_WIDTH: f64 = 220.0;
pub const MENU_MAX_WIDTH: f64 = 400.0;
// Approximate mono glyph width at the menu font size, used for width estimates.
const MENU_CHAR_WIDTH: f64 = 7.5;
const COMPLETION_MENU_ID: &str = "workspace-sql-completion";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuGeometry {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub max_height: f64,
}

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
    let max_left = (editor_width - menu_width - 8.0).max(0.0);
    let left = caret_x.min(max_left).max(0.0);
    if caret_y + line_height + menu_height > editor_height {
        (left, (caret_y - menu_height).max(0.0), true)
    } else {
        (left, caret_y + line_height, false)
    }
}

pub fn menu_height_for_items(count: usize) -> f64 {
    (count.clamp(1, MENU_VISIBLE_ROWS) as f64 * MENU_ROW_HEIGHT) + MENU_VERTICAL_CHROME
}

pub fn menu_width_for_items(items: &[CompletionItem]) -> f64 {
    let longest = items
        .iter()
        .map(|item| {
            let label = item.label.chars().count() as f64;
            let detail = item.detail.chars().count().min(24) as f64;
            let kind = kind_label(item.kind).len() as f64;
            label + detail + kind + 8.0
        })
        .fold(0.0_f64, f64::max);
    (longest * MENU_CHAR_WIDTH + 36.0).clamp(MENU_MIN_WIDTH, MENU_MAX_WIDTH)
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
                "fontVariantLigatures", "letterSpacing", "textTransform", "wordSpacing",
                "textIndent", "tabSize", "MozTabSize", "whiteSpace", "wordWrap",
                "overflowWrap", "lineHeight",
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

pub fn should_refresh_menu_caret(menu_len: usize) -> bool {
    menu_len > 0
}

pub fn apply_menu_item_if_current(
    sql: &str,
    source_sql: &str,
    item: &CompletionItem,
) -> Option<(String, usize)> {
    if sql != source_sql {
        return None;
    }
    if item.replace.start > item.replace.end
        || item.replace.end > sql.len()
        || !sql.is_char_boundary(item.replace.start)
        || !sql.is_char_boundary(item.replace.end)
    {
        return None;
    }
    Some(apply_menu_item(sql, item))
}

pub fn map_completion_key(event: &KeyboardEvent) -> CompletionKey {
    map_completion_key_parts(&event.key(), event.code(), event.modifiers())
}

pub fn map_completion_key_parts(key: &Key, code: Code, modifiers: Modifiers) -> CompletionKey {
    let control = modifiers.contains(Modifiers::CONTROL);
    let meta = modifiers.contains(Modifiers::META);
    let alt = modifiers.contains(Modifiers::ALT);
    let shift = modifiers.contains(Modifiers::SHIFT);

    if control && !meta && (code == Code::Space || matches!(key, Key::Character(ch) if ch == " ")) {
        return CompletionKey::CtrlSpace;
    }
    if alt && (code == Code::BracketRight || matches!(key, Key::Character(ch) if ch == "]")) {
        return CompletionKey::AltRBracket;
    }
    if alt && (code == Code::BracketLeft || matches!(key, Key::Character(ch) if ch == "[")) {
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
        Key::PageUp => CompletionKey::PageUp,
        Key::PageDown => CompletionKey::PageDown,
        Key::Home => CompletionKey::Home,
        Key::End => CompletionKey::End,
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
        CompletionKind::Function => "fn",
        CompletionKind::Procedure => "proc",
    }
}

/// Byte ranges within `label` matched by the typed `token` (case-insensitive
/// greedy subsequence, so a contiguous prefix match stays one range). Used to
/// underline matched characters the way Zed does.
pub fn matched_char_ranges(label: &str, token: &str) -> Vec<Range<usize>> {
    if token.is_empty() {
        return Vec::new();
    }
    let token_chars: Vec<char> = token.chars().collect();
    let mut matched_index = 0usize;
    let mut ranges: Vec<Range<usize>> = Vec::new();

    for (byte_index, ch) in label.char_indices() {
        if matched_index >= token_chars.len() {
            break;
        }
        if ch.eq_ignore_ascii_case(&token_chars[matched_index]) {
            let end = byte_index + ch.len_utf8();
            match ranges.last_mut() {
                Some(last) if last.end == byte_index => last.end = end,
                _ => ranges.push(byte_index..end),
            }
            matched_index += 1;
        }
    }

    if matched_index == token_chars.len() {
        ranges
    } else {
        Vec::new()
    }
}

fn label_parts<'a>(label: &'a str, token: &str) -> Vec<(&'a str, bool)> {
    let ranges = matched_char_ranges(label, token);
    let mut parts = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut position = 0;
    for range in ranges {
        if range.start > position {
            parts.push((&label[position..range.start], false));
        }
        parts.push((&label[range.start..range.end], true));
        position = range.end;
    }
    if position < label.len() {
        parts.push((&label[position..], false));
    }
    parts
}

fn menu_scroll_script(active_index: usize) -> String {
    format!(
        r#"
        (() => {{
            const menu = document.getElementById({id:?});
            if (!menu) return true;
            const item = menu.children[{active_index}];
            if (!item) return true;
            const top = item.offsetTop;
            const bottom = top + item.offsetHeight;
            if (top < menu.scrollTop) {{
                menu.scrollTop = top;
            }} else if (bottom > menu.scrollTop + menu.clientHeight) {{
                menu.scrollTop = bottom - menu.clientHeight;
            }}
            return true;
        }})()
        "#,
        id = COMPLETION_MENU_ID,
    )
}

#[component]
pub fn SqlCompletionMenu(
    items: Vec<CompletionItem>,
    active_index: usize,
    token: String,
    geometry: MenuGeometry,
    on_accept: EventHandler<usize>,
) -> Element {
    let item_count = items.len();
    use_effect(use_reactive(
        &(active_index, item_count),
        |(active_index, _)| {
            spawn(async move {
                let _ = document::eval(&menu_scroll_script(active_index)).await;
            });
        },
    ));

    rsx! {
        div {
            class: "sql-editor__autocomplete",
            id: COMPLETION_MENU_ID,
            role: "listbox",
            style: format!(
                "left: {left}px; top: {top}px; width: {width}px; max-height: {max_height}px;",
                left = geometry.left,
                top = geometry.top,
                width = geometry.width,
                max_height = geometry.max_height,
            ),
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
                    span {
                        class: "sql-editor__autocomplete-label",
                        for (text, matched) in label_parts(&item.label, &token) {
                            if matched {
                                span {
                                    class: "sql-editor__autocomplete-match",
                                    {text.to_string()}
                                }
                            } else {
                                {text.to_string()}
                            }
                        }
                    }
                    if !item.detail.is_empty() && item.detail != kind_label(item.kind) {
                        span { class: "sql-editor__autocomplete-detail", "{item.detail}" }
                    }
                    span {
                        class: format!(
                            "sql-editor__autocomplete-kind sql-editor__autocomplete-kind--{}",
                            kind_label(item.kind),
                        ),
                        "{kind_label(item.kind)}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MENU_MAX_WIDTH,
        MENU_MIN_WIDTH,
        MENU_ROW_HEIGHT,
        MENU_VISIBLE_ROWS,
        MenuGeometry,
        apply_menu_item_if_current,
        autocomplete_offset,
        caret_anchor_script,
        map_completion_key_parts,
        matched_char_ranges,
        menu_height_for_items,
        menu_width_for_items,
        should_refresh_menu_caret,
    };
    use crate::completion::{
        keyboard::CompletionKey,
        keywords::{CompletionItem, CompletionKind},
    };
    use dioxus::prelude::{Code, Key, Modifiers};

    fn item(label: &str, detail: &str) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            detail: detail.into(),
            kind: CompletionKind::Column,
            replace: 0..0,
        }
    }

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

    #[test]
    fn autocomplete_offset_clamps_to_editor_width() {
        let (left, _, _) = autocomplete_offset(500.0, 20.0, 18.0, 80.0, 300.0, 320.0, 240.0);
        assert!(left <= 320.0 - 240.0, "menu must keep a right-edge margin");
    }

    #[test]
    fn control_space_forces_menu_meta_space_does_not() {
        let space = Key::Character(" ".into());
        assert_eq!(
            map_completion_key_parts(&space, Code::Space, Modifiers::CONTROL),
            CompletionKey::CtrlSpace
        );
        assert_eq!(
            map_completion_key_parts(&space, Code::Space, Modifiers::META),
            CompletionKey::Character(' ')
        );
        assert_eq!(
            map_completion_key_parts(&space, Code::Space, Modifiers::CONTROL | Modifiers::META),
            CompletionKey::Character(' ')
        );
    }

    #[test]
    fn page_and_home_end_keys_map_for_menu_navigation() {
        assert_eq!(
            map_completion_key_parts(&Key::PageUp, Code::PageUp, Modifiers::empty()),
            CompletionKey::PageUp
        );
        assert_eq!(
            map_completion_key_parts(&Key::PageDown, Code::PageDown, Modifiers::empty()),
            CompletionKey::PageDown
        );
        assert_eq!(
            map_completion_key_parts(&Key::Home, Code::Home, Modifiers::empty()),
            CompletionKey::Home
        );
        assert_eq!(
            map_completion_key_parts(&Key::End, Code::End, Modifiers::empty()),
            CompletionKey::End
        );
    }

    #[test]
    fn caret_anchor_script_applies_textarea_scroll() {
        let script = caret_anchor_script("workspace-sql-editor");
        assert!(script.contains("pre.scrollTop = editor.scrollTop"));
        assert!(script.contains("pre.scrollLeft = editor.scrollLeft"));
        assert!(script.contains("overflowWrap"));
    }

    #[test]
    fn should_refresh_menu_caret_only_when_open() {
        assert!(!should_refresh_menu_caret(0));
        assert!(should_refresh_menu_caret(1));
    }

    #[test]
    fn apply_menu_item_if_current_rejects_stale_sql_and_bad_range() {
        let sql = "SELECT * FROM us";
        let replace_item = CompletionItem {
            label: "users".into(),
            detail: String::new(),
            kind: CompletionKind::Table,
            replace: 14..16,
        };
        let (next, cursor) = apply_menu_item_if_current(sql, sql, &replace_item).unwrap();
        assert_eq!(next, "SELECT * FROM users");
        assert_eq!(cursor, next.len());

        assert!(apply_menu_item_if_current("SELECT * FROM u", sql, &replace_item).is_none());

        let stale = CompletionItem {
            replace: 0..80,
            ..replace_item.clone()
        };
        assert!(apply_menu_item_if_current(sql, sql, &stale).is_none());
    }

    #[test]
    fn menu_height_scales_with_visible_rows() {
        assert_eq!(menu_height_for_items(1), MENU_ROW_HEIGHT + 10.0);
        assert_eq!(
            menu_height_for_items(MENU_VISIBLE_ROWS),
            MENU_VISIBLE_ROWS as f64 * MENU_ROW_HEIGHT + 10.0
        );
        assert_eq!(
            menu_height_for_items(50),
            menu_height_for_items(MENU_VISIBLE_ROWS)
        );
    }

    #[test]
    fn menu_width_clamps_to_bounds() {
        assert_eq!(menu_width_for_items(&[item("id", "")]), MENU_MIN_WIDTH);
        let long = item(&"x".repeat(200), "");
        assert_eq!(menu_width_for_items(&[long]), MENU_MAX_WIDTH);
    }

    #[test]
    fn matched_char_ranges_highlight_subsequence() {
        assert_eq!(
            matched_char_ranges("users", "usr"),
            vec![0..2, 3..4],
            "greedy subsequence merges adjacent matches"
        );
        assert_eq!(matched_char_ranges("SELECT", "sel"), vec![0..3]);
        assert!(matched_char_ranges("orders", "xyz").is_empty());
        assert!(matched_char_ranges("users", "").is_empty());
    }

    #[test]
    fn geometry_defaults_are_zeroed() {
        assert_eq!(
            MenuGeometry::default(),
            MenuGeometry {
                left: 0.0,
                top: 0.0,
                width: 0.0,
                max_height: 0.0,
            }
        );
    }
}
