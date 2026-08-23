use dioxus::prelude::*;
use models::DatabaseKind;

use super::remote_draft::RemoteConnectionDraft;

#[component]
pub(crate) fn RemoteEditorFields(
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
                    value: "{current.host}",
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
                    value: "{current.port}",
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
                value: "{current.username}",
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
                value: "{current.password}",
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
                value: "{current.database}",
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
pub(crate) fn RemoteSshTunnelFields(
    mut draft: Signal<RemoteConnectionDraft>,
    disabled: bool,
) -> Element {
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
                            value: "{current.ssh_host}",
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
                            value: "{current.ssh_port}",
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
                            value: "{current.ssh_username}",
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
                            value: "{current.ssh_private_key_path}",
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
