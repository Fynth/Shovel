use dioxus::prelude::*;
use models::{
    ClickHouseFormData, ConnectionRequest, DatabaseKind, MySqlFormData, PostgresFormData,
    SavedConnection, SqliteFormData, SshTunnelConfig,
};
use rfd::AsyncFileDialog;

use super::{forms::connection_status_class, kind_selector::KindSelector};

#[derive(Clone, PartialEq)]
struct RemoteConnectionDraft {
    host: String,
    port: String,
    username: String,
    password: String,
    database: String,
    ssl_mode: String,
    ssh_enabled: bool,
    ssh_host: String,
    ssh_port: String,
    ssh_username: String,
    ssh_private_key_path: String,
}

impl RemoteConnectionDraft {
    fn postgres_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            username: "postgres".to_string(),
            ssl_mode: "prefer".to_string(),
            password: String::new(),
            database: "postgres".to_string(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    fn clickhouse_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "8123".to_string(),
            username: "default".to_string(),
            ssl_mode: String::new(),
            password: String::new(),
            database: "default".to_string(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    fn mysql_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "3306".to_string(),
            username: "root".to_string(),
            ssl_mode: "preferred".to_string(),
            password: String::new(),
            database: String::new(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    fn from_postgres_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::Postgres(data) => Self::from_postgres(data),
            _ => Self::postgres_default(),
        }
    }

    fn from_clickhouse_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::ClickHouse(data) => Self::from_clickhouse(data),
            _ => Self::clickhouse_default(),
        }
    }

    fn from_mysql_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::MySql(data) => Self::from_mysql(data),
            _ => Self::mysql_default(),
        }
    }

    fn from_postgres(data: &PostgresFormData) -> Self {
        Self {
            host: data.host.clone(),
            ssl_mode: data.ssl_mode.clone(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled: data.ssh_tunnel.is_some(),
            ssh_host: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.host.clone())
                .unwrap_or_default(),
            ssh_port: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.port.to_string())
                .unwrap_or_else(|| "22".to_string()),
            ssh_username: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.username.clone())
                .unwrap_or_default(),
            ssh_private_key_path: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.private_key_path.clone())
                .unwrap_or_default(),
        }
    }

    fn from_clickhouse(data: &ClickHouseFormData) -> Self {
        Self {
            host: data.host.clone(),
            ssl_mode: String::new(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled: data.ssh_tunnel.is_some(),
            ssh_host: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.host.clone())
                .unwrap_or_default(),
            ssh_port: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.port.to_string())
                .unwrap_or_else(|| "22".to_string()),
            ssh_username: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.username.clone())
                .unwrap_or_default(),
            ssh_private_key_path: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.private_key_path.clone())
                .unwrap_or_default(),
        }
    }

    fn from_mysql(data: &MySqlFormData) -> Self {
        Self {
            host: data.host.clone(),
            ssl_mode: data.ssl_mode.clone(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled: data.ssh_tunnel.is_some(),
            ssh_host: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.host.clone())
                .unwrap_or_default(),
            ssh_port: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.port.to_string())
                .unwrap_or_else(|| "22".to_string()),
            ssh_username: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.username.clone())
                .unwrap_or_default(),
            ssh_private_key_path: data
                .ssh_tunnel
                .as_ref()
                .map(|ssh| ssh.private_key_path.clone())
                .unwrap_or_default(),
        }
    }

    fn ssh_tunnel(&self) -> Option<SshTunnelConfig> {
        if !self.ssh_enabled {
            return None;
        }

        Some(SshTunnelConfig {
            host: self.ssh_host.clone(),
            port: self.ssh_port.parse().unwrap_or(22),
            username: self.ssh_username.clone(),
            private_key_path: self.ssh_private_key_path.clone(),
        })
    }
}

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
                            p { class: "{save_status_class}", "{save_status_value}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SqliteEditorFields(mut path: Signal<String>, disabled: bool) -> Element {
    rsx! {
        div {
            class: "field",
            label {
                class: "field__label",
                r#for: "edit-sqlite-path",
                "SQLite file path"
            }
            div {
                class: "connect-form__path-row",
                input {
                    class: "input connect-form__path-input",
                    id: "edit-sqlite-path",
                    value: "{path}",
                    placeholder: "/path/to/app.db",
                    disabled,
                    oninput: move |event| path.set(event.value()),
                }
                button {
                    class: "button button--ghost",
                    r#type: "button",
                    disabled,
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
    }
}

#[component]
fn RemoteEditorFields(
    mut draft: Signal<RemoteConnectionDraft>,
    kind: DatabaseKind,
    disabled: bool,
) -> Element {
    let (host_label, host_placeholder, username_placeholder, database_placeholder, port_default) =
        match kind {
            DatabaseKind::Postgres => (
                "Host",
                "localhost or postgres://user:pass@host:5432/db",
                "postgres",
                "postgres",
                "5432",
            ),
            DatabaseKind::MySql => (
                "Host",
                "localhost or mysql://user:pass@host:3306/db",
                "root",
                "Optional default database",
                "3306",
            ),
            DatabaseKind::ClickHouse => (
                "Host",
                "localhost or https://host:8443",
                "default",
                "default",
                "8123",
            ),
            DatabaseKind::Sqlite => ("Host", "", "", "", ""),
        };
    let current = draft();

    rsx! {
        div {
            class: "connect-form__grid",
            div {
                class: "field",
                label { class: "field__label", r#for: "edit-host", "{host_label}" }
                input {
                    class: "input",
                    id: "edit-host",
                    value: current.host.clone(),
                    placeholder: "{host_placeholder}",
                    disabled,
                    oninput: move |event| {
                        let value = event.value();
                        draft.with_mut(|draft| draft.host = value);
                    },
                }
            }

            div {
                class: "field",
                label { class: "field__label", r#for: "edit-port", "Port" }
                input {
                    class: "input",
                    id: "edit-port",
                    value: current.port.clone(),
                    placeholder: "{port_default}",
                    disabled,
                    oninput: move |event| {
                        let value = event.value();
                        draft.with_mut(|draft| draft.port = value);
                    },
                }
            }
        }

        div {
            class: "field",
            label { class: "field__label", r#for: "edit-username", "Username" }
            input {
                class: "input",
                id: "edit-username",
                value: current.username.clone(),
                placeholder: "{username_placeholder}",
                disabled,
                oninput: move |event| {
                    let value = event.value();
                    draft.with_mut(|draft| draft.username = value);
                },
            }
        }

        div {
            class: "field",
            label { class: "field__label", r#for: "edit-password", "Password" }
            input {
                class: "input",
                id: "edit-password",
                r#type: "password",
                value: current.password.clone(),
                placeholder: "••••••••",
                disabled,
                oninput: move |event| {
                    let value = event.value();
                    draft.with_mut(|draft| draft.password = value);
                },
            }
        }

        div {
            class: "field",
            label { class: "field__label", r#for: "edit-database", "Database" }
            input {
                class: "input",
                id: "edit-database",
                value: current.database.clone(),
                placeholder: "{database_placeholder}",
                disabled,
                oninput: move |event| {
                    let value = event.value();
                    draft.with_mut(|draft| draft.database = value);
                },
            }
        }

        RemoteSshTunnelFields {
            draft,
            disabled,
        }
    }
}

#[component]
fn RemoteSshTunnelFields(mut draft: Signal<RemoteConnectionDraft>, disabled: bool) -> Element {
    let current = draft();

    rsx! {
        div {
            class: "connect-form__ssh",
            div {
                class: "connect-form__ssh-header",
                div {
                    p { class: "connect-screen__section-title", "SSH Tunnel" }
                    p {
                        class: "connect-screen__status connect-screen__status--hint",
                        "Forward the database port through the local OpenSSH client using agent or private key authentication."
                    }
                }
                button {
                    class: if current.ssh_enabled {
                        "button button--ghost button--small button--active"
                    } else {
                        "button button--ghost button--small"
                    },
                    r#type: "button",
                    disabled,
                    onclick: move |_| {
                        draft.with_mut(|draft| draft.ssh_enabled = !draft.ssh_enabled);
                    },
                    if current.ssh_enabled {
                        "Disable SSH"
                    } else {
                        "Enable SSH"
                    }
                }
            }

            if current.ssh_enabled {
                div {
                    class: "connect-form__grid connect-form__ssh-grid",
                    div {
                        class: "field",
                        label { class: "field__label", r#for: "edit-ssh-host", "SSH Host" }
                        input {
                            class: "input",
                            id: "edit-ssh-host",
                            value: current.ssh_host.clone(),
                            placeholder: "bastion.example.com",
                            disabled,
                            oninput: move |event| {
                                let value = event.value();
                                draft.with_mut(|draft| draft.ssh_host = value);
                            },
                        }
                    }
                    div {
                        class: "field",
                        label { class: "field__label", r#for: "edit-ssh-port", "SSH Port" }
                        input {
                            class: "input",
                            id: "edit-ssh-port",
                            value: current.ssh_port.clone(),
                            placeholder: "22",
                            disabled,
                            oninput: move |event| {
                                let value = event.value();
                                draft.with_mut(|draft| draft.ssh_port = value);
                            },
                        }
                    }
                }

                div {
                    class: "connect-form__grid connect-form__ssh-grid",
                    div {
                        class: "field",
                        label { class: "field__label", r#for: "edit-ssh-username", "SSH Username" }
                        input {
                            class: "input",
                            id: "edit-ssh-username",
                            value: current.ssh_username.clone(),
                            placeholder: "ubuntu",
                            disabled,
                            oninput: move |event| {
                                let value = event.value();
                                draft.with_mut(|draft| draft.ssh_username = value);
                            },
                        }
                    }
                    div {
                        class: "field",
                        label { class: "field__label", r#for: "edit-ssh-key", "Private Key Path" }
                        input {
                            class: "input",
                            id: "edit-ssh-key",
                            value: current.ssh_private_key_path.clone(),
                            placeholder: "~/.ssh/id_ed25519 (optional if agent is configured)",
                            disabled,
                            oninput: move |event| {
                                let value = event.value();
                                draft.with_mut(|draft| draft.ssh_private_key_path = value);
                            },
                        }
                    }
                }
            }
        }
    }
}
