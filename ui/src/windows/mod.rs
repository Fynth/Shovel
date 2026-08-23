//! Native OS dialog windows (separate from the main window).
//!
//! Each window is a real top-level OS window with its own webview, decorations,
//! and event loop slot. The main `Shovel` window stays independent — the only
//! shared state in step 1 is the compiled app stylesheet, injected into the
//! new window via `<document::Style>` so the placeholder matches the rest of
//! the app's design tokens.
//!
//! ## Cross-window state
//!
//! Dioxus 0.7 stores [`dioxus::signals::Signal::global`] values per
//! [`dioxus_core::VirtualDom`]. A separate native window therefore does NOT
//! see the main window's globals (e.g. `APP_UI_SETTINGS`,
//! `APP_SQL_FORMAT_SETTINGS`) — they would silently re-default.
//!
//! To keep the persistence effects in `app.rs` working, dialog windows must
//! NOT mirror globals locally. Instead they receive a [`DialogBridge`] from the
//! main window and stream change snapshots back over it. The main window owns
//! the receiver and applies the snapshot to its real global state.

use dioxus::{
    desktop::{Config, LogicalSize, WindowBuilder, window},
    prelude::*,
};

/// Compiled app stylesheet (grass output of `styles/app.scss`).
///
/// Lives in the `app` crate and is embedded into every Shovel window so the
/// design tokens and base styles are available without re-running grass from
/// `ui`. This mirrors the pattern in `app/src/main.rs` (APP_CSS).
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../app/assets/app.css"));

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
/// `bridge` is the only prop — the settings window needs nothing else. The
/// snapshot data is held by the main window's globals; the dialog pushes
/// changes through `bridge`, never the other way around.
#[derive(Props, Clone, PartialEq)]
pub struct SettingsWindowRootProps {
    pub bridge: DialogBridge<SettingsSnapshot>,
}

/// Open the settings window as a separate native OS window.
///
/// `bridge` is the sender half created by [`create_settings_bridge`] — the
/// main window keeps the matching receiver and applies incoming snapshots to
/// its real global state.
///
/// Spawns a non-blocking task that builds a new [`VirtualDom`], configures a
/// [`WindowBuilder`] with decorations enabled, and hands it to Dioxus via
/// `DesktopContext::new_window`. The future resolves once the window is ready.
pub fn open_settings_window(bridge: DialogBridge<SettingsSnapshot>) {
    spawn(async move {
        let dom = VirtualDom::new_with_props(
            SettingsWindowRoot,
            SettingsWindowRootProps { bridge },
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
/// Step 2 keeps this as a minimal stub that proves the [`DialogBridge`]
/// round-trips compile: pressing the "Save" button builds a demo
/// [`SettingsSnapshot`] (using the settings types' `Default` impls), sends it
/// through the bridge, and closes the window. The full settings UI lives in
/// `ui/src/layout/settings_modal.rs` and will be mounted here in a later step.
#[component]
pub fn SettingsWindowRoot(props: SettingsWindowRootProps) -> Element {
    let bridge = props.bridge;

    rsx! {
        document::Style { "{APP_CSS}" }
        div { class: "settings-window-root theme-dark",
            header { class: "settings-window-root__header",
                h1 { "Settings" }
            }
            main { class: "settings-window-root__body",
                p { class: "settings-window-root__hint", "Settings window (in progress)" }
                button {
                    class: "settings-window-root__save",
                    onclick: move |_| {
                        bridge.send(SettingsSnapshot {
                            ui: models::AppUiSettings::default(),
                            sql: models::SqlFormatSettings::default(),
                        });
                        window().close();
                    },
                    "Save"
                }
            }
        }
    }
}
