use crate::{components::tooltip_target::TooltipTarget, screens::SqlFormatSettingsFields};
use dioxus::prelude::*;
use models::{
    ActiveModel,
    AiModelEntry,
    AiProviderKind,
    AppThemePreference,
    AppUiSettings,
    BuiltinProviderSpec,
    CustomNativeProvider,
    NullDisplay,
    SqlFormatSettings,
    UiDensity,
    WorkspaceSplitMode,
    builtin_providers,
    normalize_native_chat_url,
};
use std::collections::BTreeMap;

use super::{
    SettingsSectionProps,
    reset_ui_preserving_secrets,
    sync_section_props,
    widgets::{ColorField, FontSelect, SliderField},
};

const UI_FONT_OPTIONS: [(&str, &str); 3] = [
    (
        "SF Pro Text, IBM Plex Sans, Segoe UI, sans-serif",
        "System UI",
    ),
    ("IBM Plex Sans, sans-serif", "IBM Plex Sans"),
    ("Segoe UI, sans-serif", "Segoe UI"),
];

const EDITOR_FONT_OPTIONS: [(&str, &str); 4] = [
    (
        "JetBrains Mono, SF Mono, Cascadia Code, monospace",
        "JetBrains Mono",
    ),
    ("SF Mono, ui-monospace, monospace", "SF Mono"),
    ("Cascadia Code, ui-monospace, monospace", "Cascadia Code"),
    ("ui-monospace, monospace", "ui-monospace"),
];

fn font_options(options: &[(&str, &str)]) -> Vec<(String, String)> {
    options
        .iter()
        .map(|(family, label)| ((*family).to_string(), (*label).to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Section components
// ---------------------------------------------------------------------------
//
// Each section is rendered as a sibling of the nav inside
// `.settings-modal__content` — only the sections belonging to the active
// category are mounted at a given time. Every section helper takes
// `SettingsSectionProps { settings, sql_settings, on_change }` so closures can
// build and emit the full pair on each edit (the dialog bridge needs a
// complete snapshot, not a partial diff).

#[component]
pub(super) fn AppearanceSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);
    let accent = settings
        .theme_overrides
        .primary
        .clone()
        .unwrap_or_else(|| "#5eb1ff".to_string());
    let ui_font = settings
        .theme_overrides
        .font_family
        .clone()
        .unwrap_or_else(|| UI_FONT_OPTIONS[0].0.to_string());
    let editor_font = settings
        .theme_overrides
        .font_family_mono
        .clone()
        .unwrap_or_else(|| EDITOR_FONT_OPTIONS[0].0.to_string());
    let ui_font_size = settings.theme_overrides.font_size.unwrap_or(12);
    let radius = settings.theme_overrides.radius_small.unwrap_or(7);

    rsx! {
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.theme = AppThemePreference::Dark;
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.theme = AppThemePreference::Light;
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                    "Light"
                }
            }
            div {
                class: "settings-modal__segmented settings-modal__segmented--density",
                role: "group",
                aria_label: "UI density",
                for variant in UiDensity::ALL {
                    button {
                        key: "{variant.css_class()}",
                        class: if settings.density == variant {
                            "button button--ghost button--small button--active"
                        } else {
                            "button button--ghost button--small"
                        },
                        aria_pressed: settings.density == variant,
                        onclick: move |_| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.density = variant;
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                        "{variant.label()}"
                    }
                }
            }
            p {
                class: "settings-modal__section-hint",
                "Compact for an IDE-style dense workspace; Comfortable for larger tap targets."
            }
            ColorField {
                label: "Accent".to_string(),
                value: accent,
                on_change: move |hex| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.theme_overrides.primary = Some(hex);
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            FontSelect {
                label: "UI font".to_string(),
                value: ui_font,
                options: font_options(&UI_FONT_OPTIONS),
                on_change: move |family| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.theme_overrides.font_family = Some(family);
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            FontSelect {
                label: "Editor font".to_string(),
                value: editor_font,
                options: font_options(&EDITOR_FONT_OPTIONS),
                on_change: move |family| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.theme_overrides.font_family_mono = Some(family);
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            SliderField {
                label: "UI font size".to_string(),
                value: ui_font_size,
                min: 10,
                max: 16,
                on_change: move |n| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.theme_overrides.font_size = Some(n);
                    next.theme_overrides.font_size_small = Some(n.saturating_sub(1).max(10));
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            SliderField {
                label: "Corner radius".to_string(),
                value: radius,
                min: 0,
                max: 12,
                on_change: move |n| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.theme_overrides.radius_small = Some(n);
                    next.theme_overrides.radius_medium = Some(n + 2);
                    next.theme_overrides.radius_large = Some(n + 4);
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
        }
    }
}

#[component]
pub(super) fn DatabaseSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Database" }
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
                                let mut next = section_props_signal.read().settings.clone();
                                next.default_page_size = parse_u32_in_range(
                                    &event.value(),
                                    section_props_signal.read().settings.default_page_size,
                                    10,
                                    1000,
                                );
                                on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.restore_session_on_launch = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.read_only_mode = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Read-only mode (block write SQL, imports, and table edits)" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.behavior.confirm_before_drop,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.behavior.confirm_before_drop = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Confirm before drop" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.behavior.confirm_before_truncate,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.behavior.confirm_before_truncate = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Confirm before truncate" }
                }
            }
        }
    }
}

