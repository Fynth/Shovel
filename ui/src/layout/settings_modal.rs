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
use models::{AppThemePreference, AppUiSettings, SqlFormatSettings, UiDensity, WorkspaceSplitMode};

/// Top-level categories shown in the left navigation sidebar.
///
/// Each variant groups one or more section helpers together. The active
/// category is tracked in a local [`Signal`] in [`SettingsModal`] — the modal
/// never reads or writes the category to globals, because it has to work in
/// the standalone native settings window (its own VirtualDom, no shared
/// globals) and inside the in-app overlay host alike.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    Appearance,
    Database,
    Editor,
    Grid,
    Navigation,
    Advanced,
    Config,
}

impl SettingsCategory {
    /// Stable id used for `key=` attributes + list iteration. Mirrors the
    /// variant name in lowercase so Dioxus diffs are stable across rebuilds.
    pub fn id(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Database => "database",
            Self::Editor => "editor",
            Self::Grid => "grid",
            Self::Navigation => "navigation",
            Self::Advanced => "advanced",
            Self::Config => "config",
        }
    }

    /// Short label rendered inside the nav button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Database => "Database",
            Self::Editor => "Editor",
            Self::Grid => "Grid",
            Self::Navigation => "Navigation",
            Self::Advanced => "Advanced",
            Self::Config => "Config",
        }
    }

    /// One-line description shown beneath the nav label. Helps users pick the
    /// right category without opening it first.
    pub fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Theme, density, and visual styling",
            Self::Database => "Connection and data-handling defaults",
            Self::Editor => "SQL editor formatting and completions",
            Self::Grid => "Result-grid and row-rendering options",
            Self::Navigation => "Explorer, sidebar, and panel layout",
            Self::Advanced => "Agent API keys, workspace defaults, and resets",
            Self::Config => "config.toml file location and reload",
        }
    }

    /// All categories in render order (stable, sidebar visual order).
    pub const ALL: &'static [SettingsCategory] = &[
        Self::Appearance,
        Self::Database,
        Self::Editor,
        Self::Grid,
        Self::Navigation,
        Self::Advanced,
        Self::Config,
    ];
}

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

/// Section-level props shared by every section helper.
///
/// Each helper accepts the current settings + sql pair plus the shared
/// `on_change` callback. Sections own these values (not a `Signal<...>`)
/// because Dioxus 0.7's `#[component]` macro does not auto-derive
/// `Properties` for `Signal<T>` where `T` is itself a `Props` struct — the
/// type-family it understands (`ReadSignal`, `ReadOnlySignal`, `WriteSignal`)
/// does not include the wrapping `Signal<T>` form. Cloning the pair is cheap
/// relative to the rest of the diff work and keeps every helper `#[component]`.
#[derive(Props, Clone, PartialEq)]
pub struct SettingsSectionProps {
    pub settings: AppUiSettings,
    pub sql_settings: SqlFormatSettings,
    pub on_change: Callback<(AppUiSettings, SqlFormatSettings)>,
}

/// Props for the empty-state placeholder used by categories that have no
/// sections yet.
#[derive(Props, Clone, PartialEq)]
struct CategoryEmptyStateProps {
    category: SettingsCategory,
    hint: String,
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
    let sql_settings = props.read().sql_settings.clone();

    // Local-only category switcher — does NOT escape the component. The
    // native settings window is a separate VirtualDom, so writing this to a
    // global would silently not propagate back. Keeping it as a local signal
    // is what allows the same SettingsModal to mount in both the in-app
    // overlay and the standalone window.
    let mut active_category = use_signal(|| SettingsCategory::Appearance);

