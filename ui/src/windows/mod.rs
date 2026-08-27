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
    layout::{SettingsModal, ToastContainer},
    screens::{
        connect::edit_connection_modal::EditConnectionModal,
        workspace::components::{
            BlobData,
            BlobViewer,
            DataDiffViewer,
            ErDiagramState,
            ErDiagramViewer,
            explorer::{
                create_table_modal::{CreateTableModal, CreateTableTarget},
                duplicate_table_modal::{DuplicateTableModal, DuplicateTableTarget},
                rename_table_modal::{RenameTableModal, RenameTableTarget},
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

/// Snapshot of UI + SQL settings the settings dialog writes back to the main
/// window.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSnapshot {
    pub ui: models::AppUiSettings,
    pub sql: models::SqlFormatSettings,
}

/// Build a fresh `(DialogBridge<SettingsSnapshot>, Receiver<SettingsSnapshot>)`
/// pair.
pub fn create_settings_bridge() -> (
    DialogBridge<SettingsSnapshot>,
    tokio::sync::mpsc::UnboundedReceiver<SettingsSnapshot>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`SettingsWindowRoot`].
#[derive(Props, Clone, PartialEq)]
pub struct SettingsWindowRootProps {
    pub bridge: DialogBridge<SettingsSnapshot>,
    pub initial_ui: models::AppUiSettings,
    pub initial_sql: models::SqlFormatSettings,
}

/// Open the settings window as a separate native OS window, seeded from the
/// main window's current settings and streaming edits back over `bridge`.
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
        // 960×720 keeps the 168px category nav beside the content pane.
        // Min 720×520 stays above the 560px stacked-nav breakpoint.
        .with_inner_size(LogicalSize::new(960.0, 720.0))
        .with_min_inner_size(LogicalSize::new(720.0, 520.0))
        .with_resizable(true)
        // Decorations ON on X11 / Windows (native frame + minimize / close),
        // OFF on Wayland (compositor already draws its own chrome — adding
        // Dioxus decorations would duplicate the title bar).
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the settings window.
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
    let density_class = ui().density.css_class();
    let theme_css = ui().theme_overrides.to_css();

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
        document::Style { {APP_CSS.to_string()} }
        if !theme_css.is_empty() {
            style { {theme_css} }
        }
        div { class: "settings-window-shell {theme_class} {density_class}",
            SettingsModal {
                settings: ui(),
                sql_settings: sql(),
                on_change,
                on_close,
            }
            ToastContainer {}
        }
    }
}

// ---------------------------------------------------------------------------
// Connection-edit dialog window
// ---------------------------------------------------------------------------

/// Snapshot the connection-edit dialog streams back to the main window on save.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionEditSnapshot {
    pub connection: models::SavedConnection,
}

/// Build a fresh `(DialogBridge<ConnectionEditSnapshot>, Receiver)` pair for a
/// single edit-window session.
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