#[component]
pub(super) fn EditorBehaviorSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Editor" }
            }
            SliderField {
                label: "Font size".to_string(),
                value: settings.editor.font_size,
                min: 10,
                max: 22,
                on_change: move |n| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.editor.font_size = n;
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            SliderField {
                label: "Tab size".to_string(),
                value: settings.editor.tab_size,
                min: 1,
                max: 8,
                on_change: move |n| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.editor.tab_size = n;
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.editor.word_wrap,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.editor.word_wrap = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Word wrap" }
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.editor.show_line_numbers,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.editor.show_line_numbers = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Show line numbers" }
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.editor.auto_format_on_run,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.editor.auto_format_on_run = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Auto-format SQL on run" }
            }
        }
    }
}

#[component]
pub(super) fn GridSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Grid" }
            }
            SliderField {
                label: "Row height".to_string(),
                value: settings.grid.row_height,
                min: 18,
                max: 48,
                on_change: move |n| {
                    let mut next = section_props_signal.read().settings.clone();
                    next.grid.row_height = n;
                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                },
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.grid.zebra,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.grid.zebra = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Zebra stripes" }
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.grid.wrap_cells,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.grid.wrap_cells = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Wrap cells" }
            }
            div {
                class: "field",
                span { class: "field__label", "Null display" }
                div {
                    class: "settings-modal__segmented settings-modal__segmented--density",
                    role: "group",
                    aria_label: "Null display",
                    for variant in NullDisplay::ALL {
                        button {
                            key: "{variant.label()}",
                            class: if settings.grid.null_display == variant {
                                "button button--ghost button--small button--active"
                            } else {
                                "button button--ghost button--small"
                            },
                            aria_pressed: settings.grid.null_display == variant,
                            onclick: move |_| {
                                let mut next = section_props_signal.read().settings.clone();
                                next.grid.null_display = variant;
                                on_change.call((next, section_props_signal.read().sql_settings.clone()));
                            },
                            "{variant.label()}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn NavigationSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Navigation" }
            }
            div {
                class: "settings-modal__group",
                span {
                    class: "settings-modal__group-title",
                    "Explorer view"
                }
                p {
                    class: "settings-modal__section-hint",
                    "Controls which object types the connection explorer tree renders. Changes apply immediately."
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_schemas,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_schemas = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show schemas" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_tables,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_tables = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show tables" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_views,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_views = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show views (incl. materialized views)" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_columns,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_columns = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show column children under tables" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_system_objects,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_system_objects = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show system objects (pg_catalog, information_schema, mysql, sys, system)" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.show_row_counts,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.show_row_counts = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show estimated row counts next to tables" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.explorer.sort_alphabetical,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.explorer.sort_alphabetical = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Sort objects alphabetically within each group" }
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_saved_queries = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_connections = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_explorer = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_history = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_sql_editor = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show SQL editor by default" }
                }
                div {
                    class: "field",
                    span { class: "field__label", "Editor / result split" }
                    div {
                        class: "settings-modal__segmented settings-modal__segmented--split-mode",
                        role: "group",
                        aria_label: "Editor and result split mode",
                        for variant in WorkspaceSplitMode::ALL {
                            button {
                                key: "{variant.css_class()}",
                                class: if settings.split_mode == variant {
                                    "button button--ghost button--small button--active"
                                } else {
                                    "button button--ghost button--small"
                                },
                                aria_pressed: settings.split_mode == variant,
                                onclick: move |_| {
                                    let mut next = section_props_signal.read().settings.clone();
                                    next.split_mode = variant;
                                    on_change.call((next, section_props_signal.read().sql_settings.clone()));
                                },
                                "{variant.short_label()}"
                            }
                        }
                    }
                    p {
                        class: "settings-modal__section-hint",
                        "Single pane stacks the editor and result vertically. Side by side puts them in two columns. Stacked split keeps the stacked geometry with an explicit split divider."
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
                        checked: settings.show_agent_panel,
                        disabled: !settings.ai_features_enabled,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_agent_panel = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show ACP agent panel by default" }
                }
                label {
                    class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: settings.show_bottom_panel,
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.show_bottom_panel = event.checked();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                    span { "Show bottom dock (Output / Messages / Query Log / Transactions / Problems) by default" }
                }
            }
        }
    }
}

