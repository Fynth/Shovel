use dioxus::prelude::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

static SELECTION_SYNC_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorSelection {
    pub start: usize,
    pub end: usize,
}

impl EditorSelection {
    pub fn collapsed(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn clamped(self, sql: &str) -> Self {
        let len = sql.len();
        Self {
            start: clamp_to_char_boundary(sql, self.start.min(len)),
            end: clamp_to_char_boundary(sql, self.end.min(len)),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn clamp_to_char_boundary(sql: &str, index: usize) -> usize {
    let mut index = index.min(sql.len());
    while index > 0 && !sql.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn current_token_range(sql: &str, selection: EditorSelection) -> std::ops::Range<usize> {
    let selection = selection.clamped(sql);
    let start = selection.start.min(selection.end);
    let end = selection.start.max(selection.end);

    if start != end {
        return start..end;
    }

    let cursor = end;
    let mut range_start = cursor;
    for (index, ch) in sql[..cursor].char_indices().rev() {
        if is_token_boundary(ch) {
            break;
        }
        range_start = index;
    }

    let mut range_end = cursor;
    for (offset, ch) in sql[cursor..].char_indices() {
        if is_token_boundary(ch) {
            break;
        }
        range_end = cursor + offset + ch.len_utf8();
    }

    range_start..range_end
}

#[cfg_attr(not(test), allow(dead_code))]
fn is_token_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | ';'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '+'
                | '-'
                | '*'
                | '/'
                | '='
                | '<'
                | '>'
                | ':'
        )
}

#[allow(dead_code)]
pub fn clean_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, ',' | ';' | '(' | ')' | '\n' | '\r')
        })
        .to_string()
}

#[allow(dead_code)]
pub fn normalize_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\'' | '`'))
        .collect::<String>()
        .trim()
        .to_ascii_lowercase()
}

#[allow(dead_code)]
pub fn apply_suggestion(
    sql: &str,
    selection: EditorSelection,
    replacement: &str,
) -> (String, usize) {
    let range = current_token_range(sql, selection);
    let mut next_sql = String::with_capacity(sql.len() + replacement.len());
    next_sql.push_str(&sql[..range.start]);
    next_sql.push_str(replacement);
    next_sql.push_str(&sql[range.end..]);
    (next_sql, range.start + replacement.len())
}

pub fn sync_editor_selection(editor_selection: Signal<EditorSelection>, editor_id: &'static str) {
    sync_editor_selection_with_delay(editor_selection, editor_id, 0);
}

pub fn sync_editor_selection_debounced(
    editor_selection: Signal<EditorSelection>,
    editor_id: &'static str,
) {
    sync_editor_selection_with_delay(editor_selection, editor_id, 16);
}

fn sync_editor_selection_with_delay(
    mut editor_selection: Signal<EditorSelection>,
    editor_id: &'static str,
    delay_ms: u64,
) {
    let request_id = SELECTION_SYNC_REQUEST_ID.fetch_add(1, Ordering::SeqCst) + 1;
    spawn(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            if SELECTION_SYNC_REQUEST_ID.load(Ordering::SeqCst) != request_id {
                return;
            }
        }

        let Ok((_value, start, end)) =
            document::eval(&editor_value_and_selection_query_script(editor_id))
                .join::<(String, usize, usize)>()
                .await
        else {
            return;
        };
        if SELECTION_SYNC_REQUEST_ID.load(Ordering::SeqCst) != request_id {
            return;
        }

        editor_selection.set(EditorSelection { start, end });
    });
}

pub fn editor_value_and_selection_query_script(editor_id: &str) -> String {
    format!(
        r#"
        (() => {{
            const editor = document.getElementById({editor_id:?});
            if (!editor) {{
                return ["", 0, 0];
            }}
            const toByteIndex = (value, utf16Offset) =>
                new TextEncoder().encode(value.slice(0, utf16Offset)).length;
            const value = editor.value ?? "";
            const start = editor.selectionStart ?? value.length ?? 0;
            const end = editor.selectionEnd ?? start;
            return [
                value,
                toByteIndex(value, start),
                toByteIndex(value, end)
            ];
        }})()
        "#
    )
}

#[allow(dead_code)]
pub fn set_editor_selection_script(editor_id: &str, position: usize) -> String {
    format!(
        r#"
        (() => {{
            const editor = document.getElementById({editor_id:?});
            if (!editor) {{
                return false;
            }}
            const encoder = new TextEncoder();
            const value = editor.value ?? "";
            let utf16Position = 0;
            let byteOffset = 0;
            for (const ch of value) {{
                const nextByteOffset = byteOffset + encoder.encode(ch).length;
                if (nextByteOffset > {position}) {{
                    break;
                }}
                byteOffset = nextByteOffset;
                utf16Position += ch.length;
            }}
            editor.focus();
            editor.setSelectionRange(utf16Position, utf16Position);
            return true;
        }})()
        "#
    )
}

#[allow(dead_code)]
pub fn set_editor_value_script(
    editor_id: &str,
    value: &str,
    position: usize,
    focus: bool,
) -> String {
    let value = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
        (() => {{
            const editor = document.getElementById({editor_id:?});
            if (!editor) {{
                return false;
            }}
            const nextValue = {value};
            const encoder = new TextEncoder();
            if (editor.value !== nextValue) {{
                editor.value = nextValue;
            }}
            let utf16Position = 0;
            let byteOffset = 0;
            for (const ch of nextValue) {{
                const nextByteOffset = byteOffset + encoder.encode(ch).length;
                if (nextByteOffset > {position}) {{
                    break;
                }}
                byteOffset = nextByteOffset;
                utf16Position += ch.length;
            }}
            if ({focus} || document.activeElement === editor) {{
                if ({focus}) {{
                    editor.focus();
                }}
                editor.setSelectionRange(utf16Position, utf16Position);
            }}
            return true;
        }})()
        "#
    )
}

#[cfg(test)]
mod tests {
    use super::{EditorSelection, current_token_range};

    #[test]
    fn selection_clamps_invalid_utf8_offsets_to_char_boundaries() {
        let sql = "select * from пользователи";
        let selection = EditorSelection { start: 15, end: 15 }.clamped(sql);
        assert!(sql.is_char_boundary(selection.start));
        assert_eq!(selection.start, selection.end);
    }

    #[test]
    fn current_token_range_handles_multibyte_identifiers() {
        let sql = "select * from пользователи";
        let cursor = sql.find("ль").unwrap() + 1;
        let range = current_token_range(
            sql,
            EditorSelection {
                start: cursor,
                end: cursor,
            },
        );
        assert_eq!(&sql[range], "пользователи");
    }
}
