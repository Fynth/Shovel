//! Tooltip wiring for non-icon elements.
//!
//! [`IconButton`](crate::screens::workspace::components::icon_button::IconButton)
//! provides tooltips for icon buttons; this module offers the same
//! hover/move/leave/blur behavior for plain text buttons, tree nodes, and
//! text cells.
//!
//! The wrapper is a `<span>` (inline-flex) so it doesn't disturb flex layouts.

use crate::app_state::{hide_tooltip, show_tooltip};
use dioxus::prelude::*;

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