#[component]
pub(super) fn DeepSeekAgentSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.deepseek.enabled = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            let value = event.value();
                            next.deepseek.api_key = value.clone();
                            if value.trim().is_empty() {
                                next.deepseek.enabled = false;
                            }
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.deepseek.base_url = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.deepseek.model = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                            let mut next = section_props_signal.read().settings.clone();
                            next.deepseek.reasoning_effort = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.deepseek.thinking_enabled = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
    }
}

#[component]
pub(super) fn OllamaSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Ollama" }
                p {
                    class: "settings-modal__section-hint",
                    "Local or remote Ollama endpoint for the embedded SQL agent. API key is optional for local servers."
                }
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.ollama.enabled,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.ollama.enabled = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Use Ollama as an embedded SQL agent" }
            }
            div {
                class: "settings-modal__grid",
                div {
                    class: "field",
                    span { class: "field__label", "API Key" }
                    input {
                        class: "input",
                        r#type: "password",
                        placeholder: "optional",
                        value: "{settings.ollama.api_key}",
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.ollama.api_key = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                }
                div {
                    class: "field",
                    span { class: "field__label", "Base URL" }
                    input {
                        class: "input",
                        placeholder: "http://localhost:11434/api",
                        value: "{settings.ollama.base_url}",
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.ollama.base_url = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                }
                div {
                    class: "field",
                    span { class: "field__label", "Model" }
                    input {
                        class: "input",
                        placeholder: "llama3.2",
                        value: "{settings.ollama.model}",
                        oninput: move |event| {
                            let mut next = section_props_signal.read().settings.clone();
                            next.ollama.model = event.value();
                            on_change.call((next, section_props_signal.read().sql_settings.clone()));
                        },
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn LanguageModelsSection(props: SettingsSectionProps) -> Element {
    let on_change = props.on_change;
    let section = use_signal(|| props.clone());
    let extra_drafts = use_signal(BTreeMap::<String, String>::new);
    let mut custom_name = use_signal(String::new);
    let mut custom_url = use_signal(String::new);
    let mut custom_key = use_signal(String::new);
    let settings = section.read().settings.clone();
    let native_specs: Vec<BuiltinProviderSpec> = builtin_providers()
        .iter()
        .copied()
        .filter(|spec| spec.kind() == AiProviderKind::NativeHttp)
        .collect();
    let active_label = match settings.ai_catalog.active.as_ref() {
        Some(active) if !active.model.trim().is_empty() => {
            format!("{} / {}", active.provider, active.model)
        }
        Some(active) => active.provider.clone(),
        None => "None".to_string(),
    };

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Language models" }
            }
            p {
                class: "settings-modal__section-hint",
                "Default model: {active_label}. Keys stay in the OS keyring, not in JSON."
            }
            for spec in native_specs {
                {
                    native_http_provider_card(spec, &settings, section, on_change, extra_drafts)
                }
            }
            div { class: "settings-modal__group",
                span { class: "settings-modal__group-title", "Custom providers" }
                for custom in settings.ai_catalog.custom_native.clone() {
                    {
                        custom_native_provider_card(
                            custom,
                            &settings,
                            section,
                            on_change,
                            extra_drafts,
                        )
                    }
                }
                div { class: "settings-modal__grid",
                    div { class: "field",
                        span { class: "field__label", "Name" }
                        input {
                            class: "input",
                            placeholder: "My provider",
                            value: "{custom_name()}",
                            oninput: move |event| custom_name.set(event.value()),
                        }
                    }
                    div { class: "field",
                        span { class: "field__label", "Base URL" }
                        input {
                            class: "input",
                            placeholder: "http://localhost:8080",
                            value: "{custom_url()}",
                            oninput: move |event| custom_url.set(event.value()),
                        }
                    }
                    div { class: "field",
                        span { class: "field__label", "API Key" }
                        input {
                            class: "input",
                            r#type: "password",
                            placeholder: "sk-...",
                            value: "{custom_key()}",
                            oninput: move |event| custom_key.set(event.value()),
                        }
                    }
                }
                div { class: "settings-modal__section-actions",
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| {
                            let id = new_custom_native_id();
                            let name = custom_name().trim().to_string();
                            let base_url = custom_url().trim().to_string();
                            let key = custom_key();
                            emit_ui_update(section, on_change, |next| {
                                if !key.trim().is_empty() {
                                    next.set_lm_api_key(&id, key.clone());
                                }
                                next.ai_catalog.custom_native.push(CustomNativeProvider {
                                    id,
                                    name,
                                    base_url,
                                    models: Vec::new(),
                                });
                            });
                            custom_name.set(String::new());
                            custom_url.set(String::new());
                            custom_key.set(String::new());
                        },
                        "Add provider"
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn AdvancedSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "AI features" }
                div {
                    class: "settings-modal__section-actions",
                    TooltipTarget {
                        label: "Reset workspace, panels, and AI settings to their defaults (API keys and keyboard shortcuts are preserved)".to_string(),
                        button {
                            class: "button button--ghost button--small",
                            onclick: move |_| {
                                let next = reset_ui_preserving_secrets(
                                    &section_props_signal.read().settings,
                                );
                                on_change.call((next, section_props_signal.read().sql_settings.clone()));
                            },
                            "Reset UI"
                        }
                    }
                }
            }
            label {
                class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: settings.ai_features_enabled,
                    oninput: move |event| {
                        let mut next = section_props_signal.read().settings.clone();
                        next.ai_features_enabled = event.checked();
                        if !event.checked() {
                            next.show_agent_panel = false;
                        }
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.ai_response_language = event.value();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.ai_auto_apply_completions = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
                    },
                }
                span { "Auto-apply inline AI completions (insert after a short idle pause; otherwise press Tab to accept)" }
            }
        }
    }
}

#[component]
pub(super) fn SqlFormattingSection(props: SettingsSectionProps) -> Element {
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
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
                                on_change.call((section_props_signal.read().settings.clone(), SqlFormatSettings::default()));
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
                sql_settings: section_props_signal.read().sql_settings.clone(),
                settings: section_props_signal.read().settings.clone(),
                on_change,
            }
        }
    }
}

