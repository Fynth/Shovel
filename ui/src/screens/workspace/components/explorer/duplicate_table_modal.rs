use super::{quote_sql_identifier, quoted_table_name_preview};
use crate::screens::workspace::actions::read_only_mode_block_status;
use dioxus::prelude::*;
use models::{DatabaseConnection, DatabaseKind, TablePreviewSource};

/// See `create_table_modal::ModalConnection` — same rationale, satisfies
/// the `#[component]` props derive for the duplicate window.
#[derive(Clone)]
pub struct ModalConnection(pub Option<DatabaseConnection>);

impl PartialEq for ModalConnection {
    fn eq(&self, other: &Self) -> bool {
        self.0.is_some() == other.0.is_some()
    }
}

#[derive(Clone, PartialEq)]
pub struct DuplicateTableTarget {
    pub session_id: u64,
    pub connection_name: String,
    pub kind: DatabaseKind,
    pub source: TablePreviewSource,
}

#[derive(Clone, PartialEq)]
struct DuplicateTableDraft {
    table_name: String,
    copy_data: bool,
}

#[component]
pub fn DuplicateTableModal(
    target: DuplicateTableTarget,
    session: ModalConnection,
    read_only: bool,
    on_saved: Callback<String>,
    on_close: Callback<()>,
) -> Element {
    let mut draft = use_signal(|| default_duplicate_table_draft(&target));
    let mut duplicate_error = use_signal(String::new);
    let mut duplicate_inflight = use_signal(|| false);
    let current_draft = draft();
    let read_only_mode = read_only;
    let can_submit = duplicate_table_form_valid(&target, &current_draft)
        && !duplicate_inflight()
        && !read_only_mode;
    let preview_sql = duplicate_table_preview_sql(&target, &current_draft);

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| {
                if !duplicate_inflight() {
                    on_close(());
                }
            },
            div {
                class: "settings-modal table-modal",
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "table-modal__body",
                    div {
                        class: "table-modal__grid",
                        div {
                            class: "field",
                            span { class: "field__label", "Source table" }
                            input {
                                class: "input",
                                value: target.source.qualified_name.clone(),
                                readonly: true,
                            }
                        }
                        div {
                            class: "field",
                            span { class: "field__label", "New table name" }
                            input {
                                class: "input",
                                value: current_draft.table_name.clone(),
                                placeholder: "products_copy",
                                oninput: move |event| {
                                    let value = event.value();
                                    draft.with_mut(|draft| draft.table_name = value);
                                },
                            }
                        }
                    }

                    div {
                        class: "table-modal__section",
                        label {
                            class: "settings-modal__toggle",
                            input {
                                r#type: "checkbox",
                                checked: current_draft.copy_data,
                                oninput: move |event| {
                                    let checked = event.checked();
                                    draft.with_mut(|draft| draft.copy_data = checked);
                                },
                            }
                            span { "Copy existing rows into the duplicated table" }
                        }
                        p {
                            class: "table-modal__hint table-modal__hint--boxed",
                            match target.kind {
                                DatabaseKind::Sqlite => {
                                    "SQLite duplicates the table definition and can optionally copy all rows. Indexes and triggers are not copied."
                                }
                                DatabaseKind::Postgres => {
                                    "PostgreSQL duplicates the table with LIKE INCLUDING ALL and can optionally copy all rows."
                                }
                                DatabaseKind::MySql => {
                                    "MySQL duplicates the table with CREATE TABLE LIKE and can optionally copy all rows."
                                }
                                DatabaseKind::ClickHouse => {
                                    "ClickHouse duplicates the CREATE TABLE definition and can optionally copy all rows with INSERT SELECT."
                                }
                            }
                        }
                    }

                    div {
                        class: "table-modal__preview",
                        span { class: "field__label", "Preview" }
                        pre {
                            class: "table-modal__preview-sql",
                            "{preview_sql}"
                        }
                    }

                    if !duplicate_error().is_empty() {
                        p {
                            class: "table-modal__error",
                            "{duplicate_error}"
                        }
                    }

                    div {
                        class: "table-modal__actions",
                        button {
                            class: "button button--ghost",
                            disabled: duplicate_inflight(),
                            onclick: move |_| on_close(()),
                            "Cancel"
                        }
                        button {
                            class: "button button--primary",
                            disabled: !can_submit,
                            onclick: move |_| {
                                if duplicate_inflight() {
                                    return;
                                }
                                if read_only_mode {
                                    duplicate_error.set(read_only_mode_block_status("table duplication"));
                                    return;
                                }

                                let draft_value = draft();
                                let source = target.source.clone();
                                let target_kind = target.kind;
                                let next_table_name = draft_value.table_name.trim().to_string();
                                if next_table_name.is_empty() {
                                    duplicate_error.set("Enter a new table name.".to_string());
                                    return;
                                }
                                if next_table_name == source.table_name {
                                    duplicate_error.set(
                                        "New table name must be different from the source table."
                                            .to_string(),
                                    );
                                    return;
                                }

                                let Some(connection) = session.0.clone() else {
                                    duplicate_error.set(
                                        "The connection was closed before the table could be duplicated."
                                            .to_string(),
                                    );
                                    return;
                                };

                                duplicate_error.set(String::new());
                                duplicate_inflight.set(true);

                                spawn(async move {
                                    let result = services::duplicate_table(
                                        connection,
                                        source.clone(),
                                        next_table_name.clone(),
                                        draft_value.copy_data,
                                    )
                                    .await;

                                    duplicate_inflight.set(false);
                                    match result {
                                        Ok(()) => {
                                            on_saved(duplicated_qualified_name(
                                                &source,
                                                target_kind,
                                                &next_table_name,
                                            ));
                                        }
                                        Err(err) => {
                                            duplicate_error.set(err.to_string());
                                        }
                                    }
                                });
                            },
                            if duplicate_inflight() {
                                "Duplicating..."
                            } else {
                                "Duplicate table"
                            }
                        }
                    }
                }
            }
        }
    }
}

