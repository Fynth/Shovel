use crate::{
    app_state::{
        APP_SHOW_SETTINGS_MODAL,
        APP_SQL_FORMAT_SETTINGS,
        APP_UI_SETTINGS,
        close_settings_modal,
        reset_ui_settings,
        set_ai_features_enabled,
        set_codestral_api_key,
        set_codestral_enabled,
        set_codestral_model,
        set_deepseek_api_key,
        set_deepseek_base_url,
        set_deepseek_enabled,
        set_deepseek_model,
        set_deepseek_reasoning_effort,
        set_deepseek_thinking_enabled,
        set_default_page_size,
        set_read_only_mode,
        set_restore_session_on_launch,
        set_show_agent_panel,
        set_show_connections,
        set_show_explorer,
        set_show_history,
        set_show_saved_queries,
        set_show_sql_editor,
        set_theme_preference,
    },
    screens::SqlFormatSettingsFields,
};
use dioxus::prelude::*;
use models::AppThemePreference;

#[component]
#[allow(clippy::redundant_closure)]
pub fn SettingsModal() -> Element {
    if !APP_SHOW_SETTINGS_MODAL() {
        return VNode::empty();
    }

    let mut sql_format_settings = use_signal(|| APP_SQL_FORMAT_SETTINGS());
    let settings = APP_UI_SETTINGS();

    use_effect(move || {
        let settings = sql_format_settings();
        if APP_SQL_FORMAT_SETTINGS() != settings {
            *APP_SQL_FORMAT_SETTINGS.write() = settings;
        }
    });

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| close_settings_modal(),
            div {
                class: "settings-modal",
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "settings-modal__header",
                    div {
                        class: "settings-modal__header-copy",
                        h2 { class: "settings-modal__title", "Settings" }
                        p {
                            class: "settings-modal__hint",
                            "Theme, workspace defaults and SQL formatting are saved automatically."
                        }
                    }
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| close_settings_modal(),
                        "Close"
                    }
                }

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
                            button {
                                class: if settings.theme == AppThemePreference::Dark {
                                    "button button--ghost button--small button--active"
                                } else {
                                    "button button--ghost button--small"
                                },
                                onclick: move |_| {
                                    set_theme_preference(AppThemePreference::Dark);
                                },
                                "Dark"
                            }
                            button {
                                class: if settings.theme == AppThemePreference::Light {
                                    "button button--ghost button--small button--active"
                                } else {
                                    "button button--ghost button--small"
                                },
                                onclick: move |_| {
                                    set_theme_preference(AppThemePreference::Light);
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
                                    set_deepseek_enabled(event.checked());
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
                                        set_deepseek_api_key(event.value());
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
                                        set_deepseek_base_url(event.value());
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
                                        set_deepseek_model(event.value());
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
                                        set_deepseek_reasoning_effort(event.value());
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
                                    set_deepseek_thinking_enabled(event.checked());
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
                            button {
                                class: "button button--ghost button--small",
                                onclick: move |_| reset_ui_settings(),
                                "Reset UI"
                            }
                        }
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
                                        set_default_page_size(parse_u32_in_range(
                                            &event.value(),
                                            settings.default_page_size,
                                            10,
                                            1000,
                                        ));
                                    },
                                }
                            }
                        }
                        p {
                            class: "settings-modal__section-hint",
                            "Tool panels can be dragged between the left sidebar and the right inspector."
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.ai_features_enabled,
                                oninput: move |event| {
                                    set_ai_features_enabled(event.checked());
                                },
                            }
                            span { "Enable AI features (ACP panel, prompts, and SQL actions)" }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.restore_session_on_launch,
                                oninput: move |event| {
                                    set_restore_session_on_launch(event.checked());
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
                                    set_read_only_mode(event.checked());
                                },
                            }
                            span { "Read-only mode (block write SQL, imports, and table edits)" }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.show_saved_queries,
                                oninput: move |event| {
                                    set_show_saved_queries(event.checked());
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
                                    set_show_connections(event.checked());
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
                                    set_show_explorer(event.checked());
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
                                    set_show_history(event.checked());
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
                                    set_show_sql_editor(event.checked());
                                },
                            }
                            span { "Show SQL editor by default" }
                        }
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: settings.show_agent_panel,
                                disabled: !settings.ai_features_enabled,
                                oninput: move |event| {
                                    set_show_agent_panel(event.checked());
                                },
                            }
                            span { "Show ACP agent panel by default" }
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
                            button {
                                class: "button button--ghost button--small",
                                onclick: move |_| sql_format_settings.set(models::SqlFormatSettings::default()),
                                "Reset SQL"
                            }
                        }
                        SqlFormatSettingsFields {
                            settings: sql_format_settings,
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
                                    set_codestral_enabled(event.checked());
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
                                    set_codestral_api_key(event.value());
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
                                    set_codestral_model(event.value());
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

fn parse_u32_in_range(value: &str, fallback: u32, min: u32, max: u32) -> u32 {
    value
        .parse::<u32>()
        .map(|parsed| parsed.clamp(min, max))
        .unwrap_or(fallback)
}
