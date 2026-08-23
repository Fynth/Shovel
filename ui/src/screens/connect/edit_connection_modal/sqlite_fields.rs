use dioxus::prelude::*;
use rfd::AsyncFileDialog;

#[component]
pub fn SqliteEditorFields(mut path: Signal<String>, disabled: bool) -> Element {
    rsx! {
        div {
            class: "field",
            label {
                class: "field__label",
                r#for: "edit-sqlite-path",
                "SQLite file path"
            }
            div {
                class: "connect-form__path-row",
                input {
                    class: "input connect-form__path-input",
                    id: "edit-sqlite-path",
                    value: "{path}",
                    placeholder: "/path/to/app.db",
                    disabled,
                    oninput: move |event| path.set(event.value()),
                }
                button {
                    class: "button button--ghost",
                    r#type: "button",
                    disabled,
                    onclick: move |_| {
                        spawn(async move {
                            let file = AsyncFileDialog::new()
                                .add_filter("SQLite database", &["db", "sqlite", "sqlite3", "db3"])
                                .add_filter("All files", &["*"])
                                .pick_file()
                                .await;

                            if let Some(file) = file {
                                path.set(file.path().display().to_string());
                            }
                        });
                    },
                    "Browse"
                }
            }
        }
    }
}
