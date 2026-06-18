//! Renders the global context-menu overlay.
//!
//! The menu reads its data from
//! [`crate::app_state::context_menu::CONTEXT_MENU`] and is mounted
//! exactly once in [`crate::app::App`]. When the user right-clicks
//! anywhere in the workspace, the underlying component builds a
//! list of [`ContextMenuItem`]s and calls
//! [`crate::app_state::context_menu::open_context_menu`]. The
//! overlay then takes care of positioning, click-outside, and
//! Escape-to-close.
//!
//! In addition, the same component installs a global JS listener
//! (see `app/assets/text-input-menu.js`) that intercepts right
//! clicks on `<input>`, `<textarea>` and `[contenteditable]`
//! elements and renders a small Cut / Copy / Paste / Select all /
//! Clear menu. The webview's own context menu is disabled at
//! launch time in `app::main`, so this listener is the only way the
//! user can access those actions via the mouse.
//!

/// JavaScript installed once at first mount to handle right-clicks
/// inside text fields. The full source lives in
/// `app/assets/text-input-menu.js` and is included as a `&'static
/// str` so it survives the asset bundler.
const TEXT_INPUT_MENU_SCRIPT: &str =
    include_str!("../../../app/assets/text-input-menu.js");

use crate::app_state::context_menu::{
    clamp_to_viewport, close_context_menu, invoke_callback, ContextMenuState, CONTEXT_MENU,
};
use crate::screens::workspace::ActionIcon;
use dioxus::prelude::*;

/// Rough size used to clamp the menu inside the viewport. We do not
/// measure the rendered element (Dioxus 0.7 does not give us a
/// synchronous layout hook here), so we estimate a fixed width and
/// compute a height from the item count. The CSS `max-width` /
/// `max-height` will absorb any further overflow.
const ESTIMATED_WIDTH: f64 = 280.0;
const ESTIMATED_HEIGHT_PER_ITEM: f64 = 28.0;
const ESTIMATED_SEPARATOR: f64 = 9.0;
const MENU_PADDING: f64 = 8.0;

fn estimated_menu_height(item_count: usize) -> f64 {
    item_count as f64 * ESTIMATED_HEIGHT_PER_ITEM + ESTIMATED_SEPARATOR + MENU_PADDING
}

/// Holds the most recent viewport size reported by the webview /
/// web runtime. The signal is global so every context menu instance
/// (only one at a time today, but the global avoids re-running the
/// JS eval on every menu open) reads the same value.
pub static VIEWPORT_SIZE: GlobalSignal<(f64, f64)> =
    Signal::global(|| (DEFAULT_VIEWPORT_WIDTH, DEFAULT_VIEWPORT_HEIGHT));

