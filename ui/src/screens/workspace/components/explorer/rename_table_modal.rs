use super::{quote_sql_identifier, quoted_table_name_preview};
use crate::screens::workspace::actions::read_only_mode_block_status;
use dioxus::prelude::*;
use models::{DatabaseConnection, DatabaseKind, TablePreviewSource};

/// See `create_table_modal::ModalConnection` — same rationale, satisfies
/// the `#[component]` props derive for the rename window.
#[derive(Clone)]
pub struct ModalConnection(pub Option<DatabaseConnection>);

impl PartialEq for ModalConnection {
    fn eq(&self, other: &Self) -> bool {
        self.0.is_some() == other.0.is_some()
    }
}

#[derive(Clone, PartialEq)]
pub struct RenameTableTarget {
    pub session_id: u64,
    pub connection_name: String,
    pub kind: DatabaseKind,
    pub source: TablePreviewSource,
}

#[derive(Clone, PartialEq)]
struct RenameTableDraft {
    table_name: String,
}

#[component]
pub fn RenameTableModal(
    target: RenameTableTarget,
    session: ModalConnection,
    read_only: bool,
    on_saved: Callback<String>,
    on_close: Callback<()>,
) -> Element {
    let mut draft = use_signal(|| default_rename_table_draft(&target));
    let mut rename_error = use_signal(String::new);
    let mut rename_inflight = use_signal(|| false);
    let current_draft = draft();
    let can_submit =
        rename_table_form_valid(&target, &current_draft) && !rename_inflight() && !read_only;
    let preview_sql = rename_table_preview_sql(&target, &current_draft);

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| {
                if !rename_inflight() {
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
                            span { class: "field__label", "Current table" }
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
                                placeholder: "products_new",
                                oninput: move |event| {
                                    let value = event.value();
                                    draft.with_mut(|draft| draft.table_name = value);
                                },
                            }
                        }
                    }

                    div {
                        class: "table-modal__section",
                        p {
                            class: "table-modal__hint table-modal__hint--boxed",
                            match target.kind {
                                DatabaseKind::Sqlite | DatabaseKind::Postgres => {
                                    "The table is renamed with ALTER TABLE … RENAME TO. The new name applies to the whole table, including its rows and schema."
                                }
                                DatabaseKind::MySql => {
                                    "The table is renamed with RENAME TABLE … TO. The new name applies to the whole table."
                                }
                                DatabaseKind::ClickHouse => {
                                    "The table is renamed with RENAME TABLE … TO. The new name applies to the whole table."
                                }
                            }
                        }
                    }

                    div {
                        class: "table-modal__preview",
                        span { class: "field__label", "Preview" }
                        pre {
                            class: "table-modal__preview-sql",
                            {preview_sql.to_string()}
                        }
                    }

                    if !rename_error().is_empty() {
                        p {
                            class: "table-modal__error",
                            "{rename_error}"
                        }
                    }

                    div {
                        class: "table-modal__actions",
                        button {
                            class: "button button--ghost",
                            disabled: rename_inflight(),
                            onclick: move |_| on_close(()),
                            "Cancel"
                        }
                        button {
                            class: "button button--primary",
                            disabled: !can_submit,
                            onclick: move |_| {
                                if rename_inflight() {
                                    return;
                                }
                                if read_only {
                                    rename_error.set(read_only_mode_block_status("table rename"));
                                    return;
                                }

                                let source = target.source.clone();
                                let target_kind = target.kind;
                                let next_table_name = current_draft.table_name.trim().to_string();
                                if next_table_name.is_empty() {
                                    rename_error.set("Enter a new table name.".to_string());
                                    return;
                                }
                                if next_table_name == source.table_name {
                                    rename_error.set(
                                        "New table name must be different from the current table."
                                            .to_string(),
                                    );
                                    return;
                                }

                                let Some(connection) = session.0.clone() else {
                                    rename_error.set(
                                        "The connection was closed before the table could be renamed."
                                            .to_string(),
                                    );
                                    return;
                                };

                                rename_error.set(String::new());
                                rename_inflight.set(true);

                                spawn(async move {
                                    let result = services::rename_table(
                                        connection,
                                        source.clone(),
                                        next_table_name.clone(),
                                    )
                                    .await;

                                    rename_inflight.set(false);
                                    match result {
                                        Ok(()) => {
                                            on_saved(renamed_qualified_name(
                                                &source,
                                                target_kind,
                                                &next_table_name,
                                            ));
                                        }
                                        Err(err) => {
                                            rename_error.set(err.to_string());
                                        }
                                    }
                                });
                            },
                            if rename_inflight() {
                                "Renaming..."
                            } else {
                                "Rename table"
                            }
                        }
                    }
                }
            }
        }
    }
}

fn default_rename_table_draft(target: &RenameTableTarget) -> RenameTableDraft {
    RenameTableDraft {
        table_name: format!("{}_renamed", target.source.table_name.trim()),
    }
}

fn rename_table_form_valid(target: &RenameTableTarget, draft: &RenameTableDraft) -> bool {
    let table_name = draft.table_name.trim();
    !table_name.is_empty() && table_name != target.source.table_name.trim()
}

fn rename_table_preview_sql(target: &RenameTableTarget, draft: &RenameTableDraft) -> String {
    let table_name = draft.table_name.trim();
    if table_name.is_empty() {
        return "-- enter a new table name".to_string();
    }

    let source_name = target.source.qualified_name.trim();
    let target_name = renamed_qualified_name(&target.source, target.kind, table_name);

    match target.kind {
        DatabaseKind::Sqlite | DatabaseKind::Postgres => {
            format!("ALTER TABLE {source_name} RENAME TO {target_name};")
        }
        DatabaseKind::MySql | DatabaseKind::ClickHouse => {
            format!("RENAME TABLE {source_name} TO {target_name};")
        }
    }
}

fn renamed_qualified_name(
    source: &TablePreviewSource,
    kind: DatabaseKind,
    table_name: &str,
) -> String {
    match kind {
        DatabaseKind::Sqlite => quote_sql_identifier(table_name.trim()),
        DatabaseKind::Postgres | DatabaseKind::MySql | DatabaseKind::ClickHouse =>
            quoted_table_name_preview(kind, source.schema.as_deref(), table_name.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::renamed_qualified_name;
    use models::{DatabaseKind, TablePreviewSource};

    #[test]
    fn rename_target_name_matches_explorer_qualified_name_format() {
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
            renamed_qualified_name(&sqlite_source, DatabaseKind::Sqlite, "products_new"),
            r#""products_new""#
        );
        assert_eq!(
            renamed_qualified_name(&postgres_source, DatabaseKind::Postgres, "products_new"),
            r#""public"."products_new""#
        );
        assert_eq!(
            renamed_qualified_name(&mysql_source, DatabaseKind::MySql, "products_new"),
            "`app`.`products_new`"
        );
    }
}
