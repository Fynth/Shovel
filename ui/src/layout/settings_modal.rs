//! Pure, prop-driven settings form.
//!
//! Owns no global state. Receives the current [`models::AppUiSettings`] and
//! [`models::SqlFormatSettings`] as props and reports every user edit through
//! a single [`on_change`] callback that emits the updated pair.
//!
//! This makes the component reusable in any host:
//!
//! - The main window mounts it as an in-app overlay (caller pushes each edit
//!   back to the global [`crate::app_state`] signals).
//! - The native settings window mounts it inside [`crate::windows::SettingsWindowRoot`]
//!   and forwards each edit through a [`crate::windows::DialogBridge`] to the
//!   main window, which owns the real globals + persistence effects.
//!
//! [`on_change`]: SettingsModalProps::on_change

use crate::{components::tooltip_target::TooltipTarget, screens::SqlFormatSettingsFields};
use dioxus::prelude::*;
use models::{AppThemePreference, AppUiSettings, SqlFormatSettings};

/// Props for [`SettingsModal`].
#[derive(Props, Clone, PartialEq)]
pub struct SettingsModalProps {
    /// Current UI settings snapshot to render. Must be the latest value the
    /// caller holds — every keystroke re-emits the updated pair via
    /// [`SettingsModalProps::on_change`] and the caller is expected to feed it
    /// back through props on the next render.
    pub settings: AppUiSettings,
    /// Current SQL formatting snapshot to render. Same ownership contract as
    /// [`SettingsModalProps::settings`].
    pub sql_settings: SqlFormatSettings,
    /// Called with `(updated_ui, updated_sql)` whenever the user edits a
    /// field. The component does not store the new value itself; the caller
    /// is responsible for updating state and re-supplying it via props.
    pub on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
    /// Called when the user dismisses the form. The component never closes
    /// itself — closing is a host responsibility (in the in-app overlay host
    /// it hides the modal; in the native window host it closes the OS window).
    pub on_close: Callback<()>,
}