#[component]
pub(super) fn CodeStralCompletionSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());
    sync_section_props(section_props_signal, &props);

    rsx! {
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.codestral.enabled = event.checked();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        let value = event.value();
                        next.codestral.api_key = value.clone();
                        if value.trim().is_empty() {
                            next.codestral.enabled = false;
                        }
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
                        let mut next = section_props_signal.read().settings.clone();
                        next.codestral.model = event.value();
                        on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
    // Write incoming props back — `use_signal(|| value)` only captures mount
    // time, and SQL edits must emit the latest UI snapshot.
    let mut sql_settings_signal = use_signal(|| sql_settings.clone());
    let mut settings_signal = use_signal(|| settings.clone());
    if *sql_settings_signal.peek() != sql_settings {
        sql_settings_signal.set(sql_settings.clone());
    }
    if *settings_signal.peek() != settings {
        settings_signal.set(settings.clone());
    }

    // Local working copy — `SqlFormatSettingsFields` mutates this signal.
    let mut local_sql = use_signal(|| sql_settings.clone());

    // Whenever the prop changes (e.g. bridge round-tripped a fresh value from
    // the main window), re-seed the local signal. `use_effect` with an
    // equality check avoids overwriting unsaved edits if the prop value is
    // already current.
    use_effect(move || {
        if *local_sql.peek() != *sql_settings_signal.read() {
            local_sql.set(sql_settings_signal.read().clone());
        }
    });

    // Propagate every local change upstream through `on_change`.
    use_effect(move || {
        on_change.call((settings_signal.read().clone(), local_sql()));
    });

    rsx! {
        SqlFormatSettingsFields { settings: local_sql }
    }
}

/// Shows where `config.toml` lives and lets the user open it in the OS file
/// manager or reload it. The config file is the declarative customization
/// surface — every setting in this modal can also be set there.
#[component]
pub(super) fn ConfigSection(props: SettingsSectionProps) -> Element {
    let config_path = use_memo(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            })
            .join("shovel")
            .join("config.toml")
    });
    let config_path = config_path();
    let path_display = config_path.display().to_string();
    let current_settings = props.settings.clone();
    let sql_settings = props.sql_settings.clone();
    let on_change = props.on_change;

    rsx! {
        section {
            class: "settings-modal__section",
            div {
                class: "settings-modal__section-header",
                div {
                    h3 { class: "settings-modal__section-title", "Config file" }
                    p {
                        class: "settings-modal__section-hint",
                        "Every setting in this modal can also be set declaratively in config.toml. Edit the file and reload to apply."
                    }
                }
            }
            div {
                class: "settings-modal__group",
                span { class: "settings-modal__group-title", "Location" }
                div {
                    class: "settings-modal__config-path",
                    code { {path_display.to_string()} }
                }
                p {
                    class: "settings-modal__section-hint",
                    "On first launch Shovel writes a default config.toml here. On Linux it lives under ~/.local/share/shovel; on Windows under %LOCALAPPDATA%\\shovel."
                }
            }
            div {
                class: "settings-modal__group",
                span { class: "settings-modal__group-title", "Actions" }
                div {
                    class: "settings-modal__actions",
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| {
                            let _ = open_config_file(&config_path);
                        },
                        "Open config folder"
                    }
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| {
                            match reload_config(&current_settings) {
                                Ok(next) => {
                                    on_change.call((next, sql_settings.clone()));
                                }
                                Err(err) => crate::app_state::toast_error(err),
                            }
                        },
                        "Reload config"
                    }
                }
            }
        }
    }
}