fn default_duplicate_table_draft(target: &DuplicateTableTarget) -> DuplicateTableDraft {
    DuplicateTableDraft {
        table_name: format!("{}_copy", target.source.table_name.trim()),
        copy_data: true,
    }
}

fn duplicate_table_form_valid(target: &DuplicateTableTarget, draft: &DuplicateTableDraft) -> bool {
    let table_name = draft.table_name.trim();
    !table_name.is_empty() && table_name != target.source.table_name.trim()
}

fn duplicate_table_preview_sql(
    target: &DuplicateTableTarget,
    draft: &DuplicateTableDraft,
) -> String {
    let table_name = draft.table_name.trim();
    if table_name.is_empty() {
        return "-- enter a new table name".to_string();
    }

    let source_name = target.source.qualified_name.trim();
    let target_name = duplicated_qualified_name(&target.source, target.kind, table_name);

    match target.kind {
        DatabaseKind::Sqlite => {
            let create_sql =
                format!("CREATE TABLE {target_name} /* definition copied from {source_name} */");
            if draft.copy_data {
                format!("{create_sql};\nINSERT INTO {target_name} SELECT * FROM {source_name};")
            } else {
                format!("{create_sql};")
            }
        }
        DatabaseKind::Postgres => {
            let create_sql =
                format!("CREATE TABLE {target_name} (LIKE {source_name} INCLUDING ALL)");
            if draft.copy_data {
                format!("{create_sql};\nINSERT INTO {target_name} SELECT * FROM {source_name};")
            } else {
                format!("{create_sql};")
            }
        }
        DatabaseKind::MySql => {
            let create_sql = format!("CREATE TABLE {target_name} LIKE {source_name}");
            if draft.copy_data {
                format!("{create_sql};\nINSERT INTO {target_name} SELECT * FROM {source_name};")
            } else {
                format!("{create_sql};")
            }
        }
        DatabaseKind::ClickHouse => {
            let create_sql =
                format!("CREATE TABLE {target_name} /* definition copied from {source_name} */");
            if draft.copy_data {
                format!("{create_sql};\nINSERT INTO {target_name} SELECT * FROM {source_name};")
            } else {
                format!("{create_sql};")
            }
        }
    }
}

fn duplicated_qualified_name(
    source: &TablePreviewSource,
    kind: DatabaseKind,
    table_name: &str,
) -> String {
    match kind {
        DatabaseKind::Sqlite => quote_sql_identifier(table_name.trim()),
        DatabaseKind::Postgres | DatabaseKind::MySql | DatabaseKind::ClickHouse => {
            quoted_table_name_preview(kind, source.schema.as_deref(), table_name.trim())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::duplicated_qualified_name;
    use models::{DatabaseKind, TablePreviewSource};

    #[test]
    fn duplicate_target_name_matches_explorer_qualified_name_format() {
        let sqlite_source = TablePreviewSource {
            schema: Some("main".to_string()),
            table_name: "products".to_string(),
            qualified_name: r#""products""#.to_string(),
        };
        let postgres_source = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: "products".to_string(),
            qualified_name: r#""public"."products""#.to_string(),
        };
        let mysql_source = TablePreviewSource {
            schema: Some("app".to_string()),
            table_name: "products".to_string(),
            qualified_name: "`app`.`products`".to_string(),
        };

        assert_eq!(
            duplicated_qualified_name(&sqlite_source, DatabaseKind::Sqlite, "products_copy"),
            r#""products_copy""#
        );
        assert_eq!(
            duplicated_qualified_name(&postgres_source, DatabaseKind::Postgres, "products_copy"),
            r#""public"."products_copy""#
        );
        assert_eq!(
            duplicated_qualified_name(&mysql_source, DatabaseKind::MySql, "products_copy"),
            "`app`.`products_copy`"
        );
    }
}
