//! Shared settings-form widgets. Section fill (Task 8/9) consumes these.

use crate::app_state::keyboard::combo_from_event;
use dioxus::prelude::*;
use models::parse_hex_color;

pub fn clamp_u32(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

#[component]
#[allow(dead_code)] // Appearance/Grid sections (Task 8) consume this widget
pub fn ColorField(label: String, value: String, on_change: EventHandler<String>) -> Element {
    rsx! {
        div { class: "field",
            span { class: "field__label", {label} }
            input {
                r#type: "color",
                value: value.clone(),
                oninput: move |event| {
                    if let Some(hex) = parse_hex_color(&event.value()) {
                        on_change.call(hex);
                    }
                },
            }
            input {
                class: "input",
                value,
                oninput: move |event| {
                    if let Some(hex) = parse_hex_color(&event.value()) {
                        on_change.call(hex);
                    }
                },
            }
        }
    }
}

#[component]
#[allow(dead_code)] // Appearance section (Task 8) consumes this widget
pub fn FontSelect(
    label: String,
    value: String,
    options: Vec<(String, String)>,
    on_change: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "field",
            span { class: "field__label", {label} }
            select {
                class: "input",
                value: value.clone(),
                style: "font-family: {value}",
                oninput: move |event| {
                    on_change.call(event.value());
                },
                for (family, option_label) in options.iter().cloned() {
                    option {
                        key: "{family}",
                        value: family.clone(),
                        style: "font-family: {family}",
                        {option_label}
                    }
                }
            }
        }
    }
}

#[component]
#[allow(dead_code)] // Appearance/Editor/Grid sections (Task 8) consume this widget
pub fn SliderField(
    label: String,
    value: u32,
    min: u32,
    max: u32,
    on_change: EventHandler<u32>,
) -> Element {
    rsx! {
        div { class: "field",
            span { class: "field__label", {label} }
            div { class: "settings-modal__slider",
                input {
                    r#type: "range",
                    min: "{min}",
                    max: "{max}",
                    value: "{value}",
                    oninput: move |event| {
                        if let Ok(parsed) = event.value().parse::<u32>() {
                            on_change.call(clamp_u32(parsed, min, max));
                        }
                    },
                }
                input {
                    class: "input",
                    r#type: "number",
                    min: "{min}",
                    max: "{max}",
                    value: "{value}",
                    oninput: move |event| {
                        if let Ok(parsed) = event.value().parse::<u32>() {
                            on_change.call(clamp_u32(parsed, min, max));
                        }
                    },
                }
            }
        }
    }
}

#[component]
#[allow(dead_code)] // Keyboard section (Task 9) consumes this widget
pub fn KeyCapture(
    current: String,
    on_change: EventHandler<String>,
    on_clear: EventHandler<()>,
) -> Element {
    let mut listening = use_signal(|| false);
    let button_label = if listening() {
        "Press keys…".to_string()
    } else {
        current.clone()
    };

    rsx! {
        button {
            class: "button button--ghost button--small",
            r#type: "button",
            tabindex: "0",
            onclick: move |_| {
                listening.set(true);
            },
            onkeydown: move |event: KeyboardEvent| {
                if !listening() {
                    return;
                }
                event.prevent_default();
                match event.key() {
                    Key::Escape => {
                        listening.set(false);
                    }
                    Key::Backspace => {
                        on_clear.call(());
                    }
                    key => {
                        if let Some(combo) = combo_from_event(&key, event.modifiers()) {
                            on_change.call(combo);
                            listening.set(false);
                        }
                    }
                }
            },
            {button_label}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_clamp_rejects_out_of_range() {
        assert_eq!(clamp_u32(5, 10, 16), 10);
        assert_eq!(clamp_u32(20, 10, 16), 16);
        assert_eq!(clamp_u32(12, 10, 16), 12);
    }
}
