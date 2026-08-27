//! Keyboard shortcuts section: filterable table, capture, conflict, reset.

use dioxus::prelude::*;
use models::{DEFAULT_KEYBINDINGS, KeybindingMap, combo_conflict, effective_keybindings};

use super::{SettingsSectionProps, widgets::KeyCapture};

/// Human labels for bindable action ids. Order matches [`DEFAULT_KEYBINDINGS`].
const ACTION_LABELS: &[(&str, &str)] = &[
    ("focus_editor", "Focus SQL editor"),
    ("format_sql", "Format SQL"),
    ("new_tab", "New tab"),
    ("close_tab", "Close tab"),
    ("next_tab", "Next tab"),
    ("refresh_explorer", "Refresh explorer"),
    ("focus_filter_panel", "Focus result filter"),
    ("save_query", "Save query"),
    ("close_overlay", "Close overlay"),
    ("command_palette", "Command palette"),
    ("global_search", "Global search"),
    ("rename_selected", "Rename selected"),
    ("delete_selected", "Drop selected"),
    ("focus_agent_composer", "Focus agent composer"),
    ("new_connection", "New connection"),
    ("open_settings", "Open settings"),
];

fn action_label(id: &str) -> &str {
    ACTION_LABELS
        .iter()
        .find_map(|(action_id, label)| (*action_id == id).then_some(*label))
        .unwrap_or(id)
}

#[component]
pub(super) fn KeyboardSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    let mut filter = use_signal(String::new);
    let mut conflict = use_signal(|| None::<(String, String)>);

    let filter_text = filter().to_ascii_lowercase();
    let conflict_state = conflict();
    let effective = effective_keybindings(&settings.keybindings);
    let visible: Vec<(&str, &str)> = DEFAULT_KEYBINDINGS
        .iter()
        .copied()
        .filter(|(id, _)| action_label(id).to_ascii_lowercase().contains(&filter_text))
        .collect();

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Keyboard" }
                div {
                    class: "settings-modal__section-actions",
                    button {
                        class: "button button--ghost button--small",
                        r#type: "button",
                        onclick: move |_| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.keybindings = KeybindingMap::new();
                            conflict.set(None);
                            on_change.call((
                                next,
                                section_props_signal.read().sql_settings.clone(),
                            ));
                        },
                        "Reset all"
                    }
                }
            }
            p {
                class: "settings-modal__section-hint",
                "Click a shortcut, then press the new keys. Escape cancels. Backspace or Reset row restores the default."
            }
            div {
                class: "field",
                span { class: "field__label", "Filter" }
                input {
                    class: "input",
                    r#type: "search",
                    placeholder: "Filter shortcuts",
                    value: "{filter}",
                    oninput: move |event| filter.set(event.value()),
                }
            }
            for (action_id, default_combo) in visible.iter().copied() {
                {
                    let current = effective
                        .get(action_id)
                        .cloned()
                        .unwrap_or_else(|| default_combo.to_string());
                    let error = conflict_state.as_ref().and_then(|(id, message)| {
                        (id.as_str() == action_id).then(|| message.clone())
                    });
                    let label = action_label(action_id);
                    rsx! {
                        div {
                            key: "{action_id}",
                            class: "field",
                            span { class: "field__label", {label} }
                            div {
                                class: "settings-modal__actions",
                                KeyCapture {
                                    current,
                                    on_change: move |combo: String| {
                                        let mut next =
                                            section_props_signal.read().settings.clone();
                                        let effective =
                                            effective_keybindings(&next.keybindings);
                                        if let Some(other) =
                                            combo_conflict(action_id, &combo, &effective)
                                        {
                                            let other_label = action_label(&other);
                                            conflict.set(Some((
                                                action_id.to_string(),
                                                format!("already used by {other_label}"),
                                            )));
                                            return;
                                        }
                                        conflict.set(None);
                                        next.keybindings.insert(action_id.to_string(), combo);
                                        on_change.call((
                                            next,
                                            section_props_signal.read().sql_settings.clone(),
                                        ));
                                    },
                                    on_clear: move |_| {
                                        let mut next =
                                            section_props_signal.read().settings.clone();
                                        next.keybindings.remove(action_id);
                                        conflict.set(None);
                                        on_change.call((
                                            next,
                                            section_props_signal.read().sql_settings.clone(),
                                        ));
                                    },
                                }
                                button {
                                    class: "button button--ghost button--small",
                                    r#type: "button",
                                    onclick: move |_| {
                                        let mut next =
                                            section_props_signal.read().settings.clone();
                                        next.keybindings.remove(action_id);
                                        conflict.set(None);
                                        on_change.call((
                                            next,
                                            section_props_signal.read().sql_settings.clone(),
                                        ));
                                    },
                                    "Reset row"
                                }
                            }
                            if let Some(message) = error {
                                p {
                                    class: "settings-modal__section-hint",
                                    role: "alert",
                                    {message}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigning_duplicate_combo_is_rejected() {
        let current = KeybindingMap::new();
        let effective = effective_keybindings(&current);
        assert!(combo_conflict("format_sql", "Ctrl+T", &effective).is_some());
    }

    #[test]
    fn reset_row_removes_override() {
        let mut map = KeybindingMap::new();
        map.insert("format_sql".into(), "Ctrl+Alt+F".into());
        map.remove("format_sql");
        let effective = effective_keybindings(&map);
        assert_eq!(
            effective.get("format_sql").map(String::as_str),
            Some("Ctrl+Shift+F")
        );
    }
}
