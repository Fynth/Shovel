//! Cell value editor — a resizable in-grid modal for inspecting and
//! editing large/text/JSON cell values.
//!
//! Mirrors the existing `results__details` aside (overlay + Escape +
//! overlay click close) so it does not need a new window host. The
//! caller supplies the initial cell value, a `commit` callback that
//! routes through the same `commit_cell_edit` path the inline editor
//! uses, and an `editable` flag that gates the "Apply" button and the
//! JSON parse hint.
//!
//! Two modes:
//! - **Text** — a textarea seeded with the raw value. Always available
//!   for non-empty values.
//! - **JSON** — pretty-printed read-only view of the parsed value when
//!   the cell parses as JSON. Falls back to a disabled "JSON" tab with
//!   a hint when the value is not valid JSON.

use dioxus::{html::input_data::MouseButton, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueEditorMode {
    Text,
    Json,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ValueEditorResize {
    pub start_x: f64,
    pub start_width: f64,
}

#[derive(Clone, PartialEq)]
pub struct ValueEditorState {
    pub column_name: String,
    pub value: String,
    pub editable: bool,
    pub mode: ValueEditorMode,
    pub width: f64,
}

#[component]
pub fn ValueEditor(
    state: ValueEditorState,
    on_value_change: EventHandler<String>,
    on_mode_change: EventHandler<ValueEditorMode>,
    on_apply: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut draft = use_signal(|| state.value.clone());
    let mut width = use_signal(|| state.width);
    let mut resize = use_signal(|| None::<ValueEditorResize>);

    let state_value_for_effect = state.value.clone();
    use_effect(move || {
        draft.set(state_value_for_effect.clone());
    });

    let trimmed_value = state.value.trim();
    let json_valid = serde_json::from_str::<serde_json::Value>(trimmed_value).is_ok();
    let json_pretty = if json_valid {
        serde_json::from_str::<serde_json::Value>(trimmed_value)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let on_overlay_click = move |event: MouseEvent| {
        // Only close when the click is on the backdrop itself, not on
        // a descendant. The Dioxus 0.7 desktop event bubbles from
        // descendants; gating on `event.trigger_button()` keeps
        // left-click only.
        if event.trigger_button() != Some(MouseButton::Primary) {
            return;
        }
        on_close.call(());
    };

    let on_resize_mousedown = move |event: MouseEvent| {
        if event.trigger_button() != Some(MouseButton::Primary) {
            return;
        }
        event.prevent_default();
        event.stop_propagation();
        resize.set(Some(ValueEditorResize {
            start_x: event.client_coordinates().x,
            start_width: width(),
        }));
    };

    rsx! {
        div {
            class: "value-editor",
            onclick: on_overlay_click,
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            div {
                class: "value-editor__panel",
                style: format!("--value-editor-width: {}px;", width()),
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "value-editor__header",
                    div {
                        class: "value-editor__copy",
                        h3 { class: "value-editor__title", "{state.column_name}" }
                        p { class: "value-editor__hint", "Inspect and edit this cell's value." }
                    }
                    button {
                        class: "button button--ghost button--small",
                        "aria-label": "Close value editor",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                }
                div {
                    class: "value-editor__tabs",
                    button {
                        class: if state.mode == ValueEditorMode::Text {
                            "button button--ghost button--small button--active"
                        } else {
                            "button button--ghost button--small"
                        },
                        onclick: move |_| on_mode_change.call(ValueEditorMode::Text),
                        "Text"
                    }
                    button {
                        class: if state.mode == ValueEditorMode::Json {
                            "button button--ghost button--small button--active"
                        } else {
                            "button button--ghost button--small"
                        },
                        disabled: !json_valid,
                        title: if json_valid {
                            "View the value as formatted JSON"
                        } else {
                            "This value is not valid JSON"
                        },
                        onclick: move |_| on_mode_change.call(ValueEditorMode::Json),
                        "JSON"
                    }
                    div { class: "value-editor__tabs-spacer" }
                    if state.editable {
                        button {
                            class: "button button--primary button--small",
                            "aria-label": "Apply value change",
                            disabled: draft() == state.value,
                            onclick: move |_| on_apply.call(draft()),
                            "Apply"
                        }
                    } else {
                        span { class: "value-editor__readonly", "Read-only" }
                    }
                }
                div { class: "value-editor__body",
                    if state.mode == ValueEditorMode::Text {
                        textarea {
                            class: "input value-editor__textarea",
                            readonly: !state.editable,
                            spellcheck: false,
                            value: "{draft()}",
                            placeholder: "Empty value",
                            oninput: move |event| {
                                let value = event.value();
                                draft.set(value.clone());
                                on_value_change.call(value);
                            },
                        }
                    } else if json_valid {
                        pre { class: "value-editor__json", "{json_pretty}" }
                    } else {
                        div { class: "value-editor__json-empty",
                            "This value is not valid JSON. Switch to the Text tab to edit."
                        }
                    }
                }
                div {
                    class: "value-editor__resize",
                    onmousedown: on_resize_mousedown,
                }
                if let Some(active) = resize() {
                    div {
                        style: "position:fixed;inset:0;z-index:9999;cursor:col-resize;",
                        onmousemove: move |event| {
                            let delta = event.client_coordinates().x - active.start_x;
                            let new_width = (active.start_width + delta).clamp(360.0, 1200.0);
                            width.set(new_width);
                        },
                        onmouseup: move |_| resize.set(None),
                    }
                }
            }
        }
    }
}

/// Pure helper used by the editor host (and the unit tests) to decide
/// whether the JSON tab should be enabled. Trims the value first so
/// trailing whitespace does not turn a valid JSON document into an
/// invalid one.
pub fn is_valid_json(value: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(value.trim()).is_ok()
}

/// Pure helper used by the unit tests to pretty-print a JSON value.
#[allow(dead_code)]
pub fn pretty_json(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value.trim())
        .ok()
        .and_then(|parsed| serde_json::to_string_pretty(&parsed).ok())
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_json_accepts_object() {
        assert!(is_valid_json(r#"{"k":1}"#));
        assert!(is_valid_json("[1, 2, 3]"));
    }

    #[test]
    fn is_valid_json_rejects_garbage() {
        assert!(!is_valid_json("not json"));
        assert!(!is_valid_json(""));
    }

    #[test]
    fn is_valid_json_tolerates_surrounding_whitespace() {
        assert!(is_valid_json("  {\"k\":1}\n"));
    }

    #[test]
    fn pretty_json_formats_object() {
        let pretty = pretty_json(r#"{"k":1,"nested":{"a":2}}"#);
        assert!(pretty.contains("\"k\": 1"));
        assert!(pretty.contains("\"nested\""));
    }

    #[test]
    fn pretty_json_returns_empty_for_invalid() {
        assert_eq!(pretty_json("not json"), "");
        assert_eq!(pretty_json(""), "");
    }

    #[test]
    fn value_editor_state_default_width_is_in_bounds() {
        let state = ValueEditorState {
            column_name: "name".to_string(),
            value: "Ada".to_string(),
            editable: true,
            mode: ValueEditorMode::Text,
            width: 480.0,
        };
        assert!(state.width >= 360.0);
        assert!(state.width <= 1200.0);
    }
}
