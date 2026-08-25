use crate::app_state::add_connection_session;
use dioxus::prelude::*;
use models::{ConnectionRequest, SqliteFormData};
use rfd::AsyncFileDialog;

use super::{connection_status_class, format_connection_error};

#[component]
pub fn SqliteForm(mut saved_connections_revision: Signal<u64>) -> Element {
    let mut path = use_signal(|| "".to_string());
    let mut status = use_signal(String::new);
    let status_value = status();
    let status_class = connection_status_class(&status_value);

    // Сборка ConnectionRequest из текущего пути. Замыкание захватывает
    // только Copy-сигнал, поэтому само является Copy и переиспользуется
    // и обработчиком Connect, и кнопкой Test.
    let build_request = move || {
        ConnectionRequest::Sqlite(SqliteFormData {
            path: path().trim().to_string(),
        })
    };

    rsx! {
        form {
            class: "connect-form",
            onsubmit: move |event| {
                event.prevent_default();

                if path().trim().is_empty() {
                    status.set("SQLite file path is required".to_string());
                    return;
                }

                status.set("Connecting...".to_string());
                let request = build_request();

                spawn(async move {
                    match services::connect_and_save_request(request.clone()).await {
                        Ok(result) => {
                            add_connection_session(request, result.connection);
                            saved_connections_revision += 1;
                            match result.save_warning {
                                Some(err) => status.set(format!(
                                    "Connected, but failed to save connection: {err}"
                                )),
                                None => status.set("Connected".to_string()),
                            }
                        }
                        Err(err) => status.set(format_connection_error(err)),
                    }
                });
            },
            div {
                class: "field",
                label {
                    class: "field__label",
                    r#for: "sqlite-path",
                    "SQLite file path"
                }
                div {
                    class: "connect-form__path-row",
                    input {
                        class: "input connect-form__path-input",
                        id: "sqlite-path",
                        value: "{path}",
                        placeholder: "/path/to/app.db",
                        oninput: move |event| {
                            path.set(event.value());
                        }
                    }
                    button {
                        class: "button button--ghost",
                        r#type: "button",
                        onclick: move |_| {
                            spawn(async move {
                                let file = AsyncFileDialog::new()
                                    .add_filter("SQLite database", &["db", "sqlite", "sqlite3", "db3"])
                                    .add_filter("All files", &["*"])
                                    .pick_file()
                                    .await;

                                if let Some(file) = file {
                                    path.set(file.path().display().to_string());
                                }
                            });
                        },
                        "Browse"
                    }
                }
            }

            div {
                class: "connect-form__actions",
                button {
                    class: "button button--primary connect-form__submit",
                    r#type: "submit",
                    "Connect"
                }
                button {
                    class: "button button--ghost connect-form__test",
                    r#type: "button",
                    onclick: move |_| {
                        if path().trim().is_empty() {
                            status.set("SQLite file path is required".to_string());
                            return;
                        }

                        status.set("Testing...".to_string());
                        let request = build_request();
                        spawn(async move {
                            match services::test_connection(request).await {
                                Ok(()) => status.set("Connected (test only)".to_string()),
                                Err(err) => status.set(format_connection_error(err)),
                            }
                        });
                    },
                    "Test"
                }
                if !status_value.is_empty() {
                    p { class: status_class.to_string(), {status_value.to_string()} }
                }
            }
        }
    }
}