/// Open the config file's parent folder in the OS file manager.
fn open_config_file(path: &std::path::Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "config path has no parent".to_string())?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(parent)
            .spawn()
            .map_err(|err| format!("failed to open config folder: {err}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|err| format!("failed to open config folder: {err}"))?;
    }
    Ok(())
}

/// Reload `config.toml` and merge it onto `current`.
///
/// The settings window is a separate VirtualDom, so this must not read or
/// write main-window globals. The caller emits the merged snapshot through
/// `on_change`, which updates the window locally and bridges to the main
/// window. `ShovelConfig::load` errors are returned as-is for toasting.
fn reload_config(current: &AppUiSettings) -> Result<AppUiSettings, String> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join("shovel");
    let path = data_dir.join("config.toml");
    let Some(config) = models::ShovelConfig::load(&path)? else {
        return Ok(current.clone());
    };
    let mut settings = current.clone();
    config.apply_to(&mut settings);
    Ok(settings)
}

fn emit_ui_update(
    mut section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
    update: impl FnOnce(&mut AppUiSettings),
) {
    let sql = section.peek().sql_settings.clone();
    let mut next = section.peek().settings.clone();
    update(&mut next);
    section.write().settings = next.clone();
    on_change.call((next, sql));
}

fn new_custom_native_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("custom:{nanos:032x}")
}

fn catalog_refresh_base_url(settings: &AppUiSettings, slug: &str, default_base: &str) -> String {
    if let Some(custom) = settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == slug)
    {
        return normalize_native_chat_url(&custom.base_url, &custom.base_url);
    }
    let override_url = settings
        .ai_catalog
        .overrides
        .get(slug)
        .map(|over| over.base_url.as_str())
        .unwrap_or("");
    normalize_native_chat_url(override_url, default_base)
}

fn merge_refreshed_models_into(
    settings: &mut AppUiSettings,
    slug: &str,
    fetched: Vec<AiModelEntry>,
) {
    if let Some(custom) = settings
        .ai_catalog
        .custom_native
        .iter_mut()
        .find(|custom| custom.id == slug)
    {
        for model in fetched {
            if custom.models.iter().any(|existing| existing.id == model.id) {
                continue;
            }
            custom.models.push(model);
        }
        return;
    }
    let builtin_ids: Vec<&str> = builtin_providers()
        .iter()
        .find(|spec| spec.slug == slug)
        .map(|spec| spec.builtin_models.iter().map(|(id, _)| *id).collect())
        .unwrap_or_default();
    let extra = &mut settings
        .ai_catalog
        .overrides
        .entry(slug.to_string())
        .or_default()
        .extra_models;
    for model in fetched {
        if builtin_ids.iter().any(|id| *id == model.id) {
            continue;
        }
        if extra.iter().any(|existing| existing.id == model.id) {
            continue;
        }
        extra.push(model);
    }
}

fn refresh_catalog_models(
    mut section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
    slug: String,
    default_base: String,
) {
    let settings = section.peek().settings.clone();
    let api_key = settings.lm_api_key(&slug);
    let base_url = catalog_refresh_base_url(&settings, &slug, &default_base);
    spawn(async move {
        match services::refresh_provider_models(&slug, &base_url, &api_key).await {
            Ok(models) => {
                let sql = section.peek().sql_settings.clone();
                let mut next = section.peek().settings.clone();
                merge_refreshed_models_into(&mut next, &slug, models);
                section.write().settings = next.clone();
                on_change.call((next, sql));
            }
            Err(err) => crate::app_state::toast_error(err),
        }
    });
}

fn extra_draft_value(drafts: Signal<BTreeMap<String, String>>, slug: &str) -> String {
    drafts.read().get(slug).cloned().unwrap_or_default()
}

