use super::{
    default_schema_name,
    quote_clickhouse_identifier,
    quote_sql_identifier,
    quoted_table_name_preview,
};
use crate::{
    app_state::session_connection,
    screens::workspace::actions::{read_only_mode_block_status, read_only_mode_enabled},
};
use dioxus::prelude::*;
use models::DatabaseKind;
use std::collections::HashSet;

const CUSTOM_TYPE_VALUE: &str = "__custom__";

#[derive(Clone, PartialEq)]
pub(super) struct CreateTableTarget {
    pub(super) session_id: u64,
    pub(super) connection_name: String,
    pub(super) kind: DatabaseKind,
    pub(super) schemas: Vec<String>,
}

#[derive(Clone, PartialEq)]
struct CreateTableDraft {
    schema: String,
    table_name: String,
    columns: Vec<CreateTableColumnDraft>,
    clickhouse_engine: ClickHouseEnginePreset,
}

#[derive(Clone, PartialEq)]
struct CreateTableColumnDraft {
    name: String,
    data_type: String,
    default_value: String,
    not_null: bool,
    key: bool,
    unique: bool,
    auto_increment: bool,
}

#[derive(Clone, Debug)]
struct CreateTableRequestPayload {
    columns_sql: String,
    clickhouse_engine: Option<String>,
}

