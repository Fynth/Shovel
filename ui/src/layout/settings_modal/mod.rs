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

mod widgets;
mod sections;
mod keyboard;

use dioxus::prelude::*;
use models::{AppUiSettings, SqlFormatSettings};

use keyboard::KeyboardSection;
use sections::{
    AppearanceSection,
    CategoryEmptyState,
    CodeStralCompletionSection,
    ConfigSection,
    DeepSeekAgentSection,
    SqlFormattingSection,
    WorkspaceSection,
};

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
    Keyboard,
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
            Self::Keyboard => "keyboard",
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
            Self::Keyboard => "Keyboard",
            Self::Advanced => "Advanced",
            Self::Config => "Config",
        }
    }

    /// One-line description for the category. Unused in the label-only nav;
    /// kept so tests can still assert every pane is documented.
    #[allow(dead_code)]
    pub fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Theme, density, and visual styling",
            Self::Database => "Connection and data-handling defaults",
            Self::Editor => "SQL editor formatting and completions",
            Self::Grid => "Result-grid and row-rendering options",
            Self::Navigation => "Explorer, sidebar, and panel layout",
            Self::Keyboard => "Shortcuts and keybindings",
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
        Self::Keyboard,
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
                            SettingsCategory::Keyboard => rsx! {
                                KeyboardSection { ..section_props }
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
                "Keyboard",
                "Advanced",
                "Config",
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