fn native_http_provider_card(
    spec: BuiltinProviderSpec,
    settings: &AppUiSettings,
    section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
    mut extra_drafts: Signal<BTreeMap<String, String>>,
) -> Element {
    let slug = spec.slug.to_string();
    let over = settings
        .ai_catalog
        .overrides
        .get(spec.slug)
        .cloned()
        .unwrap_or_default();
    let api_key = settings.lm_api_key(spec.slug);
    let extra_draft = extra_draft_value(extra_drafts, spec.slug);
    let enabled_slug = slug.clone();
    let key_slug = slug.clone();
    let url_slug = slug.clone();
    let refresh_slug = slug.clone();
    let add_slug = slug.clone();
    let thinking_slug = slug.clone();
    let reasoning_slug = slug.clone();
    let default_base = spec.default_base_url.to_string();

    rsx! {
        div { class: "settings-modal__group",
            span { class: "settings-modal__group-title", "{spec.label}" }
            label { class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: over.enabled,
                    oninput: move |event| {
                        let checked = event.checked();
                        let slug = enabled_slug.clone();
                        emit_ui_update(section, on_change, move |next| {
                            next.ai_catalog.overrides.entry(slug).or_default().enabled = checked;
                        });
                    },
                }
                span { "Enabled" }
            }
            div { class: "settings-modal__grid",
                div { class: "field",
                    span { class: "field__label", "API Key" }
                    input {
                        class: "input",
                        r#type: "password",
                        placeholder: "sk-...",
                        value: api_key,
                        oninput: move |event| {
                            let value = event.value();
                            let slug = key_slug.clone();
                            emit_ui_update(section, on_change, move |next| {
                                next.set_lm_api_key(&slug, value);
                            });
                        },
                    }
                }
                div { class: "field",
                    span { class: "field__label", "Base URL" }
                    input {
                        class: "input",
                        placeholder: "{spec.default_base_url}",
                        value: "{over.base_url}",
                        oninput: move |event| {
                            let value = event.value();
                            let slug = url_slug.clone();
                            emit_ui_update(section, on_change, move |next| {
                                next.ai_catalog.overrides.entry(slug).or_default().base_url =
                                    value;
                            });
                        },
                    }
                }
            }
            if spec.slug == "deepseek" {
                label { class: "settings-modal__toggle",
                    input {
                        r#type: "checkbox",
                        checked: over.thinking_enabled,
                        oninput: move |event| {
                            let checked = event.checked();
                            let slug = thinking_slug.clone();
                            emit_ui_update(section, on_change, move |next| {
                                next.ai_catalog
                                    .overrides
                                    .entry(slug)
                                    .or_default()
                                    .thinking_enabled = checked;
                            });
                        },
                    }
                    span { "Enable thinking mode when the selected model supports it" }
                }
                div { class: "field",
                    span { class: "field__label", "Reasoning effort" }
                    select {
                        class: "input",
                        value: "{over.reasoning_effort}",
                        onchange: move |event| {
                            let value = event.value();
                            let slug = reasoning_slug.clone();
                            emit_ui_update(section, on_change, move |next| {
                                next.ai_catalog
                                    .overrides
                                    .entry(slug)
                                    .or_default()
                                    .reasoning_effort = value;
                            });
                        },
                        option { value: "low", "low" }
                        option { value: "medium", "medium" }
                        option { value: "high", "high" }
                    }
                }
            }
            for (model_id, model_label) in spec.builtin_models.iter().copied() {
                {
                    builtin_model_row(
                        slug.clone(),
                        model_id,
                        model_label,
                        &over.hidden_builtin_ids,
                        settings,
                        section,
                        on_change,
                    )
                }
            }
            for extra in over.extra_models.iter().cloned() {
                {
                    extra_model_row(
                        slug.clone(),
                        extra,
                        false,
                        settings,
                        section,
                        on_change,
                    )
                }
            }
            div { class: "settings-modal__grid",
                div { class: "field",
                    span { class: "field__label", "Add extra model" }
                    input {
                        class: "input",
                        placeholder: "model-id",
                        value: extra_draft,
                        oninput: move |event| {
                            extra_drafts.write().insert(slug.clone(), event.value());
                        },
                    }
                }
            }
            div { class: "settings-modal__section-actions",
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| {
                        let id = extra_drafts
                            .write()
                            .remove(&add_slug)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        if id.is_empty() {
                            return;
                        }
                        let slug = add_slug.clone();
                        emit_ui_update(section, on_change, move |next| {
                            let extra = &mut next
                                .ai_catalog
                                .overrides
                                .entry(slug)
                                .or_default()
                                .extra_models;
                            if extra.iter().any(|existing| existing.id == id) {
                                return;
                            }
                            extra.push(AiModelEntry {
                                id,
                                label: String::new(),
                            });
                        });
                    },
                    "Add extra model"
                }
                if spec.supports_model_refresh() {
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| {
                            refresh_catalog_models(
                                section,
                                on_change,
                                refresh_slug.clone(),
                                default_base.clone(),
                            );
                        },
                        "Refresh"
                    }
                }
            }
        }
    }
}