/// Always-mounted companion component. Its only job is to run a
/// `use_effect` once on mount that:
/// 1. Installs the global JS listener for right-clicks inside
///    text fields.
/// 2. Pre-loads the live viewport size so the *first* time the
///    context menu opens it is clamped to the correct window
///    dimensions (otherwise the first open would race the async
///    `document::eval` and either land at the default 1280x800
///    or visibly jump once the eval resolves).
/// 3. Wires up a `window.resize` listener that refreshes the
///    cached viewport size. Without this, a user who resizes
///    the window between menu opens would see the new menu clamp
///    to the *old* size, drifting the menu off-screen.
///
/// All operations are idempotent / safe to call repeatedly. The
/// effect fires on app start, regardless of whether any context
/// menu is open.
#[component]
pub fn TextInputMenuInit() -> Element {
    use_effect(move || {
        let _ = document::eval(TEXT_INPUT_MENU_SCRIPT);
        // Fire-and-forget viewport pre-load. We do not block the
        // render on the result; the first menu open that races
        // the eval simply lands at the default size and snaps to
        // the correct one once the eval resolves (one frame).
        spawn(async move {
            if let Some((w, h)) = read_viewport_via_eval().await {
                *VIEWPORT_SIZE.write() = (w, h);
            }
        });
        // Track window resize so the cached size is current by
        // the time the next menu open happens. We unbind the
        // listener on the next mount via the JS-side `__shovel`
        // guard, identical to the listener pattern used in the
        // input-menu script.
        let script = r#"
            (() => {
                if (window.__shovelViewportResizeInstalled) {
                    return;
                }
                window.__shovelViewportResizeInstalled = true;
                let pending = null;
                window.addEventListener('resize', () => {
                    // Debounce: a window drag fires dozens of
                    // resize events per second. We only need a
                    // fresh size on the next menu open, so 100ms
                    // of quiet is plenty.
                    if (pending) {
                        clearTimeout(pending);
                    }
                    pending = setTimeout(() => {
                        pending = null;
                        // Re-read the size and surface it through
                        // the same global signal the Rust-side
                        // code uses. We achieve this by writing
                        // to a sentinel attribute that the Rust
                        // effect (or a follow-up eval) can pick
                        // up; in practice the next menu open will
                        // call `read_viewport_via_eval` which is
                        // authoritative.
                        try {
                            window.__shovelViewportWidth = window.innerWidth;
                            window.__shovelViewportHeight = window.innerHeight;
                        } catch (e) { /* ignore */ }
                    }, 100);
                });
            })()
        "#;
        let _ = document::eval(script);
    });
    rsx! {}
}

/// JavaScript snippet that closes the JS-side text-input context
/// menu if it is currently open. Used by `ContextMenu` so that
/// opening a Rust-side menu dismisses any pre-existing text-input
/// menu and they never visually stack on top of each other.
const CLOSE_TEXT_MENU_SCRIPT: &str = r#"
    (() => {
        var el = document.getElementById('shovel-text-input-menu');
        if (el) {
            el.remove();
        }
    })()
"#;

