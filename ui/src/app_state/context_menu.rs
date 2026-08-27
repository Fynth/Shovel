//! Global state and helpers for the context-menu overlay.
//!
//! The context menu is opened by any UI element that wants to expose
//! right-click actions. The menu itself is rendered by
//! [`crate::components::context_menu::ContextMenu`], which is mounted
//! once at the top of the application tree (see
//! [`crate::app`]) and reads [`CONTEXT_MENU`].
//!
//! We deliberately keep the menu data in a single [`GlobalSignal`] so
//! that any component can open the menu without prop-drilling a
//! `Signal<Option<...>>` through deep trees. Closing is done either
//! by [`close_context_menu`] (programmatic) or by the menu component
//! itself when the user clicks outside / presses Escape / scrolls.
//!
//! Click handlers are stored separately in a thread-local
//! [`CallbackId`] -> closure table, so that the menu items in the
//! global signal remain `Send`-friendly plain data (label, icon,
//! danger, disabled, separator, callback id).

use dioxus::prelude::*;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::screens::workspace::ActionIcon;

/// Stable identifier for a registered callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallbackId(pub u64);

impl CallbackId {
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

type ClipboardResult = Result<(), String>;

// Registry of menu click handlers. Stored in a thread-local so that
// callbacks can close over Dioxus signals (which are not `Send`).
thread_local! {
    static CALLBACKS: std::cell::RefCell<HashMap<CallbackId, Box<dyn FnMut()>>> =
        std::cell::RefCell::new(HashMap::new());
}

// Persistent clipboard handle for the menu. We keep our own
// `thread_local` rather than reaching into `agent_panel` to keep
// the menu self-contained.
thread_local! {
    static PERSISTENT_CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> =
        const { std::cell::RefCell::new(None) };
}

/// A single menu entry. Plain data, no `Fn`-trait, so the global
/// signal can hold it across threads.
#[derive(Clone, PartialEq)]
pub struct ContextMenuItem {
    pub label: String,
    pub icon: Option<ActionIcon>,
    pub danger: bool,
    pub disabled: bool,
    pub separator_before: bool,
    /// When `true`, the overlay draws a checkmark indicator next to the label.
    pub active: bool,
    pub callback: CallbackId,
}

impl ContextMenuItem {
    pub fn new(label: impl Into<String>, callback: impl FnMut() + 'static) -> Self {
        let id = CallbackId::next();
        CALLBACKS.with(|cell| {
            cell.borrow_mut().insert(id, Box::new(callback));
        });
        Self {
            label: label.into(),
            icon: None,
            danger: false,
            disabled: false,
            separator_before: false,
            active: false,
            callback: id,
        }
    }

    pub fn with_icon(mut self, icon: ActionIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn separator(mut self) -> Self {
        self.separator_before = true;
        self
    }

    /// Mark the item as currently "on" so the overlay renders a checkmark indicator.
    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }
}

/// Snapshot of the menu that is currently visible.
#[derive(Clone, PartialEq)]
pub struct ContextMenuState {
    pub x: f64,
    pub y: f64,
    pub items: Vec<ContextMenuItem>,
}

pub static CONTEXT_MENU: GlobalSignal<Option<ContextMenuState>> = Signal::global(|| None);

/// Open a context menu at the given screen coordinates.
pub fn open_context_menu(x: f64, y: f64, items: Vec<ContextMenuItem>) {
    *CONTEXT_MENU.write() = Some(ContextMenuState { x, y, items });
}

/// In-app confirm overlay. Native `rfd` dialogs often return `No`
/// immediately on Wayland/Hyprland without showing a window, so
/// destructive explorer actions (drop/truncate) go through this instead.
#[derive(Clone, PartialEq)]
pub struct ConfirmDialogState {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub danger: bool,
    pub callback: CallbackId,
}

pub static CONFIRM_DIALOG: GlobalSignal<Option<ConfirmDialogState>> = Signal::global(|| None);

pub fn open_confirm_dialog(
    title: impl Into<String>,
    message: impl Into<String>,
    confirm_label: impl Into<String>,
    danger: bool,
    callback: impl FnOnce() + 'static,
) {
    let id = CallbackId::next();
    let mut callback = Some(callback);
    CALLBACKS.with(|cell| {
        cell.borrow_mut().insert(
            id,
            Box::new(move || {
                if let Some(cb) = callback.take() {
                    cb();
                }
            }),
        );
    });
    *CONFIRM_DIALOG.write() = Some(ConfirmDialogState {
        title: title.into(),
        message: message.into(),
        confirm_label: confirm_label.into(),
        danger,
        callback: id,
    });
}

pub fn close_confirm_dialog() {
    if CONFIRM_DIALOG().is_some() {
        *CONFIRM_DIALOG.write() = None;
    }
}

/// Close the menu, if one is open. Safe to call from anywhere.
pub fn close_context_menu() {
    if CONTEXT_MENU().is_some() {
        *CONTEXT_MENU.write() = None;
    }
}

/// Invoke the callback registered under the given id. Panics only if
/// the id is unknown — which would mean a programming error (the
/// callback was registered as part of building the menu items, so
/// any id that appears in a menu must be in the table).
pub fn invoke_callback(id: CallbackId) {
    let mut taken: Option<Box<dyn FnMut()>> = None;
    CALLBACKS.with(|cell| {
        taken = cell.borrow_mut().remove(&id);
    });
    let Some(mut cb) = taken else {
        eprintln!("context_menu: no callback registered for {id:?}");
        crate::app_state::toast_error(
            "That menu action is no longer available. Right-click again.",
        );
        return;
    };
    cb();
}

/// Copy `text` to the system clipboard. Uses the same thread-local
/// `arboard::Clipboard` pattern as `agent_panel::messages` so the
/// handle does not get dropped between calls on Linux/Wayland.
///
/// On failure, falls back to a browser-style `navigator.clipboard`
/// write via `dioxus::document::eval`. (This is a no-op in the
/// desktop app's webview when `arboard` is available, but provides
/// a graceful degradation.)
pub fn copy_to_clipboard(text: String) -> ClipboardResult {
    let result = PERSISTENT_CLIPBOARD.with(|cell| {
        let mut clipboard = cell.borrow_mut();
        if clipboard.is_none() {
            *clipboard = Some(arboard::Clipboard::new().map_err(|err| err.to_string())?);
        }
        let clipboard = clipboard.as_mut().expect("just-initialized");
        clipboard
            .set_text(text.clone())
            .map_err(|err| err.to_string())?;
        Ok::<(), String>(())
    });

    if result.is_ok() {
        return Ok(());
    }

    // Native clipboard failed — try the webview clipboard as a
    // last-ditch fallback. We do not consider the desktop app's
    // own webview, but this keeps the door open for environments
    // where arboard is unavailable (e.g. WSL without X, some CI
    // runners).
    let script = match serde_json::to_string(&text) {
        Ok(value) => format!(
            r#"(async () => {{
                try {{
                    await navigator.clipboard.writeText({value});
                    return true;
                }} catch (err) {{
                    return String(err);
                }}
            }})()"#
        ),
        Err(err) => {
            return Err(format!(
                "native clipboard failed and JSON encode failed: {err}"
            ));
        }
    };
    let _ = document::eval(&script);

    Err(result
        .err()
        .unwrap_or_else(|| "clipboard unavailable".to_string()))
}