    // Common section props — cloned for every section helper so each one has
    // its own owned `on_change` callback (it is `Clone`) and its own read-only
    // snapshot of the current settings.
    let section_props = SettingsSectionProps {
        settings: settings.clone(),
        sql_settings: sql_settings.clone(),
        on_change,
    };

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| on_close.call(()),
            div {
                class: "settings-modal",
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "settings-modal__body",
                    nav {
                        class: "settings-modal__nav",
                        aria_label: "Settings categories",
                        for category in SettingsCategory::ALL.iter().copied() {
                            button {
                                key: "{category.id()}",
                                class: if *active_category.read() == category {
                                    "settings-modal__nav-item settings-modal__nav-item--active"
                                } else {
                                    "settings-modal__nav-item"
                                },
                                aria_pressed: *active_category.read() == category,
                                onclick: move |_| {
                                    active_category.set(category);
                                },
                                span {
                                    class: "settings-modal__nav-label",
                                    "{category.label()}"
                                }
                                span {
                                    class: "settings-modal__nav-description",
                                    "{category.description()}"
                                }
                            }
                        }
                    }
                    div {
                        class: "settings-modal__content",
                        role: "region",
                        aria_label: "{active_category.read().label()} settings",
                        match *active_category.read() {
                            SettingsCategory::Appearance => rsx! {
                                AppearanceSection { ..section_props }
                            },
                            SettingsCategory::Database => rsx! {
                                CategoryEmptyState {
                                    category: *active_category.read(),
                                    hint: "Per-database connection preferences will live here. Today, connection defaults are managed per-connection in the connect screen.",
                                }
                            },
                            SettingsCategory::Editor => rsx! {
                                SqlFormattingSection { ..section_props.clone() }
                                CodeStralCompletionSection { ..section_props }
                            },
                            SettingsCategory::Grid => rsx! {
                                CategoryEmptyState {
                                    category: *active_category.read(),
                                    hint: "Result-grid options (row height, virtualization buffer, column defaults) will live here.",
                                }
                            },
                            SettingsCategory::Navigation => rsx! {
                                CategoryEmptyState {
                                    category: *active_category.read(),
                                    hint: "Explorer, sidebar, and keybinding defaults will live here. The explorer section underneath has the controls today.",
                                }
                            },
                            SettingsCategory::Advanced => rsx! {
                                DeepSeekAgentSection { ..section_props.clone() }
                                WorkspaceSection { ..section_props }
                            },
                            SettingsCategory::Config => rsx! {
                                ConfigSection {}
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Rendered for categories that have no sections yet. Keeps the category
/// discoverable in the nav while explicitly inviting future work.
#[component]
fn CategoryEmptyState(props: CategoryEmptyStateProps) -> Element {
    let category = props.category;
    let hint = props.hint;
    rsx! {
        section {
            class: "settings-modal__section settings-modal__section--empty",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "{category.label()}" }
            }
            p { class: "settings-modal__section-hint", {hint.to_string()} }
            p { class: "settings-modal__section-hint", "More options will land here as the editor grows." }
        }
    }
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
fn AppearanceSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());

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
        }
    }
}

#[component]
fn DeepSeekAgentSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());

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
fn WorkspaceSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());

    rsx! {
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
                                next.deepseek.api_key = section_props_signal.read().settings.deepseek.api_key.clone();
                                next.codestral.api_key = section_props_signal.read().settings.codestral.api_key.clone();
                                on_change.call((next, section_props_signal.read().sql_settings.clone()));
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
}

#[component]
fn SqlFormattingSection(props: SettingsSectionProps) -> Element {
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());

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
fn CodeStralCompletionSection(props: SettingsSectionProps) -> Element {
    let settings = props.settings.clone();
    let on_change = props.on_change;
    let section_props_signal = use_signal(|| props.clone());

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

/// Shows where `config.toml` lives and lets the user open it in the OS file
/// manager or reload it. The config file is the declarative customization
/// surface — every setting in this modal can also be set there.
#[component]
fn ConfigSection() -> Element {
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
                            let _ = reload_config();
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

/// Reload the config file and re-apply it to the current UI settings.
fn reload_config() -> Result<(), String> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join("shovel");
    let path = data_dir.join("config.toml");
    let Some(config) = models::ShovelConfig::load(&path)? else {
        return Ok(());
    };
    let mut settings = crate::app_state::APP_UI_SETTINGS();
    config.apply_to(&mut settings);
    crate::app_state::replace_ui_settings(settings);
    Ok(())
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
            ShowFlag::BottomPanel => ui.show_bottom_panel = value,
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
        BottomPanel,
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
        assert_eq!(after.show_bottom_panel, before.show_bottom_panel);

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
        current = toggle_show_flag(current, ShowFlag::BottomPanel, false);
        assert!(!current.show_bottom_panel);
    }

    #[test]
    fn parse_u32_in_range_clamps_and_falls_back() {
        assert_eq!(parse_u32_in_range("", 50, 10, 1000), 50);
        assert_eq!(parse_u32_in_range("not-a-number", 50, 10, 1000), 50);
        assert_eq!(parse_u32_in_range("5", 50, 10, 1000), 10);
        assert_eq!(parse_u32_in_range("9999", 50, 10, 1000), 1000);
        assert_eq!(parse_u32_in_range("250", 50, 10, 1000), 250);
    }

    /// The category labels must match the variant order so the nav renders
    /// visually stable (appearance → ... → advanced). Adding a variant in
    /// `SettingsCategory::ALL` without updating one of these helpers will
    /// fail this test and force the author to fix it.
    #[test]
    fn category_order_matches_all_constant() {
        let labels: Vec<&str> = SettingsCategory::ALL
            .iter()
            .copied()
            .map(SettingsCategory::label)
            .collect();
        assert_eq!(
            labels,
            vec![
                "Appearance",
                "Database",
                "Editor",
                "Grid",
                "Navigation",
                "Advanced",
                "Config"
            ]
        );
    }

    /// Each category exposes a non-empty label + description so the nav
    /// never renders an empty button.
    #[test]
    fn every_category_has_label_and_description() {
        for category in SettingsCategory::ALL.iter().copied() {
            assert!(!category.label().is_empty());
            assert!(!category.description().is_empty());
            assert!(!category.id().is_empty());
        }
    }
}
