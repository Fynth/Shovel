use models::{EditableTableContext, QueryPage, TablePreviewSource};
use sqlx::{Column, Row, TypeInfo};

pub(crate) const LOCATOR_COLUMN: &str = "__shovel_locator";

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

pub(crate) fn postgres_rows_to_paginated_page(
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

pub(crate) fn postgres_preview_rows_to_paginated_page(
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