/// Open the connection-edit dialog as a separate native OS window, seeding the
/// form from `saved_connection` and streaming the saved result over `bridge`.
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
#[component]
pub fn EditConnectionWindowRoot(props: EditConnectionWindowRootProps) -> Element {
    let bridge = props.bridge;
    let saved_connection = props.saved_connection;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
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
#[derive(Props, Clone, PartialEq)]
pub struct CreateTableWindowRootProps {
    pub bridge: DialogBridge<CreateTableResult>,
    pub target: CreateTableTarget,
    /// Live session resolved by the main window before opening the dialog.
    /// `None` means the connection was closed in the meantime.
    pub session_id: Option<u64>,
    pub read_only: bool,
    /// Active theme class (e.g. `"theme-dark"`) for the modal's CSS tokens.
    pub theme_class: String,
}

/// Open the create-table dialog as a separate native OS window.
pub fn open_create_table_window(
    bridge: DialogBridge<CreateTableResult>,
    target: CreateTableTarget,
    session_id: Option<u64>,
    read_only: bool,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            CreateTableWindowRoot,
            CreateTableWindowRootProps {
                bridge,
                target,
                session_id,
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
#[component]
pub fn CreateTableWindowRoot(props: CreateTableWindowRootProps) -> Element {
    let bridge = props.bridge;
    let target = props.target;
    let session_id = props.session_id;
    let read_only = props.read_only;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
        div { class: "table-window-shell {theme_class}",
            CreateTableModal {
                target,
                session_id,
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
#[derive(Props, Clone, PartialEq)]
pub struct DuplicateTableWindowRootProps {
    pub bridge: DialogBridge<DuplicateTableResult>,
    pub target: DuplicateTableTarget,
    /// Live session resolved by the main window before opening the dialog.
    pub session_id: Option<u64>,
    pub read_only: bool,
    pub theme_class: String,
}

/// Open the duplicate-table dialog as a separate native OS window.
pub fn open_duplicate_table_window(
    bridge: DialogBridge<DuplicateTableResult>,
    target: DuplicateTableTarget,
    session_id: Option<u64>,
    read_only: bool,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            DuplicateTableWindowRoot,
            DuplicateTableWindowRootProps {
                bridge,
                target,
                session_id,
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
    let session_id = props.session_id;
    let read_only = props.read_only;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
        div { class: "table-window-shell {theme_class}",
            DuplicateTableModal {
                target,
                session_id,
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
// Rename-table window
// ---------------------------------------------------------------------------

/// Snapshot the rename-table dialog streams back to the main window on save.
#[derive(Clone, Debug, PartialEq)]
pub struct RenameTableResult {
    pub new_qualified_name: String,
}

/// Build a fresh `(DialogBridge<RenameTableResult>, Receiver)` pair for one
/// rename-table window session.
pub fn create_rename_table_bridge() -> (
    DialogBridge<RenameTableResult>,
    tokio::sync::mpsc::UnboundedReceiver<RenameTableResult>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DialogBridge { sender: tx }, rx)
}

/// Props for [`RenameTableWindowRoot`].
#[derive(Props, Clone, PartialEq)]
pub struct RenameTableWindowRootProps {
    pub bridge: DialogBridge<RenameTableResult>,
    pub target: RenameTableTarget,
    /// Live session resolved by the main window before opening the dialog.
    pub session_id: Option<u64>,
    pub read_only: bool,
    pub theme_class: String,
}

/// Open the rename-table dialog as a separate native OS window.
pub fn open_rename_table_window(
    bridge: DialogBridge<RenameTableResult>,
    target: RenameTableTarget,
    session_id: Option<u64>,
    read_only: bool,
    theme_class: String,
) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            RenameTableWindowRoot,
            RenameTableWindowRootProps {
                bridge,
                target,
                session_id,
                read_only,
                theme_class,
            },
        );
        let config = rename_table_window_config();
        let _pending = window().new_window(dom, config).await;
    });
}

fn rename_table_window_config() -> Config {
    let window_builder = WindowBuilder::new()
        .with_title("Rename Table")
        .with_inner_size(LogicalSize::new(640.0, 480.0))
        .with_resizable(true)
        .with_decorations(should_use_native_decorations());

    Config::new().with_window(window_builder)
}

/// Root component for the rename-table dialog window.
#[component]
pub fn RenameTableWindowRoot(props: RenameTableWindowRootProps) -> Element {
    let bridge = props.bridge;
    let target = props.target;
    let session_id = props.session_id;
    let read_only = props.read_only;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
        div { class: "table-window-shell {theme_class}",
            RenameTableModal {
                target,
                session_id,
                read_only,
                on_saved: move |new_qualified_name: String| {
                    bridge.send(RenameTableResult { new_qualified_name });
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
#[derive(Props, Clone, PartialEq)]
pub struct ErDiagramWindowRootProps {
    pub diagram: ErDiagramState,
    /// Active theme class (e.g. `"theme-dark"`) for the viewer's CSS tokens.
    pub theme_class: String,
}

/// Open the ER-diagram viewer as a separate native OS window.
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
#[component]
pub fn ErDiagramWindowRoot(props: ErDiagramWindowRootProps) -> Element {
    let diagram = props.diagram;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
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
#[derive(Props, Clone, PartialEq)]
pub struct BlobWindowRootProps {
    pub blob: BlobData,
    /// Active theme class (e.g. `"theme-dark"`) for the viewer's CSS tokens.
    pub theme_class: String,
}

/// Open the BLOB viewer as a separate native OS window.
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
#[component]
pub fn BlobWindowRoot(props: BlobWindowRootProps) -> Element {
    let blob = props.blob;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
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
#[component]
pub fn DataDiffWindowRoot(props: DataDiffWindowRootProps) -> Element {
    let left = props.left;
    let right = props.right;
    let left_label = props.left_label;
    let right_label = props.right_label;
    let theme_class = props.theme_class;

    rsx! {
        document::Style { {APP_CSS.to_string()} }
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
