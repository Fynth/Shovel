mod remote_draft;
mod remote_fields;
mod sqlite_fields;

use dioxus::prelude::*;
use models::{
    ClickHouseFormData,
    ConnectionRequest,
    DatabaseKind,
    MySqlFormData,
    PostgresFormData,
    SavedConnection,
    SqliteFormData,
};
use remote_draft::RemoteConnectionDraft;

use super::{forms::connection_status_class, kind_selector::KindSelector};
use remote_fields::RemoteEditorFields;
use sqlite_fields::SqliteEditorFields;

#[component]
pub fn EditConnectionModal(
    saved_connection: SavedConnection,
    on_saved: Callback<SavedConnection>,
    on_close: Callback<()>,
) -> Element {
    let selected_kind = use_signal(|| saved_connection.request.kind());
    let sqlite_path = use_signal(|| match &saved_connection.request {
        ConnectionRequest::Sqlite(data) => data.path.clone(),
        _ => String::new(),
    });
    let postgres_draft =
        use_signal(|| RemoteConnectionDraft::from_postgres_request(&saved_connection.request));
    let mysql_draft =
        use_signal(|| RemoteConnectionDraft::from_mysql_request(&saved_connection.request));
    let clickhouse_draft =
        use_signal(|| RemoteConnectionDraft::from_clickhouse_request(&saved_connection.request));
    let mut save_status = use_signal(String::new);
    let mut save_inflight = use_signal(|| false);
    let save_status_value = save_status();
    let save_status_class = connection_status_class(&save_status_value);

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| {
                if !save_inflight() {
                    on_close(());
                }
            },
            div {
                class: "settings-modal connect-screen__editor-modal",
                onclick: move |event| event.stop_propagation(),
                form {
                    class: "settings-modal__body connect-form",
                    onsubmit: move |event| {
                        event.prevent_default();

                        let next_request = match selected_kind() {
                            DatabaseKind::Sqlite => {
                                let path = sqlite_path().trim().to_string();
                                if path.is_empty() {
                                    save_status.set("Error: SQLite file path is required.".to_string());
                                    return;
                                }
                                ConnectionRequest::Sqlite(SqliteFormData { path })
                            }
                            DatabaseKind::Postgres => {
                                let draft = postgres_draft();
                                let ssh_tunnel = draft.ssh_tunnel();
                                ConnectionRequest::Postgres(PostgresFormData {
                                    host: draft.host,
                                    port: draft.port.parse().unwrap_or(5432),
                                    username: draft.username,
                                    password: draft.password,
                                    database: draft.database,
                                    ssl_mode: draft.ssl_mode,
                                    ssh_tunnel,
                                })
                            }
                            DatabaseKind::MySql => {
                                let draft = mysql_draft();
                                let ssh_tunnel = draft.ssh_tunnel();
                                ConnectionRequest::MySql(MySqlFormData {
                                    host: draft.host,
                                    port: draft.port.parse().unwrap_or(3306),
                                    username: draft.username,
                                    password: draft.password,
                                    database: draft.database,
                                    ssl_mode: draft.ssl_mode,
                                    ssh_tunnel,
                                })
                            }
                            DatabaseKind::ClickHouse => {
                                let draft = clickhouse_draft();
                                let ssh_tunnel = draft.ssh_tunnel();
                                ConnectionRequest::ClickHouse(ClickHouseFormData {
                                    host: draft.host,
                                    port: draft.port.parse().unwrap_or(8123),
                                    username: draft.username,
                                    password: draft.password,
                                    database: draft.database,
                                    ssh_tunnel,
                                })
                            }
                        };

                        let previous_identity_key = saved_connection.request.identity_key();
                        let updated_name = saved_connection.name.clone();
                        let next_request_for_callback = next_request.clone();
                        save_status.set("Saving...".to_string());
                        save_inflight.set(true);

                        spawn(async move {
                            match services::replace_connection_request(previous_identity_key, next_request)
                                .await
                            {
                                Ok(()) => {
                                    save_inflight.set(false);
                                    on_saved(SavedConnection {
                                        name: updated_name,
                                        request: next_request_for_callback,
                                    });
                                }
                                Err(err) => {
                                    save_inflight.set(false);
                                    save_status.set(format!("Error: {err}"));
                                }
                            }
                        });
                    },

                    div {
                        class: "settings-modal__section",
                        p {
                            class: "connect-screen__status connect-screen__status--hint",
                            "{saved_connection.name}"
                        }
                        KindSelector {
                            selected_kind,
                        }

                        match selected_kind() {
                            DatabaseKind::Sqlite => rsx! {
                                SqliteEditorFields {
                                    path: sqlite_path,
                                    disabled: save_inflight(),
                                }
                            },
                            DatabaseKind::Postgres => rsx! {
                                RemoteEditorFields {
                                    draft: postgres_draft,
                                    kind: DatabaseKind::Postgres,
                                    disabled: save_inflight(),
                                }
                            },
                            DatabaseKind::MySql => rsx! {
                                RemoteEditorFields {
                                    draft: mysql_draft,
                                    kind: DatabaseKind::MySql,
                                    disabled: save_inflight(),
                                }
                            },
                            DatabaseKind::ClickHouse => rsx! {
                                RemoteEditorFields {
                                    draft: clickhouse_draft,
                                    kind: DatabaseKind::ClickHouse,
                                    disabled: save_inflight(),
                                }
                            },
                        }
                    }

                    div {
                        class: "connect-form__actions connect-screen__editor-actions",
                        div {
                            class: "connect-screen__editor-buttons",
                            button {
                                class: "button button--ghost",
                                r#type: "button",
                                disabled: save_inflight(),
                                onclick: move |_| on_close(()),
                                "Cancel"
                            }
                            button {
                                class: "button button--primary connect-form__submit",
                                r#type: "submit",
                                disabled: save_inflight(),
                                if save_inflight() {
                                    "Saving..."
                                } else {
                                    "Save changes"
                                }
                            }
                        }
                        if !save_status_value.is_empty() {
                            p { class: save_status_class.to_string(), {save_status_value.to_string()} }
                        }
                    }
                }
            }
        }
    }
}
