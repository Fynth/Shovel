use crate::app_state::add_connection_session;
use dioxus::prelude::*;
use models::{ConnectionRequest, MySqlFormData, SshTunnelConfig};

use super::{
    SshTunnelFields,
    connection_status_class,
    format_connection_error,
    validate_required_fields,
};

#[component]
pub fn MySqlForm(mut saved_connections_revision: Signal<u64>) -> Element {
    let mut host = use_signal(|| "localhost".to_string());
    let mut port = use_signal(|| "3306".to_string());
    let mut username = use_signal(|| "root".to_string());
    let mut password = use_signal(|| "".to_string());
    let mut database = use_signal(String::new);
    let mut ssl_mode = use_signal(|| "preferred".to_string());
    let ssh_enabled = use_signal(|| false);
    let ssh_host = use_signal(String::new);
    let ssh_port = use_signal(|| "22".to_string());
    let ssh_username = use_signal(String::new);
    let ssh_private_key_path = use_signal(String::new);
    let mut status = use_signal(String::new);
    let status_value = status();
    let status_class = connection_status_class(&status_value);

    // Сборка ConnectionRequest из текущих значений формы. Замыкание
    // захватывает только Copy-сигналы, поэтому само является Copy и
    // переиспользуется и обработчиком Connect, и кнопкой Test.
    let build_request = move || {
        ConnectionRequest::MySql(MySqlFormData {
            host: host(),
            port: port().parse().unwrap_or(3306),
            username: username(),
            password: password(),
            database: database(),
            ssl_mode: ssl_mode(),
            ssh_tunnel: if ssh_enabled() {
                Some(SshTunnelConfig {
                    host: ssh_host(),
                    port: ssh_port().parse().unwrap_or(22),
                    username: ssh_username(),
                    private_key_path: ssh_private_key_path(),
                })
            } else {
                None
            },
        })
    };

    rsx! {
        form {
            class: "connect-form",
            onsubmit: move |event| {
                event.prevent_default();

                if let Err(err) = validate_required_fields(&[
                    ("Host", host().as_str()),
                    ("Username", username().as_str()),
                ]) {
                    status.set(err);
                    return;
                }

                status.set("Connecting...".to_string());
                let request = build_request();

                spawn(async move {
                    match services::connect_and_save_request(request.clone()).await {
                        Ok(result) => {
                            add_connection_session(request, result.handle);
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
                class: "connect-form__grid",
                div {
                    class: "field",
                    label { class: "field__label", r#for: "mysql-host", "Host" }
                    input {
                        class: "input",
                        id: "mysql-host",
                        value: "{host}",
                        placeholder: "localhost or mysql://user:pass@host:3306/db",
                        oninput: move |event| host.set(event.value()),
                    }
                }

                div {
                    class: "field",
                    label { class: "field__label", r#for: "mysql-port", "Port" }
                    input {
                        class: "input",
                        id: "mysql-port",
                        value: "{port}",
                        placeholder: "3306",
                        oninput: move |event| port.set(event.value()),
                    }
                }
            }

            div {
                class: "field",
                label { class: "field__label", r#for: "mysql-username", "Username" }
                input {
                    class: "input",
                    id: "mysql-username",
                    value: "{username}",
                    placeholder: "root",
                    oninput: move |event| username.set(event.value()),
                }
            }

            div {
                class: "field",
                label { class: "field__label", r#for: "mysql-password", "Password" }
                input {
                    class: "input",
                    id: "mysql-password",
                    r#type: "password",
                    value: "{password}",
                    placeholder: "••••••••",
                    oninput: move |event| password.set(event.value()),
                }
            }

            div {
                class: "field",
                label { class: "field__label", r#for: "mysql-database", "Database" }
                input {
                    class: "input",
                    id: "mysql-database",
                    value: "{database}",
                    placeholder: "Optional default database",
                    oninput: move |event| database.set(event.value()),
                }
            }

            div {
                class: "field",
                label { class: "field__label", r#for: "mysql-ssl-mode", "SSL Mode" }
                select {
                    class: "input",
                    id: "mysql-ssl-mode",
                    value: "{ssl_mode}",
                    onchange: move |event| ssl_mode.set(event.value()),
                    option { value: "disabled", "disabled" }
                    option { value: "preferred", "preferred" }
                    option { value: "required", "required" }
                    option { value: "verify-ca", "verify-ca" }
                    option { value: "verify-identity", "verify-identity" }
                }
            }

            SshTunnelFields {
                enabled: ssh_enabled,
                host: ssh_host,
                port: ssh_port,
                username: ssh_username,
                private_key_path: ssh_private_key_path,
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
                        if let Err(err) = validate_required_fields(&[
                            ("Host", host().as_str()),
                            ("Username", username().as_str()),
                        ]) {
                            status.set(err);
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
