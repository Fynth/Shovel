mod forms;
mod kind_selector;
mod recent_connections;

// `edit_connection_modal` is also mounted from a separate native OS window
// (see `crate::windows::EditConnectionWindowRoot`). It must therefore be
// reachable across the `screens::connect` module boundary.
pub mod edit_connection_modal;

use crate::app_state::{APP_STATE, show_workspace, toast_error};
use dioxus::prelude::*;
use models::DatabaseKind;

use self::{
    forms::{ClickHouseForm, MySqlForm, PostgresForm, SqliteForm},
    kind_selector::KindSelector,
    recent_connections::RecentConnections,
};

#[component]
pub fn DbConnect() -> Element {
    let selected_kind = use_signal(|| DatabaseKind::Sqlite);
    let saved_connections_revision = use_signal(|| 0_u64);
    let mut saved_connections_error_reported = use_signal(|| false);
    let has_sessions = APP_STATE.read().has_sessions();
    let saved_connections = use_resource(move || {
        let _ = saved_connections_revision();
        async move { services::load_saved_connections().await }
    });

    use_effect(move || {
        let Some(result) = saved_connections() else {
            return;
        };
        if let Err(err) = result
            && !saved_connections_error_reported()
        {
            toast_error(format!("Failed to load recent connections: {err}"));
            saved_connections_error_reported.set(true);
        }
    });

    rsx! {
        section {
            class: if has_sessions {
                "connect-screen connect-screen--overlay"
            } else {
                "connect-screen"
            },
            div {
                class: "connect-screen__panel",
                div {
                    class: "connect-screen__hero",
                    div {
                        class: "connect-screen__hero-topbar",
                        div {
                            p { class: "connect-screen__eyebrow", "Developer Workspace" }
                            h1 { class: "connect-screen__title", "Connect to a database" }
                        }
                        if has_sessions {
                            button {
                                class: "button button--ghost",
                                onclick: move |_| show_workspace(),
                                "Back to Workspace"
                            }
                        }
                    }
                    p {
                        class: "connect-screen__subtitle",
                        "Manage local and remote connections with a desktop workflow tuned for query editing, inspection and result browsing."
                    }
                }

                RecentConnections {
                    saved_connections: saved_connections().and_then(Result::ok),
                    saved_connections_revision,
                }

                div {
                    class: "connect-screen__section",
                    KindSelector { selected_kind }

                    match selected_kind() {
                        DatabaseKind::Sqlite => rsx! { SqliteForm { saved_connections_revision } },
                        DatabaseKind::Postgres => rsx! { PostgresForm { saved_connections_revision } },
                        DatabaseKind::MySql => rsx! { MySqlForm { saved_connections_revision } },
                        DatabaseKind::ClickHouse => rsx! { ClickHouseForm { saved_connections_revision } },
                    }
                }
            }
        }
    }
}
