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

use crate::{
    layout::SettingsModal,
    screens::{
        connect::edit_connection_modal::EditConnectionModal,
        workspace::components::{
            BlobData, BlobViewer, DataDiffViewer, ErDiagramState, ErDiagramViewer,
            explorer::{
                create_table_modal,
                create_table_modal::{CreateTableModal, CreateTableTarget},
                duplicate_table_modal,
                duplicate_table_modal::{DuplicateTableModal, DuplicateTableTarget},
            },
        },
    },
};
use dioxus::{
    desktop::{Config, LogicalSize, WindowBuilder, window},
    prelude::*,
};
use models::QueryPage;

/// Compiled app stylesheet (grass output of `styles/app.scss`).
///
/// Lives in the `app` crate and is embedded into every Shovel window so the
/// design tokens and base styles are available without re-running grass from
/// `ui`. This mirrors the pattern in `app/src/main.rs` (APP_CSS).
const APP_CSS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../app/assets/app.css"
));

/// Pure helper that maps a Wayland-session verdict to the right decoration
/// answer, extracted so it can be unit-tested without touching process-global
/// env vars.
///
/// On Wayland (e.g. Hyprland, Sway, GNOME Wayland) the compositor draws its
/// own window chrome, so we disable Dioxus/tao decorations to avoid a double
/// title bar. On X11 and Windows we keep native decorations (the user gets a
/// real frame with minimize / maximize / close buttons).
fn decorations_for(is_wayland: bool) -> bool {
    !is_wayland
}

/// Decide whether dialog windows should request native decorations from
/// tao/Dioxus.
///
/// Wayland is detected via the two standard signals the session publishes:
/// `XDG_SESSION_TYPE=wayland` (set by systemd-logind / display managers) and
/// `WAYLAND_DISPLAY` (set by any Wayland compositor). Either is sufficient —
/// distros vary on which one they expose, and some apps (Flatpak portals,
/// nested compositors) only set one of them.
fn should_use_native_decorations() -> bool {
    let is_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var("WAYLAND_DISPLAY").is_ok();
    decorations_for(is_wayland)
}

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
        // Decorations ON on X11 / Windows (native frame + minimize / close),
        // OFF on Wayland (compositor already draws its own chrome — adding
        // Dioxus decorations would duplicate the title bar).
        .with_decorations(should_use_native_decorations());

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
        .with_decorations(should_use_native_decorations());

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

// ---------------------------------------------------------------------------
// Create-table dialog window
// ---------------------------------------------------------------------------

/// Snapshot the create-table dialog streams back to the main window on save.
///
/// The dialog calls `services::create_table` itself using the connection the
/// main window passed in via props. On success it fires `on_saved(())` and
/// the window root sends an empty `CreateTableResult` over the bridge — the
/// main window's receiver task uses it as the signal to bump
/// `tree_reload += 1`. The dialog then closes itself. `on_close` covers the
/// user-initiated dismiss paths (Close / Cancel / backdrop click) and
/// closes the window without sending a result.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateTableResult {}

/// Build a fresh `(DialogBridge<CreateTableResult>, Receiver)` pair for one
/// create-table window session.
pub fn create_table_bridge() -> (
    DialogBridge<CreateTableResult>,
    tokio::sync::mpsc::UnboundedReceiver<CreateTableResult>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`CreateTableWindowRoot`].
#[derive(Props, Clone)]
pub struct CreateTableWindowRootProps {
    pub bridge: DialogBridge<CreateTableResult>,
    pub target: CreateTableTarget,
    /// Live connection resolved by the main window before opening the dialog.
    /// `None` means the connection was closed in the meantime.
    pub connection: Option<models::DatabaseConnection>,
    pub read_only: bool,
    /// Active theme class (e.g. `"theme-dark"`) for the modal's CSS tokens.
    pub theme_class: String,
}

// `DatabaseConnection` does not implement `PartialEq` (sqlx pools are
// opaque), so we cannot derive it on this struct. The dialog window is
// opened once and its props never change for the lifetime of the window,
// so the comparison only needs to match the seed values the main window
// hands in — the connection is treated as opaque.
impl PartialEq for CreateTableWindowRootProps {
    fn eq(&self, other: &Self) -> bool {
        self.bridge == other.bridge
            && self.target == other.target
            && self.connection.is_some() == other.connection.is_some()
            && self.read_only == other.read_only
            && self.theme_class == other.theme_class
    }
}

