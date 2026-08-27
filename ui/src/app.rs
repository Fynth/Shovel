use crate::{
    app_state::{
        APP_APP_BEHAVIOR,
        APP_EDITOR_BEHAVIOR,
        APP_KEYBINDINGS,
        APP_PANEL_BEHAVIOR,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        APP_THEME,
        APP_THEME_OVERRIDES,
        APP_TOOLTIP,
        APP_UI_DENSITY,
        APP_UI_SETTINGS,
        replace_ui_settings,
        restore_connection_sessions,
        toast_error,
    },
    components::{
        command_palette::CommandPalette,
        context_menu::{ContextMenu, TextInputMenuInit},
        global_search::GlobalSearch,
    },
    layout::{StatusBar, ToastContainer, Toolbar},
    screens::{DbConnect, Workspace},
};
use dioxus::prelude::*;
use models::{AppUiSettings, SqlFormatSettings};

#[component]
pub fn App() -> Element {
    let mut restored_once = use_signal(|| false);
    let mut startup_loaded = use_signal(|| false);
    let mut startup_error_reported = use_signal(|| false);
    let mut last_saved_ui_settings = use_signal(|| None::<AppUiSettings>);
    let mut last_saved_sql_settings = use_signal(|| None::<SqlFormatSettings>);
    let startup_settings =
        use_resource(move || async move { services::load_app_startup_settings().await });

    use_effect(move || {
        let Some(result) = startup_settings() else {
            return;
        };
        if startup_loaded() {
            return;
        }

        let startup = match result {
            Ok(startup) => startup,
            Err(err) => {
                if !startup_error_reported() {
                    toast_error(format!("Failed to load app settings: {err}"));
                    startup_error_reported.set(true);
                }
                last_saved_ui_settings.set(Some(APP_UI_SETTINGS()));
                last_saved_sql_settings.set(Some(APP_SQL_FORMAT_SETTINGS()));
                startup_loaded.set(true);
                restored_once.set(true);
                return;
            }
        };

        replace_ui_settings(startup.ui_settings.clone());
        *APP_SQL_FORMAT_SETTINGS.write() = startup.sql_format_settings.clone();
        // Apply deep-customization overrides from config.toml.
        *APP_THEME_OVERRIDES.write() = startup.theme_overrides.clone().unwrap_or_default();
        *APP_KEYBINDINGS.write() = startup.keybindings.clone().unwrap_or_default();
        *APP_EDITOR_BEHAVIOR.write() = startup.editor.clone().unwrap_or_default();
        *APP_PANEL_BEHAVIOR.write() = startup.panels.clone().unwrap_or_default();
        *APP_APP_BEHAVIOR.write() = startup.behavior.clone().unwrap_or_default();
        last_saved_ui_settings.set(Some(startup.ui_settings.clone()));
        last_saved_sql_settings.set(Some(startup.sql_format_settings.clone()));
        startup_loaded.set(true);

        if restored_once() || !startup.ui_settings.restore_session_on_launch {
            restored_once.set(true);
            return;
        }

        restored_once.set(true);
        let config_connections = startup.config_connections.clone();
        spawn(async move {
            let Ok(result) = services::restore_saved_sessions().await else {
                toast_error("Failed to restore saved sessions.");
                return;
            };
            if !result.failed_requests.is_empty() {
                let failed_labels = result
                    .failed_requests
                    .iter()
                    .take(3)
                    .map(|(request, _)| request.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                let summary = if result.failed_requests.len() > 3 {
                    format!(
                        "Failed to restore {} saved sessions: {} and more.",
                        result.failed_requests.len(),
                        failed_labels
                    )
                } else {
                    format!(
                        "Failed to restore {} saved sessions: {}.",
                        result.failed_requests.len(),
                        failed_labels
                    )
                };
                toast_error(summary);
            }
            if result.restored.is_empty() {
                return;
            }

            restore_connection_sessions(
                result.restored,
                result.active_connection_name,
                result.tab_drafts,
            );

            // Auto-connect any connections declared in `config.toml`.
            for connection in config_connections {
                let Some(request) = connection.to_request() else {
                    continue;
                };
                match services::connect_and_save_request(request.clone()).await {
                    Ok(saved) => {
                        crate::app_state::add_connection_session(request, saved.handle);
                    }
                    Err(err) => {
                        toast_error(format!("Failed to connect {}: {err}", connection.name));
                    }
                }
            }
        });
    });

    use_effect(move || {
        if !startup_loaded() {
            return;
        }

        let settings = APP_UI_SETTINGS();
        *APP_THEME.write() = settings.theme.css_class().to_string();

        if last_saved_ui_settings().as_ref() == Some(&settings) {
            return;
        }

        last_saved_ui_settings.set(Some(settings.clone()));
        spawn(async move {
            if let Err(err) = services::save_app_ui_settings_with_secrets(settings).await {
                toast_error(format!("Failed to save app settings: {err}"));
            }
        });
    });

    use_effect(move || {
        if !startup_loaded() {
            return;
        }

        let settings = APP_SQL_FORMAT_SETTINGS();
        if last_saved_sql_settings().as_ref() == Some(&settings) {
            return;
        }

        last_saved_sql_settings.set(Some(settings.clone()));
        spawn(async move {
            if let Err(err) = services::save_sql_format_settings(settings).await {
                toast_error(format!("Failed to save SQL format settings: {err}"));
            }
        });
    });

    let theme_name = APP_THEME();
    let density_class = APP_UI_DENSITY().css_class();
    let theme_css = APP_THEME_OVERRIDES().to_css();
    let (has_sessions, should_show_connect) = {
        let app_state = APP_STATE.read();
        (
            app_state.has_sessions(),
            app_state.show_connection_screen || !app_state.has_sessions(),
        )
    };

    rsx! {
            if !theme_css.is_empty() {
                style { {theme_css.to_string()} }
            }
            div {
                class: "app {theme_name} {density_class}",
                Toolbar {}
                main {
                    class: if has_sessions {
                        "app__body"
                    } else {
                        "app__body app__body--welcome"
                    },
                    if has_sessions {
                        ErrorBoundary {
                            handle_error: |_| {
                                rsx! {
                                    div {
                                        class: "workspace-error",
                                        p { "Something went wrong. Please restart the application." }
                                    }
                                }
                            },
                            Workspace {}
                        }
                        if should_show_connect {
                            div {
                                class: "app__overlay",
                                DbConnect {}
                            }
                        }
                    } else {
                        DbConnect {}
                    }
                    if let Some(tooltip) = APP_TOOLTIP() {
                        div {
                            class: "app__tooltip-layer",
                            div {
                                class: "app__tooltip",
                                left: "{tooltip.x:.0}px",
                                top: "{tooltip.y:.0}px",
                                "{tooltip.label}"
                            }
                        }
                    }
                ToastContainer {}
            }
            StatusBar {}
            TextInputMenuInit {}
            ContextMenu {}
            CommandPalette {}
            GlobalSearch {}
        }
    }
}
