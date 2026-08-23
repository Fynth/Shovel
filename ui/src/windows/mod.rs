//! Native OS dialog windows (separate from the main window).
//!
//! Each window is a real top-level OS window with its own webview, decorations,
//! and event loop slot. The main `Shovel` window stays independent — the only
//! shared state is the compiled app stylesheet, injected into the new window
//! via `<document::Style>` so the placeholder matches the rest of the app's
//! design tokens.
//!
//! ## Cross-window state
//!
//! Dioxus 0.7 stores [`dioxus::signals::Signal::global`] values per
//! [`dioxus_core::VirtualDom`]. A separate native window therefore does NOT
//! see the main window's globals (e.g. `APP_UI_SETTINGS`,
//! `APP_SQL_FORMAT_SETTINGS`) — they would silently re-default.
//!
//! Dialog windows must NOT mirror globals locally. Instead they receive a
//! [`DialogBridge`] from the main window and stream change snapshots back over
//! it. The main window owns the receiver and applies each snapshot to its
//! real global state — that triggers the existing persistence effects in
//! `app.rs` to write the new value to disk.

use crate::{layout::SettingsModal, screens::connect::edit_connection_modal::EditConnectionModal};
use dioxus::{
    desktop::{Config, LogicalSize, WindowBuilder, window},
    prelude::*,
};

/// Compiled app stylesheet (grass output of `styles/app.scss`).
///
/// Lives in the `app` crate and is embedded into every Shovel window so the
/// design tokens and base styles are available without re-running grass from
/// `ui`. This mirrors the pattern in `app/src/main.rs` (APP_CSS).
const APP_CSS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../app/assets/app.css"
));

/// A thread-safe channel a dialog window uses to stream changes back to the
/// main window.
///
/// `DialogBridge` only holds the sender half — the main window owns the
/// receiver and is responsible for applying the value to the real global state
/// (so persistence effects in `app.rs` keep working). The dialog window never
/// reads from this channel; it is a write-only pipe back to the main window.
///
/// Cloning is cheap (it is just an `UnboundedSender<T>` handle).
#[derive(Clone)]
pub struct DialogBridge<T> {
    sender: tokio::sync::mpsc::UnboundedSender<T>,
}

/// Two `DialogBridge<T>` are equal iff they were created from the same
/// channel. We can't `#[derive(PartialEq)]` because `UnboundedSender<T>`
/// doesn't implement it, but it exposes `same_channel` which gives us the
/// same semantics.
impl<T> PartialEq for DialogBridge<T> {
    fn eq(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

impl<T: Send + 'static> DialogBridge<T> {
    /// Send a value to the main window. If the receiver has been dropped
    /// (e.g. main window closed), the value is silently discarded — the
    /// dialog never blocks on the main window's lifetime.
    pub fn send(&self, value: T) {
        let _ = self.sender.send(value);
    }
}

/// A snapshot of the settings the dialog window writes back to the main window.
///
/// `ui` covers the user-facing toggles (`AppUiSettings`) and `sql` covers the
/// SQL formatter knobs (`SqlFormatSettings`). The main window receives one of
/// these and applies it via the same setters used by the in-app modal.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSnapshot {
    pub ui: models::AppUiSettings,
    pub sql: models::SqlFormatSettings,
}

/// Build a fresh `(DialogBridge<SettingsSnapshot>, Receiver<SettingsSnapshot>)`
/// pair.
///
/// The main window passes the sender half (as a [`DialogBridge`]) to the
/// dialog window via props, and keeps the receiver to apply incoming snapshots
/// to its real global state.
pub fn create_settings_bridge() -> (
    DialogBridge<SettingsSnapshot>,
    tokio::sync::mpsc::UnboundedReceiver<SettingsSnapshot>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`SettingsWindowRoot`].
///
/// The window mirrors the latest snapshot the user has committed into the
/// bridge, but it needs an initial value to render before any edit happens —
/// `initial_ui` / `initial_sql` are that seed. `bridge` carries every user
/// edit back to the main window, which owns the real globals + persistence.
#[derive(Props, Clone, PartialEq)]
pub struct SettingsWindowRootProps {
    pub bridge: DialogBridge<SettingsSnapshot>,
    pub initial_ui: models::AppUiSettings,
    pub initial_sql: models::SqlFormatSettings,
}

/// Open the settings window as a separate native OS window.
///
/// `bridge` is the sender half created by [`create_settings_bridge`] — the
/// main window keeps the matching receiver and applies incoming snapshots to
/// its real global state. `initial_ui` / `initial_sql` are the snapshots the
/// main window currently holds; the new window uses them as its seed values
/// so the user sees the current settings the moment the window opens.
///
/// Spawns a non-blocking task that builds a new [`VirtualDom`], configures a
/// [`WindowBuilder`] with decorations enabled, and hands it to Dioxus via
/// `DesktopContext::new_window`. The future resolves once the window is ready.
pub fn open_settings_window(
    bridge: DialogBridge<SettingsSnapshot>,
    initial_ui: models::AppUiSettings,
    initial_sql: models::SqlFormatSettings,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            SettingsWindowRoot,
            SettingsWindowRootProps {
                bridge,
                initial_ui,
                initial_sql,
            },
        );
        let config = settings_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn settings_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Shovel Settings")
        .with_inner_size(LogicalSize::new(560.0, 640.0))
        .with_resizable(true)
        // Decorations ON — a real OS title bar + close button. This is what
        // makes the settings surface feel like a separate OS window rather
        // than an overlay on the main webview.
        .with_decorations(true);

    Config::new().with_window(window_builder)
}