/// Open the create-table dialog as a separate native OS window.
pub fn open_create_table_window(
    bridge: DialogBridge<CreateTableResult>,
    target: CreateTableTarget,
    connection: Option<models::DatabaseConnection>,
    read_only: bool,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            CreateTableWindowRoot,
            CreateTableWindowRootProps {
                bridge,
                target,
                connection,
                read_only,
                theme_class,
            },
        );
        let config = create_table_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn create_table_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Create Table")
        .with_inner_size(LogicalSize::new(720.0, 720.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the create-table dialog window.
///
/// Wraps the prop-driven [`CreateTableModal`] in a themed shell so the
/// design tokens resolve correctly. The connection is resolved by the main
/// window (so this window's isolated globals never need it) and passed via
/// `connection` prop. On save the root forwards an empty
/// [`CreateTableResult`] over the bridge — the main window's receiver uses
/// that to bump `tree_reload` and re-fetch the explorer.
#[component]
pub fn CreateTableWindowRoot(props: CreateTableWindowRootProps) -> Element {
    let bridge = props.bridge;
    let target = props.target;
    let connection = props.connection;
    let read_only = props.read_only;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "table-window-shell {theme_class}",
            CreateTableModal {
                target,
                connection: create_table_modal::ModalConnection(connection),
                read_only,
                on_saved: move |_| {
                    bridge.send(CreateTableResult {});
                    window().close();
                },
                on_close: move |_| {
                    window().close();
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Duplicate-table dialog window
// ---------------------------------------------------------------------------

/// Snapshot the duplicate-table dialog streams back to the main window on save.
///
/// Carries the qualified name of the newly created table so the main window
/// can update its `selected_node` signal to point at the duplicate. The
/// dialog then closes itself. `on_close` closes the window without
/// sending a result.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateTableResult {
    pub new_qualified_name: String,
}

/// Build a fresh `(DialogBridge<DuplicateTableResult>, Receiver)` pair for
/// one duplicate-table window session.
pub fn create_duplicate_table_bridge() -> (
    DialogBridge<DuplicateTableResult>,
    tokio::sync::mpsc::UnboundedReceiver<DuplicateTableResult>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`DuplicateTableWindowRoot`].
#[derive(Props, Clone)]
pub struct DuplicateTableWindowRootProps {
    pub bridge: DialogBridge<DuplicateTableResult>,
    pub target: DuplicateTableTarget,
    /// Live connection resolved by the main window before opening the dialog.
    pub session: Option<models::DatabaseConnection>,
    pub read_only: bool,
    pub theme_class: String,
}

// See `CreateTableWindowRootProps` — `DatabaseConnection` is opaque so we
// cannot derive `PartialEq` and instead compare the connection as
// present/absent (the dialog window is opened once and its props never
// change for the lifetime of the window).
impl PartialEq for DuplicateTableWindowRootProps {
    fn eq(&self, other: &Self) -> bool {
        self.bridge == other.bridge
            && self.target == other.target
            && self.session.is_some() == other.session.is_some()
            && self.read_only == other.read_only
            && self.theme_class == other.theme_class
    }
}

/// Open the duplicate-table dialog as a separate native OS window.
pub fn open_duplicate_table_window(
    bridge: DialogBridge<DuplicateTableResult>,
    target: DuplicateTableTarget,
    session: Option<models::DatabaseConnection>,
    read_only: bool,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            DuplicateTableWindowRoot,
            DuplicateTableWindowRootProps {
                bridge,
                target,
                session,
                read_only,
                theme_class,
            },
        );
        let config = duplicate_table_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn duplicate_table_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Duplicate Table")
        .with_inner_size(LogicalSize::new(640.0, 540.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the duplicate-table dialog window.
#[component]
pub fn DuplicateTableWindowRoot(props: DuplicateTableWindowRootProps) -> Element {
    let bridge = props.bridge;
    let target = props.target;
    let session = props.session;
    let read_only = props.read_only;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "table-window-shell {theme_class}",
            DuplicateTableModal {
                target,
                session: duplicate_table_modal::ModalConnection(session),
                read_only,
                on_saved: move |new_qualified_name: String| {
                    bridge.send(DuplicateTableResult { new_qualified_name });
                    window().close();
                },
                on_close: move |_| {
                    window().close();
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ER-diagram viewer window
// ---------------------------------------------------------------------------

/// Props for [`ErDiagramWindowRoot`].
///
/// Data flows IN via props only — the window is view-only, so there is no
/// [`DialogBridge`] to stream anything back. Closing the window calls
/// `window().close()` directly.
#[derive(Props, Clone, PartialEq)]
pub struct ErDiagramWindowRootProps {
    pub diagram: ErDiagramState,
    /// Active theme class (e.g. `"theme-dark"`) for the viewer's CSS tokens.
    pub theme_class: String,
}

/// Open the ER-diagram viewer as a separate native OS window.
///
/// The caller (main workspace window) builds the [`ErDiagramState`] from
/// `tree_sections` + foreign keys, then hands the resolved value to a brand
/// new [`VirtualDom`] here. The new window has its own globals
/// ([`Signal::global`] values do not carry across windows), but does not need
/// any — the diagram is fully encoded in the props.
///
/// Spawns a non-blocking task that configures a [`WindowBuilder`] with
/// decorations enabled and hands it to Dioxus via
/// `DesktopContext::new_window`.
pub fn open_er_diagram_window(diagram: ErDiagramState, theme_class: String) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            ErDiagramWindowRoot,
            ErDiagramWindowRootProps {
                diagram,
                theme_class,
            },
        );
        let config = er_diagram_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn er_diagram_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("ER Diagram")
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the ER-diagram viewer window.
///
/// Mounts the prop-driven [`ErDiagramViewer`] inside a themed
/// `.er-diagram-window-shell` so design tokens resolve correctly. The
/// viewer's `on_close` simply calls `window().close()` — the main window no
/// longer needs to flip a gate signal because the diagram lives in its own
/// OS window.
#[component]
pub fn ErDiagramWindowRoot(props: ErDiagramWindowRootProps) -> Element {
    let diagram = props.diagram;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "er-diagram-window-shell {theme_class}",
            ErDiagramViewer {
                diagram,
                on_close: move |_| {
                    window().close();
                },
                on_table_click: move |_table_name: String| {},
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BLOB viewer window
// ---------------------------------------------------------------------------

/// Props for [`BlobWindowRoot`].
///
/// Data flows IN via props only — the window is view-only, so there is no
/// [`DialogBridge`] to stream anything back. Closing the window calls
/// `window().close()` directly.
#[derive(Props, Clone, PartialEq)]
pub struct BlobWindowRootProps {
    pub blob: BlobData,
    /// Active theme class (e.g. `"theme-dark"`) for the viewer's CSS tokens.
    pub theme_class: String,
}

/// Open the BLOB viewer as a separate native OS window.
///
/// The caller (main workspace window) builds the [`BlobData`] (raw bytes +
/// optional MIME type), then hands the resolved value to a brand new
/// [`VirtualDom`]. The new window does not need any globals — the blob is
/// fully encoded in the props.
pub fn open_blob_window(blob: BlobData, theme_class: String) {
    spawn(async move {
        let dom =
            VirtualDom::new_with_props(BlobWindowRoot, BlobWindowRootProps { blob, theme_class });
        let config = blob_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn blob_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Blob Viewer")
        .with_inner_size(LogicalSize::new(720.0, 640.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the BLOB viewer window.
///
/// Mounts the prop-driven [`BlobViewer`] inside a themed
/// `.blob-viewer-window-shell` so design tokens resolve correctly. The
/// viewer's `on_close` simply calls `window().close()`.
#[component]
pub fn BlobWindowRoot(props: BlobWindowRootProps) -> Element {
    let blob = props.blob;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "blob-viewer-window-shell {theme_class}",
            BlobViewer {
                blob,
                on_close: move |_| {
                    window().close();
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Data-diff viewer window
// ---------------------------------------------------------------------------

/// Props for [`DataDiffWindowRoot`].
///
/// Data flows IN via props only — the window is view-only, so there is no
/// [`DialogBridge`] to stream anything back. The two [`QueryPage`] values
/// are the pinned result and the currently-displayed result the caller wants
/// to compare; labels describe each side in the diff header.
#[derive(Props, Clone, PartialEq)]
pub struct DataDiffWindowRootProps {
    pub left: Option<QueryPage>,
    pub right: Option<QueryPage>,
    pub left_label: String,
    pub right_label: String,
    /// Active theme class (e.g. `"theme-dark"`) for the viewer's CSS tokens.
    pub theme_class: String,
}

/// Open the data-diff viewer as a separate native OS window.
///
/// The caller (result table in the main workspace window) hands the
/// already-resolved left/right [`QueryPage`] values plus their labels to a
/// brand new [`VirtualDom`]. The diff itself is computed inside the window
/// via the existing `calculate_diff` helper, so no extra plumbing is needed.
pub fn open_data_diff_window(
    left: Option<QueryPage>,
    right: Option<QueryPage>,
    left_label: String,
    right_label: String,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            DataDiffWindowRoot,
            DataDiffWindowRootProps {
                left,
                right,
                left_label,
                right_label,
                theme_class,
            },
        );
        let config = data_diff_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn data_diff_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Data Diff")
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the data-diff viewer window.
///
/// Mounts the prop-driven [`DataDiffViewer`] inside a themed
/// `.data-diff-window-shell` so design tokens resolve correctly. The
/// viewer's `on_close` simply calls `window().close()` — the main window no
/// longer needs to flip a `show_compare` gate signal because the diff lives
/// in its own OS window.
#[component]
pub fn DataDiffWindowRoot(props: DataDiffWindowRootProps) -> Element {
    let left = props.left;
    let right = props.right;
    let left_label = props.left_label;
    let right_label = props.right_label;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "data-diff-window-shell {theme_class}",
            DataDiffViewer {
                left_data: left,
                right_data: right,
                left_label,
                right_label,
                on_close: move |_| {
                    window().close();
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decorations_for;

    /// Wayland compositors draw their own chrome, so Dioxus must skip its
    /// own title bar to avoid rendering it twice.
    #[test]
    fn decorations_for_wayland_disables_native_chrome() {
        assert!(!decorations_for(true));
    }

    /// X11, Windows, macOS and headless sessions keep the OS frame so the
    /// user still gets a working minimize / close button.
    #[test]
    fn decorations_for_non_wayland_keeps_native_chrome() {
        assert!(decorations_for(false));
    }
}
