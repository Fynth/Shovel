use crate::{
    app_state::{APP_THEME, add_connection_session},
    windows,
};
use dioxus::prelude::*;
use models::SavedConnection;

use super::forms::connection_status_class;

pub fn recent_connections_loading_text() -> &'static str {
    "Loading connections…"
}

pub fn recent_connections_empty_text() -> &'static str {
    "No saved connections yet."
}

pub fn format_connection_failed_error(err: impl std::fmt::Display) -> String {
    format!("Connection failed: {err}")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_verbose_loading_text(text: &str) -> bool {
    text == "Loading saved connections..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_text_is_short() {
        let text = recent_connections_loading_text();
        assert!(!is_verbose_loading_text(text));
        assert!(text.len() < "Loading saved connections...".len());
    }

    #[test]
    fn empty_state_text_is_preserved() {
        assert_eq!(recent_connections_empty_text(), "No saved connections yet.");
    }

    #[test]
    fn connection_error_uses_display_not_debug() {
        let formatted = format_connection_failed_error("timeout");
        assert_eq!(formatted, "Connection failed: timeout");
        assert!(!formatted.contains(":?"));
    }

    #[test]
    fn detects_verbose_loading_text() {
        assert!(is_verbose_loading_text("Loading saved connections..."));
        assert!(!is_verbose_loading_text("Loading connections…"));
        assert!(!is_verbose_loading_text("Loading..."));
    }
}

#[component]
pub fn RecentConnections(
    saved_connections: Option<Vec<SavedConnection>>,
    mut saved_connections_revision: Signal<u64>,
) -> Element {
    let mut status = use_signal(String::new);
    let status_value = status();
    let status_class = connection_status_class(&status_value);

    rsx! {
        section {
            class: "connect-screen__recent",
            h2 { class: "connect-screen__section-title", "Recent Connections" }
            match saved_connections {
                Some(connections) if connections.is_empty() => rsx! {
                    p { class: "empty-state", "{recent_connections_empty_text()}" }
                },
                Some(connections) => rsx! {
                    div {
                        class: "connect-screen__recent-list",
                        for saved_connection in connections {
                            div {
                                class: "recent-connection",
                                div {
                                    class: "recent-connection__meta",
                                    p { class: "recent-connection__name", "{saved_connection.name}" }
                                }
                                div {
                                    class: "recent-connection__actions",
                                    button {
                                        class: "button button--ghost button--small",
                                            onclick: {
                                                let connection_to_edit = saved_connection.clone();
                                                move |_| {
                                                    let conn = connection_to_edit.clone();
                                                    let (bridge, mut rx) = windows::create_connection_edit_bridge();
                                                    spawn(async move {
                                                        while rx.recv().await.is_some() {
                                                            saved_connections_revision += 1;
                                                            status.set("Connection updated".to_string());
                                                        }
                                                    });
                                                    windows::open_connection_edit_window(
                                                        bridge,
                                                        conn,
                                                        APP_THEME(),
                                                    );
                                                }
                                            },
                                        "Edit"
                                    }
                                    button {
                                        class: "button button--ghost",
                                        onclick: {
                                            let request = saved_connection.request.clone();
                                            move |_| {
                                                let request_to_connect = request.clone();
                                                let request_to_register = request.clone();
                                                spawn(async move {
                                                    match services::connect_and_save_request(request_to_connect).await {
                                                        Ok(result) => {
                                                            add_connection_session(request_to_register, result.connection);
                                                            saved_connections_revision += 1;
                                                            match result.save_warning {
                                                                Some(err) => status.set(format!(
                                                                    "Connected, but failed to update saved connections: {err}"
                                                                )),
                                                                None => status.set("Connected".to_string()),
                                                            }
                                                        }
                                                        Err(err) => {
                                                            status.set(format_connection_failed_error(err));
                                                        }
                                                    }
                                                });
                                            }
                                        },
                                        "Connect"
                                    }
                                }
                            }
                        }
                    }
                },
                None => rsx! {
                    p { class: "empty-state", "{recent_connections_loading_text()}" }
                },
            }
            if !status().is_empty() {
                p { class: status_class.to_string(), {status_value.to_string()} }
            }
        }
    }
}
