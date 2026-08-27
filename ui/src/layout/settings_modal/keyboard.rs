//! Keyboard shortcuts section. Stubbed empty until Task 9.

use dioxus::prelude::*;

use super::SettingsSectionProps;

/// Empty Keyboard category pane. Task 9 fills the keybinding table.
#[component]
pub(super) fn KeyboardSection(props: SettingsSectionProps) -> Element {
    let _ = props;
    rsx! {
        section {
            class: "settings-modal__section settings-modal__section--empty",
            div {
                class: "settings-modal__section-header",
                h3 { class: "settings-modal__section-title", "Keyboard" }
            }
        }
    }
}