/// Clamp the menu's top-left corner to the viewport so it does not
/// overflow. The caller is expected to have measured the menu
/// element after it has been laid out, then to feed the size back
/// via this function. Since we cannot easily measure the rendered
/// menu from inside the render tree, we use conservative defaults
/// (a max width of 320 and a max height of 480) and let the browser
/// / webview handle the rest via `max-width` / `max-height` in CSS.
pub fn clamp_to_viewport(x: f64, y: f64, menu_w: f64, menu_h: f64, vw: f64, vh: f64) -> (f64, f64) {
    let pad = 4.0;
    let max_x = (vw - menu_w - pad).max(pad);
    let max_y = (vh - menu_h - pad).max(pad);
    (x.clamp(pad, max_x), y.clamp(pad, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_ids_are_unique() {
        let a = CallbackId::next();
        let b = CallbackId::next();
        assert_ne!(a, b);
    }

    #[test]
    fn invoke_callback_can_register_a_follow_up_callback() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let nested_ran = Arc::new(AtomicBool::new(false));
        let nested_flag = nested_ran.clone();
        let id = CallbackId::next();
        CALLBACKS.with(|cell| {
            cell.borrow_mut().insert(
                id,
                Box::new(move || {
                    let nested_id = CallbackId::next();
                    let nested_flag = nested_flag.clone();
                    CALLBACKS.with(|cell| {
                        cell.borrow_mut().insert(
                            nested_id,
                            Box::new(move || {
                                nested_flag.store(true, Ordering::SeqCst);
                            }),
                        );
                    });
                    invoke_callback(nested_id);
                }),
            );
        });
        invoke_callback(id);
        assert!(
            nested_ran.load(Ordering::SeqCst),
            "nested confirm-style callback must run without panicking"
        );
    }

    #[test]
    fn clamp_inside_viewport_is_unchanged() {
        let (x, y) = clamp_to_viewport(100.0, 100.0, 200.0, 200.0, 1280.0, 800.0);
        assert_eq!((x, y), (100.0, 100.0));
    }

    #[test]
    fn clamp_right_overflow_pulls_back() {
        let (x, _) = clamp_to_viewport(2000.0, 100.0, 200.0, 200.0, 1280.0, 800.0);
        assert!(x < 2000.0);
        assert!(x > 0.0);
    }

    #[test]
    fn clamp_bottom_overflow_pulls_back() {
        let (_, y) = clamp_to_viewport(100.0, 2000.0, 200.0, 200.0, 1280.0, 800.0);
        assert!(y < 2000.0);
        assert!(y > 0.0);
    }

    #[test]
    fn clamp_negative_origin_is_pushed_inside() {
        let (x, y) = clamp_to_viewport(-50.0, -50.0, 200.0, 200.0, 1280.0, 800.0);
        assert!(x >= 0.0);
        assert!(y >= 0.0);
    }

    #[test]
    fn clamp_handles_tiny_viewport() {
        let (x, y) = clamp_to_viewport(50.0, 50.0, 500.0, 500.0, 100.0, 100.0);
        // When the menu is larger than the viewport, the clamp
        // function falls back to a small positive offset rather
        // than producing negative coordinates.
        assert!(x >= 0.0);
        assert!(y >= 0.0);
    }
}