#[component]
pub fn SettingsModal(props: SettingsModalProps) -> Element {
    // Wrap `props` in a `Signal` so every per-control closure below can take a
    // cheap `Signal` clone (instead of an expensive `AppUiSettings` clone) and
    // read the latest value at event time. Without this, the first `move`
    // closure captures `props` by value and every subsequent closure loses
    // access — `AppUiSettings` is not `Copy`, and field accesses inside
    // closures move the value out of the props struct.
    let props = use_signal(|| props);
    let on_change = props.read().on_change;
    let on_close = props.read().on_close;
    // Local clone for the render body (kept out of closures to avoid moves).
    // The render body never reads the SQL formatter settings directly — those
    // flow through `SqlFormatFieldsAdapter`, which owns its own signal.
    let settings = props.read().settings.clone();

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| on_close.call(()),
            div {
                class: "settings-modal",
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "settings-modal__body",
                    section {
                        class: "settings-modal__section",
                        div {
                            class: "settings-modal__section-header",
                            h3 { class: "settings-modal__section-title", "Appearance" }
                        }
                        div {
                            class: "settings-modal__segmented",
                            role: "group",
                            aria_label: "Theme preference",
                            button {
                                class: if settings.theme == AppThemePreference::Dark {
                                    "button button--ghost button--small button--active"
                                } else {
                                    "button button--ghost button--small"
                                },
                                aria_pressed: settings.theme == AppThemePreference::Dark,
                                onclick: move |_| {
                                    let mut next = props.read().settings.clone();
                                    next.theme = AppThemePreference::Dark;
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                                "Dark"
                            }
                            button {
                                class: if settings.theme == AppThemePreference::Light {
                                    "button button--ghost button--small button--active"
                                } else {
                                    "button button--ghost button--small"
                                },
                                aria_pressed: settings.theme == AppThemePreference::Light,
                                onclick: move |_| {
                                    let mut next = props.read().settings.clone();
                                    next.theme = AppThemePreference::Light;
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                                "Light"
                            }
                        }
                    }

                    section {
                        class: "settings-modal__section",
                        div {
                            class: "settings-modal__section-header",
                            h3 { class: "settings-modal__section-title", "DeepSeek Agent" }
                            p {
                                class: "settings-modal__section-hint",
                                "Primary API-key agent for database chat, SQL generation and SQL fixes."
                            }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.deepseek.enabled,
                                disabled: settings.deepseek.api_key.is_empty(),
                                oninput: move |event| {
                                    let mut next = props.read().settings.clone();
                                    next.deepseek.enabled = event.checked();
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                            }
                            span { "Use DeepSeek as the default embedded SQL agent" }
                        }
                        div {
                            class: "settings-modal__grid",
                            div {
                                class: "field",
                                span { class: "field__label", "API Key" }
                                input {
                                    class: "input",
                                    r#type: "password",
                                    placeholder: "sk-...",
                                    value: "{settings.deepseek.api_key}",
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        let value = event.value();
                                        next.deepseek.api_key = value.clone();
                                        if value.trim().is_empty() {
                                            next.deepseek.enabled = false;
                                        }
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                            }
                            div {
                                class: "field",
                                span { class: "field__label", "Base URL" }
                                input {
                                    class: "input",
                                    placeholder: "https://api.deepseek.com",
                                    value: "{settings.deepseek.base_url}",
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.deepseek.base_url = event.value();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                            }
                            div {
                                class: "field",
                                span { class: "field__label", "Model" }
                                select {
                                    class: "input",
                                    value: "{settings.deepseek.model}",
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.deepseek.model = event.value();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                    option { value: "deepseek-chat", "deepseek-chat (fast, recommended)" }
                                    option { value: "deepseek-v4-pro", "deepseek-v4-pro (reasoning)" }
                                    option { value: "deepseek-v4-flash", "deepseek-v4-flash (reasoning, fast)" }
                                }
                            }
                            div {
                                class: "field",
                                span { class: "field__label", "Reasoning effort" }
                                select {
                                    class: "input",
                                    value: "{settings.deepseek.reasoning_effort}",
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.deepseek.reasoning_effort = event.value();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                    option { value: "low", "low" }
                                    option { value: "medium", "medium" }
                                    option { value: "high", "high" }
                                }
                            }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.deepseek.thinking_enabled,
                                oninput: move |event| {
                                    let mut next = props.read().settings.clone();
                                    next.deepseek.thinking_enabled = event.checked();
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                            }
                            span { "Enable DeepSeek thinking mode when the selected model supports it" }
                        }
                        if settings.deepseek.api_key.is_empty() {
                            p {
                                class: "settings-modal__section-hint",
                                "Enter a DeepSeek API key to enable the embedded DeepSeek agent. Get your key from "
                                a {
                                    href: "https://platform.deepseek.com/api_keys",
                                    target: "_blank",
                                    "platform.deepseek.com"
                                }
                            }
                        }
                    }

                    section {
                        class: "settings-modal__section",
                        div {
                            class: "settings-modal__section-header",
                            h3 { class: "settings-modal__section-title", "Workspace" }
                            div {
                                class: "settings-modal__section-actions",
                                TooltipTarget {
                                    label: "Reset workspace, panels, and AI settings to their defaults (API keys are preserved)".to_string(),
                                    button {
                                        class: "button button--ghost button--small",
                                        onclick: move |_| {
                                            // Reset to defaults, but preserve the user's
                                            // API keys (they live in the OS keyring and
                                            // are not part of the JSON-serialized
                                            // AppUiSettings payload).
                                            let mut next = AppUiSettings::default();
                                            next.deepseek.api_key = props.read().settings.deepseek.api_key.clone();
                                            next.codestral.api_key = props.read().settings.codestral.api_key.clone();
                                            on_change.call((next, props.read().sql_settings.clone()));
                                        },
                                        "Reset UI"
                                    }
                                }
                            }
                        }
                        div {
                            class: "settings-modal__group",
                            span { class: "settings-modal__group-title", "Defaults" }
                            div {
                                class: "settings-modal__grid",
                                div {
                                    class: "field",
                                    span { class: "field__label", "Default page size" }
                                    input {
                                        class: "input",
                                        r#type: "number",
                                        min: "10",
                                        max: "1000",
                                        value: "{settings.default_page_size}",
                                        oninput: move |event| {
                                            let mut next = props.read().settings.clone();
                                            next.default_page_size = parse_u32_in_range(
                                                &event.value(),
                                                props.read().settings.default_page_size,
                                                10,
                                                1000,
                                            );
                                            on_change.call((next, props.read().sql_settings.clone()));
                                        },
                                    }
                                }
                            }
                        }
                        div {
                            class: "settings-modal__group",
                            span {
                                class: "settings-modal__group-title",
                                "Session and safety"
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.restore_session_on_launch,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.restore_session_on_launch = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Restore previous session on launch" }
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.read_only_mode,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.read_only_mode = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Read-only mode (block write SQL, imports, and table edits)" }
                            }
                        }
                        div {
                            class: "settings-modal__group",
                            span {
                                class: "settings-modal__group-title",
                                "Visible panels by default"
                            }
                            p {
                                class: "settings-modal__section-hint",
                                "Tool panels can be dragged between the left sidebar and the right inspector."
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_saved_queries,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_saved_queries = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show saved queries panel by default" }
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_connections,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_connections = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show connections panel by default" }
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_explorer,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_explorer = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show explorer by default" }
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_history,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_history = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show history by default" }
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_sql_editor,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_sql_editor = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show SQL editor by default" }
                            }
                            label {
                                class: if !settings.ai_features_enabled {
                                    "settings-modal__toggle settings-modal__toggle--disabled"
                                } else {
                                    "settings-modal__toggle"
                                },
                                aria_disabled: !settings.ai_features_enabled,
                                input {
                                    r#type: "checkbox",
                                    checked: settings.show_agent_panel,
                                    disabled: !settings.ai_features_enabled,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.show_agent_panel = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Show ACP agent panel by default" }
                            }
                        }
                        div {
                            class: "settings-modal__group",
                            span {
                                class: "settings-modal__group-title",
                                "AI features"
                            }
                            label {
                                class: "settings-modal__toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: settings.ai_features_enabled,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.ai_features_enabled = event.checked();
                                        if !event.checked() {
                                            next.show_agent_panel = false;
                                        }
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Enable AI features (ACP panel, prompts, and SQL actions)" }
                            }
                            div {
                                class: "field",
                                label {
                                    class: "field__label",
                                    "AI response language"
                                }
                                input {
                                    class: "input",
                                    r#type: "text",
                                    placeholder: "English",
                                    value: "{settings.ai_response_language}",
                                    disabled: !settings.ai_features_enabled,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.ai_response_language = event.value();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                            }
                            label {
                                class: if !settings.ai_features_enabled {
                                    "settings-modal__toggle settings-modal__toggle--disabled"
                                } else {
                                    "settings-modal__toggle"
                                },
                                aria_disabled: !settings.ai_features_enabled,
                                input {
                                    r#type: "checkbox",
                                    checked: settings.ai_auto_apply_completions,
                                    disabled: !settings.ai_features_enabled,
                                    oninput: move |event| {
                                        let mut next = props.read().settings.clone();
                                        next.ai_auto_apply_completions = event.checked();
                                        on_change.call((next, props.read().sql_settings.clone()));
                                    },
                                }
                                span { "Auto-apply inline AI completions (insert after a short idle pause; otherwise press Tab to accept)" }
                            }
                        }
                    }

                    section {
                        class: "settings-modal__section",
                        div {
                            class: "settings-modal__section-header",
                            div {
                                h3 { class: "settings-modal__section-title", "SQL Formatting" }
                                p {
                                    class: "settings-modal__section-hint",
                                    "Controls keyword case, wrapping, joins and inline arguments."
                                }
                            }
                            div {
                                class: "settings-modal__section-actions",
                                TooltipTarget {
                                    label: "Reset SQL formatting options (keyword case, wrapping, joins, inline arguments) to defaults".to_string(),
                                    button {
                                        class: "button button--ghost button--small",
                                        onclick: move |_| {
                                            on_change.call((props.read().settings.clone(), SqlFormatSettings::default()));
                                        },
                                        "Reset SQL"
                                    }
                                }
                            }
                        }
                        // `SqlFormatSettingsFields` owns a `Signal<SqlFormatSettings>`.
                        // The adapter below owns a local signal that mirrors the
                        // prop, hands it to the field component, and watches the
                        // signal with `use_effect` to bubble every change through
                        // `on_change`. This keeps the SQL formatter fields
                        // reusable (they only need a `Signal`) while satisfying
                        // the prop-driven contract of `SettingsModal`.
                        SqlFormatFieldsAdapter {
                            sql_settings: props.read().sql_settings.clone(),
                            settings: props.read().settings.clone(),
                            on_change,
                        }
                    }

                    section {
                        class: "settings-modal__section",
                        div {
                            class: "settings-modal__section-header",
                            h3 { class: "settings-modal__section-title", "CodeStral Completion" }
                            p {
                                class: "settings-modal__section-hint",
                                "AI-powered SQL code completion via CodeStral API."
                            }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.codestral.enabled,
                                disabled: settings.codestral.api_key.is_empty(),
                                oninput: move |event| {
                                    let mut next = props.read().settings.clone();
                                    next.codestral.enabled = event.checked();
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                            }
                            span { "Enable CodeStral inline completion" }
                        }
                        div {
                            class: "field",
                            span { class: "field__label", "API Key" }
                            input {
                                class: "input",
                                r#type: "password",
                                placeholder: "sk-...",
                                value: "{settings.codestral.api_key}",
                                oninput: move |event| {
                                    let mut next = props.read().settings.clone();
                                    let value = event.value();
                                    next.codestral.api_key = value.clone();
                                    if value.trim().is_empty() {
                                        next.codestral.enabled = false;
                                    }
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                            }
                        }
                        div {
                            class: "field",
                            span { class: "field__label", "Model" }
                            input {
                                class: "input",
                                placeholder: "codestral-latest",
                                value: "{settings.codestral.model}",
                                oninput: move |event| {
                                    let mut next = props.read().settings.clone();
                                    next.codestral.model = event.value();
                                    on_change.call((next, props.read().sql_settings.clone()));
                                },
                            }
                        }
                        if settings.codestral.api_key.is_empty() {
                            p {
                                class: "settings-modal__section-hint",
                                "Enter an API key to enable CodeStral completion. Get your key from "
                                a {
                                    href: "https://codestral.mistral.ai/",
                                    target: "_blank",
                                    "codestral.mistral.ai"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Thin adapter that re-exposes [`SqlFormatSettingsFields`] (which expects a
/// `Signal<SqlFormatSettings>`) while keeping [`SettingsModal`] prop-driven.
///
/// `SqlFormatSettingsFields` mutates the `Signal` it owns directly, so we
/// can't hook into individual edits. Instead, the adapter:
/// 1. Mirrors the incoming `sql_settings` prop into a local signal.
/// 2. Hands that signal to `SqlFormatSettingsFields`.
/// 3. Watches the local signal with `use_effect` and emits the latest value
///    through `on_change` whenever it changes.
///
/// When the parent supplies a new `sql_settings` prop (e.g. after a
/// round-trip through the dialog bridge) the local signal is reset to match.
#[component]
fn SqlFormatFieldsAdapter(
    sql_settings: SqlFormatSettings,
    settings: AppUiSettings,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
) -> Element {
    // Wrap both props in signals so the multiple `move` closures below can
    // each capture a cheap `Signal` clone instead of moving the owned values.
    let sql_settings = use_signal(move || sql_settings);
    let settings = use_signal(move || settings);

    // Local working copy — `SqlFormatSettingsFields` mutates this signal.
    let mut local_sql = use_signal(move || sql_settings.read().clone());

    // Whenever the prop changes (e.g. bridge round-tripped a fresh value from
    // the main window), re-seed the local signal. `use_effect` with an
    // equality check avoids overwriting unsaved edits if the prop value is
    // already current.
    use_effect(move || {
        if local_sql.peek().clone() != sql_settings.read().clone() {
            local_sql.set(sql_settings.read().clone());
        }
    });

    // Propagate every local change upstream through `on_change`.
    use_effect(move || {
        on_change.call((settings.read().clone(), local_sql()));
    });

    rsx! {
        SqlFormatSettingsFields { settings: local_sql }
    }
}

fn parse_u32_in_range(value: &str, fallback: u32, min: u32, max: u32) -> u32 {
    value
        .parse::<u32>()
        .map(|parsed| parsed.clamp(min, max))
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::AppUiSettings;

    /// Pure toggle helper: returns a clone of `ui` with `flag` set to `value`.
    /// Mirrors the `set_show_*` helpers in `crate::app_state` but is exposed
    /// here as a pure function so the SettingsModal prop contract can be
    /// exercised without standing up a renderer.
    fn toggle_show_flag(mut ui: AppUiSettings, flag: ShowFlag, value: bool) -> AppUiSettings {
        match flag {
            ShowFlag::SavedQueries => ui.show_saved_queries = value,
            ShowFlag::Connections => ui.show_connections = value,
            ShowFlag::Explorer => ui.show_explorer = value,
            ShowFlag::History => ui.show_history = value,
            ShowFlag::SqlEditor => ui.show_sql_editor = value,
            ShowFlag::AgentPanel => ui.show_agent_panel = value,
        }
        ui
    }

    #[derive(Copy, Clone, Debug)]
    #[allow(dead_code)]
    enum ShowFlag {
        SavedQueries,
        Connections,
        Explorer,
        History,
        SqlEditor,
        AgentPanel,
    }

    #[test]
    fn toggle_show_flag_only_mutates_target_field() {
        let before = AppUiSettings::default();
        let after = toggle_show_flag(before.clone(), ShowFlag::Explorer, false);

        // Unrelated flags preserved.
        assert_eq!(after.show_saved_queries, before.show_saved_queries);
        assert_eq!(after.show_connections, before.show_connections);
        assert_eq!(after.show_history, before.show_history);
        assert_eq!(after.show_sql_editor, before.show_sql_editor);
        assert_eq!(after.show_agent_panel, before.show_agent_panel);

        // Target flag flipped.
        assert!(before.show_explorer);
        assert!(!after.show_explorer);

        // Deepseek/Codestral config preserved (reset UI keeps API keys).
        assert_eq!(after.deepseek, before.deepseek);
        assert_eq!(after.codestral, before.codestral);
    }

    #[test]
    fn toggle_show_flag_round_trips_all_variants() {
        // Sanity check that every `ShowFlag` variant maps to its own field —
        // guards against copy-paste regressions in the match arms.
        let mut current = AppUiSettings::default();
        current = toggle_show_flag(current, ShowFlag::SavedQueries, false);
        assert!(!current.show_saved_queries);
        current = toggle_show_flag(current, ShowFlag::Connections, true);
        assert!(current.show_connections);
        current = toggle_show_flag(current, ShowFlag::History, true);
        assert!(current.show_history);
        current = toggle_show_flag(current, ShowFlag::SqlEditor, true);
        assert!(current.show_sql_editor);
        current = toggle_show_flag(current, ShowFlag::AgentPanel, true);
        assert!(current.show_agent_panel);
    }

    #[test]
    fn parse_u32_in_range_clamps_and_falls_back() {
        assert_eq!(parse_u32_in_range("", 50, 10, 1000), 50);
        assert_eq!(parse_u32_in_range("not-a-number", 50, 10, 1000), 50);
        assert_eq!(parse_u32_in_range("5", 50, 10, 1000), 10);
        assert_eq!(parse_u32_in_range("9999", 50, 10, 1000), 1000);
        assert_eq!(parse_u32_in_range("250", 50, 10, 1000), 250);
    }
}
