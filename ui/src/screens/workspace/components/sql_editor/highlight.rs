use dioxus::prelude::*;
use std::cell::RefCell;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

const SQL_HIGHLIGHT_NAMES: [&str; 21] = [
    "attribute",
    "boolean",
    "comment",
    "conditional",
    "field",
    "float",
    "function.call",
    "keyword",
    "keyword.operator",
    "number",
    "operator",
    "parameter",
    "punctuation.bracket",
    "punctuation.delimiter",
    "spell",
    "storageclass",
    "string",
    "type",
    "type.builtin",
    "type.qualifier",
    "variable",
];

#[derive(Clone, PartialEq)]
struct SqlHighlightSegment {
    class_name: &'static str,
    text: String,
}

thread_local! {
    static SQL_HIGHLIGHT_CONFIG: RefCell<Option<HighlightConfiguration>> =
        RefCell::new(build_highlight_config());
}

#[component]
pub(super) fn SqlHighlightContent(
    sql: String,
    inline_cursor_position: Option<usize>,
    inline_suffix: Option<String>,
) -> Element {
    let inline_cursor_position = inline_cursor_position.unwrap_or(sql.len()).min(sql.len());
    let highlighted_before = use_memo(use_reactive(
        (&sql, &inline_cursor_position, &inline_suffix),
        |(sql, inline_cursor_position, inline_suffix)| {
            if inline_suffix
                .as_ref()
                .is_some_and(|suffix| !suffix.is_empty())
            {
                highlight_sql(&sql[..inline_cursor_position])
            } else {
                highlight_sql(&sql)
            }
        },
    ));
    rsx! {
        if sql.is_empty() && inline_suffix.is_none() {
            span {
                class: "sql-editor__placeholder",
                "-- Write SQL here. Syntax highlighting is powered by tree-sitter."
            }
        } else {
            for segment in highlighted_before() {
                span {
                    class: format!("sql-editor__token {}", segment.class_name),
                    "{segment.text}"
                }
            }
            if let Some(suffix) = inline_suffix {
                if !suffix.is_empty() {
                    span {
                        class: "sql-editor__token sql-editor__token--inline",
                        "{suffix}"
                    }
                }
            }
        }
    }
}

fn build_highlight_config() -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(
        tree_sitter_sequel::LANGUAGE.into(),
        "sql",
        tree_sitter_sequel::HIGHLIGHTS_QUERY,
        "",
        "",
    )
    .ok()?;
    config.configure(&SQL_HIGHLIGHT_NAMES);
    Some(config)
}

fn highlight_sql(sql: &str) -> Vec<SqlHighlightSegment> {
    if sql.is_empty() {
        return Vec::new();
    }

    SQL_HIGHLIGHT_CONFIG.with(|config| {
        let config = config.borrow();
        let Some(config) = config.as_ref() else {
            return vec![plain_segment(sql)];
        };

        let mut highlighter = Highlighter::new();
        let events = match highlighter.highlight(config, sql.as_bytes(), None, |_| None) {
            Ok(events) => events,
            Err(_) => return vec![plain_segment(sql)],
        };

        let mut segments = Vec::new();
        let mut highlight_stack = Vec::<usize>::new();

        for event in events {
            match event {
                Ok(HighlightEvent::HighlightStart(highlight)) => highlight_stack.push(highlight.0),
                Ok(HighlightEvent::HighlightEnd) => {
                    highlight_stack.pop();
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    let text = &sql[start..end];
                    push_segment(
                        &mut segments,
                        token_class(highlight_stack.last().copied()),
                        text,
                    );
                }
                Err(_) => return vec![plain_segment(sql)],
            }
        }

        segments
    })
}

fn push_segment(segments: &mut Vec<SqlHighlightSegment>, class_name: &'static str, text: &str) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = segments.last_mut()
        && last.class_name == class_name
    {
        last.text.push_str(text);
        return;
    }

    segments.push(SqlHighlightSegment {
        class_name,
        text: text.to_string(),
    });
}

fn plain_segment(sql: &str) -> SqlHighlightSegment {
    SqlHighlightSegment {
        class_name: "sql-editor__token--plain",
        text: sql.to_string(),
    }
}

fn token_class(highlight_index: Option<usize>) -> &'static str {
    match highlight_index.and_then(|index| SQL_HIGHLIGHT_NAMES.get(index).copied()) {
        Some("keyword" | "conditional" | "storageclass" | "type.qualifier") => {
            "sql-editor__token--keyword"
        }
        Some("string") => "sql-editor__token--string",
        Some("number" | "float" | "boolean") => "sql-editor__token--number",
        Some("comment") => "sql-editor__token--comment",
        Some("function.call") => "sql-editor__token--function",
        Some("type" | "type.builtin") => "sql-editor__token--type",
        Some("field" | "attribute" | "parameter" | "variable") => "sql-editor__token--attribute",
        Some("operator" | "keyword.operator") => "sql-editor__token--operator",
        Some("punctuation.bracket" | "punctuation.delimiter") => "sql-editor__token--punctuation",
        _ => "sql-editor__token--plain",
    }
}
