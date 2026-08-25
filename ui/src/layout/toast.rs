use crate::app_state::{APP_TOAST, AppToast, ToastKind, dismiss_toast};
use dioxus::prelude::*;

#[component]
pub fn ToastContainer() -> Element {
    let toasts = APP_TOAST();
    if toasts.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            class: "toast-container",
            for toast in toasts {
                ToastItem { key: "{toast.id}", toast }
            }
        }
    }
}

#[component]
fn ToastItem(toast: AppToast) -> Element {
    let class = match toast.kind {
        ToastKind::Info => "toast toast--info",
        ToastKind::Success => "toast toast--success",
        ToastKind::Warning => "toast toast--warning",
        ToastKind::Error => "toast toast--error",
    };
    let icon = match toast.kind {
        ToastKind::Info => "ℹ",
        ToastKind::Success => "✓",
        ToastKind::Warning => "⚠",
        ToastKind::Error => "✕",
    };
    rsx! {
        div {
            class: class.to_string(),
            div {
                class: "toast__icon",
                {icon.to_string()}
            }
            div {
                class: "toast__message",
                "{toast.message}"
            }
            button {
                class: "toast__close",
                onclick: move |_| dismiss_toast(toast.id),
                "×"
            }
        }
    }
}