#[derive(Clone)]
struct ResolvedCreateTableColumn {
    name: String,
    sql: String,
    key: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClickHouseEnginePreset {
    MergeTree,
    ReplacingMergeTree,
    Log,
}

#[component]
pub(super) fn CreateTableModal(
    target: CreateTableTarget,
    tree_reload: Signal<u64>,
    mut show_create_table: Signal<bool>,
) -> Element {
    let mut draft = use_signal(|| default_create_table_draft(&target));
    let mut create_error = use_signal(String::new);
    let mut create_inflight = use_signal(|| false);
    let current_draft = draft();
    let read_only_mode = read_only_mode_enabled();
    let can_submit = create_table_form_valid(target.kind, &current_draft)
        && !create_inflight()
        && !read_only_mode;
    let preview_sql = create_table_preview_sql(target.kind, &current_draft);

    rsx! {
        div {
            class: "settings-modal__backdrop",
            onclick: move |_| show_create_table.set(false),
            div {
                class: "settings-modal table-modal",
                onclick: move |event| event.stop_propagation(),
                div {
                    class: "settings-modal__header",
                    div {
                        class: "settings-modal__header-copy",
                        h2 { class: "settings-modal__title", "Create Table" }
                        p {
                            class: "settings-modal__hint",
                            "Create a new table in {target.connection_name}."
                        }
                    }
                    button {
                        class: "button button--ghost button--small",
                        onclick: move |_| show_create_table.set(false),
                        "Close"
                    }
                }

                div {
                    class: "table-modal__body",
                    div {
                        class: "table-modal__grid",
                        if target.kind == DatabaseKind::Sqlite {
                            div {
                                class: "field",
                                span { class: "field__label", "Schema" }
                                input {
                                    class: "input",
                                    value: current_draft.schema.clone(),
                                    readonly: true,
                                }
                            }
                        } else if !target.schemas.is_empty() {
                            div {
                                class: "field",
                                span { class: "field__label", "Schema" }
                                select {
                                    class: "input",
                                    value: current_draft.schema.clone(),
                                    oninput: move |event| {
                                        let value = event.value();
                                        draft.with_mut(|draft| draft.schema = value);
                                    },
                                    for schema in target.schemas.iter().cloned() {
                                        option {
                                            value: schema.clone(),
                                            "{schema}"
                                        }
                                    }
                                }
                            }
                        }

                        div {
                            class: "field",
                            span { class: "field__label", "Table name" }
                            input {
                                class: "input",
                                value: current_draft.table_name.clone(),
                                placeholder: "events",
                                oninput: move |event| {
                                    let value = event.value();
                                    draft.with_mut(|draft| draft.table_name = value);
                                },
                            }
                        }
                    }

                    div {
                        class: "table-modal__section",
                        div {
                            class: "table-modal__section-header",
                            div {
                                class: "table-modal__section-copy",
                                span { class: "field__label", "Columns" }
                                p {
                                    class: "table-modal__hint",
                                    if target.kind == DatabaseKind::ClickHouse {
                                        "Unchecked Required wraps the type in Nullable(...). Sort key columns become ORDER BY."
                                    } else {
                                        "Configure each column with UI fields instead of writing column SQL by hand."
                                    }
                                }
                            }
                            button {
                                class: "button button--ghost button--small",
                                disabled: create_inflight(),
                                onclick: move |_| {
                                    draft.with_mut(|draft| draft.columns.push(new_create_table_column(target.kind)));
                                },
                                "Add column"
                            }
                        }

                        if current_draft.columns.is_empty() {
                            p {
                                class: "empty-state",
                                "Add at least one column."
                            }
                        } else {
                            div {
                                class: "table-modal__columns",
                                for (index, column) in current_draft.columns.iter().cloned().enumerate() {
                                    div {
                                        key: "{index}",
                                        class: "table-modal__column-card",
                                        div {
                                            class: "table-modal__column-header",
                                            div {
                                                class: "table-modal__column-title",
                                                if column.name.trim().is_empty() {
                                                    "Column {index + 1}"
                                                } else {
                                                    "{column.name}"
                                                }
                                            }
                                            button {
                                                class: "button button--ghost button--small",
                                                disabled: current_draft.columns.len() <= 1 || create_inflight(),
                                                onclick: move |_| {
                                                    draft.with_mut(|draft| {
                                                        if draft.columns.len() > 1 && index < draft.columns.len() {
                                                            draft.columns.remove(index);
                                                        }
                                                    });
                                                },
                                                "Remove"
                                            }
                                        }

                                        div {
                                            class: "table-modal__column-grid",
                                            div {
                                                class: "field",
                                                span { class: "field__label", "Name" }
                                                input {
                                                    class: "input",
                                                    value: column.name.clone(),
                                                    placeholder: if index == 0 { "id" } else { "name" },
                                                    oninput: move |event| {
                                                        let value = event.value();
                                                        draft.with_mut(|draft| {
                                                            if let Some(column) = draft.columns.get_mut(index) {
                                                                column.name = value;
                                                            }
                                                        });
                                                    },
                                                }
                                            }

                                            div {
                                                class: "field",
                                                span { class: "field__label", "Type" }
                                                select {
                                                    class: "input",
                                                    value: selected_create_table_type_value(target.kind, &column.data_type),
                                                    oninput: move |event| {
                                                        let value = event.value();
                                                        draft.with_mut(|draft| {
                                                            if let Some(column) = draft.columns.get_mut(index) {
                                                                column.data_type = apply_selected_create_table_type(
                                                                    target.kind,
                                                                    &column.data_type,
                                                                    &value,
                                                                );
                                                            }
                                                        });
                                                    },
                                                    for data_type in create_table_type_options(target.kind) {
                                                        option {
                                                            value: *data_type,
                                                            "{data_type}"
                                                        }
                                                    }
                                                    option {
                                                        value: CUSTOM_TYPE_VALUE,
                                                        "Custom"
                                                    }
                                                }
                                                if is_custom_create_table_type(target.kind, &column.data_type) {
                                                    input {
                                                        class: "input",
                                                        value: column.data_type.clone(),
                                                        placeholder: create_table_type_placeholder(target.kind, index),
                                                        oninput: move |event| {
                                                            let value = event.value();
                                                            draft.with_mut(|draft| {
                                                                if let Some(column) = draft.columns.get_mut(index) {
                                                                    column.data_type = value;
                                                                }
                                                            });
                                                        },
                                                    }
                                                }
                                            }

                                            div {
                                                class: "field",
                                                span { class: "field__label", "Default" }
                                                input {
                                                    class: "input",
                                                    value: column.default_value.clone(),
                                                    placeholder: create_table_default_placeholder(target.kind, index),
                                                    oninput: move |event| {
                                                        let value = event.value();
                                                        draft.with_mut(|draft| {
                                                            if let Some(column) = draft.columns.get_mut(index) {
                                                                column.default_value = value;
                                                            }
                                                        });
                                                    },
                                                }
                                            }
                                        }

                                        div {
                                            class: "table-modal__column-toggles",
                                            label {
                                                class: "settings-modal__toggle",
                                                input {
                                                    r#type: "checkbox",
                                                    checked: column.not_null,
                                                    oninput: move |event| {
                                                        let checked = event.checked();
                                                        draft.with_mut(|draft| {
                                                            if let Some(column) = draft.columns.get_mut(index) {
                                                                column.not_null = checked;
                                                            }
                                                        });
                                                    },
                                                }
                                                span { "{column_required_label(target.kind)}" }
                                            }

                                            label {
                                                class: "settings-modal__toggle",
                                                input {
                                                    r#type: "checkbox",
                                                    checked: column.key,
                                                    oninput: move |event| {
                                                        let checked = event.checked();
                                                        draft.with_mut(|draft| {
                                                            if let Some(column) = draft.columns.get_mut(index) {
                                                                column.key = checked;
                                                            }
                                                        });
                                                    },
                                                }
                                                span { "{column_key_label(target.kind)}" }
                                            }

                                            if target.kind != DatabaseKind::ClickHouse {
                                                label {
                                                    class: "settings-modal__toggle",
                                                    input {
                                                        r#type: "checkbox",
                                                        checked: column.unique,
                                                        oninput: move |event| {
                                                            let checked = event.checked();
                                                            draft.with_mut(|draft| {
                                                                if let Some(column) = draft.columns.get_mut(index) {
                                                                    column.unique = checked;
                                                                }
                                                            });
                                                        },
                                                    }
                                                    span { "Unique" }
                                                }

                                                label {
                                                    class: "settings-modal__toggle",
                                                    input {
                                                        r#type: "checkbox",
                                                        checked: column.auto_increment,
                                                        oninput: move |event| {
                                                            let checked = event.checked();
                                                            draft.with_mut(|draft| {
                                                                if let Some(column) = draft.columns.get_mut(index) {
                                                                    column.auto_increment = checked;
                                                                    if checked {
                                                                        column.key = true;
                                                                    }
                                                                }
                                                            });
                                                        },
                                                    }
                                                    span { "{column_auto_increment_label(target.kind)}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if target.kind == DatabaseKind::ClickHouse {
                        div {
                            class: "table-modal__grid",
                            div {
                                class: "field",
                                span { class: "field__label", "Engine" }
                                select {
                                    class: "input",
                                    value: current_draft.clickhouse_engine.as_value(),
                                    oninput: move |event| {
                                        let value = event.value();
                                        draft.with_mut(|draft| {
                                            draft.clickhouse_engine = ClickHouseEnginePreset::from_value(&value);
                                        });
                                    },
                                    option { value: ClickHouseEnginePreset::MergeTree.as_value(), "MergeTree" }
                                    option { value: ClickHouseEnginePreset::ReplacingMergeTree.as_value(), "ReplacingMergeTree" }
                                    option { value: ClickHouseEnginePreset::Log.as_value(), "Log" }
                                }
                            }

                            div {
                                class: "field",
                                span { class: "field__label", "Order key" }
                                p {
                                    class: "table-modal__hint table-modal__hint--boxed",
                                    "{clickhouse_order_by_summary(&current_draft.columns)}"
                                }
                            }
                        }
                    }

                    div {
                        class: "table-modal__preview",
                        span { class: "field__label", "Preview" }
                        pre {
                            class: "table-modal__preview-sql",
                            if preview_sql.trim().is_empty() {
                                "-- SQL preview will appear here"
                            } else {
                                "{preview_sql}"
                            }
                        }
                    }

                    if !create_error().trim().is_empty() {
                        p {
                            class: "table-modal__error",
                            "{create_error}"
                        }
                    }

                    div {
                        class: "table-modal__actions",
                        button {
                            class: "button button--ghost button--small",
                            disabled: create_inflight(),
                            onclick: move |_| show_create_table.set(false),
                            "Cancel"
                        }
                        button {
                            class: "button button--primary button--small",
                            disabled: !can_submit,
                            onclick: {
                                let target = target.clone();
                                move |_| {
                                    if read_only_mode_enabled() {
                                        create_error.set(read_only_mode_block_status("table creation"));
                                        return;
                                    }

                                    let draft_value = draft();
                                    let table_name = draft_value.table_name.trim().to_string();
                                    if table_name.is_empty() {
                                        create_error.set("Table name is required.".to_string());
                                        return;
                                    }

                                    let request = match build_create_table_request(target.kind, &draft_value) {
                                        Ok(request) => request,
                                        Err(err) => {
                                            create_error.set(err);
                                            return;
                                        }
                                    };

                                    let schema = normalized_schema_input(target.kind, &draft_value.schema);
                                    create_error.set(String::new());
                                    create_inflight.set(true);

                                    spawn(async move {
                                        let Some(connection) = session_connection(target.session_id) else {
                                            create_error.set("The connection was closed.".to_string());
                                            create_inflight.set(false);
                                            return;
                                        };

                                        let result = services::create_table(
                                            connection,
                                            schema,
                                            table_name,
                                            request.columns_sql,
                                            request.clickhouse_engine,
                                        )
                                        .await;

                                        create_inflight.set(false);
                                        match result {
                                            Ok(()) => {
                                                show_create_table.set(false);
                                                tree_reload += 1;
                                            }
                                            Err(err) => {
                                                create_error.set(err.to_string());
                                            }
                                        }
                                    });
                                }
                            },
                            if create_inflight() { "Creating..." } else { "Create table" }
                        }
                    }
                }
            }
        }
    }
}

fn default_create_table_draft(target: &CreateTableTarget) -> CreateTableDraft {
    CreateTableDraft {
        schema: preferred_schema_name(target.kind, &target.schemas),
        table_name: String::new(),
        columns: default_create_table_columns(target.kind),
        clickhouse_engine: ClickHouseEnginePreset::default_for(target.kind),
    }
}

fn default_create_table_columns(kind: DatabaseKind) -> Vec<CreateTableColumnDraft> {
    match kind {
        DatabaseKind::Sqlite => vec![
            CreateTableColumnDraft {
                name: "id".to_string(),
                data_type: "INTEGER".to_string(),
                default_value: String::new(),
                not_null: true,
                key: true,
                unique: false,
                auto_increment: true,
            },
            CreateTableColumnDraft {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                default_value: String::new(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
            CreateTableColumnDraft {
                name: "created_at".to_string(),
                data_type: "TEXT".to_string(),
                default_value: "CURRENT_TIMESTAMP".to_string(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
        ],
        DatabaseKind::Postgres => vec![
            CreateTableColumnDraft {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                default_value: String::new(),
                not_null: true,
                key: true,
                unique: false,
                auto_increment: true,
            },
            CreateTableColumnDraft {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                default_value: String::new(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
            CreateTableColumnDraft {
                name: "created_at".to_string(),
                data_type: "TIMESTAMPTZ".to_string(),
                default_value: "now()".to_string(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
        ],
        DatabaseKind::MySql => vec![
            CreateTableColumnDraft {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                default_value: String::new(),
                not_null: true,
                key: true,
                unique: false,
                auto_increment: true,
            },
            CreateTableColumnDraft {
                name: "name".to_string(),
                data_type: "VARCHAR(255)".to_string(),
                default_value: String::new(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
            CreateTableColumnDraft {
                name: "created_at".to_string(),
                data_type: "TIMESTAMP".to_string(),
                default_value: "CURRENT_TIMESTAMP".to_string(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
        ],
        DatabaseKind::ClickHouse => vec![
            CreateTableColumnDraft {
                name: "id".to_string(),
                data_type: "UInt64".to_string(),
                default_value: String::new(),
                not_null: true,
                key: true,
                unique: false,
                auto_increment: false,
            },
            CreateTableColumnDraft {
                name: "name".to_string(),
                data_type: "String".to_string(),
                default_value: String::new(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
            CreateTableColumnDraft {
                name: "created_at".to_string(),
                data_type: "DateTime".to_string(),
                default_value: "now()".to_string(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: false,
            },
        ],
    }
}

fn new_create_table_column(kind: DatabaseKind) -> CreateTableColumnDraft {
    CreateTableColumnDraft {
        name: String::new(),
        data_type: create_table_default_type(kind, false).to_string(),
        default_value: String::new(),
        not_null: false,
        key: false,
        unique: false,
        auto_increment: false,
    }
}

fn create_table_default_type(kind: DatabaseKind, is_identity: bool) -> &'static str {
    match kind {
        DatabaseKind::Sqlite =>
            if is_identity {
                "INTEGER"
            } else {
                "TEXT"
            },
        DatabaseKind::Postgres =>
            if is_identity {
                "BIGINT"
            } else {
                "TEXT"
            },
        DatabaseKind::MySql =>
            if is_identity {
                "BIGINT"
            } else {
                "VARCHAR(255)"
            },
        DatabaseKind::ClickHouse =>
            if is_identity {
                "UInt64"
            } else {
                "String"
            },
    }
}

fn create_table_type_placeholder(kind: DatabaseKind, index: usize) -> &'static str {
    match (kind, index) {
        (DatabaseKind::Sqlite, 0) => "INTEGER",
        (DatabaseKind::Sqlite, _) => "TEXT",
        (DatabaseKind::Postgres, 0) => "BIGINT",
        (DatabaseKind::Postgres, _) => "TEXT",
        (DatabaseKind::MySql, 0) => "BIGINT",
        (DatabaseKind::MySql, _) => "VARCHAR(255)",
        (DatabaseKind::ClickHouse, 0) => "UInt64",
        (DatabaseKind::ClickHouse, _) => "String",
    }
}

fn create_table_type_options(kind: DatabaseKind) -> &'static [&'static str] {
    match kind {
        DatabaseKind::Sqlite => &[
            "INTEGER", "TEXT", "REAL", "NUMERIC", "BLOB", "BOOLEAN", "DATE", "DATETIME", "JSON",
        ],
        DatabaseKind::Postgres => &[
            "SMALLINT",
            "INTEGER",
            "BIGINT",
            "NUMERIC",
            "REAL",
            "DOUBLE PRECISION",
            "BOOLEAN",
            "TEXT",
            "VARCHAR(255)",
            "UUID",
            "JSONB",
            "DATE",
            "TIMESTAMPTZ",
            "BYTEA",
        ],
        DatabaseKind::MySql => &[
            "TINYINT",
            "SMALLINT",
            "INT",
            "BIGINT",
            "DECIMAL(18,2)",
            "FLOAT",
            "DOUBLE",
            "BOOLEAN",
            "VARCHAR(255)",
            "TEXT",
            "JSON",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "BLOB",
            "UUID",
        ],
        DatabaseKind::ClickHouse => &[
            "Int32",
            "Int64",
            "UInt32",
            "UInt64",
            "Float32",
            "Float64",
            "Decimal(18,2)",
            "Bool",
            "String",
            "FixedString(16)",
            "UUID",
            "Date",
            "Date32",
            "DateTime",
            "DateTime64(3)",
            "JSON",
        ],
    }
}

fn selected_create_table_type_value(kind: DatabaseKind, data_type: &str) -> &str {
    create_table_type_options(kind)
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(data_type.trim()))
        .unwrap_or(CUSTOM_TYPE_VALUE)
}

fn is_custom_create_table_type(kind: DatabaseKind, data_type: &str) -> bool {
    selected_create_table_type_value(kind, data_type) == CUSTOM_TYPE_VALUE
}

fn apply_selected_create_table_type(
    kind: DatabaseKind,
    current_data_type: &str,
    selected_value: &str,
) -> String {
    if selected_value == CUSTOM_TYPE_VALUE {
        if is_custom_create_table_type(kind, current_data_type) {
            current_data_type.to_string()
        } else {
            String::new()
        }
    } else {
        selected_value.to_string()
    }
}

fn create_table_default_placeholder(kind: DatabaseKind, index: usize) -> &'static str {
    match (kind, index) {
        (DatabaseKind::Sqlite, 2) => "CURRENT_TIMESTAMP",
        (DatabaseKind::Postgres, 2) => "now()",
        (DatabaseKind::MySql, 2) => "CURRENT_TIMESTAMP",
        (DatabaseKind::ClickHouse, 2) => "now()",
        _ => "Optional expression",
    }
}

fn column_required_label(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::ClickHouse => "Required",
        DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql => "Not null",
    }
}

fn column_key_label(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::ClickHouse => "Sort key",
        DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql => "Primary key",
    }
}

fn column_auto_increment_label(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::Sqlite => "Auto id",
        DatabaseKind::Postgres => "Identity",
        DatabaseKind::MySql => "Auto increment",
        DatabaseKind::ClickHouse => "",
    }
}

fn create_table_form_valid(kind: DatabaseKind, draft: &CreateTableDraft) -> bool {
    !draft.table_name.trim().is_empty() && build_create_table_request(kind, draft).is_ok()
}

fn normalized_schema_input(kind: DatabaseKind, value: &str) -> Option<String> {
    let value = value.trim();
    match kind {
        DatabaseKind::Sqlite => Some(if value.is_empty() { "main" } else { value }.to_string()),
        _ if value.is_empty() => None,
        _ => Some(value.to_string()),
    }
}

fn preferred_schema_name(kind: DatabaseKind, schemas: &[String]) -> String {
    let preferred = default_schema_name(kind);
    schemas
        .iter()
        .find(|schema| schema.eq_ignore_ascii_case(&preferred))
        .cloned()
        .or_else(|| schemas.first().cloned())
        .unwrap_or(preferred)
}

fn create_table_preview_sql(kind: DatabaseKind, draft: &CreateTableDraft) -> String {
    let table_name = draft.table_name.trim();
    let schema = normalized_schema_input(kind, &draft.schema);
    let table_name = if table_name.is_empty() {
        "<table_name>"
    } else {
        table_name
    };
    let qualified_name = quoted_table_name_preview(kind, schema.as_deref(), table_name);
    let definition = preview_create_table_columns_sql(kind, &draft.columns);

    match kind {
        DatabaseKind::ClickHouse => format!(
            "CREATE TABLE {qualified_name} {definition}\n{}",
            preview_clickhouse_engine_clause(draft)
        ),
        _ => format!("CREATE TABLE {qualified_name} {definition}"),
    }
}

fn preview_create_table_columns_sql(
    kind: DatabaseKind,
    columns: &[CreateTableColumnDraft],
) -> String {
    if columns.is_empty() {
        return "(\n  -- add at least one column\n)".to_string();
    }

    let key_count = columns.iter().filter(|column| column.key).count();
    let mut lines = columns
        .iter()
        .map(|column| preview_create_table_column_sql(kind, column, key_count))
        .collect::<Vec<_>>();

    if matches!(
        kind,
        DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql
    ) && key_count > 1
    {
        let keys = columns
            .iter()
            .filter(|column| column.key)
            .map(|column| match kind {
                DatabaseKind::MySql => quote_clickhouse_identifier(preview_column_name(column)),
                DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::ClickHouse =>
                    quote_sql_identifier(preview_column_name(column)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("PRIMARY KEY ({keys})"));
    }

    format_columns_block(lines)
}

fn preview_create_table_column_sql(
    kind: DatabaseKind,
    column: &CreateTableColumnDraft,
    key_count: usize,
) -> String {
    let name = match kind {
        DatabaseKind::ClickHouse | DatabaseKind::MySql =>
            quote_clickhouse_identifier(preview_column_name(column)),
        DatabaseKind::Sqlite | DatabaseKind::Postgres =>
            quote_sql_identifier(preview_column_name(column)),
    };
    let data_type = column
        .data_type
        .trim()
        .to_string()
        .chars()
        .collect::<String>();
    let data_type = if data_type.trim().is_empty() {
        create_table_default_type(kind, column.auto_increment).to_string()
    } else if kind == DatabaseKind::ClickHouse && !column.not_null {
        clickhouse_preview_data_type(&data_type)
    } else {
        data_type
    };

    let default_value = column.default_value.trim();
    match kind {
        DatabaseKind::Sqlite =>
            if column.auto_increment {
                format!("{name} INTEGER PRIMARY KEY AUTOINCREMENT")
            } else {
                let mut parts = vec![format!("{name} {data_type}")];
                if !default_value.is_empty() {
                    parts.push(format!("DEFAULT {default_value}"));
                }
                if column.not_null && !(key_count == 1 && column.key) {
                    parts.push("NOT NULL".to_string());
                }
                if column.unique && !column.key {
                    parts.push("UNIQUE".to_string());
                }
                if key_count == 1 && column.key {
                    parts.push("PRIMARY KEY".to_string());
                }
                parts.join(" ")
            },
        DatabaseKind::Postgres => {
            let mut parts = vec![format!("{name} {data_type}")];
            if column.auto_increment {
                parts.push("GENERATED BY DEFAULT AS IDENTITY".to_string());
            }
            if !default_value.is_empty() {
                parts.push(format!("DEFAULT {default_value}"));
            }
            if column.not_null && !(key_count == 1 && column.key) {
                parts.push("NOT NULL".to_string());
            }
            if column.unique && !column.key {
                parts.push("UNIQUE".to_string());
            }
            if key_count == 1 && column.key {
                parts.push("PRIMARY KEY".to_string());
            }
            parts.join(" ")
        }
        DatabaseKind::MySql => {
            let mut parts = vec![format!("{name} {data_type}")];
            if column.auto_increment {
                parts.push("AUTO_INCREMENT".to_string());
            }
            if !default_value.is_empty() {
                parts.push(format!("DEFAULT {default_value}"));
            }
            if column.not_null && !(key_count == 1 && column.key) {
                parts.push("NOT NULL".to_string());
            }
            if column.unique && !column.key {
                parts.push("UNIQUE".to_string());
            }
            if key_count == 1 && column.key {
                parts.push("PRIMARY KEY".to_string());
            }
            parts.join(" ")
        }
        DatabaseKind::ClickHouse => {
            let mut parts = vec![format!("{name} {data_type}")];
            if !default_value.is_empty() {
                parts.push(format!("DEFAULT {default_value}"));
            }
            parts.join(" ")
        }
    }
}

fn preview_column_name(column: &CreateTableColumnDraft) -> &str {
    if column.name.trim().is_empty() {
        "<column_name>"
    } else {
        column.name.trim()
    }
}

fn build_create_table_request(
    kind: DatabaseKind,
    draft: &CreateTableDraft,
) -> Result<CreateTableRequestPayload, String> {
    let columns = resolve_create_table_columns(kind, &draft.columns)?;
    let columns_sql = format_columns_block(
        columns
            .iter()
            .map(|column| column.sql.clone())
            .collect::<Vec<_>>(),
    );
    let clickhouse_engine = if kind == DatabaseKind::ClickHouse {
        Some(build_clickhouse_engine_clause(draft, &columns))
    } else {
        None
    };

    Ok(CreateTableRequestPayload {
        columns_sql,
        clickhouse_engine,
    })
}

fn resolve_create_table_columns(
    kind: DatabaseKind,
    columns: &[CreateTableColumnDraft],
) -> Result<Vec<ResolvedCreateTableColumn>, String> {
    if columns.is_empty() {
        return Err("Add at least one column.".to_string());
    }

    let mut seen = HashSet::new();
    let key_count = columns.iter().filter(|column| column.key).count();
    let auto_increment_count = columns
        .iter()
        .filter(|column| column.auto_increment)
        .count();

    if matches!(
        kind,
        DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql
    ) && auto_increment_count > 1
    {
        return Err("Only one auto-generated key column is supported.".to_string());
    }
    if matches!(
        kind,
        DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql
    ) && auto_increment_count == 1
        && key_count != 1
    {
        return Err("Auto-generated keys require exactly one primary key column.".to_string());
    }

    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let name = column.name.trim();
            if name.is_empty() {
                return Err(format!("Column {} needs a name.", index + 1));
            }

            let normalized_name = name.to_ascii_lowercase();
            if !seen.insert(normalized_name) {
                return Err(format!("Duplicate column name: {name}."));
            }

            let data_type = column.data_type.trim();
            if data_type.is_empty() {
                return Err(format!("Column {name} needs a type."));
            }

            match kind {
                DatabaseKind::Sqlite =>
                    resolve_sqlite_create_table_column(column, data_type, key_count),
                DatabaseKind::Postgres =>
                    resolve_postgres_create_table_column(column, data_type, key_count),
                DatabaseKind::MySql =>
                    resolve_mysql_create_table_column(column, data_type, key_count),
                DatabaseKind::ClickHouse =>
                    resolve_clickhouse_create_table_column(column, data_type),
            }
        })
        .collect()
}

fn resolve_sqlite_create_table_column(
    column: &CreateTableColumnDraft,
    data_type: &str,
    key_count: usize,
) -> Result<ResolvedCreateTableColumn, String> {
    let name = column.name.trim();
    let default_value = column.default_value.trim();
    let quoted_name = quote_sql_identifier(name);

    if column.auto_increment {
        if !column.key {
            return Err(format!(
                "SQLite auto id requires {} to be the primary key.",
                name
            ));
        }
        if key_count != 1 {
            return Err("SQLite auto id only works with a single-column primary key.".to_string());
        }
        if !sqlite_identity_type_supported(data_type) {
            return Err(format!(
                "SQLite auto id requires an INTEGER-like type for {name}."
            ));
        }
        if !default_value.is_empty() {
            return Err(format!(
                "SQLite auto id column {name} cannot also define DEFAULT."
            ));
        }

        return Ok(ResolvedCreateTableColumn {
            name: name.to_string(),
            sql: format!("{quoted_name} INTEGER PRIMARY KEY AUTOINCREMENT"),
            key: true,
        });
    }

    let mut parts = vec![format!("{quoted_name} {data_type}")];
    if !default_value.is_empty() {
        parts.push(format!("DEFAULT {default_value}"));
    }
    if column.not_null && !(key_count == 1 && column.key) {
        parts.push("NOT NULL".to_string());
    }
    if column.unique && !column.key {
        parts.push("UNIQUE".to_string());
    }
    if key_count == 1 && column.key {
        parts.push("PRIMARY KEY".to_string());
    }

    Ok(ResolvedCreateTableColumn {
        name: name.to_string(),
        sql: parts.join(" "),
        key: column.key,
    })
}

fn resolve_postgres_create_table_column(
    column: &CreateTableColumnDraft,
    data_type: &str,
    key_count: usize,
) -> Result<ResolvedCreateTableColumn, String> {
    let name = column.name.trim();
    let default_value = column.default_value.trim();
    let quoted_name = quote_sql_identifier(name);

    if column.auto_increment && !column.key {
        return Err(format!(
            "PostgreSQL identity requires {} to be part of the primary key.",
            name
        ));
    }
    if column.auto_increment && !postgres_identity_type_supported(data_type) {
        return Err(format!(
            "PostgreSQL identity requires an integer type for {name}."
        ));
    }
    if column.auto_increment && !default_value.is_empty() {
        return Err(format!(
            "PostgreSQL identity column {name} cannot also define DEFAULT."
        ));
    }

    let mut parts = vec![format!("{quoted_name} {data_type}")];
    if column.auto_increment {
        parts.push("GENERATED BY DEFAULT AS IDENTITY".to_string());
    }
    if !default_value.is_empty() {
        parts.push(format!("DEFAULT {default_value}"));
    }
    if column.not_null && !(key_count == 1 && column.key) {
        parts.push("NOT NULL".to_string());
    }
    if column.unique && !column.key {
        parts.push("UNIQUE".to_string());
    }
    if key_count == 1 && column.key {
        parts.push("PRIMARY KEY".to_string());
    }

    Ok(ResolvedCreateTableColumn {
        name: name.to_string(),
        sql: parts.join(" "),
        key: column.key,
    })
}

fn resolve_clickhouse_create_table_column(
    column: &CreateTableColumnDraft,
    data_type: &str,
) -> Result<ResolvedCreateTableColumn, String> {
    let name = column.name.trim();
    let default_value = column.default_value.trim();
    let quoted_name = quote_clickhouse_identifier(name);
    let resolved_type = if column.not_null {
        data_type.to_string()
    } else {
        wrap_clickhouse_nullable(data_type)
    };

    let mut parts = vec![format!("{quoted_name} {resolved_type}")];
    if !default_value.is_empty() {
        parts.push(format!("DEFAULT {default_value}"));
    }

    Ok(ResolvedCreateTableColumn {
        name: name.to_string(),
        sql: parts.join(" "),
        key: column.key,
    })
}

fn resolve_mysql_create_table_column(
    column: &CreateTableColumnDraft,
    data_type: &str,
    key_count: usize,
) -> Result<ResolvedCreateTableColumn, String> {
    let name = column.name.trim();
    let default_value = column.default_value.trim();
    let quoted_name = quote_clickhouse_identifier(name);

    if column.auto_increment && !column.key {
        return Err(format!(
            "MySQL auto increment requires {} to be part of the primary key.",
            name
        ));
    }
    if column.auto_increment && !mysql_identity_type_supported(data_type) {
        return Err(format!(
            "MySQL auto increment requires an integer type for {name}."
        ));
    }
    if column.auto_increment && !default_value.is_empty() {
        return Err(format!(
            "MySQL auto increment column {name} cannot also define DEFAULT."
        ));
    }

    let mut parts = vec![format!("{quoted_name} {data_type}")];
    if column.auto_increment {
        parts.push("AUTO_INCREMENT".to_string());
    }
    if !default_value.is_empty() {
        parts.push(format!("DEFAULT {default_value}"));
    }
    if column.not_null && !(key_count == 1 && column.key) {
        parts.push("NOT NULL".to_string());
    }
    if column.unique && !column.key {
        parts.push("UNIQUE".to_string());
    }
    if key_count == 1 && column.key {
        parts.push("PRIMARY KEY".to_string());
    }

    Ok(ResolvedCreateTableColumn {
        name: name.to_string(),
        sql: parts.join(" "),
        key: column.key,
    })
}

fn format_columns_block(lines: Vec<String>) -> String {
    if lines.is_empty() {
        "(\n  -- add at least one column\n)".to_string()
    } else {
        format!("(\n{}\n)", lines.join(",\n"))
    }
}

fn build_clickhouse_engine_clause(
    draft: &CreateTableDraft,
    columns: &[ResolvedCreateTableColumn],
) -> String {
    let key_columns = columns
        .iter()
        .filter(|column| column.key)
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>();
    let order_by = clickhouse_order_by_expression(&key_columns);
    match draft.clickhouse_engine {
        ClickHouseEnginePreset::MergeTree => {
            format!("ENGINE = MergeTree() ORDER BY {order_by}")
        }
        ClickHouseEnginePreset::ReplacingMergeTree => {
            format!("ENGINE = ReplacingMergeTree() ORDER BY {order_by}")
        }
        ClickHouseEnginePreset::Log => "ENGINE = Log".to_string(),
    }
}

fn preview_clickhouse_engine_clause(draft: &CreateTableDraft) -> String {
    let key_columns = draft
        .columns
        .iter()
        .filter(|column| column.key)
        .map(preview_column_name)
        .collect::<Vec<_>>();
    let order_by = clickhouse_order_by_expression(&key_columns);
    match draft.clickhouse_engine {
        ClickHouseEnginePreset::MergeTree => {
            format!("ENGINE = MergeTree() ORDER BY {order_by}")
        }
        ClickHouseEnginePreset::ReplacingMergeTree => {
            format!("ENGINE = ReplacingMergeTree() ORDER BY {order_by}")
        }
        ClickHouseEnginePreset::Log => "ENGINE = Log".to_string(),
    }
}

fn clickhouse_order_by_summary(columns: &[CreateTableColumnDraft]) -> String {
    let keys = columns
        .iter()
        .filter(|column| column.key)
        .map(|column| column.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();

    if keys.is_empty() {
        "No sort key selected. MergeTree engines will use ORDER BY tuple().".to_string()
    } else {
        format!("ORDER BY {}", clickhouse_order_by_expression(&keys))
    }
}

fn clickhouse_order_by_expression(keys: &[&str]) -> String {
    match keys {
        [] => "tuple()".to_string(),
        [single] => quote_clickhouse_identifier(single),
        _ => format!(
            "({})",
            keys.iter()
                .map(|name| quote_clickhouse_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn wrap_clickhouse_nullable(data_type: &str) -> String {
    if data_type.trim_start().starts_with("Nullable(") {
        data_type.to_string()
    } else {
        format!("Nullable({data_type})")
    }
}

fn clickhouse_preview_data_type(data_type: &str) -> String {
    wrap_clickhouse_nullable(data_type.trim())
}

fn sqlite_identity_type_supported(data_type: &str) -> bool {
    data_type.to_ascii_lowercase().contains("int")
}

fn postgres_identity_type_supported(data_type: &str) -> bool {
    matches!(
        data_type.trim().to_ascii_lowercase().as_str(),
        "smallint" | "integer" | "bigint" | "int2" | "int4" | "int8"
    )
}

fn mysql_identity_type_supported(data_type: &str) -> bool {
    matches!(
        data_type.trim().to_ascii_lowercase().as_str(),
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint"
    )
}

impl ClickHouseEnginePreset {
    fn default_for(kind: DatabaseKind) -> Self {
        match kind {
            DatabaseKind::ClickHouse => Self::MergeTree,
            DatabaseKind::Sqlite | DatabaseKind::Postgres | DatabaseKind::MySql => Self::Log,
        }
    }

    fn as_value(self) -> &'static str {
        match self {
            Self::MergeTree => "merge_tree",
            Self::ReplacingMergeTree => "replacing_merge_tree",
            Self::Log => "log",
        }
    }

    fn from_value(value: &str) -> Self {
        match value {
            "replacing_merge_tree" => Self::ReplacingMergeTree,
            "log" => Self::Log,
            _ => Self::MergeTree,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClickHouseEnginePreset,
        CreateTableColumnDraft,
        CreateTableDraft,
        build_create_table_request,
        preview_clickhouse_engine_clause,
        selected_create_table_type_value,
    };
    use models::DatabaseKind;

    #[test]
    fn builds_sqlite_create_table_from_ui_fields() {
        let draft = CreateTableDraft {
            schema: "main".to_string(),
            table_name: "events".to_string(),
            columns: vec![
                CreateTableColumnDraft {
                    name: "id".to_string(),
                    data_type: "INTEGER".to_string(),
                    default_value: String::new(),
                    not_null: true,
                    key: true,
                    unique: false,
                    auto_increment: true,
                },
                CreateTableColumnDraft {
                    name: "name".to_string(),
                    data_type: "TEXT".to_string(),
                    default_value: String::new(),
                    not_null: true,
                    key: false,
                    unique: true,
                    auto_increment: false,
                },
            ],
            clickhouse_engine: ClickHouseEnginePreset::Log,
        };

        let request = build_create_table_request(DatabaseKind::Sqlite, &draft).expect("request");
        assert_eq!(
            request.columns_sql,
            "(\n\"id\" INTEGER PRIMARY KEY AUTOINCREMENT,\n\"name\" TEXT NOT NULL UNIQUE\n)"
        );
        assert!(request.clickhouse_engine.is_none());
    }

    #[test]
    fn rejects_postgres_identity_without_primary_key() {
        let draft = CreateTableDraft {
            schema: "public".to_string(),
            table_name: "events".to_string(),
            columns: vec![CreateTableColumnDraft {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                default_value: String::new(),
                not_null: true,
                key: false,
                unique: false,
                auto_increment: true,
            }],
            clickhouse_engine: ClickHouseEnginePreset::Log,
        };

        let err = build_create_table_request(DatabaseKind::Postgres, &draft).unwrap_err();
        assert!(err.contains("primary key"));
    }

    #[test]
    fn builds_mysql_create_table_from_ui_fields() {
        let draft = CreateTableDraft {
            schema: "app".to_string(),
            table_name: "events".to_string(),
            columns: vec![
                CreateTableColumnDraft {
                    name: "id".to_string(),
                    data_type: "BIGINT".to_string(),
                    default_value: String::new(),
                    not_null: true,
                    key: true,
                    unique: false,
                    auto_increment: true,
                },
                CreateTableColumnDraft {
                    name: "name".to_string(),
                    data_type: "VARCHAR(255)".to_string(),
                    default_value: String::new(),
                    not_null: true,
                    key: false,
                    unique: false,
                    auto_increment: false,
                },
            ],
            clickhouse_engine: ClickHouseEnginePreset::Log,
        };

        let request = build_create_table_request(DatabaseKind::MySql, &draft).expect("request");
        assert_eq!(
            request.columns_sql,
            "(\n`id` BIGINT AUTO_INCREMENT PRIMARY KEY,\n`name` VARCHAR(255) NOT NULL\n)"
        );
        assert!(request.clickhouse_engine.is_none());
    }

    #[test]
    fn previews_clickhouse_engine_from_sort_key_columns() {
        let draft = CreateTableDraft {
            schema: "default".to_string(),
            table_name: "events".to_string(),
            columns: vec![
                CreateTableColumnDraft {
                    name: "tenant_id".to_string(),
                    data_type: "UInt64".to_string(),
                    default_value: String::new(),
                    not_null: true,
                    key: true,
                    unique: false,
                    auto_increment: false,
                },
                CreateTableColumnDraft {
                    name: "created_at".to_string(),
                    data_type: "DateTime".to_string(),
                    default_value: "now()".to_string(),
                    not_null: true,
                    key: true,
                    unique: false,
                    auto_increment: false,
                },
            ],
            clickhouse_engine: ClickHouseEnginePreset::MergeTree,
        };

        assert_eq!(
            preview_clickhouse_engine_clause(&draft),
            "ENGINE = MergeTree() ORDER BY (`tenant_id`, `created_at`)"
        );
    }

    #[test]
    fn preserves_custom_type_selection_separately_from_presets() {
        assert_eq!(
            selected_create_table_type_value(DatabaseKind::Postgres, "JSONB"),
            "JSONB"
        );
        assert_eq!(
            selected_create_table_type_value(DatabaseKind::Postgres, "citext"),
            "__custom__"
        );
    }
}
