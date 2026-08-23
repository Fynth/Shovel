//! Reusable tooltip wiring for non-icon elements.
//!
//! Most icon buttons get their tooltip for free from
//! [`crate::screens::workspace::components::icon_button::IconButton`].
//! This module provides the same UX for plain text buttons, tree nodes,
//! result cells, etc. without forcing every consumer to wire the
//! hover/move/leave/blur handlers manually.
//!
//! Usage:
//! ```ignore
//! rsx! {
//!     TooltipTarget { label: "Reset UI to defaults",
//!         button { class: "button button--ghost", onclick: ..., "Reset UI" }
//!     }
//! }
//! ```
//!
//! The wrapper element is intentionally a `<span>` (inline-flex) so it
//! doesn't disturb flex layouts; the actual hovered element receives the
//! events through bubbling.

use crate::app_state::{hide_tooltip, show_tooltip};
use dioxus::prelude::*;

/// Wrap any inline content with hover/focus tooltip behavior.
///
/// The tooltip itself is rendered once at the document level (see
/// `app.rs`); this component only feeds the [`crate::app_state::APP_TOOLTIP`]
/// global signal. The fade-in + cursor offset lives in `.app__tooltip`.
#[component]
pub fn TooltipTarget(label: String, children: Element) -> Element {
    let label_enter = label.clone();
    let label_move = label.clone();

    rsx! {
        span {
            class: "tooltip-target",
            tabindex: "-1",
            onmouseenter: move |event| {
                let position = event.client_coordinates();
                show_tooltip(label_enter.clone(), position.x, position.y);
            },
            onmousemove: move |event| {
                let position = event.client_coordinates();
                show_tooltip(label_move.clone(), position.x, position.y);
            },
            onmouseleave: move |_| hide_tooltip(),
            onmousedown: move |_| hide_tooltip(),
            onblur: move |_| hide_tooltip(),
            {children}
        }
    }
}