fn builtin_model_row(
    slug: String,
    model_id: &str,
    model_label: &str,
    hidden_ids: &[String],
    settings: &AppUiSettings,
    section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
) -> Element {
    let model_id = model_id.to_string();
    let display = if model_label.trim().is_empty() {
        model_id.clone()
    } else {
        model_label.to_string()
    };
    let hidden = hidden_ids.iter().any(|id| id == &model_id);
    let hide_slug = slug.clone();
    let hide_id = model_id.clone();
    let default_slug = slug;
    let default_id = model_id.clone();
    let is_default = settings
        .ai_catalog
        .active
        .as_ref()
        .is_some_and(|active| active.provider == default_slug && active.model == default_id);
    let default_label = if is_default {
        "Default"
    } else {
        "Use as default"
    };

    rsx! {
        div { class: "settings-modal__grid",
            span { class: "field__label", {display} }
            label { class: "settings-modal__toggle",
                input {
                    r#type: "checkbox",
                    checked: hidden,
                    oninput: move |event| {
                        let checked = event.checked();
                        let slug = hide_slug.clone();
                        let model_id = hide_id.clone();
                        emit_ui_update(section, on_change, move |next| {
                            let hidden = &mut next
                                .ai_catalog
                                .overrides
                                .entry(slug)
                                .or_default()
                                .hidden_builtin_ids;
                            if checked {
                                if !hidden.iter().any(|id| id == &model_id) {
                                    hidden.push(model_id);
                                }
                            } else {
                                hidden.retain(|id| id != &model_id);
                            }
                        });
                    },
                }
                span { "Hide" }
            }
            button {
                class: "button button--ghost button--small",
                onclick: move |_| {
                    let provider = default_slug.clone();
                    let model = default_id.clone();
                    emit_ui_update(section, on_change, move |next| {
                        next.ai_catalog.active = Some(ActiveModel { provider, model });
                    });
                },
                {default_label}
            }
        }
    }
}

fn extra_model_row(
    slug: String,
    extra: AiModelEntry,
    custom: bool,
    settings: &AppUiSettings,
    section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
) -> Element {
    let display = extra.display_label().to_string();
    let model_id = extra.id.clone();
    let default_slug = slug.clone();
    let default_id = model_id.clone();
    let remove_slug = slug;
    let remove_id = model_id.clone();
    let is_default = settings
        .ai_catalog
        .active
        .as_ref()
        .is_some_and(|active| active.provider == default_slug && active.model == default_id);
    let default_label = if is_default {
        "Default"
    } else {
        "Use as default"
    };

    rsx! {
        div { class: "settings-modal__grid",
            span { class: "field__label", {display} }
            button {
                class: "button button--ghost button--small",
                onclick: move |_| {
                    let provider = default_slug.clone();
                    let model = default_id.clone();
                    emit_ui_update(section, on_change, move |next| {
                        next.ai_catalog.active = Some(ActiveModel { provider, model });
                    });
                },
                {default_label}
            }
            button {
                class: "button button--ghost button--small",
                onclick: move |_| {
                    let slug = remove_slug.clone();
                    let model_id = remove_id.clone();
                    emit_ui_update(section, on_change, move |next| {
                        if custom {
                            if let Some(provider) = next
                                .ai_catalog
                                .custom_native
                                .iter_mut()
                                .find(|provider| provider.id == slug)
                            {
                                provider.models.retain(|model| model.id != model_id);
                            }
                        } else {
                            next.ai_catalog
                                .overrides
                                .entry(slug)
                                .or_default()
                                .extra_models
                                .retain(|model| model.id != model_id);
                        }
                    });
                },
                "Remove"
            }
        }
    }
}