/// Root component for the settings window.
///
/// Mounts the prop-driven [`SettingsModal`] inside a `.settings-window-shell`
/// wrapper. The shell just centers/fills the window; the modal content reuses
/// its existing `.settings-modal` styles (already tokenized in
/// `styles/components/_settings-modal.scss`). Two local signals mirror the
/// current UI + SQL settings; every edit flows through both the local signal
/// (so the field re-renders with the new value) and the [`DialogBridge`] (so
/// the main window's globals stay in sync, triggering its persistence
/// effects).
#[component]
pub fn SettingsWindowRoot(props: SettingsWindowRootProps) -> Element {
    let bridge = props.bridge;
    let initial_ui = props.initial_ui;
    let initial_sql = props.initial_sql;
    let mut ui = use_signal(move || initial_ui.clone());
    let mut sql = use_signal(move || initial_sql.clone());

    // Reflect the main window's current theme so tokens resolve to the right
    // palette. The user can still switch theme from inside the modal — that
    // edit goes through the bridge and the next render picks it up.
    let theme_class = ui().theme.css_class().to_string();

    let on_change =
        move |(next_ui, next_sql): (models::AppUiSettings, models::SqlFormatSettings)| {
            ui.set(next_ui.clone());
            sql.set(next_sql.clone());
            bridge.send(SettingsSnapshot {
                ui: next_ui,
                sql: next_sql,
            });
        };

    let on_close = move |_| {
        window().close();
    };

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "settings-window-shell {theme_class}",
            SettingsModal {
                settings: ui(),
                sql_settings: sql(),
                on_change,
                on_close,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Connection-edit dialog window
// ---------------------------------------------------------------------------

/// Snapshot the connection-edit dialog streams back to the main window on save.
///
/// The dialog calls `services::replace_connection_request` itself, then sends
/// the resulting `SavedConnection` back over the bridge. The main window
/// receives the snapshot in a `spawn`-ed receiver task, bumps the
/// `saved_connections_revision` signal so the recent-connections list
/// re-fetches, and writes a status message. The dialog window then closes
/// itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionEditSnapshot {
    pub connection: models::SavedConnection,
}

/// Build a fresh `(DialogBridge<ConnectionEditSnapshot>, Receiver)` pair for a
/// single edit-window session.
///
/// The main window passes the sender half (as a [`DialogBridge`]) to the new
/// dialog window via [`EditConnectionWindowRoot`] props, and keeps the
/// receiver so it can apply the resulting snapshot to its local signals.
pub fn create_connection_edit_bridge() -> (
    DialogBridge<ConnectionEditSnapshot>,
    tokio::sync::mpsc::UnboundedReceiver<ConnectionEditSnapshot>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`EditConnectionWindowRoot`].
#[derive(Props, Clone, PartialEq)]
pub struct EditConnectionWindowRootProps {
    pub bridge: DialogBridge<ConnectionEditSnapshot>,
    pub saved_connection: models::SavedConnection,
    /// Active theme class (e.g. `"theme-dark"`) for the modal's CSS tokens.
    pub theme_class: String,
}

/// Open the connection-edit dialog as a separate native OS window.
///
/// `bridge` is the sender half created by [`create_connection_edit_bridge`] —
/// the main window keeps the matching receiver and applies incoming
/// snapshots to its `saved_connections_revision` signal + status display.
/// `saved_connection` is the row the user picked from the recent-connections
/// list; the dialog seeds its form fields from it. `theme_class` is the
/// active theme the main window currently uses, so the dialog renders with
/// matching CSS tokens.
///
/// Spawns a non-blocking task that builds a new [`VirtualDom`], configures a
/// [`WindowBuilder`] with decorations enabled, and hands it to Dioxus via
/// `DesktopContext::new_window`.
pub fn open_connection_edit_window(
    bridge: DialogBridge<ConnectionEditSnapshot>,
    saved_connection: models::SavedConnection,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            EditConnectionWindowRoot,
            EditConnectionWindowRootProps {
                bridge,
                saved_connection,
                theme_class,
            },
        );
        let config = connection_edit_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn connection_edit_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Edit Connection")
        .with_inner_size(LogicalSize::new(640.0, 720.0))
        .with_resizable(true)
        .with_decorations(true);

    Config::new().with_window(window_builder)
}

/// Root component for the connection-edit dialog window.
///
/// Wraps the prop-driven [`EditConnectionModal`] inside a themed
/// `.connect-window-shell` so the form fields resolve to the right design
/// tokens (the modal itself reuses `.settings-modal` / `.connect-form`
/// styling). The modal is responsible for calling
/// `services::replace_connection_request`; on success it fires `on_saved`
/// with the updated `SavedConnection`, which this root forwards over the
/// bridge and then closes the window. `on_close` covers the user-initiated
/// dismiss paths (Cancel, X, backdrop click) — they close the window without
/// sending a snapshot.
#[component]
pub fn EditConnectionWindowRoot(props: EditConnectionWindowRootProps) -> Element {
    let bridge = props.bridge;
    let saved_connection = props.saved_connection;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "connect-window-shell {theme_class}",
            EditConnectionModal {
                saved_connection,
                on_saved: move |updated: models::SavedConnection| {
                    bridge.send(ConnectionEditSnapshot { connection: updated });
                    window().close();
                },
                on_close: move |_| {
                    window().close();
                },
            }
        }
    }
}