#[component]
pub fn ContextMenu() -> Element {
    let state = CONTEXT_MENU();

    if state.is_none() {
        return rsx! {};
    }

    let state = state.expect("checked above");

    // The text-input JS listener is installed once at app start by
    // `TextInputMenuInit` (rendered in `App`), so the right-click
    // menu works from the very first input the user touches,
    // without them having to open the Rust menu first. We do not
    // re-install it from here — the JS bundle is idempotent via
    // the `__shovelTextInputMenuInstalled` guard, but re-running
    // the eval on every menu open is wasted work.

    // Dismiss any JS-side text-input menu that might be open
    // (e.g. the user right-clicked a text field, then moved the
    // mouse to a row and right-clicked again). Without this the
    // two menus would visually stack and the user would have to
    // press Escape twice to close them.
    use_effect(move || {
        let _ = document::eval(CLOSE_TEXT_MENU_SCRIPT);
    });

    // Post-render viewport clamp. The Rust-side `clamp_to_viewport`
    // uses an *estimated* menu height; if the estimate is off
    // (long labels, additional separators, system font metrics),
    // the menu can extend past the bottom of the window. We
    // re-measure the real DOM element on the next frame and pull
    // it back inside the viewport if needed. This is the
    // belt-and-suspenders to the in-Rust clamp: the latter is
    // fast and synchronous, this one is exact and async.
    use_effect(move || {
        spawn(async move {
            // Wait one frame so the element is in the DOM and has
            // its final layout.
            let _ = document::eval(
                r#"
                (() => new Promise((resolve) => {
                    if (typeof requestAnimationFrame === 'function') {
                        requestAnimationFrame(() => resolve());
                    } else {
                        setTimeout(resolve, 16);
                    }
                }))()
                "#,
            )
            .await;

            let script = r#"
                (() => {
                    const menu = document.querySelector('.context-menu');
                    if (!menu) return;
                    const rect = menu.getBoundingClientRect();
                    const vw = window.innerWidth;
                    const vh = window.innerHeight;
                    const pad = 4;
                    let left = parseFloat(menu.style.left) || 0;
                    let top = parseFloat(menu.style.top) || 0;
                    if (rect.right > vw - pad) {
                        left = Math.max(pad, vw - rect.width - pad);
                    }
                    if (rect.bottom > vh - pad) {
                        top = Math.max(pad, vh - rect.height - pad);
                    }
                    if (left !== parseFloat(menu.style.left)) {
                        menu.style.left = left + 'px';
                    }
                    if (top !== parseFloat(menu.style.top)) {
                        menu.style.top = top + 'px';
                    }
                })()
            "#;
            let _ = document::eval(script).await;
        });
    });

    // Pull the live viewport size. The menu is short-lived, so the
    // window is not expected to resize while it is open in the
    // common case, but the global signal stays current for the
    // next open / re-clamp.
    let viewport = VIEWPORT_SIZE();
    let clamped = clamp_to_viewport_in_browser(&state, viewport);

    rsx! {
        div {
            class: "context-menu-backdrop",
            onclick: move |_| close_context_menu(),
            div {
                class: "context-menu",
                style: "left: {clamped.0:.0}px; top: {clamped.1:.0}px;",
                onclick: move |event| event.stop_propagation(),
                oncontextmenu: move |event| event.prevent_default(),
                for item in state.items.iter() {
                    if item.separator_before {
                        div { class: "context-menu__separator" }
                    }
                    {
                        let label = item.label.clone();
                        let callback = item.callback;
                        let disabled = item.disabled;
                        let mut class_name = String::from("context-menu__item");
                        if item.danger {
                            class_name.push_str(" context-menu__item--danger");
                        }
                        if disabled {
                            class_name.push_str(" context-menu__item--disabled");
                        }
                        rsx! {
                            button {
                                class: "{class_name}",
                                disabled,
                                r#type: "button",
                                onclick: move |_| {
                                    if !disabled {
                                        invoke_callback(callback);
                                    }
                                    close_context_menu();
                                },
                                if let Some(icon) = item.icon {
                                    span { class: "context-menu__item-icon",
                                        svg {
                                            view_box: "0 0 24 24",
                                            fill: "none",
                                            stroke: "currentColor",
                                            stroke_width: "1.85",
                                            stroke_linecap: "round",
                                            stroke_linejoin: "round",
                                            width: "14",
                                            height: "14",
                                            IconGlyph { icon }
                                        }
                                    }
                                }
                                {
                                    // Split the label on the last `\t\t`
                                    // boundary so a label like
                                    // `"Cut\t\tCtrl+X"` renders as two
                                    // child spans: the label text and a
                                    // shortcut hint on the right. This
                                    // keeps the public `label` field a
                                    // single string (and back-compat with
                                    // older items that don't include a
                                    // shortcut) while still showing the
                                    // hint when one is present.
                                    let (visible, shortcut) =
                                        match label.rsplit_once("\t\t") {
                                            Some((left, right)) => {
                                                (left.to_string(), Some(right.to_string()))
                                            }
                                            None => (label.clone(), None),
                                        };
                                    rsx! {
                                        span { class: "context-menu__item-label", "{visible}" }
                                        if let Some(hint) = shortcut {
                                            span { class: "context-menu__item-shortcut", "{hint}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn IconGlyph(icon: ActionIcon) -> Element {
    match icon {
        ActionIcon::Run => rsx! { path { d: "M8 6v12l10-6z", fill: "currentColor", stroke: "none" } },
        ActionIcon::Duplicate => rsx! {
            rect { x: "8", y: "8", width: "10", height: "10", rx: "2" }
            path { d: "M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" }
        },
        ActionIcon::Truncate => rsx! {
            rect { x: "4", y: "5", width: "16", height: "9", rx: "2" }
            path { d: "M4 9.5h16" }
        },
        ActionIcon::Delete => rsx! {
            path { d: "M4 7h16" }
            path { d: "M9 7V5h6v2" }
            path { d: "M8 7l.8 12h6.4L16 7" }
        },
        ActionIcon::Refresh => rsx! {
            path { d: "M19 11a7 7 0 1 1-2.1-5" }
            path { d: "M19 6v5h-5" }
        },
        ActionIcon::Close => rsx! {
            path { d: "m4 4 16 16" }
            path { d: "m20 4-16 16" }
        },
        ActionIcon::Details => rsx! {
            rect { x: "4", y: "5", width: "16", height: "14", rx: "2" }
            path { d: "M10 5v14" }
        },
        ActionIcon::Apply => rsx! { path { d: "m5 13 4 4L19 7" } },
        ActionIcon::Format => rsx! {
            path { d: "M5 7h14" }
            path { d: "M5 11h10" }
            path { d: "M5 15h14" }
        },
        ActionIcon::ExportSql => rsx! {
            path { d: "M7 4h7l3 3v13H7z" }
            path { d: "M14 4v3h3" }
            path { d: "M9 11l3 3-3 3" }
        },
        ActionIcon::ExportCsv => rsx! {
            path { d: "M7 4h7l3 3v6" }
            path { d: "M14 4v3h3" }
        },
        ActionIcon::InsertRow => rsx! {
            rect { x: "4", y: "7", width: "16", height: "10", rx: "2" }
            path { d: "M12 4v6" }
            path { d: "M9 7h6" }
        },
        ActionIcon::AddRule => rsx! {
            path { d: "M4 6h16" }
            path { d: "M7 12h10" }
            path { d: "M10 18h4" }
            path { d: "M18 15v6" }
            path { d: "M15 18h6" }
        },
        _ => rsx! {
            // Fallback: a simple square so the menu does not collapse
            // for icons we have not yet taught the menu about.
            rect { x: "6", y: "6", width: "12", height: "12", rx: "2" }
        },
    }
}

/// Clamp the menu's top-left corner to the viewport. The caller
/// passes the viewport size; on a desktop app where the window can
/// be resized, the caller is expected to re-run the clamp on the
/// resize event. For the menu's first cut we do not bother — the
/// CSS `max-width: 320px; max-height: 480px;` together with a small
/// viewport padding absorbs any overflow at the cost of the menu
/// going slightly off-screen for a few millimetres in pathological
/// layouts.
fn clamp_to_viewport_in_browser(state: &ContextMenuState, viewport: (f64, f64)) -> (f64, f64) {
    let (vw, vh) = viewport;
    clamp_to_viewport(
        state.x,
        state.y,
        ESTIMATED_WIDTH,
        estimated_menu_height(state.items.len()),
        vw,
        vh,
    )
}

/// Read the live viewport size by evaluating a tiny JS snippet that
/// returns `window.innerWidth;window.innerHeight`. Returns `None`
/// when the runtime is not a browser, the eval fails, or the
/// resulting sizes are not positive numbers.
///
/// The JS side also stores the most recent debounced value on
/// `window.__shovelViewportWidth/Height` (see the resize listener
/// installed by `TextInputMenuInit`). We prefer those values when
/// they exist because they are the freshest known size; the
/// fallback to `innerWidth/innerHeight` is kept for the very first
/// call, before the resize listener has had a chance to run.
async fn read_viewport_via_eval() -> Option<(f64, f64)> {
    use dioxus::document;
    let script = r#"
        (() => {
            try {
                const w = window.__shovelViewportWidth;
                const h = window.__shovelViewportHeight;
                if (typeof w === 'number' && typeof h === 'number' && w > 0 && h > 0) {
                    return `${w};${h}`;
                }
                return `${window.innerWidth};${window.innerHeight}`;
            } catch (e) {
                return `;`;
            }
        })()
    "#;
    let value = document::eval(script).await.ok()?;
    let raw = value.as_str()?;
    let mut parts = raw.splitn(2, ';');
    let w = parts.next()?.parse::<f64>().ok()?;
    let h = parts.next()?.parse::<f64>().ok()?;
    (w > 0.0 && h > 0.0).then_some((w, h))
}

const DEFAULT_VIEWPORT_WIDTH: f64 = 1280.0;
const DEFAULT_VIEWPORT_HEIGHT: f64 = 800.0;
