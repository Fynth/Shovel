use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct TableColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub default_value: Option<String>,
    pub unique: bool,
}

#[derive(Clone, PartialEq)]
pub struct TableEditorState {
    pub schema: Option<String>,
    pub table_name: String,
    pub columns: Vec<TableColumnDefinition>,
    pub engine: Option<String>,
    pub if_not_exists: bool,
    pub mode: TableEditorMode,
}

#[derive(Clone, PartialEq)]
#[allow(dead_code)]
pub enum TableEditorMode {
    Create,
    Alter(String),
}

#[component]
pub fn TableEditor(
    mut state: Signal<TableEditorState>,
    on_save: Callback<String>,
    on_cancel: Callback<()>,
) -> Element {
    let mut new_column_name = use_signal(String::new);
    let mut new_column_type = use_signal(String::new);
    let mut new_column_nullable = use_signal(|| true);
    let mut new_column_pk = use_signal(|| false);
    let mut new_column_default = use_signal(String::new);

    let columns = state().columns;
    let table_name = state().table_name;
    let is_create_mode = matches!(state().mode, TableEditorMode::Create);

    let sql_preview = generate_table_sql(&state());

    rsx! {
        div {
            class: "table-editor",
            div {
                class: "table-editor__header",
                h2 {
                    class: "table-editor__title",
                    if is_create_mode { "Create Table" } else { "Alter Table" }
                }
                button {
                    class: "table-editor__close",
                    onclick: move |_| on_cancel.call(()),
                    "×"
                }
            }
            div {
                class: "table-editor__body",
                div {
                    class: "table-editor__form",
                    div {
                        class: "field",
                        label {
                            class: "field__label",
                            "Table Name"
                        }
                        input {
                            class: "input",
                            r#type: "text",
                            value: "{table_name}",
                            oninput: move |event| {
                                state.with_mut(|s| s.table_name = event.value());
                            },
                            placeholder: "Enter table name"
                        }
                    }
                    if is_create_mode && state().engine.is_some() {
                        div {
                            class: "field",
                            label {
                                class: "field__label",
                                "Engine (ClickHouse)"
                            }
                            input {
                                class: "input",
                                r#type: "text",
                                value: "{state().engine.as_ref().unwrap_or(&String::new())}",
                                oninput: move |event| {
                                    state.with_mut(|s| s.engine = Some(event.value()));
                                },
                                placeholder: "MergeTree"
                            }
                        }
                    }
                    div {
                        class: "field",
                        label {
                            class: "field__checkbox",
                            input {
                                r#type: "checkbox",
                                checked: "{state().if_not_exists}",
                                onchange: move |event| {
                                    state.with_mut(|s| s.if_not_exists = event.checked());
                                }
                            }
                            "IF NOT EXISTS"
                        }
                    }
                }
                div {
                    class: "table-editor__columns",
                    h3 {
                        class: "table-editor__section-title",
                        "Columns"
                    }
                    table {
                        class: "table-editor__columns-table",
                        thead {
                            tr {
                                th { "Name" }
                                th { "Type" }
                                th { "PK" }
                                th { "Nullable" }
                                th { "Default" }
                                th { "" }
                            }
                        }
                        tbody {
                            for (idx, col) in columns.iter().enumerate() {
                                tr {
                                    td {
                                        input {
                                            class: "input",
                                            value: "{col.name}",
                                            oninput: move |event| {
                                                state.with_mut(|s| {
                                                    if s.columns.get(idx).is_some() {
                                                        s.columns[idx].name = event.value();
                                                    }
                                                });
                                            }
                                        }
                                    }
                                    td {
                                        input {
                                            class: "input",
                                            value: "{col.data_type}",
                                            oninput: move |event| {
                                                state.with_mut(|s| {
                                                    if s.columns.get(idx).is_some() {
                                                        s.columns[idx].data_type = event.value();
                                                    }
                                                });
                                            }
                                        }
                                    }
                                    td {
                                        input {
                                            r#type: "checkbox",
                                            checked: "{col.is_primary_key}",
                                            onchange: move |_| {
                                                state.with_mut(|s| {
                                                    s.columns[idx].is_primary_key = !s.columns[idx].is_primary_key;
                                                });
                                            }
                                        }
                                    }
                                    td {
                                        input {
                                            r#type: "checkbox",
                                            checked: "{col.is_nullable}",
                                            onchange: move |_| {
                                                state.with_mut(|s| {
                                                    s.columns[idx].is_nullable = !s.columns[idx].is_nullable;
                                                });
                                            }
                                        }
                                    }
                                    td {
                                        input {
                                            class: "input",
                                            value: "{col.default_value.as_deref().unwrap_or(\"\")}",
                                            oninput: move |event| {
                                                state.with_mut(|s| {
                                                    let val = event.value();
                                                    s.columns[idx].default_value = if val.is_empty() { None } else { Some(val) };
                                                });
                                            }
                                        }
                                    }
                                    td {
                                        button {
                                            class: "button button--ghost button--small",
                                            onclick: move |_| {
                                                state.with_mut(|s| {
                                                    s.columns.remove(idx);
                                                });
                                            },
                                            "×"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div {
                        class: "table-editor__add-column",
                        h4 { "Add Column" }
                        div {
                            class: "table-editor__add-column-form",
                            input {
                                class: "input",
                                placeholder: "Column name",
                                value: "{new_column_name}",
                                oninput: move |event| new_column_name.set(event.value())
                            }
                            input {
                                class: "input",
                                placeholder: "Type (e.g., VARCHAR(255))",
                                value: "{new_column_type}",
                                oninput: move |event| new_column_type.set(event.value())
                            }
                            button {
                                class: "button button--small",
                                onclick: move |_| {
                                    if new_column_name().trim().is_empty() || new_column_type().trim().is_empty() {
                                        return;
                                    }
                                    state.with_mut(|s| {
                                        s.columns.push(TableColumnDefinition {
                                            name: new_column_name().trim().to_string(),
                                            data_type: new_column_type().trim().to_string(),
                                            is_nullable: new_column_nullable(),
                                            is_primary_key: new_column_pk(),
                                            default_value: if new_column_default().trim().is_empty() { None } else { Some(new_column_default().trim().to_string()) },
                                            unique: false,
                                        });
                                    });
                                    new_column_name.set(String::new());
                                    new_column_type.set(String::new());
                                    new_column_default.set(String::new());
                                    new_column_pk.set(false);
                                },
                                "Add"
                            }
                        }
                        div {
                            class: "table-editor__add-column-options",
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: "{new_column_nullable}",
                                    onchange: move |_| new_column_nullable.toggle()
                                }
                                "Nullable"
                            }
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: "{new_column_pk}",
                                    onchange: move |_| new_column_pk.toggle()
                                }
                                "Primary Key"
                            }
                        }
                    }
                }
                div {
                    class: "table-editor__preview",
                    h3 {
                        class: "table-editor__section-title",
                        "SQL Preview"
                    }
                    pre {
                        class: "table-editor__sql",
                        "{sql_preview}"
                    }
                }
            }
            div {
                class: "table-editor__footer",
                button {
                    class: "button button--ghost",
                    onclick: move |_| on_cancel.call(()),
                    "Cancel"
                }
                button {
                    class: "button button--primary",
                    onclick: move |_| on_save.call(sql_preview.clone()),
                    if is_create_mode { "Create Table" } else { "Alter Table" }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn generate_table_sql(state: &TableEditorState) -> String {
    let mut sql = String::new();

    if state.if_not_exists {
        sql.push_str("CREATE TABLE IF NOT EXISTS ");
    } else {
        sql.push_str("CREATE TABLE ");
    }

    if let Some(schema) = &state.schema {
        sql.push_str(&format!("{}.", schema));
    }
    sql.push_str(&state.table_name);
    sql.push_str(" (\n");

    let column_defs: Vec<String> = state
        .columns
        .iter()
        .map(|col| {
            let mut def = format!("  {} {}", col.name, col.data_type);
            if !col.is_nullable {
                def.push_str(" NOT NULL");
            }
            if let Some(default) = &col.default_value {
                def.push_str(&format!(" DEFAULT {}", default));
            }
            def
        })
        .collect();

    sql.push_str(&column_defs.join(",\n"));

    let pk_cols: Vec<&str> = state
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.as_str())
        .collect();

    if !pk_cols.is_empty() {
        sql.push_str(&format!(",\n  PRIMARY KEY ({})", pk_cols.join(", ")));
    }

    sql.push_str("\n)");

    if let Some(engine) = &state.engine
        && !engine.is_empty()
    {
        sql.push_str(&format!(" ENGINE = {}", engine));
    }

    sql.push(';');
    sql
}
