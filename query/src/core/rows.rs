use models::{DatabaseError, EditableTableContext, QueryPage, TablePreviewSource};
use sqlx::{Column, Row, TypeInfo};

pub(crate) fn sqlite_rows_to_page(rows: Vec<sqlx::sqlite::SqliteRow>) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();

    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        page_size: rows.len() as u32,
        rows,
        editable: None,
        offset: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(crate) fn postgres_rows_to_page(rows: Vec<sqlx::postgres::PgRow>) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();

    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| postgres_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        page_size: rows.len() as u32,
        rows,
        editable: None,
        offset: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(crate) fn mysql_rows_to_page(rows: Vec<sqlx::mysql::MySqlRow>) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();

    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| mysql_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        page_size: rows.len() as u32,
        rows,
        editable: None,
        offset: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(super) fn sqlite_rows_to_paginated_page(
    mut rows: Vec<sqlx::sqlite::SqliteRow>,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(super) fn postgres_rows_to_paginated_page(
    mut rows: Vec<sqlx::postgres::PgRow>,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| postgres_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(super) fn mysql_rows_to_paginated_page(
    mut rows: Vec<sqlx::mysql::MySqlRow>,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| mysql_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(super) fn sqlite_preview_rows_to_paginated_page(
    mut rows: Vec<sqlx::sqlite::SqliteRow>,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .skip(1)
                .map(|c| c.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let row_locators = rows
        .iter()
        .map(|row| {
            row.try_get::<i64, _>(0)
                .map(|v| v.to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let rows = rows
        .into_iter()
        .map(|row| {
            (1..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: Some(EditableTableContext {
            source,
            row_locators,
        }),
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(super) fn postgres_preview_rows_to_paginated_page(
    mut rows: Vec<sqlx::postgres::PgRow>,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .skip(1)
                .map(|c| c.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let row_locators = rows
        .iter()
        .map(|row| row.try_get::<String, _>(0).unwrap_or_default())
        .collect::<Vec<_>>();
    let rows = rows
        .into_iter()
        .map(|row| {
            (1..row.columns().len())
                .map(|idx| postgres_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: Some(EditableTableContext {
            source,
            row_locators,
        }),
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(super) fn mysql_preview_rows_to_paginated_page(
    mut rows: Vec<sqlx::mysql::MySqlRow>,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .skip(1)
                .map(|c| c.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let row_locators = rows
        .iter()
        .map(|row| mysql_locator_to_string(row, 0))
        .collect::<Vec<_>>();
    let rows = rows
        .into_iter()
        .map(|row| {
            (1..row.columns().len())
                .map(|idx| mysql_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: Some(EditableTableContext {
            source,
            row_locators,
        }),
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

fn sqlite_cell_to_string(row: &sqlx::sqlite::SqliteRow, idx: usize) -> String {
    if let Ok(value) = row.try_get::<Option<String>, _>(idx) {
        return value.unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i16>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bool>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return value
            .map(|bytes| format!("<{} bytes>", bytes.len()))
            .unwrap_or_else(|| "NULL".to_string());
    }

    format!("<unsupported:{}>", row.columns()[idx].type_info().name())
}

fn postgres_cell_to_string(row: &sqlx::postgres::PgRow, idx: usize) -> String {
    if let Ok(value) = row.try_get::<Option<String>, _>(idx) {
        return value.unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i16>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bool>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return value
            .map(|bytes| format!("<{} bytes>", bytes.len()))
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<uuid::Uuid>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bigdecimal::BigDecimal>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>(idx) {
        return value
            .map(|value| value.0.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::Date>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::Time>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::PrimitiveDateTime>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::OffsetDateTime>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<String>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<i32>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<i64>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<f64>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<bool>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<uuid::Uuid>>, _>(idx) {
        return value
            .map(format_array)
            .unwrap_or_else(|| "NULL".to_string());
    }

    format!("<unsupported:{}>", row.columns()[idx].type_info().name())
}

fn mysql_cell_to_string(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
    if let Ok(value) = row.try_get::<Option<String>, _>(idx) {
        return value.unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i8>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i16>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<u8>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<u16>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<u32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<u64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bool>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return value
            .map(|bytes| format!("<{} bytes>", bytes.len()))
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bigdecimal::BigDecimal>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>(idx) {
        return value
            .map(|value| value.0.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::Date>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::Time>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<time::PrimitiveDateTime>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<uuid::Uuid>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }

    format!("<unsupported:{}>", row.columns()[idx].type_info().name())
}

pub(super) fn clickhouse_rows_to_page(response: models::ClickHouseJsonResponse) -> QueryPage {
    QueryPage {
        columns: response
            .meta
            .into_iter()
            .map(|column| column.name)
            .collect(),
        rows: response
            .data
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| clickhouse_json_value_to_string(&value))
                    .collect()
            })
            .collect(),
        editable: None,
        offset: 0,
        page_size: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(super) fn clickhouse_rows_to_paginated_page(
    mut response: models::ClickHouseJsonResponse,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let has_next = response.data.len() > page_size as usize;
    if has_next {
        response.data.truncate(page_size as usize);
    }

    QueryPage {
        columns: response
            .meta
            .into_iter()
            .map(|column| column.name)
            .collect(),
        rows: response
            .data
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| clickhouse_json_value_to_string(&value))
                    .collect()
            })
            .collect(),
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

fn clickhouse_json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| "<unsupported>".to_string())
        }
    }
}

fn format_array<T: ToString>(values: Vec<T>) -> String {
    format!(
        "[{}]",
        values
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn mysql_locator_to_string(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
    if let Ok(value) = row.try_get::<Option<String>, _>(idx) {
        return value.unwrap_or_default();
    }
    if let Ok(value) = row.try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>(idx) {
        return value.map(|value| value.0.to_string()).unwrap_or_default();
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return value
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
    }

    mysql_cell_to_string(row, idx)
}

pub(super) fn invalid_sqlite_locator() -> DatabaseError {
    DatabaseError::UnsupportedDriver("invalid SQLite row locator".to_string())
}