fn custom_native_provider_card(
    custom: CustomNativeProvider,
    settings: &AppUiSettings,
    section: Signal<SettingsSectionProps>,
    on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
    mut extra_drafts: Signal<BTreeMap<String, String>>,
) -> Element {
    let id = custom.id.clone();
    let api_key = settings.lm_api_key(&id);
    let extra_draft = extra_draft_value(extra_drafts, &id);
    let name_id = id.clone();
    let url_id = id.clone();
    let key_id = id.clone();
    let add_id = id.clone();
    let delete_id = id.clone();
    let refresh_id = id.clone();
    let default_id = id.clone();
    let default_model = custom
        .models
        .first()
        .map(|model| model.id.clone())
        .unwrap_or_default();
    let title = if custom.name.trim().is_empty() {
        custom.id.clone()
    } else {
        custom.name.clone()
    };

    rsx! {
        div { class: "settings-modal__group",
            span { class: "settings-modal__group-title", {title} }
            div { class: "settings-modal__grid",
                div { class: "field",
                    span { class: "field__label", "Name" }
                    input {
                        class: "input",
                        value: "{custom.name}",
                        oninput: move |event| {
                            let value = event.value();
                            let id = name_id.clone();
                            emit_ui_update(section, on_change, move |next| {
                                if let Some(provider) = next
                                    .ai_catalog
                                    .custom_native
                                    .iter_mut()
                                    .find(|provider| provider.id == id)
                                {
                                    provider.name = value;
                                }
                            });
                        },
                    }
                }
                div { class: "field",
                    span { class: "field__label", "Base URL" }
                    input {
                        class: "input",
                        placeholder: "http://localhost:8080",
                        value: "{custom.base_url}",
                        oninput: move |event| {
                            let value = event.value();
                            let id = url_id.clone();
                            emit_ui_update(section, on_change, move |next| {
                                if let Some(provider) = next
                                    .ai_catalog
                                    .custom_native
                                    .iter_mut()
                                    .find(|provider| provider.id == id)
                                {
                                    provider.base_url = value;
                                }
                            });
                        },
                    }
                }
                div { class: "field",
                    span { class: "field__label", "API Key" }
                    input {
                        class: "input",
                        r#type: "password",
                        placeholder: "sk-...",
                        value: api_key,
                        oninput: move |event| {
                            let value = event.value();
                            let id = key_id.clone();
                            emit_ui_update(section, on_change, move |next| {
                                next.set_lm_api_key(&id, value);
                            });
                        },
                    }
                }
            }
            for model in custom.models.clone() {
                {
                    extra_model_row(id.clone(), model, true, settings, section, on_change)
                }
            }
            div { class: "settings-modal__grid",
                div { class: "field",
                    span { class: "field__label", "Add extra model" }
                    input {
                        class: "input",
                        placeholder: "model-id",
                        value: extra_draft,
                        oninput: move |event| {
                            extra_drafts.write().insert(id.clone(), event.value());
                        },
                    }
                }
            }
            div { class: "settings-modal__section-actions",
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| {
                        let model_id = extra_drafts
                            .write()
                            .remove(&add_id)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        if model_id.is_empty() {
                            return;
                        }
                        let id = add_id.clone();
                        emit_ui_update(section, on_change, move |next| {
                            if let Some(provider) = next
                                .ai_catalog
                                .custom_native
                                .iter_mut()
                                .find(|provider| provider.id == id)
                            {
                                if provider.models.iter().any(|model| model.id == model_id) {
                                    return;
                                }
                                provider.models.push(AiModelEntry {
                                    id: model_id,
                                    label: String::new(),
                                });
                            }
                        });
                    },
                    "Add extra model"
                }
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| {
                        refresh_catalog_models(
                            section,
                            on_change,
                            refresh_id.clone(),
                            String::new(),
                        );
                    },
                    "Refresh"
                }
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| {
                        let provider = default_id.clone();
                        let model = default_model.clone();
                        emit_ui_update(section, on_change, move |next| {
                            next.ai_catalog.active = Some(ActiveModel { provider, model });
                        });
                    },
                    "Use as default"
                }
                button {
                    class: "button button--ghost button--small",
                    onclick: move |_| {
                        let id = delete_id.clone();
                        emit_ui_update(section, on_change, move |next| {
                            next.delete_custom_native_provider(&id);
                        });
                    },
                    "Delete"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_refreshed_models_skips_builtin_ids() {
        let mut settings = AppUiSettings::default();
        merge_refreshed_models_into(
            &mut settings,
            "openai",
            vec![
                AiModelEntry {
                    id: "gpt-5.6-sol".into(),
                    label: String::new(),
                },
                AiModelEntry {
                    id: "my-ft".into(),
                    label: String::new(),
                },
            ],
        );
        let extra = &settings.ai_catalog.overrides["openai"].extra_models;
        let ids: Vec<_> = extra.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, ["my-ft"]);
    }

    #[test]
    fn new_custom_native_id_uses_custom_prefix() {
        let id = new_custom_native_id();
        assert!(id.starts_with("custom:"));
        assert!(id.len() > "custom:".len());
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
